//src/execute_python.rs
use crate::runas;
use crate::utils::command::{command_to_string, is_admin, run_command_and_stream_output};
use crate::utils::error::Error;
use crate::utils::path::{get_python_dir, get_python_exe, path_to_abs};
use crate::{emit_error, emit_error_finish, emit_info, emit_success_finish, err};
use anyhow::anyhow;
use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::process::Command;
use tracing::{error, info};

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
    python_path: String,
    script_path: String,
    working_dir: &Path,
    envs: &[(String, String)],
    envs_to_remove: &[String],
) -> Result<(), Error> {
    let (executable, mut args) = if script_path.ends_with(".py") {
        (python_path, vec![script_path])
    } else {
        (script_path, vec![])
    };
    args.extend(std::env::args().skip(1));
    info!(app_name = app_name, executable = %executable, args = %args.join(" "), desired_cwd = %working_dir.display(), "Attempting to run script with admin privileges using runas.");

    let mut env_vars_to_set = HashMap::new();
    for (key, value) in envs {
        env_vars_to_set.insert(key.clone(), value.clone());
    }

    let _guard = ProcessContextGuard::new(working_dir, &env_vars_to_set, &envs_to_remove)?;
    emit_info!(
        app_name,
        "run as admin command: {} {}, cwd: {}",
        executable,
        args.join(" "),
        working_dir.display()
    );
    let mut runas_cmd_builder = runas::Command::new(&executable);
    for arg in &args {
        runas_cmd_builder.arg(arg);
    }
    runas_cmd_builder.show(true);

    let app_name_clone = app_name.to_string();

    let status_result = tokio::task::spawn_blocking(move || runas_cmd_builder.status()).await;

    match status_result {
        Ok(Ok(status)) => {
            if status.success() {
                emit_info!(
                    app_name_clone,
                    "Admin script (via runas) finished successfully with code {}",
                    status.code().unwrap_or(-1)
                );
                Ok(())
            } else {
                let exit_code_display = status
                    .code()
                    .map_or_else(|| "N/A".to_string(), |c| c.to_string());
                let err_msg = format!(
                    "Admin script (via runas) exited with status code: {}",
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
    python_path: String,
    script_path: String,
    working_dir: &Path,
    envs: &[(String, String)],
    envs_to_remove: &[String],
) -> Result<(), Error> {
    let (executable, mut args) = if script_path.ends_with(".py") {
        (python_path, vec![script_path])
    } else {
        (script_path, vec![])
    };
    args.extend(std::env::args().skip(1));

    let mut cmd = Command::new(executable);
    cmd.args(&args)
        .current_dir(working_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(false);

    for key in envs_to_remove {
        cmd.env_remove(key);
    }
    for (key, value) in envs {
        cmd.env(key, value);
    }
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

fn find_script_or_executable(
    script: &str,
    working_dir: &Path,
    script_dir: &Path,
) -> Result<PathBuf, Error> {
    let script_in_cwd = working_dir.join(script);
    if script_in_cwd.is_file() {
        return Ok(script_in_cwd);
    }

    let script_in_venv_scripts = script_dir.join(script);
    if script_in_venv_scripts.is_file() {
        return Ok(script_in_venv_scripts);
    }

    let extensions = if cfg!(windows) {
        vec![".exe", ".bat", ".cmd", ".ps1", ""]
    } else {
        vec!["", ".sh"]
    };

    if let Some(exec_path) = extensions
        .iter()
        .map(|ext| script_dir.join(format!("{}{}", script, ext)))
        .find(|path| path.is_file())
    {
        return Ok(exec_path);
    }

    let err_msg = format!(
        "Script '{}' not found in '{}' or as an executable in '{}'",
        script,
        working_dir.display(),
        script_dir.display()
    );
    Err(err!(err_msg))
}

pub async fn run_python_script(
    app_name: &str,
    script: &str,
    working_dir: &Path,
    as_admin: bool,
    use_pythonw: bool,
    mut envs: Vec<(String, String)>,
    envs_to_remove: Vec<String>,
) -> Result<(), Error> {
    let envs_keys: HashSet<String> = envs.iter().map(|(k, _)| k.clone()).collect();
    let envs_to_remove_keys: HashSet<String> = envs_to_remove.iter().cloned().collect();

    for (key, value) in std::env::vars() {
        if !envs_keys.contains(&key) && !envs_to_remove_keys.contains(&key) {
            envs.push((key, value));
        }
    }

    let python_dir = get_python_dir(app_name);
    let python_executable = get_python_exe(app_name, use_pythonw);

    if !python_executable.exists() {
        let err_msg = format!(
            "Python executable not found: {}",
            python_executable.display()
        );
        emit_error!(app_name, "{}", err_msg);
        return Err(err!(err_msg));
    }
    if !working_dir.is_dir() {
        let err_msg = format!(
            "Working directory not found or not a directory: {}",
            working_dir.display()
        );
        emit_error!(app_name, "{}", err_msg);
        return Err(err!(err_msg));
    }

    let script_path =
        match find_script_or_executable(script, working_dir, &python_dir.join("Scripts")) {
            Ok(result) => result,
            Err(e) => {
                emit_error!(app_name, "{}", e);
                return Err(e);
            }
        };

    let python_path_str = path_to_abs(&python_executable);
    let script_path_str = path_to_abs(&script_path);

    emit_info!(
        app_name,
        "Python Path: {}, Script Path: {}",
        python_path_str,
        script_path_str,
    );
    for (key, value) in &envs {
        emit_info!(app_name, "Env: {}={}", key, value);
    }

    let app_name_owned = app_name.to_string();
    let python_path_owned = python_path_str.clone();
    let script_path_owned = script_path_str.clone();
    let working_dir_owned = working_dir.to_path_buf();
    let envs_owned = envs;
    let envs_to_remove_owned = envs_to_remove;

    tokio::spawn(async move {
        let needs_elevation = as_admin && !is_admin();

        if needs_elevation {
            emit_info!(app_name_owned, "Elevation required, using admin execution.");
        } else if as_admin {
            emit_info!(
                app_name_owned,
                "Admin rights requested, but process is already elevated. Using standard execution."
            );
        }

        let result = if needs_elevation {
            run_python_script_as_admin_internal(
                app_name_owned.as_str(),
                python_path_owned,
                script_path_owned,
                &working_dir_owned,
                &envs_owned,
                &envs_to_remove_owned,
            )
            .await
        } else {
            run_python_script_normal_internal(
                app_name_owned.as_str(),
                python_path_owned,
                script_path_owned,
                &working_dir_owned,
                &envs_owned,
                &envs_to_remove_owned,
            )
            .await
        };
        if let Err(e) = result {
            emit_error!(app_name_owned, "Script run Error {}", e);
            emit_error_finish!(app_name_owned);
        } else {
            emit_info!(app_name_owned, "Script run Success");
            emit_success_finish!(app_name_owned);
        }
    });

    emit_info!(app_name, "Script {} run call dispatched.", app_name);

    Ok(())
}
