// filename: src/defender.rs
use tracing::{debug, error, info};
use tokio::process::Command;
use crate::utils::command::is_currently_admin;

pub async fn is_defender_excluded(folder_path: &str) -> Result<bool, String> {
    #[cfg(not(windows))]
    {
        info!("Not on Windows, skipping Defender check.");
        let _ = folder_path;
        return Ok(true);
    }

    #[cfg(windows)]
    {
        let is_admin = is_currently_admin().await;
        info!("Checking Windows Defender exclusion for '{}' is_admin {}", folder_path, is_admin);
        if !is_admin {
            return Ok(true);
        }
        let get_output = Command::new("powershell")
            .args([
                "-Command",
                "Get-MpPreference | Select-Object -ExpandProperty ExclusionPath",
            ])
            .output()
            .await;

        let exclusions = match get_output {
            Ok(output) => {
                if !output.status.success() {
                    let err_msg = format!(
                        "Failed to get Defender preferences: {}",
                        String::from_utf8_lossy(&output.stderr)
                    );
                    error!("{}", err_msg);
                    return Err(err_msg);
                }
                String::from_utf8_lossy(&output.stdout).to_string()
            }
            Err(e) => {
                let err_msg = format!("Failed to execute PowerShell to get preferences: {}", e);
                error!("{}", err_msg);
                return Err(err_msg);
            }
        };
        debug!("defender exclusions {}", exclusions);
        Ok(exclusions
            .lines()
            .any(|line| line.eq_ignore_ascii_case(folder_path)))
    }
}

pub async fn ensure_defender_exclusion(folder_path: &str) -> Result<(), String> {
    if is_defender_excluded(folder_path).await? {
        info!(
            "'{}' is already excluded or check is not applicable.",
            folder_path
        );
        return Ok(());
    }

    info!("'{}' not found in exclusion list. Adding it...", folder_path);
    let add_output = Command::new("powershell")
        .args(["-Command", "Add-MpPreference", "-ExclusionPath", folder_path])
        .output()
        .await;

    match add_output {
        Ok(output) => {
            if output.status.success() {
                info!(
                    "Successfully added '{}' to the exclusion list.",
                    folder_path
                );
                Ok(())
            } else {
                let err_msg = format!(
                    "Failed to add exclusion. Ensure you are running with administrator privileges. Error: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
                error!("{}", err_msg);
                Err(err_msg)
            }
        }
        Err(e) => {
            let err_msg = format!("Failed to execute PowerShell to add exclusion: {}", e);
            error!("{}", err_msg);
            Err(err_msg)
        }
    }
}