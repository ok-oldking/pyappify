//src/execute_python.rs
use crate::utils::command::{command_to_string, run_command_and_stream_output};
use crate::utils::error::Error;
use crate::utils::path::path_to_abs;
use crate::{emit_error, emit_error_finish, emit_info, emit_success_finish, err};
use anyhow::anyhow;
use runas;
use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::io::BufReader;
use tokio::process::Command;
use tracing::{error, info};

// RAII Guard for managing CWD and environment variables for the current process
struct ProcessContextGuard {
    original_dir: PathBuf,
    original_env_vars: HashMap<String, Option<OsString>>, // Stores original values (or None if not set)
}

impl ProcessContextGuard {
    fn new(
        new_dir: &Path,
        vars_to_set: &HashMap<String, String>, // Env vars to set with their new values
        vars_to_remove: &[String],             // Env vars to remove
    ) -> Result<Self, std::io::Error> {
        // 1. Capture all original states BEFORE making any changes to the process
        let original_dir = std::env::current_dir()?;

        let mut original_env_vars = HashMap::new();

        // Consolidate all keys we will touch to capture their original state once
        let mut all_keys_to_manage = Vec::new();
        for key in vars_to_remove {
            all_keys_to_manage.push(key.clone());
        }
        for key in vars_to_set.keys() {
            if !vars_to_remove.contains(key) {
                // Avoid duplicate if a key is in both lists (though unlikely for set/remove)
                all_keys_to_manage.push(key.clone());
            }
        }

        for key in all_keys_to_manage {
            original_env_vars.insert(key.clone(), std::env::var_os(&key));
        }

        // 2. Attempt to set the new CWD. If this fails, we bail out.
        //    The environment variables of the current process have not been modified yet.
        std::env::set_current_dir(new_dir)?;
        // If set_current_dir succeeded, original_dir and original_env_vars are populated correctly.
        // The ProcessContextGuard object will be created, and its Drop will run.

        // 3. CWD change was successful. Now change environment variables for the current process.
        for key_to_remove in vars_to_remove {
            std::env::remove_var(key_to_remove);
            info!(env_var = %key_to_remove, "Temporarily removed environment variable for current process.");
        }
        for (key_to_set, value_to_set) in vars_to_set {
            std::env::set_var(key_to_set, value_to_set);
            info!(env_var = %key_to_set, value = %value_to_set, "Temporarily set environment variable for current process.");
        }

        info!(new_cwd = %new_dir.display(), "Successfully changed CWD and updated environment variables for current process. Original state will be restored on drop.");

        Ok(Self {
            original_dir,
            original_env_vars,
        })
    }
}

impl Drop for ProcessContextGuard {
    fn drop(&mut self) {
        // Restore environment variables first
        info!("ProcessContextGuard: Restoring environment variables and CWD...");
        for (key, original_os_value) in &self.original_env_vars {
            if let Some(val) = original_os_value {
                std::env::set_var(key, val); // val is OsString, set_var takes &OsStr
                info!(env_var = %key, value = %val.to_string_lossy(), "Restored environment variable.");
            } else {
                std::env::remove_var(key);
                info!(env_var = %key, "Ensured environment variable is removed (was not originally set).");
            }
        }

        // Then restore original CWD
        if let Err(e) = std::env::set_current_dir(&self.original_dir) {
            error!(original_path = %self.original_dir.display(), "Failed to restore original working directory: {}", e);
        } else {
            info!(original_path = %self.original_dir.display(), "Successfully restored original working directory.");
        }
    }
}

async fn run_python_script_as_admin_internal(
    app_name: &str,
    python_executable: String,
    script_path: String,
    working_dir: &Path,
    python_path: String,
) -> Result<(), Error> {
    info!(app_name = app_name, script = script_path, desired_cwd = %working_dir.display(), "Attempting to run Python script with admin privileges using runas.");

    let mut env_vars_to_set = HashMap::new();
    env_vars_to_set.insert("PYTHONIOENCODING".to_string(), "utf-8".to_string());
    env_vars_to_set.insert("PYTHONUNBUFFERED".to_string(), "1".to_string());

    let mut env_vars_to_remove = vec!["PYTHONHOME".to_string()];

    if !python_path.is_empty() {
        info!(app_name = app_name, pythonpath = %python_path, "Setting PYTHONPATH for admin script.");
        env_vars_to_set.insert("PYTHONPATH".to_string(), python_path);
    } else {
        info!(
            app_name = app_name,
            "PYTHONPATH is empty, will ensure it's removed for admin script if originally set."
        );
        env_vars_to_remove.push("PYTHONPATH".to_string());
    }

    // _guard will manage CWD and environment variables for the current process scope using std::env.
    // These changes will be inherited by the process spawned by `runas`.
    let _guard = ProcessContextGuard::new(working_dir, &env_vars_to_set, &env_vars_to_remove)?;
    emit_info!(
        app_name,
        "run as admin command: {} {}, cwd: {}",
        python_executable,
        script_path,
        working_dir.display()
    );
    let mut runas_cmd_builder = runas::Command::new(python_executable);
    runas_cmd_builder.arg(script_path);

    runas_cmd_builder.show(false);

    let app_name_clone = app_name.to_string();

    let status_result = tokio::task::spawn_blocking(move || runas_cmd_builder.status()).await;

    // _guard goes out of scope here (or if an error occurred earlier and the function returned),
    // its Drop implementation will restore the original CWD and environment variables for the Rust process.

    match status_result {
        Ok(Ok(status)) => {
            if status.success() {
                emit_info!(
                    app_name_clone,
                    "Admin Python script (via runas) finished successfully with code {}",
                    status.code().unwrap_or(-1)
                );
                Ok(())
            } else {
                let exit_code_display = status
                    .code()
                    .map_or_else(|| "N/A".to_string(), |c| c.to_string());
                let err_msg = format!(
                    "Admin Python script (via runas) exited with status code: {}",
                    exit_code_display
                );
                error!(exit_code = %exit_code_display, %err_msg);
                emit_info!(app_name_clone, "{}", err_msg);
                Err(Error::from(anyhow!(err_msg)))
            }
        }
        Ok(Err(e)) => {
            let msg = format!(
                "Failed to execute script with runas (runas internal error): {}",
                e
            );
            error!(error = %e, %msg);
            Err(Error::from(anyhow!(msg)))
        }
        Err(e) => {
            let msg = format!("Failed to run blocking task for runas: {}", e);
            error!(error = %e, %msg);
            Err(Error::from(anyhow!(msg)))
        }
    }
}

async fn run_python_script_normal_internal(
    app_name: &str,
    python_executable: String,
    script_path: String,
    working_dir: &Path,
    python_path: String,
) -> Result<(), Error> {
    let mut cmd = Command::new(python_executable);
    cmd.arg(script_path)
        .current_dir(working_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(false);

    cmd.env_remove("PYTHONHOME");
    if !python_path.is_empty() {
        info!(app_name = app_name, pythonpath = %python_path, "Setting PYTHONPATH for normal script.");
        cmd.env("PYTHONPATH", python_path);
    } else {
        info!(
            app_name = app_name,
            "PYTHONPATH is empty, ensuring it's removed for normal script."
        );
        cmd.env_remove("PYTHONPATH");
    }
    cmd.env("PYTHONIOENCODING", "utf-8");
    cmd.env("PYTHONUNBUFFERED", "1");

    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
        info!("CREATE_NO_WINDOW flag set for Windows (non-admin)");
    }

    let command_description_str = command_to_string(cmd.as_std());

    run_command_and_stream_output(cmd, app_name, &command_description_str).await?;

    Ok(())
}

pub async fn run_python_script(
    app_name: &str,
    venv_path: &Path,
    script_path: &Path,
    working_dir: &Path,
    as_admin: bool,
    python_path: String,
) -> Result<(), Error> {
    let python_executable = if cfg!(windows) {
        venv_path.join("Scripts").join("python.exe")
    } else {
        venv_path.join("bin").join("python")
    };

    if !python_executable.exists() {
        let err_msg = format!(
            "Python executable not found: {}",
            python_executable.display()
        );
        emit_error!(
            app_name,
            "Python executable not found: {}",
            python_executable.display()
        );
        err!(err_msg);
    }
    if !script_path.exists() {
        let err_msg = format!("Script not found: {}", script_path.display());
        emit_error!(app_name, "Script not found: {}", script_path.display());
        err!(err_msg);
    }
    if !working_dir.is_dir() {
        let err_msg = format!(
            "Working directory not found or not a directory: {}",
            working_dir.display()
        );
        emit_error!(
            app_name,
            "Working directory not found or not a directory: {}",
            working_dir.display()
        );
        err!(err_msg);
    }

    let python_exec_str = path_to_abs(&python_executable);
    let script_path_str = path_to_abs(script_path);

    emit_info!(
        app_name,
        "Python executable: {} Script path: {}",
        python_exec_str,
        script_path_str
    );
    if !python_path.is_empty() {
        emit_info!(app_name, "PYTHONPATH will be set to: {}", python_path);
    }

    let app_name_owned = app_name.to_string();
    let python_exec_str_owned = python_exec_str.clone();
    let script_path_str_owned = script_path_str.clone();
    let working_dir_owned = working_dir.to_path_buf();
    let python_path_owned = python_path.clone();

    tokio::spawn(async move {
        let result = if as_admin {
            run_python_script_as_admin_internal(
                app_name_owned.as_str(),
                python_exec_str_owned,
                script_path_str_owned,
                &working_dir_owned,
                python_path_owned,
            )
            .await
        } else {
            run_python_script_normal_internal(
                app_name_owned.as_str(),
                python_exec_str_owned,
                script_path_str_owned,
                &working_dir_owned,
                python_path_owned,
            )
            .await
        };
        // Handle the result after awaiting the chosen future
        if let Err(e) = result {
            emit_error!(app_name_owned, "Python run Error {}", e);
            emit_error_finish!(app_name_owned);
        } else {
            emit_info!(app_name_owned, "Python run Success");
            emit_success_finish!(app_name_owned);
        }
    });

    emit_info!(app_name, "Python {} run call done.", app_name);

    Ok(())
}
