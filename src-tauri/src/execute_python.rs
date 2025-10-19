//src/execute_python.rs
use crate::utils::command::{command_to_string, run_command_and_stream_output};
use crate::utils::error::Error;
use crate::utils::path::{get_python_dir, get_python_exe, path_to_abs};
use crate::{emit_error, emit_error_finish, emit_info, emit_success_finish, err};
use crate::utils::process::RemovePythonEnvsExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::process::Command;
use tracing::info;

async fn run_python_script_normal_internal(
    app_name: &str,
    python_path: String,
    script_path: String,
    working_dir: &Path,
    envs: &[(String, String)],
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
    cmd.clear_python_envs();

    for (key, value) in envs {
        cmd.env(key, value);
        emit_info!(app_name, "set Env: {}={}", key, value);
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
    use_pythonw: bool,
    envs: Vec<(String, String)>,
) -> Result<(), Error> {
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
        emit_info!(app_name, "run_python_script Env: {}={}", key, value);
    }

    let app_name_owned = app_name.to_string();
    let python_path_owned = python_path_str.clone();
    let script_path_owned = script_path_str.clone();
    let working_dir_owned = working_dir.to_path_buf();
    let envs_owned = envs;

    tokio::spawn(async move {
        let result = run_python_script_normal_internal(
            app_name_owned.as_str(),
            python_path_owned,
            script_path_owned,
            &working_dir_owned,
            &envs_owned,
        )
            .await;
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