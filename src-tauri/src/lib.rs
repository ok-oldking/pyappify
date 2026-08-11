// src/lib.rs
mod app;
mod app_service;
mod config_manager;
mod emitter;
mod execute_python;
mod git;
mod python_env;
mod runas;
mod submodule;
mod utils;

use crate::app::{
    UPDATE_METHOD_OPTION_AUTO, UPDATE_METHOD_OPTION_AUTO_PRE_RELEASE, UPDATE_METHOD_OPTION_MANUAL,
};
use crate::app_service::{
    delete_app, get_app_icon, get_update_notes, load_apps, set_startup_overrides, setup_app,
    start_app, stop_app, update_app_preferences, update_to_version, StartupOverrides,
    AUTO_START_CHECKED,
};
use crate::config_manager::{
    get_config_payload, init_config_manager, save_configuration, update_config_item,
};
use crate::utils::defender::add_defender_exclusion;
use crate::utils::logger::LoggerBuilder;
use crate::utils::window;
use crate::utils::window::{on_window_event, send_notification_cmd};
use std::env;
use tauri::Manager;
use tracing::info;
#[macro_use]
extern crate rust_i18n;
i18n!("locales", fallback = "en");

#[derive(Clone, Debug, Default, PartialEq)]
struct CommandLineOptions {
    command: Option<String>,
    profile_name: Option<String>,
    auto_start: Option<bool>,
    update_method: Option<String>,
}

fn parse_bool_argument(value: &str) -> Option<bool> {
    match value.to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Some(true),
        "false" | "0" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn parse_update_method_argument(value: &str) -> Option<String> {
    match value.to_ascii_uppercase().replace('-', "_").as_str() {
        "MANUAL" | UPDATE_METHOD_OPTION_MANUAL => Some(UPDATE_METHOD_OPTION_MANUAL.to_string()),
        "AUTO" | UPDATE_METHOD_OPTION_AUTO => Some(UPDATE_METHOD_OPTION_AUTO.to_string()),
        "AUTO_PRE_RELEASE" | "PRERELEASE" | UPDATE_METHOD_OPTION_AUTO_PRE_RELEASE => {
            Some(UPDATE_METHOD_OPTION_AUTO_PRE_RELEASE.to_string())
        }
        _ => None,
    }
}

fn parse_command_line(args: &[String]) -> Result<CommandLineOptions, String> {
    let mut options = CommandLineOptions::default();
    let mut i = 1;
    while i < args.len() {
        let flag = args[i].as_str();
        let value = match flag {
            "-c" | "--command" | "-p" | "--profile" | "-a" | "--auto-start" | "-u"
            | "--update-method" => args
                .get(i + 1)
                .ok_or_else(|| format!("Missing value for {}", flag))?,
            _ => {
                i += 1;
                continue;
            }
        };

        match flag {
            "-c" | "--command" => options.command = Some(value.clone()),
            "-p" | "--profile" => options.profile_name = Some(value.clone()),
            "-a" | "--auto-start" => {
                options.auto_start = Some(parse_bool_argument(value).ok_or_else(|| {
                    format!(
                        "Invalid auto-start value '{}'; expected true or false",
                        value
                    )
                })?);
            }
            "-u" | "--update-method" => {
                options.update_method =
                    Some(parse_update_method_argument(value).ok_or_else(|| {
                        format!(
                        "Invalid update method '{}'; expected manual, auto, or auto-pre-release",
                        value
                    )
                    })?);
            }
            _ => unreachable!(),
        }
        i += 2;
    }

    if options.command.is_none() {
        options.command = env::var("PYAPPIFY_COMMAND").ok();
    }
    if options.profile_name.is_none() {
        options.profile_name = env::var("PYAPPIFY_PROFILE_NAME").ok();
    }
    if options.auto_start.is_none() {
        options.auto_start = env::var("PYAPPIFY_AUTO_START")
            .ok()
            .map(|value| {
                parse_bool_argument(&value).ok_or_else(|| {
                    format!(
                        "Invalid PYAPPIFY_AUTO_START value '{}'; expected true or false",
                        value
                    )
                })
            })
            .transpose()?;
    }
    if options.update_method.is_none() {
        options.update_method = env::var("PYAPPIFY_UPDATE_METHOD")
            .ok()
            .map(|value| {
                parse_update_method_argument(&value)
                    .ok_or_else(|| format!("Invalid PYAPPIFY_UPDATE_METHOD value '{}'", value))
            })
            .transpose()?;
    }

    // Existing PyAppify startup shortcuts used `-c start -n <app>`. Do not let that
    // legacy marker override a user's saved Auto Start preference. Explicit modern
    // `-c start` invocations still retain their documented force-start behavior.
    let is_legacy_startup_shortcut = options.command.as_deref() == Some("start")
        && args
            .windows(2)
            .any(|pair| pair[0] == "-n" && !pair[1].is_empty());
    if options.command.as_deref() == Some("start")
        && options.auto_start.is_none()
        && !is_legacy_startup_shortcut
    {
        options.auto_start = Some(true);
    }

    Ok(options)
}

fn has_cli_command(options: &CommandLineOptions) -> bool {
    options.command.as_deref() == Some("setup")
}

async fn handle_command_line(options: CommandLineOptions) {
    {
        let mut auto_start_lock = AUTO_START_CHECKED.lock().await;
        *auto_start_lock = true;
    }
    if let (Some(cmd), Some(p_name)) = (options.command, options.profile_name) {
        if cmd == "setup" {
            let apps = match load_apps().await {
                Ok(apps) => apps,
                Err(e) => {
                    eprintln!("Failed to load apps: {:?}", e);
                    std::process::exit(1);
                }
            };

            if let Some(app) = apps.first() {
                let a_name = &app.name;
                println!(
                    "Command-line mode: Setting up app '{}' with profile '{}'.",
                    a_name, p_name
                );
                match setup_app(a_name, &p_name).await {
                    Ok(_path) => {
                        println!("Setup successful.");
                        std::process::exit(0);
                    }
                    Err(e) => {
                        eprintln!("Setup failed: {:?}", e);
                        std::process::exit(1);
                    }
                }
            } else {
                eprintln!("No apps found to set up.");
                std::process::exit(1);
            }
        }
    }
}

#[tauri::command]
async fn show_main_window(window: tauri::Window) {
    window.show().unwrap();
    window.set_focus().unwrap();
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub async fn run() {
    let command_line_options = match parse_command_line(&env::args().collect::<Vec<_>>()) {
        Ok(options) => options,
        Err(error) => {
            eprintln!("{}", error);
            std::process::exit(2);
        }
    };
    set_startup_overrides(StartupOverrides {
        auto_start: command_line_options.auto_start,
        update_method: command_line_options.update_method.clone(),
    })
    .await;
    #[cfg(debug_assertions)]
    {
        if let Ok(current_dir) = std::env::current_dir() {
            let dev_cwd_path = current_dir.join("dev_cwd");
            if !dev_cwd_path.exists() {
                println!(
                    "'src-tauri/dev_cwd' directory not found. Attempting to create it at {}",
                    dev_cwd_path.display()
                );
                if let Err(e) = std::fs::create_dir_all(&dev_cwd_path) {
                    eprintln!(
                        "Warning: Failed to create directory {}: {}",
                        dev_cwd_path.display(),
                        e
                    );
                } else {
                    println!("Successfully created directory {}", dev_cwd_path.display());
                }
            }
            if dev_cwd_path.exists() && dev_cwd_path.is_dir() {
                if let Err(e) = std::env::set_current_dir(&dev_cwd_path) {
                    eprintln!(
                        "Warning: Failed to set working directory to {}: {}",
                        dev_cwd_path.display(),
                        e
                    );
                } else {
                    println!(
                        "Working directory set to: {}",
                        std::env::current_dir().unwrap().display()
                    );
                }
            } else {
                eprintln!("Warning: 'src-tauri/dev_cwd' does not exist or is not a directory at {}. Working directory not changed.", dev_cwd_path.display());
            }
        } else {
            eprintln!(
                "Warning: Failed to get current working directory. Working directory not changed."
            );
        }
    }

    let log_level = if cfg!(debug_assertions) {
        "debug"
    } else {
        "info"
    };
    let _ = LoggerBuilder::new()
        .log_dir("logs")
        .file_prefix("app")
        .default_level(log_level)
        .init();
    info!("Log initialized");

    #[cfg(not(debug_assertions))]
    {
        if let Some(exe_path) = env::current_exe().ok() {
            if let Some(exe_dir) = exe_path.parent() {
                if let Err(e) = env::set_current_dir(exe_dir) {
                    eprintln!("Failed to set current directory to executable path: {}", e);
                } else {
                    println!(
                        "Current directory set to: {}",
                        env::current_dir().unwrap().display()
                    );
                }
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        #[link(name = "shell32")]
        extern "system" {
            fn IsUserAnAdmin() -> i32;
        }
        let is_admin = unsafe { IsUserAnAdmin() != 0 };

        if is_admin {
            if let Ok(cwd) = std::env::current_dir() {
                let eb_webview_path = cwd.join("EBWebView");
                let mut should_run_icacls = false;

                if !eb_webview_path.exists() {
                    // Try to create it.
                    // If creation works, it is empty -> run icacls.
                    // If creation fails -> run icacls (as requested).
                    let success = std::fs::create_dir(&eb_webview_path);
                    info!("EBWebView does not exist create it {:?}", success);
                    should_run_icacls = true;
                } else {
                    // Exists. Check if empty.
                    if let Ok(mut entries) = std::fs::read_dir(&eb_webview_path) {
                        if entries.next().is_none() {
                            // Exists and empty -> run icacls
                            should_run_icacls = true;
                        }
                    }
                }

                info!("is_admin should_run_icacls:{:?}", should_run_icacls);

                if should_run_icacls {
                    use std::os::windows::process::CommandExt;
                    const CREATE_NO_WINDOW: u32 = 0x08000000;

                    let _ = std::process::Command::new("icacls")
                        .args([".", "/grant", "Users:(OI)(CI)F"])
                        .current_dir(cwd)
                        .creation_flags(CREATE_NO_WINDOW)
                        .output();
                }
            }
        }
    }

    if let Ok(cwd) = std::env::current_dir() {
        std::env::set_var("WEBVIEW2_USER_DATA_FOLDER", cwd);
    }

    if has_cli_command(&command_line_options) {
        info!("running in cli");
        let context = tauri::generate_context!();
        let app = tauri::Builder::default()
            .build(context)
            .expect("error while building tauri application in CLI mode");
        init_config_manager(app.handle());
        handle_command_line(command_line_options).await;
    } else {
        info!("running with tauri ui");
        tauri::Builder::default()
            .plugin(tauri_plugin_single_instance::init(|app, args, cwd| {
                info!("tauri_plugin_single_instance args:{:?} cwd:{}", args, cwd);
                window::show_and_focus_main_window(app.app_handle());
            }))
            .on_window_event(on_window_event)
            .plugin(tauri_plugin_opener::init())
            .plugin(tauri_plugin_notification::init())
            .setup(|app| {
                window::create_system_tray(&app).unwrap();
                let app_handle = app.handle();
                emitter::init_app_handle(app_handle.clone());
                init_config_manager(&app_handle);
                tokio::spawn(app_service::periodically_update_all_apps_running_status(
                    app_handle.clone(),
                ));
                tokio::spawn(app_service::watch_app_config_changes());
                Ok(())
            })
            .invoke_handler(tauri::generate_handler![
                show_main_window,
                start_app,
                stop_app,
                load_apps,
                get_app_icon,
                setup_app,
                delete_app,
                get_update_notes,
                update_to_version,
                update_app_preferences,
                update_config_item,
                save_configuration,
                get_config_payload,
                add_defender_exclusion,
                send_notification_cmd,
            ])
            .run(tauri::generate_context!())
            .expect("error while running tauri application");
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_command_line, CommandLineOptions};
    use crate::app::{UPDATE_METHOD_OPTION_AUTO_PRE_RELEASE, UPDATE_METHOD_OPTION_MANUAL};

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn parses_temporary_startup_overrides() {
        let parsed = parse_command_line(&args(&[
            "pyappify",
            "-c",
            "start",
            "--auto-start",
            "false",
            "--update-method",
            "auto-pre-release",
        ]))
        .unwrap();

        assert_eq!(
            parsed,
            CommandLineOptions {
                command: Some("start".to_string()),
                profile_name: None,
                auto_start: Some(false),
                update_method: Some(UPDATE_METHOD_OPTION_AUTO_PRE_RELEASE.to_string()),
            }
        );
    }

    #[test]
    fn start_command_enables_auto_start_unless_explicitly_overridden() {
        let parsed =
            parse_command_line(&args(&["pyappify", "--command", "start", "-u", "manual"])).unwrap();

        assert_eq!(parsed.auto_start, Some(true));
        assert_eq!(
            parsed.update_method.as_deref(),
            Some(UPDATE_METHOD_OPTION_MANUAL)
        );
    }

    #[test]
    fn legacy_startup_shortcut_respects_saved_auto_start() {
        let parsed =
            parse_command_line(&args(&["pyappify", "-c", "start", "-n", "example"])).unwrap();

        assert_eq!(parsed.auto_start, None);
    }

    #[test]
    fn rejects_invalid_startup_override_values() {
        let error = parse_command_line(&args(&["pyappify", "-a", "sometimes"])).unwrap_err();
        assert!(error.contains("Invalid auto-start value"));
    }
}
