// src/command.rs
use crate::utils::error::Error;
use crate::{emit_error, emit_info, ensure_some, err};
use std::process::{ExitStatus, Stdio};
use tokio::io::AsyncBufReadExt;
use tokio::process::Command;
use tracing::{debug, error, info};

pub async fn run_command_and_stream_output(
    mut command: Command,
    app_name: &str,
    command_description: &str,
) -> Result<ExitStatus, Error> {
    emit_info!(app_name, "executing command: {}", command_description);
    
    command.creation_flags(0x08000000);
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());

    let mut child = command.spawn().map_err(|e| {
        let msg = format!("Failed to spawn command ({}): {}", command_description, e);
        error!(error = %e, command = %command_description, %msg);
        err!(msg)
    })?;

    let child_pid = child
        .id()
        .map(|id| id.to_string())
        .unwrap_or_else(|| "N/A".to_string());
    info!(pid = %child_pid, cmd_desc = %command_description, "Command spawned");

    let stdout = ensure_some!(
        child.stdout.take(),
        "Could not capture stdout from command ({})",
        command_description
    )
    .map_err(|e| {
        emit_error!(app_name, "{}", e.to_string());
        err!(e.to_string())
    })?;

    let stderr = ensure_some!(
        child.stderr.take(),
        "Could not capture stderr from command ({})",
        command_description
    )
    .map_err(|e| {
        emit_error!(app_name, "{}", e.to_string());
        err!(e.to_string())
    })?;

    let mut stdout_buf_reader = tokio::io::BufReader::new(stdout);
    let mut stderr_buf_reader = tokio::io::BufReader::new(stderr);

    let app_name_for_stdout = app_name.to_string();
    let stdout_task = tokio::spawn(async move {
        let mut buffer = String::new();
        loop {
            match stdout_buf_reader.read_line(&mut buffer).await {
                Ok(0) => break,
                Ok(_) => {
                    emit_info!(app_name_for_stdout, "{}", buffer.as_str());
                    buffer.clear();
                }
                Err(e) => {
                    emit_error!(app_name_for_stdout, "Error reading stdout line: {}", e);
                    break;
                }
            }
        }
    });

    let app_name_for_stderr = app_name.to_string();
    let stderr_task = tokio::spawn(async move {
        let mut buffer = String::new();
        loop {
            match stderr_buf_reader.read_line(&mut buffer).await {
                Ok(0) => break,
                Ok(_) => {
                    let err_string = buffer.to_string();
                    buffer.clear();
                    if !err_string.trim().is_empty() && !err_string.contains("A new release of pip is available") && !err_string.contains("[notice] To update, run") {
                        emit_error!(app_name_for_stderr, "{}", err_string);
                    } else {
                        debug!("not emitting black listed stderr {}", err_string);
                    }
                }
                Err(e) => {
                    emit_error!(app_name_for_stderr, "Error reading stderr line: {}", e);
                    break;
                }
            }
        }
    });

    let status = child.wait().await?;

    if let Err(e) = tokio::try_join!(stdout_task, stderr_task) {
        error!(error = %e, cmd_desc = %command_description, "Log reading task encountered an error. This does not necessarily mean the command itself failed.");
    }

    Ok(status)
}

pub fn command_to_string(command: &std::process::Command) -> String {
    let program_path = command.get_program();
    let arguments: Vec<&str> = command.get_args().filter_map(|arg| arg.to_str()).collect();
    let mut command_string = String::new();
    if let Some(path) = program_path.to_str() {
        command_string.push_str(path);
    } else {
        command_string.push_str("<non-UTF8 program path>");
    }
    for arg in arguments {
        command_string.push(' ');
        if arg.contains(' ') || arg.contains('"') {
            command_string.push('"');
            command_string.push_str(arg.replace('"', "\"\"").as_str());
            command_string.push('"');
        } else {
            command_string.push_str(arg);
        }
    }
    command_string
}


#[cfg(windows)]
pub async fn is_currently_admin() -> bool {
    Command::new("net")
        .arg("session")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(not(windows))]
pub async fn is_currently_admin() -> bool {
    if let Ok(output) = Command::new("id").arg("-u").output().await {
        if output.status.success() {
            return String::from_utf8_lossy(&output.stdout).trim() == "0";
        }
    }
    false
}