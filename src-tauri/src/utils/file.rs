use crate::utils::command::new_cmd;
use anyhow::{Context, Result};
use std::path::Path;
use std::{fs, io};
use tracing::info;

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
