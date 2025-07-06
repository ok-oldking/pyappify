use std::path::Path;
// filename: src/defender.rs
use tracing::{debug, error, info};
use tokio::process::Command;
use crate::utils::command::is_currently_admin;
use crate::utils::path::{get_cwd, path_to_abs};

pub async fn is_defender_excluded() -> Result<bool, String> {
    #[cfg(not(windows))]
    {
        info!("Not on Windows, skipping Defender check.");
        return Ok(true);
    }
    let cwd_string = path_to_abs(get_cwd().as_ref());
    #[cfg(windows)]
    {
        let cwd = Path::new(&cwd_string);
        let is_admin = is_currently_admin().await;
        info!(
            "Checking Windows Defender exclusion for '{}' is_admin {}",
            cwd_string, is_admin
        );
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
        let excluded = exclusions
            .lines()
            .any(|excluded_line| cwd.ancestors().any(|p| p.as_os_str().eq_ignore_ascii_case(excluded_line)));

        debug!("defender exclusions {} \nexcluded:{}", exclusions, excluded);
        Ok(excluded)
    }
}


#[tauri::command]
pub async fn add_defender_exclusion() -> Result<(), String> {
    let cwd_string = path_to_abs(get_cwd().as_ref());
    let cwd = cwd_string.as_str();

    info!("'{}' not found in exclusion list. Adding it...", cwd);
    let add_output = Command::new("powershell")
        .args(["-Command", "Add-MpPreference", "-ExclusionPath", cwd])
        .output()
        .await;

    match add_output {
        Ok(output) => {
            if output.status.success() {
                info!(
                    "Successfully added '{}' to the exclusion list.",
                    cwd
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