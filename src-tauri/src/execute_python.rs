//src/execute_python.rs
use crate::utils::command::{command_to_string, run_command_and_stream_output};
use crate::utils::error::Error;
use crate::utils::path::{get_python_dir, path_to_abs};
use crate::{emit_error, emit_error_finish, emit_info, emit_success_finish, err};
use anyhow::anyhow;
use runas;
use std::collections::HashMap;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use regex::Regex;
use tokio::process::Command;
use tracing::{debug, error, info};

struct ProcessContextGuard {
    original_dir: PathBuf,
    original_env_vars: HashMap<String, Option<OsString>>,
}

impl ProcessContextGuard {
    fn new(
        new_dir: &Path,
        vars_to_set: &HashMap<String, String>,
        vars_to_remove: &[String],
    ) -> Result<Self, std::io::Error> {
        let original_dir = std::env::current_dir()?;

        let mut original_env_vars = HashMap::new();

        let mut all_keys_to_manage = Vec::new();
        for key in vars_to_remove {
            all_keys_to_manage.push(key.clone());
        }
        for key in vars_to_set.keys() {
            if !vars_to_remove.contains(key) {
                all_keys_to_manage.push(key.clone());
            }
        }

        for key in all_keys_to_manage {
            original_env_vars.insert(key.clone(), std::env::var_os(&key));
        }

        std::env::set_current_dir(new_dir)?;

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
        info!("ProcessContextGuard: Restoring environment variables and CWD...");
        for (key, original_os_value) in &self.original_env_vars {
            if let Some(val) = original_os_value {
                std::env::set_var(key, val);
                info!(env_var = %key, value = %val.to_string_lossy(), "Restored environment variable.");
            } else {
                std::env::remove_var(key);
                info!(env_var = %key, "Ensured environment variable is removed (was not originally set).");
            }
        }

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
    envs: &[(String, String)],
) -> Result<(), Error> {
    info!(app_name = app_name, script = script_path, desired_cwd = %working_dir.display(), "Attempting to run Python script with admin privileges using runas.");

    let mut env_vars_to_set = HashMap::new();
    env_vars_to_set.insert("PYTHONIOENCODING".to_string(), "utf-8".to_string());
    env_vars_to_set.insert("PYTHONUNBUFFERED".to_string(), "1".to_string());
    for (key, value) in envs {
        env_vars_to_set.insert(key.clone(), value.clone());
    }

    let env_vars_to_remove = vec!["PYTHONHOME".to_string()];

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
    envs: &[(String, String)],
) -> Result<(), Error> {
    let mut cmd = Command::new(python_executable);
    cmd.arg(script_path)
        .current_dir(working_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(false);

    cmd.env_remove("PYTHONHOME");
    for (key, value) in envs {
        cmd.env(key, value);
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

fn ensure_venv_cfg(env_dir: &Path) -> Result<(), Error> {
    let abs_env_dir = fs::canonicalize(env_dir)?;
    let file_path = abs_env_dir.join("pyvenv.cfg");

    let original_content = fs::read_to_string(&file_path)?;

    let version_regex = Regex::new(r"(?m)^\s*version\s*=\s*(\d+)\.(\d+)")?;
    let captures = version_regex
        .captures(&original_content)
        .ok_or("Could not find version info in pyvenv.cfg")?;

    let version_dir_name = format!("Python{}{}", &captures[1], &captures[2]);
    let python_dir = get_python_dir().join(version_dir_name);
    let abs_python_dir = fs::canonicalize(python_dir)?;

    let home_regex = Regex::new(r"(\s*home\s*=\s*).*")?;
    let executable_regex = Regex::new(r"(\s*executable\s*=\s*).*")?;
    let command_regex = Regex::new(r"(\s*command\s*=\s*).*")?;

    let python_exe_path = abs_python_dir.join("python.exe");

    let content = home_regex.replace(&original_content, format!("$1{}", abs_python_dir.display()));
    let content = executable_regex.replace(&content, format!("$1{}", python_exe_path.display()));
    let content = command_regex.replace(&content, format!("$1{} -m venv {}", python_exe_path.display(), abs_env_dir.display()));

    if content != original_content {
        info!("modified venv cfg {}", python_exe_path.display());
        fs::write(&file_path, content.as_ref())?;
    } else { 
        debug!("no need to modify venv cfg {}", python_exe_path.display());
    }

    Ok(())
}

pub async fn run_python_script(
    app_name: &str,
    venv_path: &Path,
    script_path: &Path,
    working_dir: &Path,
    as_admin: bool,
    envs: Vec<(String, String)>,
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
    
    ensure_venv_cfg(venv_path)?;
    
    let python_exec_str = path_to_abs(&python_executable);
    let script_path_str = path_to_abs(script_path);

    emit_info!(
        app_name,
        "Python executable: {} Script path: {}",
        python_exec_str,
        script_path_str
    );
    for (key, value) in &envs {
        emit_info!(app_name, "Env: {}={}", key, value);
    }

    let app_name_owned = app_name.to_string();
    let python_exec_str_owned = python_exec_str.clone();
    let script_path_str_owned = script_path_str.clone();
    let working_dir_owned = working_dir.to_path_buf();
    let envs_owned = envs;

    tokio::spawn(async move {
        let result = if as_admin {
            run_python_script_as_admin_internal(
                app_name_owned.as_str(),
                python_exec_str_owned,
                script_path_str_owned,
                &working_dir_owned,
                &envs_owned,
            )
                .await
        } else {
            run_python_script_normal_internal(
                app_name_owned.as_str(),
                python_exec_str_owned,
                script_path_str_owned,
                &working_dir_owned,
                &envs_owned,
            )
                .await
        };
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