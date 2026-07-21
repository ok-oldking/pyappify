use crate::utils::command::new_cmd;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::{fs, io};
use tracing::{debug, info};
use walkdir::WalkDir;

pub fn copy_dir_recursive_filtered_sync<F>(
    src: &Path,
    dst: &Path,
    exclude: &[&str],
    should_exclude: &F,
) -> io::Result<()>
where
    F: Fn(&Path) -> bool,
{
    copy_dir_recursive_filtered_inner(src, src, dst, exclude, should_exclude)
}

fn copy_dir_recursive_filtered_inner<F>(
    root_src: &Path,
    src: &Path,
    dst: &Path,
    exclude: &[&str],
    should_exclude: &F,
) -> io::Result<()>
where
    F: Fn(&Path) -> bool,
{
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
        let relative_path = src_path.strip_prefix(root_src).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "Failed to make {} relative to {}: {}",
                    src_path.display(),
                    root_src.display(),
                    error
                ),
            )
        })?;
        if should_exclude(relative_path) {
            debug!("Skipping excluded source path {}", src_path.display());
            continue;
        }
        let dst_path = dst.join(file_name_os);
        if ty.is_dir() {
            copy_dir_recursive_filtered_inner(
                root_src,
                &src_path,
                &dst_path,
                exclude,
                should_exclude,
            )?;
        } else {
            fs::copy(&src_path, &dst_path).map_err(|error| {
                io::Error::new(
                    error.kind(),
                    format!(
                        "Failed to copy {} to {}: {}",
                        src_path.display(),
                        dst_path.display(),
                        error
                    ),
                )
            })?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::copy_dir_recursive_filtered_sync;
    use std::{fs, time::SystemTime};

    #[test]
    fn filtered_copy_skips_git_ignored_directories() {
        let unique = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "pyappify-filtered-copy-{}-{unique}",
            std::process::id()
        ));
        let source = root.join("source");
        let destination = root.join("destination");

        fs::create_dir_all(source.join("node_modules/package")).unwrap();
        fs::write(source.join(".gitignore"), "node_modules/\n").unwrap();
        fs::write(source.join("app.py"), "print('tracked')").unwrap();
        fs::write(source.join("node_modules/package/index.js"), "ignored").unwrap();
        let repository = git2::Repository::init(&source).unwrap();

        copy_dir_recursive_filtered_sync(&source, &destination, &[".git"], &|relative| {
            repository.status_should_ignore(relative).unwrap()
        })
        .unwrap();

        assert_eq!(
            fs::read_to_string(destination.join("app.py")).unwrap(),
            "print('tracked')"
        );
        assert!(!destination.join("node_modules").exists());
        assert!(!destination.join(".git").exists());

        fs::remove_dir_all(root).unwrap();
    }
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

    info!(
        "Delete dir if exist: {} {:?}",
        working_dir_path.display(),
        result
    );

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
