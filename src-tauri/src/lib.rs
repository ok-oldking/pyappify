mod app_service;
mod config_manager;
mod emitter;
mod execute_python;
mod git;
mod install;
mod python_env;
mod submodule;
mod utils;

use crate::app_service::{emit_apps, load_app_details, load_apps, setup_app, start_app, stop_app};
use crate::config_manager::init_config_manager;
use crate::emitter::emit_custom_event;
use crate::utils::logger::LoggerBuilder;
use std::env;
use tauri::{Emitter, Manager};
use tracing::{debug, info};

#[tauri::command]
async fn clone_app(url: String) {
    debug!("calling clone_app {}", url);

    let app = match install::clone_app(url.clone()).await {
        Ok(a) => a,
        Err(e) => {
            let error_str = e.to_string();
            let app_name_from_url = url
                .split('/')
                .last()
                .filter(|s| !s.is_empty())
                .unwrap_or("unknown_app")
                .trim_end_matches(".git")
                .to_string();
            emit_error!(app_name_from_url.as_str(), error_str);
            emit_error_finish!(app_name_from_url.as_str());
            return;
        }
    };

    if app.config.profiles.len() > 1 {
        emit_custom_event("choose_profile", app);
        return;
    }

    if let Err(e_setup) = app_service::setup_app(app.name.as_str(), "default").await {
        if let Err(e_delete) = app_service::delete_app(app.name.as_str()).await {
            let error_str = e_delete.to_string();
            emit_error!(app.name.as_str(), error_str);
            emit_error_finish!(app.name.as_str());
            return;
        }

        let error_str = e_setup.to_string();
        emit_error!(app.name.as_str(), error_str);
        emit_error_finish!(app.name.as_str());
        return;
    }

    load_app_details(app.name.clone()).await.unwrap();

    emit_apps().await;
    emit_success_finish!(app.name.as_str());
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _ = LoggerBuilder::new()
        .log_dir("logs")
        .file_prefix("app")
        .default_level("debug")
        .init();
    info!("Log initialized");
    #[cfg(debug_assertions)]
    {
        // Only in debug (dev) mode, attempt to change the working directory
        if let Ok(current_dir) = std::env::current_dir() {
            let dev_cwd_path = current_dir.join("dev_cwd");
            // Check if the directory exists, and create it if it doesn't
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
                    // If creation fails, we can't set the working directory to it.
                    // The code will proceed without changing the directory, which might lead to issues
                    // if subsequent code relies on this directory existing.
                } else {
                    println!("Successfully created directory {}", dev_cwd_path.display());
                }
            }
            // Now attempt to set the working directory
            // We only try to set it if it exists (either existed or was just created)
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
                // This branch is hit if the directory creation failed or if it exists but isn't a directory
                eprintln!("Warning: 'src-tauri/dev_cwd' does not exist or is not a directory at {}. Working directory not changed.", dev_cwd_path.display());
            }
        } else {
            eprintln!(
                "Warning: Failed to get current working directory. Working directory not changed."
            );
        }
    }
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, cwd| {
            info!("tauri_plugin_single_instance args:{:?} cwd:{}", args, cwd);
            let _ = app
                .get_webview_window("main")
                .expect("no main window")
                .set_focus();
        }))
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let app_handle = app.handle();
            emitter::init_app_handle(app_handle.clone());
            init_config_manager(&app_handle);
            tokio::spawn(app_service::periodically_update_all_apps_running_status());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            clone_app,
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
