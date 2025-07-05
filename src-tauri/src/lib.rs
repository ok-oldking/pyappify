// src/lib.rs
mod app_service;
mod config_manager;
mod emitter;
mod execute_python;
mod git;
mod python_env;
mod submodule;
mod utils;
mod app;

use crate::app_service::{load_apps, setup_app, start_app, stop_app};
use crate::config_manager::init_config_manager;
use crate::utils::logger::LoggerBuilder;
use crate::utils::window;
use std::env;
use tauri::{Manager};
use tracing::info;
use crate::utils::window::on_window_event;

fn has_cli_command() -> bool {
    let args: Vec<String> = env::args().collect();
    let mut has_command_flag = false;
    let mut i = 1;
    while i < args.len() {
        if args[i].as_str() == "-c" {
            has_command_flag = true;
            break;
        }
        i += 1;
    }
    has_command_flag || env::var("PYAPPIFY_COMMAND").is_ok()
}

async fn handle_command_line() {
    let args: Vec<String> = env::args().collect();
    let mut command = None;
    let mut profile_name = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-c" => { command = args.get(i + 1).cloned(); i += 2; }
            "-p" => { profile_name = args.get(i + 1).cloned(); i += 2; }
            _ => i += 1,
        }
    }

    if command.is_none() { command = env::var("PYAPPIFY_COMMAND").ok(); }
    if profile_name.is_none() { profile_name = env::var("PYAPPIFY_PROFILE_NAME").ok(); }

    if let (Some(cmd), Some(p_name)) = (command, profile_name) {
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
                println!("Command-line mode: Setting up app '{}' with profile '{}'.", a_name, p_name);
                match setup_app(a_name, &p_name).await {
                    Ok(path) => {
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

    #[cfg(not(debug_assertions))]
    {
        if let Some(exe_path) = env::current_exe().ok() {
            if let Some(exe_dir) = exe_path.parent() {
                if let Err(e) = env::set_current_dir(exe_dir) {
                    eprintln!("Failed to set current directory to executable path: {}", e);
                } else {
                    println!("Current directory set to: {}", env::current_dir().unwrap().display());
                }
            }
        }
    }

    if has_cli_command() {
        let context = tauri::generate_context!();
        let app = tauri::Builder::default()
            .build(context)
            .expect("error while building tauri application in CLI mode");
        init_config_manager(app.handle());
        handle_command_line().await;
    } else {
        let log_level = if cfg!(debug_assertions) { "debug" } else { "info" };
        let _ = LoggerBuilder::new()
            .log_dir("logs")
            .file_prefix("app")
            .default_level(log_level)
            .init();
        info!("Log initialized");
        tauri::Builder::default()
            .plugin(tauri_plugin_single_instance::init(|app, args, cwd| {
                info!("tauri_plugin_single_instance args:{:?} cwd:{}", args, cwd);
                window::show_and_focus_main_window(app.app_handle());
            }))
            .on_window_event(on_window_event)
            .plugin(tauri_plugin_opener::init())
            .setup(|app| {
                window::create_system_tray(&app).unwrap();
                let app_handle = app.handle();
                emitter::init_app_handle(app_handle.clone());
                init_config_manager(&app_handle);
                tokio::spawn(app_service::periodically_update_all_apps_running_status(app_handle.clone()));
                Ok(())
            })
            .invoke_handler(tauri::generate_handler![
                show_main_window,
                start_app,
                stop_app,
                load_apps,
                setup_app,
                app_service::delete_app,
                app_service::get_update_notes,
                app_service::update_to_version,
                config_manager::update_config_item,
                config_manager::save_configuration,
                config_manager::get_config_payload,
            ])
            .run(tauri::generate_context!())
            .expect("error while running tauri application");
    }
}