// src/python_env.rs
// Added GLOBAL_CONFIG_STATE
use crate::utils::error::Error;
use crate::utils::path::get_python_dir;
// Removed ConfigState from here if it was only for the parameter
use crate::{config_manager::GLOBAL_CONFIG_STATE, emit_error, emit_info, err, utils::command};
use anyhow::{anyhow, Context, Result};
use flate2::read::GzDecoder;
use rand::distr::Alphanumeric;
use rand::Rng;
use regex::Regex;
use reqwest::blocking::Client;
use reqwest::Url;
use std::fs;
use std::io::Cursor;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use tar::Archive;
use tokio::process::Command;
use tracing::{error, info, warn};

const KNOWN_PATCHES: [(&str, &str, &str, &str); 7] = [
    ("3.13", "3.13.2", "https://github.com/astral-sh/python-build-standalone/releases/download/20250317/cpython-3.13.2+20250317-x86_64-pc-windows-msvc-install_only_stripped.tar.gz", "https://www.modelscope.cn/models/okoldking/ok/resolve/master/pythons/cpython-3.13.2+20250317-x86_64-pc-windows-msvc-install_only_stripped.tar.gz"),
    ("3.12", "3.12.10", "https://github.com/astral-sh/python-build-standalone/releases/download/20250517/cpython-3.12.10+20250517-x86_64-pc-windows-msvc-install_only_stripped.tar.gz", "https://www.modelscope.cn/models/okoldking/ok/resolve/master/pythons/cpython-3.12.10+20250517-x86_64-pc-windows-msvc-install_only_stripped.tar.gz"),
    ("3.11", "3.11.12", "https://github.com/astral-sh/python-build-standalone/releases/download/20250517/cpython-3.11.12+20250517-x86_64-pc-windows-msvc-install_only_stripped.tar.gz", "https://www.modelscope.cn/models/okoldking/ok/resolve/master/pythons/cpython-3.11.12+20250517-x86_64-pc-windows-msvc-install_only_stripped.tar.gz"),
    ("3.10", "3.10.16", "https://github.com/astral-sh/python-build-standalone/releases/download/20250317/cpython-3.10.16+20250317-x86_64-pc-windows-msvc-install_only_stripped.tar.gz", "https://www.modelscope.cn/models/okoldking/ok/resolve/master/pythons/cpython-3.10.16+20250317-x86_64-pc-windows-msvc-install_only_stripped.tar.gz"),
    ("3.9", "3.9.21", "https://github.com/astral-sh/python-build-standalone/releases/download/20250317/cpython-3.9.21+20250317-x86_64-pc-windows-msvc-install_only_stripped.tar.gz", "https://www.modelscope.cn/models/okoldking/ok/resolve/master/pythons/cpython-3.9.21+20250317-x86_64-pc-windows-msvc-install_only_stripped.tar.gz"),
    ("3.8", "3.8.20", "https://github.com/astral-sh/python-build-standalone/releases/download/20241002/cpython-3.8.20+20241002-x86_64-pc-windows-msvc-install_only_stripped.tar.gz", "https://www.modelscope.cn/models/okoldking/ok/resolve/master/pythons/cpython-3.8.20+20241002-x86_64-pc-windows-msvc-install_only_stripped.tar.gz"),
    ("3.7", "3.7.9", "https://github.com/astral-sh/python-build-standalone/releases/download/20200822/cpython-3.7.9-x86_64-pc-windows-msvc-shared-pgo-20200823T0118.tar.zst", "https://www.modelscope.cn/models/okoldking/ok/resolve/master/pythons/cpython-3.7.9-x86_64-pc-windows-msvc-shared-pgo-20200823T0118.tar.zst"),
];

fn get_download_url(patch_version: &str) -> Result<String> {
    let locale = "zh_CN";
    for patch in KNOWN_PATCHES.iter() {
        if patch.0 == patch_version || patch.1 == patch_version {
            return if locale == "zh_CN" {
                Ok(patch.3.to_string())
            } else {
                Ok(patch.2.to_string())
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
pub fn ensure_python_version(version_str: &str) -> Result<(PathBuf, String)> {
    let base_install_path = PathBuf::from(get_python_dir());
    fs::create_dir_all(&base_install_path).with_context(|| {
        format!(
            "Failed to create base install directory at {}",
            base_install_path.display()
        )
    })?;

    let (major_minor_from_param, _original_exact_version_opt) = parse_version(version_str)?;

    let version_to_ensure = get_latest_known_patch_for_major_minor(&major_minor_from_param)?;

    if let Some(installed_path) = find_installed_version(
        &base_install_path,
        &major_minor_from_param,
        Some(&version_to_ensure),
    )? {
        info!(
            "Found targeted version {} already installed at: {}",
            version_to_ensure,
            installed_path.display()
        );
        return Ok((installed_path, version_to_ensure.clone()));
    }

    info!(
        "Targeted version {} not found installed. Proceeding to download and install.",
        version_to_ensure
    );

    let install_dir = base_install_path.join(&version_to_ensure);
    let python_exe_path = install_dir.join("python.exe");

    let archive_url = get_download_url(&version_to_ensure)?;
    let archive_path = base_install_path.join(get_filename_from_url(archive_url.as_str())?);

    info!(
        "Downloading from {} to {}...",
        archive_url,
        archive_path.display()
    );
    if let Err(download_err) = download_file(archive_url.as_str(), &archive_path) {
        error!("Download from {} failed: {:#}", archive_url, download_err);
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
            "Downloading Python {} from {} failed",
            version_to_ensure, archive_url
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
    if let Err(extract_err) = extract_tar_gz(&archive_path, &install_dir) {
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
fn extract_tar_gz(archive_path: &Path, extract_to_dir: &Path) -> Result<()> {
    let tar_gz_file = fs::File::open(archive_path)
        .with_context(|| format!("Failed to open tar.gz archive: {}", archive_path.display()))?;
    let tar_stream = GzDecoder::new(tar_gz_file);
    let mut archive = Archive::new(tar_stream);

    info!(
        "Extracting archive {} to {}",
        archive_path.display(),
        extract_to_dir.display()
    );

    for entry_result in archive.entries()? {
        let mut entry = entry_result.context("Failed to read entry from tar archive")?;
        let path_in_archive = entry.path()?.into_owned();

        // Standalone builds typically have a "python/" prefix in the archive
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

#[cfg(target_os = "windows")]
fn find_installed_version(
    base_path: &Path,
    major_minor: &str,
    exact_version: Option<&str>,
) -> Result<Option<PathBuf>> {
    if !base_path.exists() {
        return Ok(None);
    }

    let mut latest_found_for_major_minor: Option<(String, PathBuf)> = None;
    let version_pattern = Regex::new(r"^\d+\.\d+\.\d+$").unwrap(); // Not ideal to unwrap here, but for this context it's probably fine. Consider lazy_static or once_cell.

    for entry_res in fs::read_dir(base_path).context("Failed to read base python install dir")? {
        let entry = entry_res.context("Failed to read entry in python install dir")?;
        let path = entry.path();
        if path.is_dir() {
            if let Some(dir_name_str) = path.file_name().and_then(|n| n.to_str()) {
                if dir_name_str.starts_with(major_minor) && version_pattern.is_match(dir_name_str) {
                    let current_python_exe = path.join("python.exe");
                    if current_python_exe.exists() {
                        if let Some(ex_ver) = exact_version {
                            if ex_ver == dir_name_str {
                                return Ok(Some(current_python_exe));
                            }
                        } else {
                            let current_version_is_later = match &latest_found_for_major_minor {
                                None => true,
                                Some((latest_known_ver_str, _)) => {
                                    compare_versions(dir_name_str, latest_known_ver_str)? > 0
                                }
                            };
                            if current_version_is_later {
                                latest_found_for_major_minor =
                                    Some((dir_name_str.to_string(), current_python_exe));
                            }
                        }
                    }
                }
            }
        }
    }

    if exact_version.is_some() {
        Ok(None)
    } else {
        Ok(latest_found_for_major_minor.map(|(_, path_buf)| path_buf))
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

fn compare_versions(v1: &str, v2: &str) -> Result<i8> {
    let parts1: Vec<u32> = v1
        .split('.')
        .map(|s| s.parse::<u32>())
        .collect::<std::result::Result<Vec<u32>, _>>() // Specify full path to avoid ambiguity with anyhow::Result
        .with_context(|| format!("Failed to parse version string v1: {}", v1))?;
    let parts2: Vec<u32> = v2
        .split('.')
        .map(|s| s.parse::<u32>())
        .collect::<std::result::Result<Vec<u32>, _>>() // Specify full path
        .with_context(|| format!("Failed to parse version string v2: {}", v2))?;

    for i in 0..std::cmp::max(parts1.len(), parts2.len()) {
        let p1 = *parts1.get(i).unwrap_or(&0);
        let p2 = *parts2.get(i).unwrap_or(&0);
        if p1 > p2 {
            return Ok(1);
        }
        if p1 < p2 {
            return Ok(-1);
        }
    }
    Ok(0)
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
fn download_file(url: &str, dest_path: &Path) -> Result<()> {
    let mut client_builder = Client::builder();
    if url.starts_with("https://www.modelscope.cn") {
        client_builder = client_builder.user_agent(get_user_agent());
    }
    let client = client_builder.build()?;
    let response = client
        .get(url)
        .send()
        .with_context(|| format!("Failed to initiate download from {}", url))?;

    let status = response.status();
    if !status.is_success() {
        let error_body = response.text().unwrap_or_else(|_| {
            String::from("(could not retrieve error body from non-success response)")
        });
        return Err(anyhow!(
            "Download from {} failed: Status {} {}",
            url,
            status,
            error_body
        ));
    }

    let mut file = fs::File::create(dest_path)
        .with_context(|| format!("Failed to create file at {}", dest_path.display()))?;

    // response is consumed here by .bytes() if status was success
    let content = response
        .bytes()
        .with_context(|| format!("Failed to read bytes from download response of {}", url))?;
    let mut content_cursor = Cursor::new(content);
    std::io::copy(&mut content_cursor, &mut file).with_context(|| {
        format!(
            "Failed to write downloaded content to {}",
            dest_path.display()
        )
    })?;
    Ok(())
}

/// Sets up a Python virtual environment (.venv) in the specified directory,
/// using the specified Python version.
#[cfg(target_os = "windows")]
pub fn setup_python_venv(app_name:String, venv_creation_dir: &Path, python_version_spec: &str) -> Result<PathBuf> {
    info!(
        "Setting up Python venv in {} using Python version spec '{}'",
        venv_creation_dir.display(),
        python_version_spec
    );
    let venv_path = venv_creation_dir.join(".venv");
    let venv_python_exe_path = venv_path.join("Scripts").join("python.exe");
    let (managed_python_exe, managed_python_actual_version) =
        ensure_python_version(python_version_spec)?;
    info!(
        "Using managed Python {} (version {}) for venv setup.",
        managed_python_exe.display(),
        managed_python_actual_version
    );
    let mut recreate_venv = true;
    if venv_path.is_dir() && venv_python_exe_path.exists() {
        info!("Found existing venv at {}", venv_path.display());
        match get_python_version_from_exe(&venv_python_exe_path) {
            Ok(venv_python_version) => {
                if venv_python_version == managed_python_actual_version {
                    info!(
                        "Venv Python version ({}) matches required ({}). Reusing existing venv.",
                        venv_python_version, managed_python_actual_version
                    );
                    recreate_venv = false;
                } else {
                    warn!("Venv Python version mismatch (found: {}, required: {}). Will recreate venv.", venv_python_version, managed_python_actual_version);
                }
            }
            Err(e) => warn!(
                "Failed to get version from existing venv Python at {}: {:#}. Will recreate venv.",
                venv_python_exe_path.display(),
                e
            ),
        }
    } else {
        info!(
            "No existing venv or venv python {} is missing. Will create/recreate venv.",
            venv_python_exe_path.display()
        );
    }

    if recreate_venv {
        if venv_path.exists() {
            info!("Removing existing venv at {}", venv_path.display());
            fs::remove_dir_all(&venv_path).with_context(|| {
                format!("Failed to remove existing venv at {}", venv_path.display())
            })?;
        }
        let venv_creation_cmd_output = std::process::Command::new(&managed_python_exe)
            .arg("-m")
            .arg("venv")
            .arg(&venv_path)
            .output()
            .with_context(|| {
                format!(
                    "Failed to execute venv creation for {} using {}",
                    venv_path.display(),
                    managed_python_exe.display()
                )
            })?;

        let stdout_str = String::from_utf8_lossy(&venv_creation_cmd_output.stdout);
        let trimmed_stdout = stdout_str.trim();
        if !trimmed_stdout.is_empty() {
            emit_info!("{}", trimmed_stdout);
        }

        let stderr_str = String::from_utf8_lossy(&venv_creation_cmd_output.stderr);
        let trimmed_stderr = stderr_str.trim();
        if !trimmed_stderr.is_empty() {
            emit_error!("{}", trimmed_stderr);
        }

        if !venv_creation_cmd_output.status.success() {
            error!("Venv creation stdout: {}", stdout_str);
            error!("Venv creation stderr: {}", stderr_str);
            return Err(anyhow!(
                "Failed to create venv at {}. Exit code: {:?}. Stderr: {}",
                venv_path.display(),
                venv_creation_cmd_output.status.code(),
                stderr_str
            ));
        }
        info!("Venv created successfully at {}", venv_path.display());
    }
    if !venv_python_exe_path.exists() {
        return Err(anyhow!(
            "Venv python.exe not found at {} after setup.",
            venv_python_exe_path.display()
        ));
    }

    Ok(venv_python_exe_path)
}
#[cfg(not(target_os = "windows"))]
pub fn setup_python_venv(_venv_creation_dir: &Path, _python_version_spec: &str) -> Result<PathBuf> {
    Err(anyhow!(
        "setup_python_venv is only implemented for Windows."
    ))
}

/// Synchronizes the Python virtual environment with a requirements.txt file using pip-sync.
#[cfg(target_os = "windows")]
pub async fn install_requirements(
    app_name: &str,
    venv_python_exe: &Path,
    requirements_txt_path: &Path,
    project_dir: &Path,
) -> Result<(), Error> {
    if !venv_python_exe.exists() {
        err!(
            "Venv Python executable not found at {}",
            venv_python_exe.display()
        );
    }
    if !requirements_txt_path.exists() {
        err!(
            "requirements.txt not found at {}",
            requirements_txt_path.display()
        );
    }
    if !project_dir.is_dir() {
        err!(
            "Project directory for pip-sync execution not found or not a directory: {}",
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

    let mut pip_install_cmd = Command::new(venv_python_exe);
    pip_install_cmd
        .arg("-m")
        .arg("pip")
        .arg("install")
        .arg("-r")
        .arg(requirements_txt_path);

    if let Some(cache_dir) = pip_cache_dir {
        pip_install_cmd.arg("--cache-dir").arg(cache_dir);
    }

    if let Some(index_url) = pip_index_url {
        pip_install_cmd.arg("--index-url").arg(index_url);
    }

    let pip_install_desc = format!("pip install -r {}", requirements_txt_path.display());

    command::run_command_and_stream_output(pip_install_cmd, app_name, &pip_install_desc).await?;

    emit_info!(
        app_name,
        "Successfully installed requirements from {}.",
        requirements_txt_path.display()
    );
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn install_requirements(
    _app_name: &str,
    _venv_python_exe: &Path,
    _requirements_txt_path: &Path,
    _project_dir: &Path,
) -> Result<()> {
    // Even though this function errors out, its signature should match.
    // Accessing GLOBAL_CONFIG_STATE here isn't strictly necessary for its current logic,
    // but if it were to do any work dependent on config, it would need it.
    // let _config_state = GLOBAL_CONFIG_STATE.get().ok_or_else(|| anyhow!("GLOBAL_CONFIG_STATE not initialized."))?;
    Err(anyhow!(
        "install_requirements (using pip-sync) is only implemented for Windows."
    ))
}

// Helper to get Python version from an executable
#[cfg(target_os = "windows")]
fn get_python_version_from_exe(python_exe_path: &Path) -> Result<String> {
    if !python_exe_path.exists() {
        return Err(anyhow!(
            "Python executable not found at {}",
            python_exe_path.display()
        ));
    }
    let version_cmd_output = std::process::Command::new(python_exe_path)
        .arg("--version")
        .creation_flags(0x08000000)
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
