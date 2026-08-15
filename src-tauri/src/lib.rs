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
    delete_app, get_app_icon, get_update_notes, get_version_list, load_app, set_startup_overrides,
    setup_app, start_app, stop_app, update_app_preferences, update_to_version, StartupOverrides,
    AUTO_START_CHECKED,
};
use crate::config_manager::{
    get_config_payload, init_config_manager, save_configuration, update_config_item,
};
use crate::utils::defender::add_defender_exclusion;
use crate::utils::logger::LoggerBuilder;
use crate::utils::window;
use crate::utils::window::{on_window_event, send_notification_cmd};
use std::{env, path::PathBuf};
use tauri::Manager;
use tracing::{error, info};
#[macro_use]
extern crate rust_i18n;
i18n!("locales", fallback = "en");

#[derive(Clone, Debug, Default, PartialEq)]
struct CommandLineOptions {
    command: Option<String>,
    profile_name: Option<String>,
    auto_start: Option<bool>,
    update_method: Option<String>,
    number_versions: Option<usize>,
    release_only: Option<bool>,
    update_to_version: Option<String>,
    response_file: Option<PathBuf>,
}

impl CommandLineOptions {
    fn requests_version_list(&self) -> bool {
        matches!(
            self.command.as_deref(),
            Some("get-version-list" | "get_version_list" | "versions")
        )
    }

    fn number_versions(&self) -> usize {
        self.number_versions.unwrap_or(10)
    }

    fn release_only(&self) -> bool {
        self.release_only.unwrap_or(true)
    }

    fn has_forwardable_request(&self) -> bool {
        self.requests_version_list() || self.update_to_version.is_some()
    }
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
            "--get-version-list" | "--get_version_list" => {
                options.command = Some("get-version-list".to_string());
                i += 1;
                continue;
            }
            "--include-prerelease" | "--include_prerelease" => {
                options.release_only = Some(false);
                i += 1;
                continue;
            }
            "-c"
            | "--command"
            | "-p"
            | "--profile"
            | "-a"
            | "--auto-start"
            | "-u"
            | "--update-method"
            | "--number-versions"
            | "--number_versions"
            | "--release-only"
            | "--release_only"
            | "--update-to-version"
            | "--update_to_version"
            | "--response-file"
            | "--response_file" => args
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
            "--number-versions" | "--number_versions" => {
                let number_versions = value.parse::<usize>().map_err(|_| {
                    format!(
                        "Invalid number_versions value '{}'; expected a positive integer",
                        value
                    )
                })?;
                if number_versions == 0 {
                    return Err("number_versions must be greater than zero".to_string());
                }
                options.number_versions = Some(number_versions);
            }
            "--release-only" | "--release_only" => {
                options.release_only = Some(parse_bool_argument(value).ok_or_else(|| {
                    format!(
                        "Invalid release_only value '{}'; expected true or false",
                        value
                    )
                })?);
            }
            "--update-to-version" | "--update_to_version" => {
                if value.trim().is_empty() {
                    return Err("update_to_version cannot be empty".to_string());
                }
                options.update_to_version = Some(value.clone());
            }
            "--response-file" | "--response_file" => {
                options.response_file = Some(PathBuf::from(value));
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
    options.command.as_deref() == Some("setup") || options.requests_version_list()
}

#[derive(serde::Serialize)]
struct CliErrorResponse<'a> {
    error: &'a str,
}

async fn write_cli_response(response_file: Option<&PathBuf>, json: &str) -> Result<(), String> {
    if let Some(response_file) = response_file {
        let mut file = tokio::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(response_file)
            .await
            .map_err(|error| {
                format!(
                    "Failed to create CLI response at '{}': {error}",
                    response_file.display()
                )
            })?;
        use tokio::io::AsyncWriteExt;
        file.write_all(json.as_bytes()).await.map_err(|error| {
            format!(
                "Failed to write CLI response to '{}': {error}",
                response_file.display()
            )
        })?;
        file.flush().await.map_err(|error| {
            format!(
                "Failed to flush CLI response to '{}': {error}",
                response_file.display()
            )
        })?;
    }
    Ok(())
}

fn error_json(message: &str) -> String {
    serde_json::to_string(&CliErrorResponse { error: message })
        .unwrap_or_else(|_| r#"{"error":"Failed to serialize error"}"#.to_string())
}

async fn execute_version_list_request(options: &CommandLineOptions) -> Result<String, String> {
    load_app()
        .await
        .map_err(|error| format!("Failed to load app: {error}"))?;
    let versions = get_version_list(options.number_versions(), options.release_only())
        .await
        .map_err(|error| format!("Failed to get version list: {error}"))?;
    serde_json::to_string(&versions)
        .map_err(|error| format!("Failed to serialize version list: {error}"))
}

async fn execute_update_request(options: &CommandLineOptions) -> Result<String, String> {
    let version = options
        .update_to_version
        .as_deref()
        .ok_or_else(|| "update_to_version is missing".to_string())?;
    let app = load_app()
        .await
        .map_err(|error| format!("Failed to load app: {error}"))?;
    update_to_version(&app.name, version)
        .await
        .map_err(|error| format!("Failed to update to version {version}: {error}"))?;
    serde_json::to_string(&serde_json::json!({
        "updated": true,
        "version": version,
    }))
    .map_err(|error| format!("Failed to serialize update result: {error}"))
}

async fn execute_forwardable_request(options: &CommandLineOptions) -> (String, bool) {
    {
        let mut auto_start_lock = AUTO_START_CHECKED.lock().await;
        *auto_start_lock = true;
    }
    let result = if options.requests_version_list() {
        execute_version_list_request(options).await
    } else {
        execute_update_request(options).await
    };
    match result {
        Ok(json) => (json, true),
        Err(message) => (error_json(&message), false),
    }
}

async fn handle_forwarded_request(options: CommandLineOptions) {
    let (json, success) = execute_forwardable_request(&options).await;
    if let Err(write_error) = write_cli_response(options.response_file.as_ref(), &json).await {
        error!("{write_error}");
    }
    if !success {
        error!("Forwarded CLI request failed: {json}");
    }
}

async fn handle_command_line(options: CommandLineOptions) {
    {
        let mut auto_start_lock = AUTO_START_CHECKED.lock().await;
        *auto_start_lock = true;
    }
    if options.requests_version_list() {
        let (json, success) = execute_forwardable_request(&options).await;
        if let Err(write_error) = write_cli_response(options.response_file.as_ref(), &json).await {
            eprintln!("{}", error_json(&write_error));
            std::process::exit(1);
        }
        println!("{json}");
        std::process::exit(if success { 0 } else { 1 });
    }
    if let (Some(cmd), Some(p_name)) = (options.command, options.profile_name) {
        if cmd == "setup" {
            let app = match load_app().await {
                Ok(app) => app,
                Err(e) => {
                    eprintln!("Failed to load app: {:?}", e);
                    std::process::exit(1);
                }
            };

            let app_name = &app.name;
            println!(
                "Command-line mode: Setting up app '{}' with profile '{}'.",
                app_name, p_name
            );
            match setup_app(app_name, &p_name).await {
                Ok(()) => {
                    println!("Setup successful.");
                    std::process::exit(0);
                }
                Err(e) => {
                    eprintln!("Setup failed: {:?}", e);
                    std::process::exit(1);
                }
            }
        }
    }
}

#[cfg(target_os = "windows")]
fn forward_to_running_instance(
    args: &[String],
    options: &CommandLineOptions,
) -> Result<Option<PathBuf>, String> {
    use std::ffi::CString;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::{
        Foundation::HWND,
        System::DataExchange::COPYDATASTRUCT,
        UI::WindowsAndMessaging::{FindWindowW, SendMessageW, WM_COPYDATA},
    };

    const WMCOPYDATA_SINGLE_INSTANCE_DATA: usize = 1542;
    let context: tauri::Context<tauri::Wry> = tauri::generate_context!();
    let identifier = &context.config().identifier;
    let encode_wide = |value: String| {
        std::ffi::OsStr::new(&value)
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<u16>>()
    };
    let class_name = encode_wide(format!("{identifier}-sic"));
    let window_name = encode_wide(format!("{identifier}-siw"));
    let hwnd: HWND = unsafe { FindWindowW(class_name.as_ptr(), window_name.as_ptr()) };
    if hwnd.is_null() {
        return Ok(None);
    }

    let response_file = options.response_file.clone().unwrap_or_else(|| {
        std::env::temp_dir().join(format!(
            "pyappify-cli-response-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ))
    });
    let mut forwarded_args = args.to_vec();
    if options.response_file.is_none() {
        forwarded_args.push("--response-file".to_string());
        forwarded_args.push(response_file.to_string_lossy().into_owned());
    }
    let cwd = std::env::current_dir().unwrap_or_default();
    let payload = format!("{}|{}\0", cwd.to_string_lossy(), forwarded_args.join("|"));
    let payload = CString::new(payload.trim_end_matches('\0'))
        .map_err(|error| format!("Invalid CLI forwarding payload: {error}"))?;
    let bytes = payload.as_bytes_with_nul();
    let copy_data = COPYDATASTRUCT {
        dwData: WMCOPYDATA_SINGLE_INSTANCE_DATA,
        cbData: bytes.len() as u32,
        lpData: bytes.as_ptr().cast_mut().cast(),
    };
    unsafe {
        SendMessageW(
            hwnd,
            WM_COPYDATA,
            0,
            &copy_data as *const COPYDATASTRUCT as isize,
        );
    }
    Ok(Some(response_file))
}

#[cfg(not(target_os = "windows"))]
fn forward_to_running_instance(
    _args: &[String],
    _options: &CommandLineOptions,
) -> Result<Option<PathBuf>, String> {
    Ok(None)
}

async fn wait_for_forwarded_response(path: &PathBuf) -> Result<String, String> {
    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(300);
    loop {
        match tokio::fs::read_to_string(path).await {
            Ok(response) if serde_json::from_str::<serde_json::Value>(&response).is_ok() => {
                let _ = tokio::fs::remove_file(path).await;
                return Ok(response);
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("Failed to read forwarded response: {error}")),
        }
        if tokio::time::Instant::now() >= deadline {
            return Err("Timed out waiting for the running PyAppify process".to_string());
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    }
}

#[tauri::command]
async fn show_main_window(window: tauri::Window) {
    window.show().unwrap();
    window.set_focus().unwrap();
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub async fn run() {
    let command_line_args = env::args().collect::<Vec<_>>();
    let command_line_options = match parse_command_line(&command_line_args) {
        Ok(options) => options,
        Err(error) => {
            eprintln!("{}", error);
            std::process::exit(2);
        }
    };
    if command_line_options.has_forwardable_request() {
        match forward_to_running_instance(&command_line_args, &command_line_options) {
            Ok(Some(response_file)) => {
                if command_line_options.response_file.is_some() {
                    return;
                }
                match wait_for_forwarded_response(&response_file).await {
                    Ok(response) => {
                        let failed = serde_json::from_str::<serde_json::Value>(&response)
                            .ok()
                            .is_some_and(|value| value.get("error").is_some());
                        println!("{response}");
                        std::process::exit(if failed { 1 } else { 0 });
                    }
                    Err(error) => {
                        eprintln!("{}", error_json(&error));
                        std::process::exit(1);
                    }
                }
            }
            Ok(None) => {}
            Err(error) => {
                eprintln!("{}", error_json(&error));
                std::process::exit(1);
            }
        }
    }
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
                if !has_cli_command(&command_line_options) {
                    println!(
                        "'src-tauri/dev_cwd' directory not found. Attempting to create it at {}",
                        dev_cwd_path.display()
                    );
                }
                if let Err(e) = std::fs::create_dir_all(&dev_cwd_path) {
                    eprintln!(
                        "Warning: Failed to create directory {}: {}",
                        dev_cwd_path.display(),
                        e
                    );
                } else {
                    if !has_cli_command(&command_line_options) {
                        println!("Successfully created directory {}", dev_cwd_path.display());
                    }
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
                    if !has_cli_command(&command_line_options) {
                        println!(
                            "Working directory set to: {}",
                            std::env::current_dir().unwrap().display()
                        );
                    }
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
        .stdout(!has_cli_command(&command_line_options))
        .init();
    info!("Log initialized");

    #[cfg(not(debug_assertions))]
    {
        if let Some(exe_path) = env::current_exe().ok() {
            if let Some(exe_dir) = exe_path.parent() {
                if let Err(e) = env::set_current_dir(exe_dir) {
                    eprintln!("Failed to set current directory to executable path: {}", e);
                } else if !has_cli_command(&command_line_options) {
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
        let initial_request = command_line_options
            .update_to_version
            .as_ref()
            .map(|_| command_line_options.clone());
        tauri::Builder::default()
            .plugin(tauri_plugin_single_instance::init(|app, args, cwd| {
                info!("tauri_plugin_single_instance args:{:?} cwd:{}", args, cwd);
                match parse_command_line(&args) {
                    Ok(options) if options.has_forwardable_request() => {
                        window::show_and_focus_main_window(app.app_handle());
                        tauri::async_runtime::spawn(handle_forwarded_request(options));
                    }
                    Ok(_) => window::show_and_focus_main_window(app.app_handle()),
                    Err(parse_error) => {
                        error!("Failed to parse forwarded arguments: {parse_error}")
                    }
                }
            }))
            .on_window_event(on_window_event)
            .plugin(tauri_plugin_opener::init())
            .plugin(tauri_plugin_notification::init())
            .setup(move |app| {
                window::create_system_tray(app).unwrap();
                let app_handle = app.handle();
                emitter::init_app_handle(app_handle.clone());
                init_config_manager(app_handle);
                tokio::spawn(app_service::periodically_update_app_running_status(
                    app_handle.clone(),
                ));
                tokio::spawn(app_service::watch_app_config_changes());
                if let Some(options) = initial_request {
                    tauri::async_runtime::spawn(handle_forwarded_request(options));
                }
                Ok(())
            })
            .invoke_handler(tauri::generate_handler![
                show_main_window,
                start_app,
                stop_app,
                load_app,
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
                number_versions: None,
                release_only: None,
                update_to_version: None,
                response_file: None,
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

    #[test]
    fn parses_version_list_defaults_and_overrides() {
        let defaults = parse_command_line(&args(&["pyappify", "--get-version-list"])).unwrap();
        assert!(defaults.requests_version_list());
        assert_eq!(defaults.number_versions(), 10);
        assert!(defaults.release_only());

        let overridden = parse_command_line(&args(&[
            "pyappify",
            "-c",
            "get_version_list",
            "--number_versions",
            "3",
            "--release_only",
            "false",
        ]))
        .unwrap();
        assert!(overridden.requests_version_list());
        assert_eq!(overridden.number_versions(), 3);
        assert!(!overridden.release_only());
    }

    #[test]
    fn parses_update_to_version_argument() {
        let parsed =
            parse_command_line(&args(&["pyappify", "--update_to_version", "v1.2.3"])).unwrap();
        assert_eq!(parsed.update_to_version.as_deref(), Some("v1.2.3"));
        assert!(parsed.has_forwardable_request());
    }
}
