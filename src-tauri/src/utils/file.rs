use std::path::{Path, PathBuf};
use std::{fs, io};
use tracing::{debug, info};
use walkdir::WalkDir;
use anyhow::{Context, Result};
use crate::utils::command::new_cmd;

pub fn copy_dir_recursive_excluding_sync(
    src: &Path,
    dst: &Path,
    exclude: &[&str],
) -> io::Result<()> {
    if !dst.exists() {
        fs::create_dir_all(dst)?;
    }
    for entry_res in fs::read_dir(src)? {
        let entry = entry_res?;
        let ty = entry.file_type()?;
        let src_path = entry.path();
        let file_name_os = src_path.file_name().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "Source path missing filename")
        })?;
        if exclude.iter().any(|ex| file_name_os == *ex) {
            continue;
        }
        let dst_path = dst.join(file_name_os);
        if ty.is_dir() {
            copy_dir_recursive_excluding_sync(&src_path, &dst_path, &[])?;
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

pub fn sync_delete_extra_files(working_dir: &Path, repo_dir: &Path) -> io::Result<()> {
    let mut paths_to_delete: Vec<PathBuf> = Vec::new();

    let walker = WalkDir::new(working_dir).into_iter().filter_entry(|entry| {
        let working_path = entry.path();
        if working_path == working_dir {
            return true; // Allow walking the root
        }
        // Check if the equivalent path exists in the repo_dir
        // If a directory in working_dir doesn't exist in repo_dir, prune it (don't walk into it)
        // and it will be caught later for deletion if it's empty or by the recursive deletion.
        let relative_path = working_path
            .strip_prefix(working_dir)
            .unwrap_or_else(|_| Path::new(""));
        if relative_path.as_os_str().is_empty() && working_path != working_dir {
            return false; // Should not happen if strip_prefix is correct
        }
        let repo_equivalent_path = repo_dir.join(relative_path);

        if entry.file_type().is_dir() {
            repo_equivalent_path.is_dir() // Keep entry for further walking only if dir exists in repo
        } else {
            true // Keep file entries for individual checks later
        }
    });

    for entry_res in walker {
        let entry = entry_res
            .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("Walkdir error: {}", e)))?;
        let working_path = entry.path();

        if working_path == working_dir {
            // Skip root working_dir itself
            continue;
        }

        let relative_path = working_path
            .strip_prefix(working_dir)
            .expect("Path strip failed unexpectedly for a non-root entry");
        let repo_equivalent_path = repo_dir.join(relative_path);

        if !repo_equivalent_path.exists() {
            paths_to_delete.push(working_path.to_path_buf());
        }
    }

    paths_to_delete.sort_by(|a, b| b.cmp(a)); // Delete files/subdirs before parent dirs

    for path_to_delete in paths_to_delete {
        if !path_to_delete.exists() {
            // Already deleted (e.g. part of a deleted parent dir)
            continue;
        }
        if path_to_delete.is_dir() {
            // Attempt to remove dir; if it fails (e.g. not empty due to files not in repo), use remove_dir_all
            if fs::remove_dir(&path_to_delete).is_err() {
                debug!(
                    "Failed to remove_dir {}, trying remove_dir_all",
                    path_to_delete.display()
                );
                fs::remove_dir_all(&path_to_delete)?;
            }
        } else {
            fs::remove_file(&path_to_delete)?;
        }
    }
    Ok(())
}

pub async fn delete_dir_if_exist(working_dir_path: &Path) -> Result<()> {
    let result = fs::remove_dir_all(working_dir_path);

    info!("Delete dir if exist: {} {:?}", working_dir_path.display(), result);

    if let Err(e) = &result {
        if e.kind() == io::ErrorKind::NotFound {
            return Ok(());
        }

        #[cfg(windows)]
        {
            let status = new_cmd("cmd")
                .args([
                    "/C",
                    "rd",
                    "/S",
                    "/Q",
                    working_dir_path
                        .to_str()
                        .context("Path contains non-UTF8 characters")?,
                ])
                .status()
                .await
                .context("Failed to spawn 'rd' command")?;
            if status.success() {
                info!("Delete dir using rd success {}", working_dir_path.display());
                return Ok(());
            }
        }
    }

    result.with_context(|| format!("Failed to remove dir {}", working_dir_path.display()))
}