// src/python_env.rs
use crate::utils::error::Error;
use crate::utils::path::{get_python_dir, get_python_exe};
use crate::{config_manager::GLOBAL_CONFIG_STATE, emit_info, emit_update_info, err, utils::command};
use anyhow::{anyhow, Context, Result};
use flate2::read::GzDecoder;
use rand::distr::Alphanumeric;
use rand::Rng;
use reqwest::Client;
use reqwest::Url;
use std::fs;
use std::io::{Read, Write};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use tar::Archive;
use tokio::process::Command;
use tracing::{error, info, warn};
use crate::utils::locale::get_locale;
use zip::ZipArchive;

const KNOWN_PATCHES: [(&str, &str, &str, &str); 7] = [
    ("3.13", "3.13.5", "https://www.python.org/ftp/python/3.13.5/python-3.13.5-amd64.zip", "https://mirrors.huaweicloud.com/python/3.13.5/python-3.13.5-amd64.zip"),
    ("3.12", "3.12.10", "https://www.python.org/ftp/python/3.12.10/python-3.12.10-amd64.zip", "https://mirrors.huaweicloud.com/python/3.12.10/python-3.12.10-amd64.zip"),
    ("3.11", "3.11.9", "https://www.python.org/ftp/python/3.11.9/python-3.11.9-amd64.zip", "https://mirrors.huaweicloud.com/python/3.11.9/python-3.11.9-amd64.zip"),
    ("3.10", "3.10.16", "https://github.com/astral-sh/python-build-standalone/releases/download/20250317/cpython-3.10.16+20250317-x86_64-pc-windows-msvc-install_only_stripped.tar.gz", "https://www.modelscope.cn/models/okoldking/ok/resolve/master/pythons/cpython-3.10.16+20250317-x86_64-pc-windows-msvc-install_only_stripped.tar.gz"),
    ("3.9", "3.9.21", "https://github.com/astral-sh/python-build-standalone/releases/download/20250317/cpython-3.9.21+20250317-x86_64-pc-windows-msvc-install_only_stripped.tar.gz", "https://www.modelscope.cn/models/okoldking/ok/resolve/master/pythons/cpython-3.9.21+20250317-x86_64-pc-windows-msvc-install_only_stripped.tar.gz"),
    ("3.8", "3.8.20", "https://github.com/astral-sh/python-build-standalone/releases/download/20241002/cpython-3.8.20+20241002-x86_64-pc-windows-msvc-install_only_stripped.tar.gz", "https://www.modelscope.cn/models/okoldking/ok/resolve/master/pythons/cpython-3.8.20+20241002-x86_64-pc-windows-msvc-install_only_stripped.tar.gz"),
    ("3.7", "3.7.9", "https://github.com/astral-sh/python-build-standalone/releases/download/20200822/cpython-3.7.9-x86_64-pc-windows-msvc-shared-pgo-20200823T0118.tar.zst", "https://www.modelscope.cn/models/okoldking/ok/resolve/master/pythons/cpython-3.7.9-x86_64-pc-windows-msvc-shared-pgo-20200823T0118.tar.zst"),
];

fn get_download_urls(patch_version: &str) -> Result<(String, String)> {
    let locale = get_locale();
    for patch in KNOWN_PATCHES.iter() {
        if patch.0 == patch_version || patch.1 == patch_version {
            return if locale == "zh_CN" {
                Ok((patch.3.to_string(), patch.2.to_string()))
            } else {
                Ok((patch.2.to_string(), patch.3.to_string()))
            };
        }
    }
    Err(anyhow!(
        "No download URL found for patch version: {}",
        patch_version
    ))
}

fn get_filename_from_url(url_string: &str) -> Result<String> {
    let parsed_url =
        Url::parse(url_string).with_context(|| format!("Failed to parse URL: '{}'", url_string))?;
    parsed_url
        .path_segments()
        .and_then(|segments| segments.last())
        .filter(|segment| !segment.is_empty())
        .map(|segment| segment.to_string())
        .ok_or_else(|| anyhow!("No filename found in the URL path of '{}'", url_string))
}

#[cfg(target_os = "windows")]
async fn ensure_python_version(app_name: &str, version_str: &str) -> Result<(PathBuf, String)> {
    let install_dir = PathBuf::from(get_python_dir(app_name));
    fs::create_dir_all(&install_dir).with_context(|| {
        format!(
            "Failed to create install directory at {}",
            install_dir.display()
        )
    })?;

    let python_exe_path = install_dir.join("python.exe");
    let (major_minor_from_param, _) = parse_version(version_str)?;

    if python_exe_path.exists() {
        match get_python_version_from_exe(&python_exe_path) {
            Ok(installed_version) => {
                let (installed_major_minor, _) = parse_version(&installed_version)?;
                if installed_major_minor == major_minor_from_param {
                    info!(
                        "Found compatible Python version {} at {}",
                        installed_version,
                        python_exe_path.display()
                    );
                    return Ok((python_exe_path, installed_version));
                } else {
                    info!(
                        "Found incompatible Python version {} (required {}). Removing and reinstalling.",
                        installed_version, major_minor_from_param
                    );
                    fs::remove_dir_all(&install_dir).with_context(|| format!("Failed to remove existing Python installation at {}", install_dir.display()))?;
                    fs::create_dir_all(&install_dir).with_context(|| format!("Failed to recreate Python installation directory at {}", install_dir.display()))?;
                }
            }
            Err(e) => {
                warn!(
                    "Existing python.exe at {} is corrupted or unusable ({}). Removing and reinstalling.",
                    python_exe_path.display(), e
                );
                fs::remove_dir_all(&install_dir).with_context(|| format!("Failed to remove corrupted Python installation at {}", install_dir.display()))?;
                fs::create_dir_all(&install_dir).with_context(|| format!("Failed to recreate Python installation directory at {}", install_dir.display()))?;
            }
        }
    }

    let version_to_ensure = get_latest_known_patch_for_major_minor(&major_minor_from_param)?;
    info!(
        "Python {} not found or incompatible. Proceeding to download and install.",
        version_to_ensure
    );

    let (primary_url, backup_url) = get_download_urls(&version_to_ensure)?;
    let archive_path = std::env::temp_dir().join(get_filename_from_url(&primary_url)?);

    let download_result = match download_file(&primary_url, &archive_path, app_name).await {
        Ok(()) => Ok(()),
        Err(e) => {
            warn!(
                "Download from primary URL {} failed: {:#}. Trying backup URL: {}",
                primary_url, e, backup_url
            );
            if archive_path.exists() {
                fs::remove_file(&archive_path).ok();
            }
            download_file(&backup_url, &archive_path, app_name).await
        }
    };

    if let Err(download_err) = download_result {
        error!(
            "Download failed from both primary and backup URLs: {:#}",
            download_err
        );
        if archive_path.exists() {
            info!(
                "Attempting to remove partially downloaded file: {}",
                archive_path.display()
            );
            if let Err(remove_file_err) = fs::remove_file(&archive_path) {
                warn!(
                    "Failed to remove partially downloaded file {}: {}",
                    archive_path.display(),
                    remove_file_err
                );
            }
        }
        return Err(download_err.context(format!(
            "Downloading Python {} from all available sources failed",
            version_to_ensure
        )));
    }
    info!(
        "Successfully downloaded archive to {}",
        archive_path.display()
    );

    info!(
        "Creating installation directory {}...",
        install_dir.display()
    );
    if let Err(create_dir_err) = fs::create_dir_all(&install_dir) {
        error!(
            "Failed to create target install directory {}: {}",
            install_dir.display(),
            create_dir_err
        );
        if archive_path.exists() {
            info!(
                "Attempting to remove downloaded archive {} as installation cannot proceed.",
                archive_path.display()
            );
            if let Err(remove_file_err) = fs::remove_file(&archive_path) {
                warn!(
                    "Failed to remove archive file {} after directory creation failure: {}",
                    archive_path.display(),
                    remove_file_err
                );
            }
        }
        return Err(anyhow!(create_dir_err).context(format!(
            "Creating install directory {} failed",
            install_dir.display()
        )));
    }

    info!(
        "Extracting archive {} to {}...",
        archive_path.display(),
        install_dir.display()
    );
    if let Err(extract_err) = extract_archive(&archive_path, &install_dir) {
        error!(
            "Extraction from {} to {} failed: {:#}",
            archive_path.display(),
            install_dir.display(),
            extract_err
        );
        info!("Attempting to clean up downloaded file and extraction folder.");
        if archive_path.exists() {
            if let Err(remove_file_err) = fs::remove_file(&archive_path) {
                warn!(
                    "Failed to remove archive file {}: {}",
                    archive_path.display(),
                    remove_file_err
                );
            }
        }
        if install_dir.exists() {
            if let Err(remove_dir_err) = fs::remove_dir_all(&install_dir) {
                warn!(
                    "Failed to remove extraction directory {}: {}",
                    install_dir.display(),
                    remove_dir_err
                );
            }
        }
        return Err(extract_err.context(format!(
            "Extracting archive {} to {} failed",
            archive_path.display(),
            install_dir.display()
        )));
    }
    info!(
        "Successfully extracted Python {} to {}",
        version_to_ensure,
        install_dir.display()
    );

    info!(
        "Removing archive file {} after successful extraction...",
        archive_path.display()
    );
    if let Err(remove_file_err) = fs::remove_file(&archive_path) {
        warn!("Failed to remove archive file {} after successful extraction: {}. This is non-critical.", archive_path.display(), remove_file_err);
    }

    if !python_exe_path.exists() {
        error!("CRITICAL: python.exe not found at {} after extraction reported success. Installation is incomplete.", python_exe_path.display());
        if install_dir.exists() {
            info!(
                "Attempting to remove incomplete installation directory: {}",
                install_dir.display()
            );
            if let Err(remove_dir_err) = fs::remove_dir_all(&install_dir) {
                warn!(
                    "Failed to remove incomplete installation directory {}: {}",
                    install_dir.display(),
                    remove_dir_err
                );
            }
        }
        return Err(anyhow!(
            "python.exe not found at {} after extraction, though extraction reported success. The installation is likely corrupt.",
            python_exe_path.display()
        ));
    }

    Ok((python_exe_path, version_to_ensure))
}

#[cfg(target_os = "windows")]
fn extract_archive(archive_path: &Path, extract_to_dir: &Path) -> Result<()> {
    let file_name = archive_path.file_name().and_then(|n| n.to_str()).ok_or_else(|| anyhow!("Could not get file name from path {}", archive_path.display()))?;

    if file_name.ends_with(".zip") {
        extract_zip(archive_path, extract_to_dir)
    } else if file_name.ends_with(".tar.gz") {
        extract_tar_gz(archive_path, extract_to_dir)
    } else {
        Err(anyhow!("Unsupported archive format: {}", file_name))
    }
}

#[cfg(target_os = "windows")]
fn extract_zip(archive_path: &Path, extract_to_dir: &Path) -> Result<()> {
    let zip_file = fs::File::open(archive_path)
        .with_context(|| format!("Failed to open zip archive: {}", archive_path.display()))?;
    let mut archive = ZipArchive::new(zip_file)
        .with_context(|| format!("Failed to read zip archive: {}", archive_path.display()))?;
    archive.extract(extract_to_dir).with_context(|| {
        format!(
            "Failed to extract zip archive {} to {}",
            archive_path.display(),
            extract_to_dir.display()
        )
    })?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn extract_tar_gz(archive_path: &Path, extract_to_dir: &Path) -> Result<()> {
    let tar_gz_file = fs::File::open(archive_path)
        .with_context(|| format!("Failed to open tar.gz archive: {}", archive_path.display()))?;
    let tar_stream = GzDecoder::new(tar_gz_file);
    let mut archive = Archive::new(tar_stream);

    for entry_result in archive.entries()? {
        let mut entry = entry_result.context("Failed to read entry from tar archive")?;
        let path_in_archive = entry.path()?.into_owned();
        let path_after_stripping_python_dir = match path_in_archive.strip_prefix("python") {
            Ok(p) => p.to_path_buf(),
            Err(_) => {
                warn!(
                    "Archive entry {} not under expected 'python/' top-level directory. Skipping.",
                    path_in_archive.display()
                );
                continue;
            }
        };

        if path_after_stripping_python_dir.as_os_str().is_empty() {
            continue;
        }

        let outpath = extract_to_dir.join(path_after_stripping_python_dir);

        if entry.header().entry_type().is_dir() {
            fs::create_dir_all(&outpath).with_context(|| {
                format!(
                    "Failed to create directory during tar extraction: {}",
                    outpath.display()
                )
            })?;
        } else {
            if let Some(p_parent) = outpath.parent() {
                if !p_parent.exists() {
                    fs::create_dir_all(p_parent).with_context(|| {
                        format!(
                            "Failed to create parent directory for file during tar extraction: {}",
                            p_parent.display()
                        )
                    })?;
                }
            }
            entry.unpack(&outpath).with_context(|| {
                format!(
                    "Failed to unpack file entry {:?} to {}",
                    path_in_archive,
                    outpath.display()
                )
            })?;
        }
    }
    Ok(())
}

fn parse_version(version_str: &str) -> Result<(String, Option<String>)> {
    let parts: Vec<&str> = version_str.split('.').collect();
    match parts.len() {
        2 => Ok((format!("{}.{}", parts[0], parts[1]), None)),
        3 => Ok((
            format!("{}.{}", parts[0], parts[1]),
            Some(version_str.to_string()),
        )),
        _ => Err(anyhow!(
            "Invalid version format: {}. Expected X.Y or X.Y.Z",
            version_str
        )),
    }
}

fn get_latest_known_patch_for_major_minor(major_minor: &str) -> Result<String> {
    info!(
        "Determining latest known patch for {} series from hardcoded list.",
        major_minor
    );
    for (major_minor_key, patch_version, _, _) in KNOWN_PATCHES.iter() {
        if *major_minor_key == major_minor {
            return Ok(patch_version.to_string());
        }
    }
    Err(anyhow!(
        "Unsupported major.minor version for resolving latest patch: {}. Please update 'KNOWN_PATCHES' if needed.",
        major_minor
    ))
}

pub fn get_supported_python_versions() -> Vec<String> {
    KNOWN_PATCHES
        .iter()
        .map(|(patch, _, _, _)| patch.to_string())
        .collect()
}

fn get_user_agent() -> String {
    let random_string: String = rand::rng()
        .sample_iter(Alphanumeric)
        .take(32)
        .map(char::from)
        .collect();
    format!(
        "modelscope/{}; python/{}; session_id/{}; platform/{}; processor/{}; env/{}; user/{}",
        "1.26.0",
        "3.12.3",
        random_string,
        "Windows-11-10.0.26100-SP0 AMD64 Family 25 Model 97 Stepping 2, AuthenticAMD",
        "AuthenticAMD",
        "custom",
        "unknown"
    )
}

async fn download_file(url: &str, dest_path: &Path, app_name: &str) -> Result<()> {
    let mut client_builder = Client::builder();
    if url.starts_with("https://www.modelscope.cn") {
        client_builder = client_builder.user_agent(get_user_agent());
    }
    let client = client_builder.build()?;
    let response = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("Failed to initiate download from {}", url))?;

    let status = response.status();
    if !status.is_success() {
        let error_body = response.text().await.unwrap_or_else(|_| {
            String::from("(could not retrieve error body from non-success response)")
        });
        return Err(anyhow!(
            "Download from {} failed: Status {} {}",
            url,
            status,
            error_body
        ));
    }

    let total_size = response
        .content_length()
        .ok_or_else(|| anyhow!("Failed to get content length from {}", url))?;

    let mut file = fs::File::create(dest_path)
        .with_context(|| format!("Failed to create file at {}", dest_path.display()))?;

    emit_info!(app_name, "Start Downloading Python from {}...", url);
    emit_info!(app_name, "Python Download Progress: 0%");
    let mut downloaded: u64 = 0;
    let mut last_reported_percent: i64 = -1;

    let mut stream = response.bytes_stream();
    while let Some(item) = futures_util::StreamExt::next(&mut stream).await {
        let chunk = item.with_context(|| format!("Failed to read chunk from download stream of {}", url))?;
        file.write_all(&chunk)
            .with_context(|| format!("Failed to write chunk to file {}", dest_path.display()))?;
        downloaded += chunk.len() as u64;

        if total_size > 0 {
            let percent = (100 * downloaded / total_size) as i64;
            if percent > last_reported_percent {
                emit_update_info!(app_name, "Python Download Progress: {}%", percent);
                last_reported_percent = percent;
            }
        }
    }
    Ok(())
}

#[cfg(target_os = "windows")]
pub async fn setup_python_env(
    app_name: String,
    python_version_spec: &str,
) -> Result<PathBuf> {
    emit_info!(
        app_name,
        "Ensuring Python installation for version spec '{}'",
        python_version_spec
    );

    let (managed_python_exe, managed_python_actual_version) =
        ensure_python_version(&app_name, python_version_spec).await?;

    emit_info!(
        app_name,
        "Using managed Python {} (version {})",
        managed_python_exe.display(),
        managed_python_actual_version
    );

    Ok(managed_python_exe)
}
#[cfg(not(target_os = "windows"))]
pub fn setup_python_env(
    _app_name: String,
    _python_version_spec: &str,
) -> Result<PathBuf> {
    Err(anyhow!(
        "setup_python_env is only implemented for Windows."
    ))
}

#[cfg(target_os = "windows")]
pub async fn install_requirements(
    app_name: &str,
    requirements: &str,
    project_dir: &Path,
    pip_args: &str,
) -> Result<(), Error> {
    let python_exe = get_python_exe(app_name, false);
    if !python_exe.exists() {
        err!(
            "Python executable not found at {}",
            python_exe.display()
        );
    }
    if !project_dir.is_dir() {
        err!(
            "Project directory for pip execution not found or not a directory: {}",
            project_dir.display()
        );
    }

    let config_state = GLOBAL_CONFIG_STATE.get().ok_or_else(|| {
        anyhow!("GLOBAL_CONFIG_STATE not initialized. Call init_config_manager first.")
    })?;

    let (pip_cache_dir, pip_index_url) = {
        let config = config_state.lock().unwrap();
        let cache_dir = config.get_effective_pip_cache_dir();
        let index_url = config.get_effective_pip_index_url();
        (cache_dir, index_url)
    };

    let mut pip_install_cmd = Command::new(python_exe);
    pip_install_cmd
        .current_dir(project_dir)
        .arg("-m")
        .arg("pip")
        .arg("install")
        .arg("--no-warn-script-location");

    let mut use_config_index_url = true;
    if !pip_args.is_empty() {
        if pip_args
            .split_whitespace()
            .any(|arg| arg == "--index-url" || arg == "-i")
        {
            use_config_index_url = false;
        }
        pip_install_cmd.args(pip_args.split_whitespace());
    }

    let pip_install_desc;
    if requirements.ends_with(".txt") {
        let requirements_path = project_dir.join(requirements);
        if !requirements_path.exists() {
            err!(
                "Requirements file not found at {}",
                requirements_path.display()
            );
        }
        pip_install_cmd.arg("-r").arg(&requirements_path);
        pip_install_desc = format!("pip install -r {}", requirements_path.display());
    } else {
        pip_install_cmd.arg(requirements);
        pip_install_desc = format!("pip install {}", requirements);
    }

    if let Some(cache_dir) = pip_cache_dir {
        pip_install_cmd.arg("--cache-dir").arg(cache_dir);
    }

    if use_config_index_url {
        emit_info!(app_name, "set --index-url {:?}", pip_index_url);

        if let Some(index_url) = pip_index_url {
            pip_install_cmd.arg("--index-url").arg(index_url);
        }
    }

    command::run_command_and_stream_output(pip_install_cmd, app_name, &pip_install_desc).await?;

    emit_info!(
        app_name,
        "Successfully installed requirements from '{}'.",
        requirements
    );
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub async fn install_requirements(
    _app_name: &str,
    _requirements: &str,
    _project_dir: &Path,
    _pip_args: &str,
) -> Result<(), Error> {
    err!("install_requirements is only implemented for Windows.")
}

#[cfg(target_os = "windows")]
fn get_python_version_from_exe(python_exe_path: &Path) -> Result<String> {
    if !python_exe_path.exists() {
        return Err(anyhow!(
            "Python executable not found at {}",
            python_exe_path.display()
        ));
    }
    let version_cmd_output = std::process::Command::new(python_exe_path)
        .creation_flags(0x08000000)
        .arg("--version")
        .output()
        .with_context(|| format!("Failed to execute {} --version", python_exe_path.display()))?;

    let stdout_str = String::from_utf8_lossy(&version_cmd_output.stdout);
    let trimmed_stdout = stdout_str.trim();

    let stderr_str = String::from_utf8_lossy(&version_cmd_output.stderr);
    let trimmed_stderr = stderr_str.trim();

    if !version_cmd_output.status.success() {
        return Err(anyhow!(
            "Python --version command failed for {}: Stdout: '{}', Stderr: '{}'",
            python_exe_path.display(),
            trimmed_stdout,
            trimmed_stderr
        ));
    }

    let version_source_str = if !trimmed_stdout.is_empty() && trimmed_stdout.starts_with("Python ")
    {
        trimmed_stdout
    } else if !trimmed_stderr.is_empty() && trimmed_stderr.starts_with("Python ") {
        warn!(
            "Python --version stdout was '{}', using stderr: '{}' for {}",
            trimmed_stdout,
            trimmed_stderr,
            python_exe_path.display()
        );
        trimmed_stderr
    } else if !trimmed_stdout.is_empty() {
        trimmed_stdout
    } else {
        return Err(anyhow!(
            "Python --version command for {} produced no usable output starting with 'Python ' on stdout or stderr. Stdout: '{}', Stderr: '{}'",
            python_exe_path.display(),
            trimmed_stdout,
            trimmed_stderr
        ));
    };

    if let Some(version_part) = version_source_str.split_whitespace().nth(1) {
        Ok(version_part.to_string())
    } else {
        Err(anyhow!(
            "Could not parse version from Python --version output: '{}' for {}",
            version_source_str,
            python_exe_path.display()
        ))
    }
}