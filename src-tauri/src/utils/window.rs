// src/utils/window.rs
use crate::emitter::get_app_handle;
use crate::utils::error::Error;
use crate::utils::path::get_start_dir;
use rust_i18n::t;
use std::fs;
use std::path::{Path, PathBuf};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{App, AppHandle, Manager, WebviewWindow, Window, WindowEvent, Wry};
use tauri_plugin_notification::NotificationExt;
use tracing::{error, info};

pub fn on_window_event(window: &Window, _event: &WindowEvent) {
    if let WindowEvent::Resized(size) = _event {
        if size.width == 0 && size.height == 0 && window.label() == "main" {
            info!("on_window_event {:?}, hide", _event);
            window.hide().unwrap();
        }
    }
}

pub fn send_notification(title: impl Into<String>, body: impl Into<String>) {
    let title = title.into();
    let body = body.into();
    info!("send_notification: {} {}", &title, &body);

    get_app_handle()
        .unwrap()
        .notification()
        .builder()
        .title(title)
        .body(body)
        .show()
        .unwrap();
}

#[tauri::command]
pub fn send_notification_cmd(title: String, body: String) {
    send_notification(title, body);
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

fn build_app_shortcut(
    target_path: &Path,
    arguments: Option<String>,
    working_dir: &Path,
    app_name: &str,
    icon_path: Option<&Path>,
    run_as_admin: bool,
) -> Result<shortcuts_rs::ShellLink, shortcuts_rs::MSLinkError> {
    let name = Some(app_name.to_string());
    let icon_location = icon_path.map(|path| path.to_string_lossy().into_owned());
    let mut link = shortcuts_rs::ShellLink::new(
        target_path,
        arguments.clone(),
        name.clone(),
        icon_location.clone(),
    )?;
    // shortcuts-rs 1.1.1 resets the StringData flags near the end of ShellLink::new.
    // Reapply these fields so arguments, display name, and icon are serialized to disk.
    link.set_arguments(arguments);
    link.set_name(name);
    link.set_icon_location(icon_location);
    link.set_working_dir(Some(working_dir.to_string_lossy().into_owned()));
    link.header_mut()
        .update_link_flags(shortcuts_rs::LinkFlags::RUN_AS_USER, run_as_admin);
    Ok(link)
}

fn existing_desktop_shortcuts(
    desktop_dir: &Path,
    app_filename: &str,
    launcher_filename: &str,
) -> (Option<PathBuf>, Option<PathBuf>) {
    let app_shortcut = desktop_dir.join(app_filename);
    let launcher_shortcut = desktop_dir.join(launcher_filename);
    (
        app_shortcut.exists().then_some(app_shortcut),
        launcher_shortcut.exists().then_some(launcher_shortcut),
    )
}

#[cfg(windows)]
fn write_app_shortcut(
    shortcut_path: &Path,
    target_path: &Path,
    arguments: Option<String>,
    working_dir: &Path,
    app_name: &str,
    icon_path: Option<&Path>,
    run_as_admin: bool,
) -> Result<(), Error> {
    info!(
        "Writing app shortcut '{}' -> '{}' (arguments: {:?}, working directory: '{}', icon: {:?}, run as admin: {})",
        shortcut_path.display(),
        target_path.display(),
        arguments,
        working_dir.display(),
        icon_path,
        run_as_admin
    );
    let link = match build_app_shortcut(
        target_path,
        arguments,
        working_dir,
        app_name,
        icon_path,
        run_as_admin,
    ) {
        Ok(link) => link,
        Err(error) => {
            error!(
                "Failed to build app shortcut '{}': {}",
                shortcut_path.display(),
                error
            );
            return Err(error.into());
        }
    };
    if let Err(error) = link.create_lnk(shortcut_path) {
        error!(
            "Failed to write app shortcut '{}': {}",
            shortcut_path.display(),
            error
        );
        return Err(error.into());
    }
    info!(
        "Created or updated app shortcut at '{}'",
        shortcut_path.display()
    );
    Ok(())
}

#[cfg(windows)]
pub async fn update_app_shortcuts(
    app_handle: AppHandle,
    app_name: String,
    target_path: PathBuf,
    arguments: Option<String>,
    working_dir: PathBuf,
    icon_path: Option<PathBuf>,
    run_as_admin: bool,
) -> Result<(), Error> {
    let shortcut_dir = get_start_dir(app_handle.clone());
    info!(
        "Updating Windows shortcuts for app '{}' in '{}'.",
        app_name,
        shortcut_dir.display()
    );

    if let Err(error) = fs::create_dir_all(&shortcut_dir) {
        error!(
            "Failed to create Start Menu shortcut directory '{}': {}",
            shortcut_dir.display(),
            error
        );
        return Err(error.into());
    }

    let shortcut_filename = format!("{}.lnk", app_name);
    let shortcut_path = shortcut_dir.join(&shortcut_filename);
    write_app_shortcut(
        &shortcut_path,
        &target_path,
        arguments.clone(),
        &working_dir,
        &app_name,
        icon_path.as_deref(),
        run_as_admin,
    )?;

    let launcher_target = std::env::current_exe()?;
    let launcher_working_dir = launcher_target
        .parent()
        .ok_or_else(|| anyhow::anyhow!("The launcher executable has no parent directory"))?;
    let launcher_name = t!("message.app_launcher_name", app_name = app_name).to_string();
    let launcher_shortcut_path = shortcut_dir.join(format!("{}.lnk", launcher_name));
    write_app_shortcut(
        &launcher_shortcut_path,
        &launcher_target,
        None,
        launcher_working_dir,
        &launcher_name,
        None,
        false,
    )?;

    let desktop_dir = match app_handle.path().desktop_dir() {
        Ok(path) => path,
        Err(error) => {
            error!("Failed to resolve the Desktop directory: {}", error);
            return Err(anyhow::Error::from(error).into());
        }
    };
    let launcher_filename = format!("{}.lnk", launcher_name);
    let (desktop_shortcut, desktop_launcher_shortcut) = existing_desktop_shortcuts(
        &desktop_dir,
        &shortcut_filename,
        &launcher_filename,
    );
    if let Some(desktop_shortcut) = desktop_shortcut {
        write_app_shortcut(
            &desktop_shortcut,
            &target_path,
            arguments,
            &working_dir,
            &app_name,
            icon_path.as_deref(),
            run_as_admin,
        )?;
    } else {
        info!(
            "No existing Desktop shortcut at '{}'; nothing to update.",
            desktop_dir.join(&shortcut_filename).display()
        );
    }
    if let Some(desktop_launcher_shortcut) = desktop_launcher_shortcut {
        write_app_shortcut(
            &desktop_launcher_shortcut,
            &launcher_target,
            None,
            launcher_working_dir,
            &launcher_name,
            None,
            false,
        )?;
    } else {
        info!(
            "No existing Desktop launcher shortcut at '{}'; nothing to update.",
            desktop_dir.join(&launcher_filename).display()
        );
    }
    info!(
        "Finished updating Windows shortcuts for app '{}'.",
        app_name
    );
    Ok(())
}

#[cfg(not(windows))]
pub async fn update_app_shortcuts(
    _app_handle: AppHandle,
    _app_name: String,
    _target_path: PathBuf,
    _arguments: Option<String>,
    _working_dir: PathBuf,
    _icon_path: Option<PathBuf>,
    _run_as_admin: bool,
) -> Result<(), Error> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{build_app_shortcut, existing_desktop_shortcuts};
    use shortcuts_rs::LinkFlags;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn app_shortcut_starts_python_directly_and_uses_its_icon() {
        let python_path = std::env::current_exe().unwrap();
        let working_dir = python_path.parent().unwrap();
        let icon_path = python_path.with_file_name("sample.ico");
        let shortcut = build_app_shortcut(
            &python_path,
            Some(r#""C:\Sample App\main.py""#.to_string()),
            working_dir,
            "Sample",
            Some(&icon_path),
            true,
        )
        .unwrap();

        assert_eq!(
            shortcut.arguments().as_deref(),
            Some(r#""C:\Sample App\main.py""#)
        );
        assert_eq!(shortcut.working_dir().as_deref(), working_dir.to_str());
        assert_eq!(shortcut.name().as_deref(), Some("Sample"));
        assert_eq!(shortcut.icon_location().as_deref(), icon_path.to_str());
        let flags = shortcut.header().link_flags();
        assert!(flags.contains(LinkFlags::HAS_ARGUMENTS));
        assert!(flags.contains(LinkFlags::HAS_NAME));
        assert!(flags.contains(LinkFlags::HAS_ICON_LOCATION));
        assert!(flags.contains(LinkFlags::HAS_WORKING_DIR));
        assert!(flags.contains(LinkFlags::RUN_AS_USER));
    }

    #[test]
    fn desktop_shortcuts_are_updated_only_when_each_one_exists() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let desktop_dir = std::env::temp_dir().join(format!(
            "pyappify-desktop-shortcuts-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&desktop_dir).unwrap();

        let (app, launcher) = existing_desktop_shortcuts(&desktop_dir, "app.lnk", "launcher.lnk");
        assert!(app.is_none());
        assert!(launcher.is_none());

        let app_path = desktop_dir.join("app.lnk");
        let launcher_path = desktop_dir.join("launcher.lnk");
        fs::write(&app_path, []).unwrap();
        let (app, launcher) = existing_desktop_shortcuts(&desktop_dir, "app.lnk", "launcher.lnk");
        assert_eq!(app.as_deref(), Some(app_path.as_path()));
        assert!(launcher.is_none());

        fs::write(&launcher_path, []).unwrap();
        let (app, launcher) = existing_desktop_shortcuts(&desktop_dir, "app.lnk", "launcher.lnk");
        assert_eq!(app.as_deref(), Some(app_path.as_path()));
        assert_eq!(
            launcher.as_deref(),
            Some(launcher_path.as_path())
        );

        fs::remove_dir_all(desktop_dir).unwrap();
    }
}
