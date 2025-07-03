// src/utils/window.rs
use tauri::{App, AppHandle, Manager, WebviewWindow, Window, WindowEvent, Wry};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tracing::info;
use std::env;
use std::fs;
use crate::utils::error::Error;
use crate::utils::path::get_start_dir;

pub fn on_window_event(window: &Window, _event: &WindowEvent) {
    if let WindowEvent::Resized(size) = _event {
        if size.width == 0 && size.height == 0 && window.label() == "main" {
            info!("on_window_event {:?}, hide", _event);
            window.hide().unwrap();
        }
    }
}


pub fn create_system_tray(app: &App<Wry>) -> anyhow::Result<()> {
    let quit_i = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&quit_i])?;

    let _tray = TrayIconBuilder::new()
        .icon(app.default_window_icon().unwrap().clone())
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "quit" => {
                println!("quit menu item was clicked");
                app.exit(0);
            }
            _ => {
                println!("menu item {:?} not handled", event.id);
            }
        })
        .on_tray_icon_event(|tray, event| match event {
            TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } => {
                info!("left click pressed and released");
                let app = tray.app_handle();
                if let Some(window) = app.get_webview_window("main") {
                    show_window(window);
                }
            }
            _ => {
                info!("unhandled event {event:?}");
            }
        })
        .build(app)?;

    Ok(())
}

pub fn show_and_focus_main_window(app: &AppHandle<Wry>) {
    if let Some(window) = app.get_webview_window("main") {
        show_window(window);
    }
}

fn show_window(window: WebviewWindow) {
    window.unminimize().unwrap();
    window.show().unwrap();
    window.set_focus().unwrap();
}

#[tauri::command]
pub async fn create_startup_shortcut(app_handle: AppHandle, name: String) -> Result<(), Error> {
    let shortcut_dir = get_start_dir(app_handle);

    fs::create_dir_all(&shortcut_dir)?;

    let shortcut_path = shortcut_dir.join(format!("{}.lnk", name));
    let exe_path = env::current_exe()?;
    let args = format!("-c start -n {}", name);
    
    let link = shortcuts_rs::ShellLink::new(&exe_path, Some(args), None, None)?;
    link.create_lnk(&shortcut_path)?;
    info!("created shortcut at {shortcut_path:?}");
    Ok(())
}
