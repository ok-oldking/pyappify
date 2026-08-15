//src/app_service.rs
use crate::app::{
    App, AppUpdateState, UPDATE_METHOD_OPTION_AUTO, UPDATE_METHOD_OPTION_AUTO_PRE_RELEASE,
    UPDATE_METHOD_OPTION_MANUAL,
};
use crate::emitter::get_app_handle;
use crate::git::ensure_repository;
use crate::runas;
use crate::utils::error::Error;
use crate::utils::file;
use crate::utils::file::delete_dir_if_exist;
use crate::utils::locale::get_locale;
use crate::utils::path::{get_app_base_path, get_app_working_dir_path, get_python_dir};
use crate::utils::window::{send_notification, update_app_shortcuts};
use crate::{
    app::{
        get_app_config_json_path, load_app_config_from_json, read_embedded_app,
        save_app_config_to_json, update_app_from_yml, Profile, YML_FILE_NAME,
    },
    emit_error, emit_error_finish, emit_info, emit_success_finish, emitter, err, execute_python,
    git, python_env,
    utils::path,
    utils::process,
};
use anyhow::{anyhow, bail, Context, Result};
use chrono::Utc;
use once_cell::sync::Lazy;
use rust_i18n::t;
use std::cmp::Ordering;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering as AtomicOrdering},
        Arc,
    },
    time::{SystemTime, UNIX_EPOCH},
};
use sysinfo::{Pid, ProcessesToUpdate, System};
use tauri::{AppHandle, Manager};
use tokio::sync::Mutex;
use tokio::task;
use tokio::time::{interval, Duration};
use tracing::{debug, error, info, warn};

pub static APP: Lazy<Mutex<Option<App>>> = Lazy::new(|| Mutex::new(None));
static APP_DIR_LOCK: Lazy<Arc<Mutex<()>>> = Lazy::new(|| Arc::new(Mutex::new(())));
static APP_LOAD_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));
pub static AUTO_START_CHECKED: Lazy<Mutex<bool>> = Lazy::new(|| Mutex::new(false));
static AUTO_START_CANCELLED: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Debug, Default, PartialEq)]
pub struct StartupOverrides {
    pub auto_start: Option<bool>,
    pub update_method: Option<String>,
}

pub static STARTUP_OVERRIDES: Lazy<Mutex<StartupOverrides>> =
    Lazy::new(|| Mutex::new(StartupOverrides::default()));

pub async fn set_startup_overrides(overrides: StartupOverrides) {
    *STARTUP_OVERRIDES.lock().await = overrides;
}

fn check_python_env_exists(app_name: &str) -> bool {
    let python_path = get_python_dir(app_name);
    let python_exe_path = python_path.join(if cfg!(windows) {
        "python.exe"
    } else {
        "bin/python"
    });
    python_path.exists() && python_exe_path.exists()
}

fn is_app_running(sys: &System, app_name: &str) -> bool {
    let app_working_dir = get_app_base_path(app_name);
    !process::get_pids_related_to_app_dir(sys, &app_working_dir).is_empty()
}

pub(crate) async fn load_app_details(app: &mut App) -> Result<()> {
    let working_dir = get_app_working_dir_path(&app.name);
    let yml_path = working_dir.join(YML_FILE_NAME);

    if yml_path.exists() {
        let yml_path_str = yml_path.to_string_lossy().into_owned();
        update_app_from_yml(app, &yml_path_str);
    }
    Ok(())
}

fn resolve_current_version_state(
    previous_known_version: Option<String>,
    available_versions: &[String],
    resolved_current: String,
) -> (Option<String>, bool) {
    if git::is_version_tag(&resolved_current) {
        let previous_release_moved_or_missing =
            previous_known_version.as_ref().is_some_and(|version| {
                git::is_release_version(version)
                    && (!available_versions
                        .iter()
                        .any(|available| available == version)
                        || resolved_current != *version)
            });

        if previous_release_moved_or_missing {
            return (
                previous_known_version.filter(|version| git::is_version_tag(version)),
                true,
            );
        }

        return (Some(resolved_current), false);
    }

    let previous_version = previous_known_version.filter(|version| git::is_version_tag(version));
    let release_available = available_versions
        .iter()
        .any(|version| git::is_release_version(version));

    (previous_version, release_available)
}

fn get_update_target<'a>(
    available_versions: &'a [String],
    update_method: &str,
) -> Option<&'a String> {
    available_versions
        .iter()
        .filter(|version| {
            update_method == UPDATE_METHOD_OPTION_AUTO_PRE_RELEASE
                || git::is_release_version(version)
        })
        .max_by(|left, right| git::compare_version_tags(left, right).unwrap_or(Ordering::Equal))
}

pub async fn get_app() -> Option<App> {
    let mut app = APP.lock().await.clone()?;
    let startup_overrides = STARTUP_OVERRIDES.lock().await.clone();
    if let Some(auto_start) = startup_overrides.auto_start {
        app.auto_start = auto_start;
    }
    if let Some(update_method) = &startup_overrides.update_method {
        app.update_method = update_method.clone();
    }
    Some(app)
}

pub(crate) async fn get_app_lock(app_name: &str) -> Result<Arc<Mutex<()>>, Error> {
    get_app_by_name(app_name).await?;
    Ok(APP_DIR_LOCK.clone())
}

async fn persist_update_state(
    app_name: &str,
    state: AppUpdateState,
    target_version: Option<String>,
    update_error: Option<String>,
) -> Result<()> {
    let app_to_save = {
        let mut app_guard = APP.lock().await;
        let app = app_guard
            .as_mut()
            .filter(|app| app.name == app_name)
            .with_context(|| format!("App '{}' not found.", app_name))?;
        app.update_state = state;
        app.update_target_version = target_version;
        app.update_error = update_error;
        app.clone()
    };

    save_app_config_to_json(&app_to_save).await?;
    emit_app().await;
    Ok(())
}

async fn ensure_app_is_ready_to_start(app_name: &str) -> Result<()> {
    let app_guard = APP.lock().await;
    let app = app_guard
        .as_ref()
        .filter(|app| app.name == app_name)
        .with_context(|| format!("App '{}' not found.", app_name))?;

    match app.update_state {
        AppUpdateState::Idle => Ok(()),
        AppUpdateState::Updating => bail!(
            "App '{}' cannot be started while it is updating to {}.",
            app_name,
            app.update_target_version.as_deref().unwrap_or("another version")
        ),
        AppUpdateState::Failed => bail!(
            "App '{}' cannot be started because its update to {} failed. Review the update console and retry the update.",
            app_name,
            app.update_target_version.as_deref().unwrap_or("another version")
        ),
    }
}

async fn rollback_interrupted_pip_sync_on_startup(app: &mut App, repo_path: &Path) -> Result<()> {
    if !app.installed {
        return Ok(());
    }

    let working_dir = get_app_working_dir_path(&app.name);
    let marker_path = working_dir.join(python_env::PIP_UPDATE_NEEDED_MARKER);
    if !marker_path.exists() {
        return Ok(());
    }

    let Some(previous_version) = app.current_version.clone() else {
        warn!(
            "Found unfinished pip sync marker for '{}', but no previous version is recorded.",
            app.name
        );
        return Ok(());
    };

    emit_info!(
        app.name,
        "Found unfinished pip dependency sync from a previous update. Rolling back Git version to {}.",
        previous_version
    );

    let rollback_oid =
        git::checkout_existing_revision(&app.name, repo_path, &previous_version).await?;
    emit_info!(
        app.name,
        "Checked out previous commit {} for version {}",
        rollback_oid,
        previous_version
    );

    update_working_from_repo(&app.name).await?;
    if marker_path.exists() {
        if let Err(e) = fs::remove_file(&marker_path) {
            warn!(
                "Rollback for '{}' completed, but failed to remove marker {}: {}",
                app.name,
                marker_path.display(),
                e
            );
        }
    }

    load_app_details(app).await?;
    app.current_version = Some(previous_version.clone());
    emit_info!(
        app.name,
        "Startup rollback complete. The app is back on version {}.",
        previous_version
    );
    Ok(())
}

async fn load_and_prepare_app_state(app_template: &App) -> Result<App> {
    let app_name = &app_template.name;
    let mut app = match load_app_config_from_json(app_name).await {
        Ok(Some(mut app_from_disk)) => {
            info!(
                "Loaded app '{}' from app.json. {:?}",
                app_name, app_from_disk
            );
            let mut sys = System::new();
            sys.refresh_processes(ProcessesToUpdate::All, true);
            app_from_disk.running = is_app_running(&sys, app_name);
            let current_profile = app_from_disk.current_profile.clone();
            app_from_disk.icon = app_template.icon.clone();
            app_from_disk.profiles = app_template.profiles.clone();
            app_from_disk.current_profile = current_profile;
            app_from_disk
        }
        Ok(None) => {
            info!(
                "app.json for '{}' not found. Creating from embedded template.",
                app_name
            );
            save_app_config_to_json(app_template).await?;
            app_template.clone()
        }
        Err(e) if is_invalid_json_error(&e) => {
            let config_path = get_app_config_json_path(app_name);
            let backup_path = backup_invalid_app_config(&config_path).await?;
            warn!(
                "app.json for '{}' is not valid JSON: {}. Backed it up to '{}' and regenerated it from the embedded template.",
                app_name,
                e,
                backup_path.display()
            );
            save_app_config_to_json(app_template).await?;
            app_template.clone()
        }
        Err(e) => return Err(e),
    };

    if app.installed && !check_python_env_exists(app_name) {
        warn!(
            "Python venv for app '{}' is missing. Deleting app artifacts and marking as not installed.",
            app_name
        );
        let app_base_path = get_app_base_path(app_name);
        if let Err(e) = delete_dir_if_exist(&app_base_path).await {
            warn!(
                "Failed to delete app directory {} during cleanup: {}",
                app_base_path.display(),
                e
            );
        }
        app.installed = false;
    }

    info!(
        "Loading full app details (git info, yml) for {}...",
        app.name
    );
    let repo_path = path::get_app_repo_path(&app.name);
    if app.installed && !repo_path.exists() {
        warn!(
            "Repository for app '{}' is missing. Marking as not installed.",
            app_name
        );
        app.installed = false;
    }

    if app.installed {
        rollback_interrupted_pip_sync_on_startup(&mut app, &repo_path).await?;
    }

    load_app_details(&mut app).await?;
    save_app_config_to_json(&app).await?;
    Ok(app)
}

fn is_invalid_json_error(error: &anyhow::Error) -> bool {
    error
        .chain()
        .find_map(|cause| cause.downcast_ref::<serde_json::Error>())
        .is_some_and(|json_error| json_error.is_syntax() || json_error.is_eof())
}

async fn backup_invalid_app_config(config_path: &Path) -> Result<PathBuf> {
    let unique_suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let backup_path = config_path.with_file_name(format!("app.json.invalid-{unique_suffix}.bak"));

    tokio::fs::copy(config_path, &backup_path)
        .await
        .with_context(|| {
            format!(
                "Failed to back up invalid app.json from '{}' to '{}'",
                config_path.display(),
                backup_path.display()
            )
        })?;

    Ok(backup_path)
}

#[tauri::command]
pub async fn load_app() -> Result<App, Error> {
    let _load_guard = APP_LOAD_LOCK.lock().await;
    {
        let app = APP.lock().await.clone();
        if app.is_some() {
            info!("App already loaded. Triggering update from disk.");
            if !update_app_from_disk().await? {
                info!("No app details changed after update check.");
            }
            emit_app().await;
            return get_app().await.ok_or_else(|| err!("App is not loaded."));
        }
    }

    let app_template = read_embedded_app();
    info!(
        "Loading the single, embedded application. profiles {:?}",
        app_template.profiles
    );

    let app = load_and_prepare_app_state(&app_template).await?;
    info!(
        "Finished loading app details. {} {}",
        app.name, app.installed
    );

    *APP.lock().await = Some(app);
    emit_app().await;

    if update_app_from_disk().await? {
        emit_app().await;
    } else {
        info!("Not emitting app from disk because no changes were detected from git.");
    }

    let mut auto_start_guard = AUTO_START_CHECKED.lock().await;
    if !*auto_start_guard {
        *auto_start_guard = true;

        let app_clone_for_checks = APP.lock().await.clone();

        if let Some(mut app) = app_clone_for_checks {
            let startup_overrides = STARTUP_OVERRIDES.lock().await.clone();
            let mut update_failed = false;
            if app.update_state != AppUpdateState::Idle {
                if let Some(retry_version) = app.update_target_version.clone() {
                    info!(
                        "Retrying persisted {:?} update for '{}' to '{}'.",
                        app.update_state, app.name, retry_version
                    );
                    send_notification(
                        app.name.clone(),
                        format!(
                            "Retrying the last interrupted or failed update to {}.",
                            retry_version
                        ),
                    );
                    match update_to_version(&app.name, &retry_version).await {
                        Ok(()) => {
                            send_notification(
                                app.name.clone(),
                                t!("message.version_update_success", version = retry_version),
                            );
                            if let Some(refreshed_app) = APP.lock().await.clone() {
                                app = refreshed_app;
                            }
                        }
                        Err(error) => {
                            error!(
                                "Startup retry for app '{}' to '{}' failed: {}",
                                app.name, retry_version, error
                            );
                            send_notification(
                                app.name.clone(),
                                format!(
                                    "The retry to update to {} failed. Open the update console for details.",
                                    retry_version
                                ),
                            );
                            update_failed = true;
                        }
                    }
                } else {
                    warn!(
                        "App '{}' has persisted update state {:?}, but no target version.",
                        app.name, app.update_state
                    );
                    send_notification(
                        app.name.clone(),
                        "The previous update did not finish and has no retry target. Review the update console.",
                    );
                    update_failed = true;
                }
            }

            let update_method = startup_overrides
                .update_method
                .clone()
                .unwrap_or_else(|| app.effective_update_method().to_string());
            let auto_start = startup_overrides.auto_start.unwrap_or(app.auto_start);

            let latest_update_version =
                get_update_target(&app.available_versions, &update_method).cloned();
            let current_version_missing = app.current_version_missing;
            let update_available = latest_update_version
                .as_ref()
                .is_some_and(|latest_version| {
                    if current_version_missing {
                        return true;
                    }
                    app.current_version.as_ref().is_some_and(|current_version| {
                        git::compare_version_tags(latest_version, current_version)
                            == Some(Ordering::Greater)
                    })
                });
            let is_latest = !update_available;

            info!(
                "First load, checking update and auto-start conditions. update_method:{}, auto_start:{}, is_latest:{}, current_version_missing:{}",
                update_method, auto_start, is_latest, current_version_missing
            );

            info!("locale is {}", get_locale());
            if !update_failed
                && app.installed
                && !app.available_versions.is_empty()
                && update_available
            {
                if current_version_missing {
                    info!(
                            "Current version is no longer available upstream. Forcing update to the latest available version."
                        );
                } else {
                    info!("App is not the latest version for the selected update method.");
                }
                let app_name_clone = app.name.clone();
                let latest_version =
                    latest_update_version.expect("update_available requires a target version");
                if current_version_missing
                    || update_method == UPDATE_METHOD_OPTION_AUTO
                    || update_method == UPDATE_METHOD_OPTION_AUTO_PRE_RELEASE
                {
                    info!(
                        "{}",
                        t!(
                            "message.new_version_update",
                            version = latest_version.clone()
                        )
                    );
                    send_notification(
                        app_name_clone.clone(),
                        t!("message.new_version_update", version = latest_version),
                    );
                    match update_to_version(&app_name_clone, &latest_version).await {
                        Ok(()) => {
                            info!("Auto Update to version {} success.", &latest_version);
                            send_notification(
                                app_name_clone,
                                t!("message.version_update_success", version = latest_version),
                            );
                        }
                        Err(error) => {
                            error!(
                                "Auto update for app '{}' to '{}' failed: {}",
                                app.name, latest_version, error
                            );
                            send_notification(
                                    app.name.clone(),
                                    format!(
                                        "Automatic update to {} failed. Open the update console for details.",
                                        latest_version
                                    ),
                                );
                            update_failed = true;
                        }
                    }
                } else {
                    send_notification(
                        app_name_clone.clone(),
                        t!("message.new_version", version = latest_version),
                    );
                }
            }

            if auto_start
                && !update_failed
                && app.update_state == AppUpdateState::Idle
                && app.installed
                && !app.available_versions.is_empty()
            {
                info!("Scheduling auto-start for '{}' in 10 seconds.", app.name);
                let app_name_clone = app.name.clone();
                let auto_start_override = startup_overrides.auto_start;
                AUTO_START_CANCELLED.store(false, AtomicOrdering::SeqCst);
                drop(auto_start_guard);
                if let Some(app_handle) = get_app_handle() {
                    let app_handle_clone = app_handle.clone();
                    tokio::spawn(async move {
                        tokio::time::sleep(Duration::from_secs(10)).await;

                        let should_start = if AUTO_START_CANCELLED.load(AtomicOrdering::SeqCst) {
                            false
                        } else {
                            APP.lock().await.as_ref().is_some_and(|current_app| {
                                current_app.name == app_name_clone
                                    && auto_start_override.unwrap_or(current_app.auto_start)
                                    && current_app.installed
                                    && !current_app.available_versions.is_empty()
                                    && current_app.update_state == AppUpdateState::Idle
                            })
                        };

                        if !should_start {
                            info!("Cancelled delayed auto-start for '{}'.", app_name_clone);
                            return;
                        }

                        info!("Auto-starting app '{}' after delay.", app_name_clone);
                        if let Err(e) = start_app(app_handle_clone, app_name_clone.clone()).await {
                            error!("Auto-start for app '{}' failed: {:?}", app_name_clone, e);
                        }
                    });
                }
            } else {
                info!(
                    "Auto-start disabled or app not ready for '{}' (enabled: {}, installed: {}, has_versions: {}).",
                    app.name,
                    auto_start,
                    app.installed,
                    !app.available_versions.is_empty()
                );
            }
        }
    }

    get_app().await.ok_or_else(|| err!("App is not loaded."))
}

async fn update_app_from_disk() -> Result<bool, Error> {
    let mut app = get_app().await.ok_or_else(|| err!("App is not loaded."))?;
    let original_app = app.clone();

    info!(
        "Updating full app details (git info and YAML) for '{}'.",
        app.name
    );
    let repo_path = path::get_app_repo_path(&app.name);
    if app.installed && repo_path.exists() {
        ensure_repository(&app).await?;
        let previous_known_version = app.current_version.clone();
        let (versions, current) = git::get_tags_and_current_version(&app.name, repo_path).await?;
        let (current_version, current_version_missing) =
            resolve_current_version_state(previous_known_version.clone(), &versions, current);
        app.current_version_missing = current_version_missing;
        if app.current_version_missing {
            warn!(
                "Current version {:?} for app '{}' no longer matches remote tags.",
                previous_known_version, app.name
            );
        }
        app.available_versions = versions;
        app.current_version = current_version;
        info!(
            "get_tags_and_current_version done for {}: {:?}",
            app.name, app.current_version
        );
    }

    if app == original_app {
        debug!("Finished updating app details from disk without changes.");
        return Ok(false);
    }

    info!("App details modified for {}. Saving to disk.", app.name);
    save_app_config_to_json(&app).await?;
    *APP.lock().await = Some(app);
    Ok(true)
}

#[tauri::command]
pub async fn delete_app(app_name: &str) -> Result<(), Error> {
    info!("Attempting to delete app: {}", app_name);
    let app_dir_lock = get_app_lock(app_name).await?;
    let _guard = app_dir_lock.lock().await;

    let app_base_path = get_app_base_path(app_name);
    if let Err(e) = delete_dir_if_exist(&app_base_path).await {
        error!("Failed to delete dir {}: {}", app_base_path.display(), e);
    } else {
        info!("Deleted dir: {}", app_base_path.display());
    }
    let mut app: App = get_app_by_name(app_name).await?;
    app.installed = false;
    save_app_config_to_json(&app).await?;
    *APP.lock().await = Some(app);
    emit_app().await;
    Ok(())
}

#[tauri::command]
pub async fn update_app_preferences(
    app_name: String,
    update_method: Option<String>,
    auto_start: Option<bool>,
) -> Result<(), Error> {
    let app_dir_lock = get_app_lock(&app_name).await?;
    let _guard = app_dir_lock.lock().await;
    let mut app = get_app_by_name(&app_name).await?;

    if let Some(update_method) = update_method {
        if !matches!(
            update_method.as_str(),
            UPDATE_METHOD_OPTION_MANUAL
                | UPDATE_METHOD_OPTION_AUTO
                | UPDATE_METHOD_OPTION_AUTO_PRE_RELEASE
        ) {
            return Err(err!("Unsupported update method: {}", update_method));
        }
        STARTUP_OVERRIDES.lock().await.update_method = None;
        app.update_method = update_method;
    }

    if let Some(auto_start) = auto_start {
        STARTUP_OVERRIDES.lock().await.auto_start = None;
        if !auto_start {
            AUTO_START_CANCELLED.store(true, AtomicOrdering::SeqCst);
        }
        app.auto_start = auto_start;
    }

    save_app_config_to_json(&app).await?;
    *APP.lock().await = Some(app);
    emit_app().await;
    Ok(())
}

pub(crate) async fn emit_app() {
    if let Some(app) = get_app().await {
        emitter::emit("app", app);
    }
}

#[tauri::command]
pub async fn get_update_notes(app_name: String, version: String) -> Result<Vec<String>, Error> {
    let app_lock = get_app_lock(&app_name).await?;
    let _guard = app_lock.lock().await;
    let app = get_app_by_name(&app_name).await?;
    let messages =
        git::get_commit_messages_for_version_diff(&app.get_repo_path(), &version).await?;
    info!(
        "get_update_notes for {} version {} messages: {:?}",
        app.name, version, messages
    );
    Ok(messages)
}

pub async fn get_version_list(
    number_versions: usize,
    release_only: bool,
) -> Result<Vec<git::VersionHistoryEntry>, Error> {
    let app = get_app().await.ok_or_else(|| err!("App is not loaded."))?;
    let app_lock = get_app_lock(&app.name).await?;
    let _guard = app_lock.lock().await;
    ensure_repository(&app).await?;
    Ok(git::get_version_history(&app.get_repo_path(), number_versions, release_only).await?)
}

async fn get_app_by_name(app_name: &str) -> Result<App, Error> {
    APP.lock()
        .await
        .as_ref()
        .filter(|app| app.name == app_name)
        .cloned()
        .ok_or_else(|| anyhow!("App '{}' not found.", app_name).into())
}

#[derive(serde::Serialize)]
pub struct AppIconAsset {
    bytes: Vec<u8>,
    mime_type: &'static str,
}

fn icon_mime_type(path: &Path) -> Option<&'static str> {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("png") => Some("image/png"),
        Some("jpg") | Some("jpeg") => Some("image/jpeg"),
        Some("gif") => Some("image/gif"),
        Some("webp") => Some("image/webp"),
        Some("svg") => Some("image/svg+xml"),
        Some("bmp") => Some("image/bmp"),
        Some("ico") => Some("image/x-icon"),
        _ => None,
    }
}

async fn resolve_app_icon_path(app_name: &str, configured_path: &str) -> Option<PathBuf> {
    let configured_path = configured_path.trim();
    if configured_path.is_empty() {
        return None;
    }

    let relative_path = Path::new(configured_path);
    if relative_path.is_absolute() {
        warn!(
            "Ignoring absolute icon path '{}' for app '{}'; icon paths must be relative.",
            configured_path, app_name
        );
        return None;
    }

    let working_dir = get_app_working_dir_path(app_name);
    let canonical_working_dir = tokio::fs::canonicalize(&working_dir).await.ok()?;
    let canonical_icon_path = tokio::fs::canonicalize(working_dir.join(relative_path))
        .await
        .ok()?;

    if !canonical_icon_path.starts_with(&canonical_working_dir) {
        warn!(
            "Ignoring icon path '{}' for app '{}' because it escapes the working directory.",
            configured_path, app_name
        );
        return None;
    }

    let metadata = tokio::fs::metadata(&canonical_icon_path).await.ok()?;
    metadata.is_file().then_some(canonical_icon_path)
}

async fn resolve_app_shortcut_icon_path(app_name: &str, configured_path: &str) -> Option<PathBuf> {
    let configured_icon = resolve_app_icon_path(app_name, configured_path).await?;
    if configured_icon
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("ico"))
    {
        return Some(configured_icon);
    }

    let ico_candidate = configured_icon.with_extension("ico");
    let canonical_ico = tokio::fs::canonicalize(ico_candidate).await.ok()?;
    let canonical_working_dir = tokio::fs::canonicalize(get_app_working_dir_path(app_name))
        .await
        .ok()?;
    let metadata = tokio::fs::metadata(&canonical_ico).await.ok()?;
    (metadata.is_file() && canonical_ico.starts_with(canonical_working_dir))
        .then_some(canonical_ico)
}

#[tauri::command]
pub async fn get_app_icon(app_name: String) -> Result<Option<AppIconAsset>, Error> {
    const MAX_ICON_BYTES: u64 = 5 * 1024 * 1024;

    let app = get_app_by_name(&app_name).await?;
    let Some(canonical_icon_path) = resolve_app_icon_path(&app_name, &app.icon).await else {
        return Ok(None);
    };

    let Some(mime_type) = icon_mime_type(&canonical_icon_path) else {
        warn!(
            "Ignoring unsupported icon file '{}' for app '{}'.",
            canonical_icon_path.display(),
            app_name
        );
        return Ok(None);
    };
    let metadata = tokio::fs::metadata(&canonical_icon_path).await?;
    if metadata.len() > MAX_ICON_BYTES {
        warn!(
            "Ignoring icon file '{}' for app '{}': it is not a file or exceeds 5 MiB.",
            canonical_icon_path.display(),
            app_name
        );
        return Ok(None);
    }

    Ok(Some(AppIconAsset {
        bytes: tokio::fs::read(canonical_icon_path).await?,
        mime_type,
    }))
}

pub async fn update_working_from_repo(app_name: &str) -> Result<()> {
    let repo_path = path::get_app_repo_path(app_name);
    let working_dir_path = get_app_working_dir_path(app_name);
    info!(
        "update_working_from_repo {}: repo_path = {}, working_dir_path = {}",
        app_name,
        repo_path.display(),
        working_dir_path.display()
    );

    if !repo_path.exists() {
        bail!("Repo for {} not at {}", app_name, repo_path.display());
    }
    if !working_dir_path.exists() {
        tokio::fs::create_dir_all(&working_dir_path)
            .await
            .with_context(|| format!("Failed to create dir {}", working_dir_path.display()))?;
    }

    let task_repo_path = repo_path.clone();
    let task_working_dir_path = working_dir_path.clone();
    task::spawn_blocking(move || -> Result<()> {
        let repository = git2::Repository::open(&task_repo_path).with_context(|| {
            format!(
                "Failed to open repository at {} before synchronizing app files",
                task_repo_path.display()
            )
        })?;
        file::copy_dir_recursive_filtered_sync(
            &task_repo_path,
            &task_working_dir_path,
            &[".git"],
            &|relative_path| match repository.status_should_ignore(relative_path) {
                Ok(ignored) => ignored,
                Err(error) => {
                    warn!(
                        "Could not determine Git ignore status for {} in {}: {}. Copying it.",
                        relative_path.display(),
                        task_repo_path.display(),
                        error
                    );
                    false
                }
            },
        )?;
        file::sync_delete_extra_files(&task_working_dir_path, &task_repo_path)?;
        Ok(())
    })
    .await??;
    Ok(())
}

fn get_profile_for_setup<'a>(
    temp_app_config: &'a App,
    profile_name: &str,
    app_name: &str,
) -> Result<(&'a Profile, String)> {
    match temp_app_config.get_profile(profile_name) {
        Some(profile) => Ok((profile, profile_name.to_string())),
        None => {
            if profile_name != "default" {
                warn!(
                    "Profile '{}' not found for setup in app '{}'. Falling back to 'default' profile.",
                    profile_name, app_name
                );
            }
            let final_profile_name_to_set = "default".to_string();
            let profile = temp_app_config.get_profile("default").ok_or_else(|| {
                anyhow!(
                    "Profile '{}' (and fallback 'default') not found in {} for app {}",
                    profile_name,
                    YML_FILE_NAME,
                    app_name
                )
            })?;
            Ok((profile, final_profile_name_to_set))
        }
    }
}

#[tauri::command]
pub async fn setup_app(app_name: &str, profile_name: &str) -> Result<(), Error> {
    let app_dir_lock = get_app_lock(app_name).await?;
    let _guard = app_dir_lock.lock().await;

    let repo_path = path::get_app_repo_path(app_name);
    let app = get_app_by_name(app_name).await?;

    ensure_repository(&app).await?;

    let working_dir_path = get_app_working_dir_path(app_name);
    if !repo_path.exists() {
        err!("Repo for {} not at {}", app_name, repo_path.display());
    }

    delete_dir_if_exist(&working_dir_path).await?;

    tokio::fs::create_dir_all(&working_dir_path)
        .await
        .with_context(|| format!("Failed to create dir {}", working_dir_path.display()))?;

    update_working_from_repo(app_name).await?;

    let yml_path = working_dir_path.join(YML_FILE_NAME);
    let yml_path_str = yml_path.to_string_lossy().into_owned();

    let mut temp_app_for_config = read_embedded_app();
    temp_app_for_config.name = app_name.to_string();
    update_app_from_yml(&mut temp_app_for_config, &yml_path_str);

    let (profile_settings_for_setup, final_profile_name_to_set) =
        get_profile_for_setup(&temp_app_for_config, profile_name, app_name)?;

    let requirements = &profile_settings_for_setup.requirements;
    let python_version_spec = &profile_settings_for_setup.requires_python;
    let pip_args = &profile_settings_for_setup.pip_args;
    python_env::setup_python_env(app_name.to_string(), python_version_spec).await?;

    if !requirements.is_empty() {
        python_env::install_requirements(app_name, requirements, &working_dir_path, pip_args)
            .await?;
    } else {
        info!(
            "No reqs in profile '{}' of {}. Skipping sync.",
            final_profile_name_to_set, YML_FILE_NAME
        );
    }

    let mut app_guard = APP.lock().await;
    if let Some(app) = app_guard.as_mut().filter(|app| app.name == app_name) {
        load_app_details(app).await?;
        app.installed = true;
        app.current_profile = final_profile_name_to_set.clone();
        let app_to_save = app.clone();
        drop(app_guard);

        if let Err(e) = save_app_config_to_json(&app_to_save).await {
            error!(
                "Failed to save app config for {} after setup (installed=true, profile='{}'): {:?}",
                app_name, final_profile_name_to_set, e
            );
        }
        info!(
            "App config json saved successfully after setup {} installed {}",
            app_to_save.name, app_to_save.installed
        );
        update_app_from_disk().await?;
        emit_app().await;
    } else {
        warn!(
            "App {} not found after setup, cannot mark as installed or set profile.",
            app_name
        );
    }

    emit_success_finish!(app_name);
    Ok(())
}

fn get_relevant_content(spec: &str, dir: &Path) -> Option<String> {
    if spec.is_empty() {
        return None;
    }
    let file_to_check = if spec.ends_with(".txt") {
        dir.join(spec)
    } else {
        dir.join("pyproject.toml")
    };
    fs::read_to_string(file_to_check).ok()
}

async fn rollback_to_previous_version(
    app_name: &str,
    repo_path: &Path,
    previous_version: &str,
    previous_revision: Option<&str>,
    reason: &str,
) -> Result<(), Error> {
    emit_info!(
        app_name,
        "{} Rolling back Git version to {}.",
        reason,
        previous_version
    );

    let mut used_revision_fallback = false;
    let rollback_oid = match git::checkout_existing_revision(app_name, repo_path, previous_version)
        .await
    {
        Ok(oid) => oid,
        Err(version_error) => {
            let Some(previous_revision) = previous_revision else {
                return Err(version_error.into());
            };
            emit_info!(
                app_name,
                "Rollback by version '{}' failed. Trying previous commit {}.",
                previous_version,
                previous_revision
            );
            used_revision_fallback = true;
            git::checkout_existing_revision(app_name, repo_path, previous_revision)
                .await
                .map_err(|revision_error| {
                    err!(
                        "Rollback by version '{}' failed: {}. Rollback by previous commit {} also failed: {}",
                        previous_version,
                        version_error,
                        previous_revision,
                        revision_error
                    )
                })?
        }
    };
    emit_info!(
        app_name,
        "Checked out previous commit {} for version {}",
        rollback_oid,
        previous_version
    );

    update_working_from_repo(app_name).await?;

    {
        let mut app_guard = APP.lock().await;
        if let Some(app) = app_guard.as_mut().filter(|app| app.name == app_name) {
            load_app_details(app).await?;
            app.current_version = Some(previous_version.to_string());
            app.current_version_missing = used_revision_fallback;
            let app_to_save = app.clone();
            drop(app_guard);
            save_app_config_to_json(&app_to_save).await?;
        }
    }

    emit_app().await;
    emit_info!(
        app_name,
        "Rollback complete. The app is back on version {}.",
        previous_version
    );
    Ok(())
}

#[tauri::command]
pub async fn update_to_version(app_name: &str, version: &str) -> Result<(), Error> {
    info!("Updating {} to version {}", app_name, version);
    let app_dir_lock = get_app_lock(app_name).await?;
    let _lock_guard = app_dir_lock.lock().await;

    ensure_app_stopped_for_update(app_name).await?;

    if let Err(state_error) = persist_update_state(
        app_name,
        AppUpdateState::Updating,
        Some(version.to_string()),
        None,
    )
    .await
    {
        let error_message = format!("Failed to record update progress: {}", state_error);
        if let Err(failure_state_error) = persist_update_state(
            app_name,
            AppUpdateState::Failed,
            Some(version.to_string()),
            Some(error_message.clone()),
        )
        .await
        {
            error!(
                "Failed to persist update failure state for '{}': {}",
                app_name, failure_state_error
            );
            emit_app().await;
        }
        emit_error!(app_name, "{}", error_message);
        emit_error_finish!(app_name);
        return Err(state_error.into());
    }

    match update_to_version_inner(app_name, version).await {
        Ok(()) => match persist_update_state(app_name, AppUpdateState::Idle, None, None).await {
            Ok(()) => {
                emit_success_finish!(app_name);
                Ok(())
            }
            Err(state_error) => {
                let error_message = format!(
                    "Update completed, but its state could not be saved: {}",
                    state_error
                );
                if let Err(failure_state_error) = persist_update_state(
                    app_name,
                    AppUpdateState::Failed,
                    Some(version.to_string()),
                    Some(error_message.clone()),
                )
                .await
                {
                    error!(
                        "Failed to persist update failure state for '{}': {}",
                        app_name, failure_state_error
                    );
                    emit_app().await;
                }
                emit_error!(app_name, "{}", error_message);
                emit_error_finish!(app_name);
                Err(state_error.into())
            }
        },
        Err(error) => {
            let error_message = error.to_string();
            if let Err(state_error) = persist_update_state(
                app_name,
                AppUpdateState::Failed,
                Some(version.to_string()),
                Some(error_message.clone()),
            )
            .await
            {
                error!(
                    "Failed to persist update failure state for '{}': {}",
                    app_name, state_error
                );
            }
            emit_error!(
                app_name,
                "Update to version {} failed: {}",
                version,
                error_message
            );
            emit_error_finish!(app_name);
            Err(error)
        }
    }
}

async fn ensure_app_stopped_for_update(app_name: &str) -> Result<(), Error> {
    let app_base_path = get_app_base_path(app_name);
    let running_pids = task::spawn_blocking(move || {
        let mut system = System::new();
        system.refresh_processes(ProcessesToUpdate::All, true);
        process::get_pids_related_to_app_dir(&system, &app_base_path)
    })
    .await?;
    if !running_pids.is_empty() {
        let pid_list = running_pids
            .iter()
            .map(|pid| pid.as_u32().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        return Err(err!(
            "Cannot update '{}': the application is still running (PID {}). Stop it completely and retry.",
            app_name,
            pid_list
        ));
    }
    Ok(())
}

async fn update_to_version_inner(app_name: &str, version: &str) -> Result<(), Error> {
    let working_dir_path = get_app_working_dir_path(app_name);

    let (previous_version, old_requirements_spec) = {
        let app_guard = APP.lock().await;
        match app_guard.as_ref().filter(|app| app.name == app_name) {
            Some(app) => (
                app.current_version.clone(),
                app.get_current_profile_settings().requirements.clone(),
            ),
            None => (None, String::new()),
        }
    };

    let repo_path = path::get_app_repo_path(app_name);
    let mut previous_revision = git::get_current_head_oid(&repo_path)
        .await
        .map(|oid| oid.to_string())
        .ok();

    // A prior update may have checked out and copied a new version before failing to
    // persist it. Restore the recorded version first so dependency comparison uses
    // the actual old files rather than a partially applied update.
    if let (Some(previous_version), Some(current_head)) =
        (previous_version.as_deref(), previous_revision.as_deref())
    {
        match git::get_revision_oid(&repo_path, previous_version).await {
            Ok(recorded_oid) if recorded_oid.to_string() != current_head => {
                emit_info!(
                    app_name,
                    "Detected an unfinished previous update. Restoring recorded version {} before retrying.",
                    previous_version
                );
                let restored_oid =
                    git::checkout_existing_revision(app_name, &repo_path, previous_version).await?;
                update_working_from_repo(app_name).await?;
                previous_revision = Some(restored_oid.to_string());
            }
            Ok(_) => {}
            Err(error) => debug!(
                "Could not resolve recorded version '{}' before updating '{}': {}",
                previous_version, app_name, error
            ),
        }
    }

    let old_content = get_relevant_content(&old_requirements_spec, &working_dir_path);
    let update_note = if previous_version.as_deref() == Some(version) {
        Vec::new()
    } else {
        match git::get_commit_messages_for_version_diff(&path::get_app_repo_path(app_name), version)
            .await
        {
            Ok(messages) => messages,
            Err(error) => {
                warn!(
                    "Failed to collect update notes for {} moving to {}: {}",
                    app_name, version, error
                );
                Vec::new()
            }
        }
    };

    let commit_oid = git::checkout_version_tag(app_name, &repo_path, version).await?;
    emit_info!(
        app_name,
        "Checked out commit {} for version {}",
        commit_oid,
        version
    );
    if let Err(sync_error) = update_working_from_repo(app_name).await {
        if let Some(previous_version) = previous_version.as_deref() {
            if let Err(rollback_error) = rollback_to_previous_version(
                app_name,
                &repo_path,
                previous_version,
                previous_revision.as_deref(),
                "App file synchronization failed.",
            )
            .await
            {
                return Err(err!(
                    "App file synchronization failed: {}. Rollback to previous version '{}' also failed: {}",
                    sync_error,
                    previous_version,
                    rollback_error
                ));
            }
        }
        return Err(err!("App file synchronization failed: {}", sync_error));
    }
    debug!("Updated working dir for app {}", app_name);

    let (new_requirements_spec, new_pip_args) = {
        let yml_path = working_dir_path.join(YML_FILE_NAME);
        let mut temp_app = read_embedded_app();
        temp_app.name = app_name.to_string();
        update_app_from_yml(&mut temp_app, &yml_path.to_string_lossy());
        match temp_app.get_profile("default") {
            Some(p) => (p.requirements.clone(), p.pip_args.clone()),
            None => (String::new(), String::new()),
        }
    };
    let new_content = get_relevant_content(&new_requirements_spec, &working_dir_path);

    let spec_changed = old_requirements_spec != new_requirements_spec;
    let content_changed = old_content != new_content;
    let needs_pip_sync = !new_requirements_spec.is_empty() && (spec_changed || content_changed);

    if needs_pip_sync {
        if spec_changed {
            emit_info!(
                app_name,
                "Requirements spec changed from '{}' to '{}'. Syncing dependencies.",
                old_requirements_spec,
                new_requirements_spec
            );
        } else {
            let file_type = if new_requirements_spec.ends_with(".txt") {
                &new_requirements_spec
            } else {
                "pyproject.toml"
            };
            emit_info!(
                app_name,
                "Content of '{}' changed. Syncing dependencies.",
                file_type
            );
        }
        if let Err(pip_error) = python_env::install_requirements(
            app_name,
            &new_requirements_spec,
            &working_dir_path,
            &new_pip_args,
        )
        .await
        {
            if let Some(previous_version) = previous_version.as_deref() {
                info!(
                    "Pip sync failed while updating {} to {}. Attempting rollback to {}.",
                    app_name, version, previous_version
                );
                if let Err(rollback_error) = rollback_to_previous_version(
                    app_name,
                    &repo_path,
                    previous_version,
                    previous_revision.as_deref(),
                    "Pip dependency sync failed.",
                )
                .await
                {
                    return Err(err!(
                        "Pip dependency sync failed: {}. Rollback to previous version '{}' also failed: {}",
                        pip_error,
                        previous_version,
                        rollback_error
                    ));
                }
            } else {
                warn!(
                    "Pip sync failed while updating {} to {}, but no previous version is recorded.",
                    app_name, version
                );
            }
            return Err(pip_error);
        }
    } else {
        emit_info!(
            app_name,
            "Requirements are up to date. Skipping dependency sync."
        );
    }

    {
        let mut app_guard = APP.lock().await;
        if let Some(app) = app_guard.as_mut().filter(|app| app.name == app_name) {
            load_app_details(app).await?;
            app.current_version = Some(version.to_string());
            app.current_version_missing = false;
            app.app_starting_version = Some(
                previous_version
                    .clone()
                    .unwrap_or_else(|| version.to_string()),
            );
            app.update_note = update_note;
            let app_to_save = app.clone();
            drop(app_guard);
            save_app_config_to_json(&app_to_save).await?;
        }
    }

    emit_info!(app_name, "Updated {} to version {}", app_name, version);
    Ok(())
}

fn build_python_execution_environment(
    app_name: &str,
    profile: &Profile,
    current_version: Option<String>,
    app_starting_version: Option<String>,
    update_note: Vec<String>,
    pyappify_version: String,
) -> Vec<(String, String)> {
    let mut envs = Vec::new();
    if !profile.python_path.is_empty() {
        envs.push(("PYTHONPATH".to_string(), profile.python_path.clone()));
    }

    let app_version = current_version.clone().unwrap_or_default();
    let starting_version = app_starting_version.unwrap_or_else(|| app_version.clone());
    let encoded_update_note = if update_note.is_empty() {
        String::new()
    } else {
        serde_json::to_string(&update_note).unwrap_or_else(|error| {
            warn!("Failed to encode PYAPPIFY_UPDATE_NOTE as JSON: {}", error);
            String::new()
        })
    };

    if let Some(version) = current_version {
        envs.push(("PYAPPIFY_APP_VERSION".to_string(), version));
    }
    envs.push((
        "PYAPPIFY_APP_STARTING_VERSION".to_string(),
        starting_version,
    ));
    envs.push(("PYAPPIFY_UPDATE_NOTE".to_string(), encoded_update_note));
    envs.push(("PYAPPIFY_APP_PROFILE".to_string(), profile.name.clone()));
    envs.push(("PYAPPIFY_LOCALE".to_string(), get_locale().to_string()));
    envs.push((
        "PYAPPIFY_APP_JSON_PATH".to_string(),
        path::path_to_abs(&get_app_config_json_path(app_name)),
    ));
    envs.push(("PYAPPIFY_PID".to_string(), std::process::id().to_string()));
    envs.push(("PYAPPIFY_UPGRADEABLE".to_string(), 1.to_string()));
    envs.push(("PYAPPIFY_VERSION".to_string(), pyappify_version));
    envs.push(("PYTHONIOENCODING".to_string(), "utf-8".to_string()));
    envs.push(("PYTHONUNBUFFERED".to_string(), "1".to_string()));
    envs.push(("PYTHONNOUSERSITE".to_string(), "1".to_string()));
    if let Ok(exe_path) = std::env::current_exe() {
        envs.push((
            "PYAPPIFY_EXECUTABLE".to_string(),
            exe_path.to_string_lossy().to_string(),
        ));
    }

    envs
}

async fn check_running_on_start(app_name: &str) -> Result<bool> {
    let start_time = tokio::time::Instant::now();
    let timeout = Duration::from_secs(10);
    let mut interval = tokio::time::interval(Duration::from_secs(1));
    let mut sys = System::new();
    let app_base_dir = get_app_base_path(app_name);

    info!(
        "Monitoring for app '{}' to start for up to 10 seconds...",
        app_name
    );

    while tokio::time::Instant::now().duration_since(start_time) < timeout {
        interval.tick().await;
        sys.refresh_processes(ProcessesToUpdate::All, true);
        let pids = process::get_pids_related_to_app_dir(&sys, &app_base_dir);
        if !pids.is_empty() {
            info!(
                "App '{}' detected as running from '{}'. Updating status.",
                app_name,
                app_base_dir.display()
            );

            let mut app_guard = APP.lock().await;
            if let Some(app) = app_guard.as_mut().filter(|app| app.name == app_name) {
                app.running = true;
            }
            drop(app_guard);

            emit_app().await;
            return Ok(true);
        }
    }

    warn!(
        "App '{}' did not appear to be running with a visible window within 10 seconds.",
        app_name
    );
    sys.refresh_processes(ProcessesToUpdate::All, true);
    let is_running_after_timeout = is_app_running(&sys, app_name);
    let mut app_guard = APP.lock().await;
    if let Some(app) = app_guard.as_mut().filter(|app| app.name == app_name) {
        if app.running != is_running_after_timeout {
            app.running = is_running_after_timeout;
            drop(app_guard);
            emit_app().await;
        }
    }

    Ok(is_running_after_timeout)
}

async fn build_app_shortcut_command(
    app_name: &str,
    profile: &Profile,
    working_dir: &Path,
) -> Result<(PathBuf, Option<String>)> {
    let python_dir = path::get_python_dir(app_name);
    let script_path = execute_python::find_script_or_executable(
        &profile.main_script,
        working_dir,
        &python_dir.join("Scripts"),
    )?;

    if profile.main_script.ends_with(".py") {
        let python_executable = path::get_python_exe(app_name, profile.use_pythonw());
        let bootstrap_path = get_app_base_path(app_name).join(".pyappify-shortcut.py");
        let env_names = serde_json::to_string(&process::PYTHON_ENVS_TO_REMOVE)?;
        let main_script = serde_json::to_string(&path::path_to_abs(&script_path))?;
        let python_path = serde_json::to_string(&profile.python_path)?;
        let bootstrap = format!(
            r#"# Generated by PyAppify. Changes will be replaced after the next successful launch.
import os
import sys

for _name in {env_names}:
    os.environ.pop(_name, None)

_system_root = os.environ.get("SystemRoot", r"C:\Windows")
os.environ["PATH"] = os.pathsep.join((
    os.path.join(_system_root, "system32"),
    _system_root,
    os.path.join(_system_root, "System32", "Wbem"),
    os.path.join(_system_root, "System32", "WindowsPowerShell", "v1.0"),
    os.path.join(_system_root, "System32", "OpenSSH"),
))
os.environ["PYTHONNOUSERSITE"] = "1"
os.environ["PYTHONIOENCODING"] = "utf-8"
os.environ["PYTHONUNBUFFERED"] = "1"

_python_path = {python_path}
if _python_path:
    os.environ["PYTHONPATH"] = _python_path

_main_script = {main_script}
os.execve(sys.executable, [sys.executable, _main_script], os.environ)
"#
        );
        tokio::fs::write(&bootstrap_path, bootstrap).await?;
        let arguments = format!("\"{}\"", path::path_to_abs(&bootstrap_path));
        Ok((python_executable, Some(arguments)))
    } else {
        Ok((script_path, None))
    }
}

#[tauri::command]
pub async fn start_app(app_handle: AppHandle, app_name: String) -> Result<(), Error> {
    AUTO_START_CANCELLED.store(true, AtomicOrdering::SeqCst);
    *AUTO_START_CHECKED.lock().await = true;
    info!("Attempting to start app: {}", app_name);
    ensure_app_is_ready_to_start(&app_name).await?;
    let app_dir_lock = get_app_lock(&app_name).await?;
    let _guard = app_dir_lock.lock().await;
    ensure_app_is_ready_to_start(&app_name).await?;

    if !check_python_env_exists(&app_name) {
        warn!(
            "Python .venv not found for '{}'. Deleting app artifacts.",
            &app_name
        );
        drop(_guard);
        delete_app(&app_name).await?;
        emit_error_finish!(&app_name);
        err!(
            "Python .venv was missing for '{}'. App has been reset. Please run setup.",
            app_name
        );
    }

    let (
        profile_to_run_with,
        working_dir,
        current_version,
        app_starting_version,
        update_note,
        configured_icon,
    ) = {
        let mut app_guard = APP.lock().await;
        if let Some(app) = app_guard.as_mut().filter(|app| app.name == app_name) {
            let working_dir = get_app_working_dir_path(&app_name);
            let marker_path = working_dir.join(python_env::PIP_UPDATE_NEEDED_MARKER);
            if marker_path.exists() {
                info!(
                    "Marker file found for app '{}'. Reloading app details before retry.",
                    app_name
                );
                if let Err(e) = load_app_details(app).await {
                    warn!("Failed to reload app details for '{}': {:?}", app_name, e);
                }
            }

            app.last_start = Utc::now();
            let profile_settings = app.get_current_profile_settings().clone();
            let current_version = app.current_version.clone();
            let app_starting_version = app.app_starting_version.clone();
            let update_note = app.update_note.clone();
            let configured_icon = app.icon.clone();
            let app_to_save = app.clone();
            drop(app_guard);

            if let Err(e) = save_app_config_to_json(&app_to_save).await {
                error!(
                    "Failed to save app config for {} after updating last_start: {:?}.",
                    app_name, e
                );
            }
            (
                profile_settings,
                get_app_working_dir_path(&app_name),
                current_version,
                app_starting_version,
                update_note,
                configured_icon,
            )
        } else {
            return Err(anyhow!("App '{}' not found.", app_name).into());
        }
    };

    if profile_to_run_with.main_script.is_empty() {
        return Err(anyhow!(
            "Main script empty for profile '{}' in app '{}'.",
            profile_to_run_with.name,
            app_name
        )
        .into());
    }

    info!(
        "Starting app '{}' (profile '{}', admin: {}, script: '{}')",
        app_name,
        profile_to_run_with.name,
        profile_to_run_with.is_admin(),
        profile_to_run_with.main_script
    );

    let marker_path = working_dir.join(python_env::PIP_UPDATE_NEEDED_MARKER);
    if marker_path.exists() {
        info!(
            "Marker file found for app '{}' at {}. Attempting to re-install requirements.",
            app_name,
            marker_path.display()
        );
        python_env::install_requirements(
            &app_name,
            &profile_to_run_with.requirements,
            &working_dir,
            &profile_to_run_with.pip_args,
        )
        .await?;
    }

    let pyappify_version = app_handle.package_info().version.to_string();
    let envs = build_python_execution_environment(
        &app_name,
        &profile_to_run_with,
        current_version,
        app_starting_version,
        update_note,
        pyappify_version,
    );
    execute_python::run_python_script(
        app_name.as_str(),
        profile_to_run_with.main_script.as_str(),
        &working_dir,
        profile_to_run_with.use_pythonw(),
        envs,
    )
    .await?;

    if check_running_on_start(&app_name).await? {
        emit_info!(
            app_name,
            "App startup confirmed. Creating or updating Windows shortcuts..."
        );
        let icon_path = resolve_app_shortcut_icon_path(&app_name, &configured_icon).await;
        if icon_path.is_none() {
            emit_info!(
                app_name,
                "No Windows-compatible app icon was found; the shortcut will use the target executable icon."
            );
        }
        let (shortcut_target, shortcut_arguments) =
            match build_app_shortcut_command(&app_name, &profile_to_run_with, &working_dir).await {
                Ok(command) => command,
                Err(error) => {
                    emit_error!(app_name, "Failed to prepare the app shortcut: {}", error);
                    return Err(error.into());
                }
            };
        emit_info!(
            app_name,
            "Shortcut target: {} (arguments: {:?}, icon: {:?}, run as admin: {})",
            shortcut_target.display(),
            shortcut_arguments,
            icon_path,
            profile_to_run_with.is_admin()
        );
        if let Err(error) = update_app_shortcuts(
            app_handle,
            app_name.clone(),
            shortcut_target,
            shortcut_arguments,
            working_dir,
            icon_path,
            profile_to_run_with.is_admin(),
        )
        .await
        {
            emit_error!(
                app_name,
                "Failed to create or update Windows shortcuts: {}",
                error
            );
            return Err(error);
        }
        emit_info!(
            app_name,
            "Windows app shortcuts were created or updated successfully."
        );
    } else {
        emit_info!(
            app_name,
            "Skipping shortcut creation because the app process was not detected after startup."
        );
    }
    Ok(())
}

fn try_kill_with_elevation(pid: Pid, app_name: &str) -> Result<()> {
    let pid_str = pid.to_string();
    info!(
        "Elevated kill for PID {} (app '{}'). Prompt may appear.",
        pid_str, app_name
    );

    #[cfg(windows)]
    let cmd = runas::Command::new("taskkill")
        .show(false)
        .args(&["/F", "/PID", &pid_str])
        .status();
    #[cfg(not(windows))]
    let cmd = runas::Command::new("kill")
        .show(false)
        .args(&["-9", &pid_str])
        .force_prompt(true)
        .status();

    match cmd {
        Ok(status) if status.success() => {
            info!("Elevated kill for PID {} success.", pid_str);
            Ok(())
        }
        Ok(status) => bail!(
            "Elevated kill for PID {} failed (code: {}).",
            pid_str,
            status.code().unwrap_or(-1)
        ),
        Err(e) => Err(anyhow::Error::from(e)).context(format!(
            "Failed to launch elevated kill for PID {}",
            pid_str
        )),
    }
}

async fn kill_app_processes(app_name: &str) -> Result<bool> {
    let app_name_clone = app_name.to_string();
    let working_dir_clone = get_app_base_path(app_name);

    task::spawn_blocking(move || -> Result<bool> {
        let mut sys_task = System::new();
        sys_task.refresh_processes(ProcessesToUpdate::All, true);
        debug!(
            "Scanning processes to stop for '{}' in '{}'",
            app_name_clone,
            working_dir_clone.display()
        );
        let pids_to_kill = process::get_pids_related_to_app_dir(&sys_task, &working_dir_clone);
        let targeted_any = !pids_to_kill.is_empty();

        for pid_to_kill in pids_to_kill {
            if let Some(process_to_kill) = sys_task.process(pid_to_kill) {
                info!(
                    "Killing {:?} (PID {}) for app '{}'",
                    process_to_kill.name(),
                    pid_to_kill.as_u32(),
                    app_name_clone
                );
                if !process_to_kill.kill() {
                    warn!(
                        "Standard kill failed for PID {} ('{}'). Attempting elevated.",
                        pid_to_kill.as_u32(),
                        app_name_clone
                    );
                    if let Err(e) = try_kill_with_elevation(pid_to_kill, &app_name_clone) {
                        error!(
                            "Elevated kill for PID {} ('{}') failed: {:?}",
                            pid_to_kill.as_u32(),
                            app_name_clone,
                            e
                        );
                    }
                }
            }
        }
        Ok(targeted_any)
    })
    .await?
}

#[tauri::command]
pub async fn stop_app(app_name: String) -> Result<(), Error> {
    info!("Attempting to stop app: {}", app_name);
    let app_dir_lock = get_app_lock(&app_name).await?;
    let _guard = app_dir_lock.lock().await;

    let any_pids_were_targeted = kill_app_processes(&app_name).await?;

    if any_pids_were_targeted {
        info!("Processes targeted for '{}'. Waiting 1s.", app_name);
        tokio::time::sleep(Duration::from_millis(1000)).await;
    } else {
        info!("No active processes for '{}'.", app_name);
    }

    let mut sys_final = System::new();
    sys_final.refresh_processes(ProcessesToUpdate::All, true);
    let currently_running_final = is_app_running(&sys_final, &app_name);
    let mut status_changed = false;

    {
        let mut app_guard = APP.lock().await;
        if let Some(app) = app_guard.as_mut().filter(|app| app.name == app_name) {
            if app.running != currently_running_final {
                debug!(
                    "Updating running status for '{}' after stop: {} -> {}",
                    app_name, app.running, currently_running_final
                );
                app.running = currently_running_final;
                status_changed = true;
            }
        } else {
            warn!(
                "App '{}' is not loaded during stop_app final update.",
                app_name
            );
        }
    }

    if status_changed {
        emit_app().await;
    }
    if currently_running_final && any_pids_were_targeted {
        warn!("App '{}' may still be running.", app_name);
    }
    Ok(())
}

fn parse_app_preferences(json: &str) -> Result<(String, bool)> {
    let value: serde_json::Value = serde_json::from_str(json)?;
    let update_method = value
        .get("update_method")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| anyhow!("app.json is missing a string update_method"))?;
    if !matches!(
        update_method,
        UPDATE_METHOD_OPTION_MANUAL
            | UPDATE_METHOD_OPTION_AUTO
            | UPDATE_METHOD_OPTION_AUTO_PRE_RELEASE
    ) {
        return Err(anyhow!("Unsupported update method: {}", update_method));
    }
    let auto_start = value
        .get("auto_start")
        .and_then(serde_json::Value::as_bool)
        .ok_or_else(|| anyhow!("app.json is missing a boolean auto_start"))?;
    Ok((update_method.to_string(), auto_start))
}

pub async fn watch_app_config_changes() {
    let mut ticker = interval(Duration::from_millis(500));
    let mut observed_contents: Option<String> = None;
    info!("Starting app.json preference watcher (500ms interval).");

    loop {
        ticker.tick().await;
        let Some(app_name) = APP.lock().await.as_ref().map(|app| app.name.clone()) else {
            observed_contents = None;
            continue;
        };
        let config_path = get_app_config_json_path(&app_name);
        let json = match tokio::fs::read_to_string(&config_path).await {
            Ok(json) => json,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                warn!(
                    "Failed to watch app config {}: {}",
                    config_path.display(),
                    error
                );
                continue;
            }
        };

        if observed_contents.as_ref() == Some(&json) {
            continue;
        }
        observed_contents = Some(json.clone());

        let (update_method, auto_start) = match parse_app_preferences(&json) {
            Ok(preferences) => preferences,
            Err(error) => {
                warn!(
                    "Ignoring invalid preferences in {}: {}",
                    config_path.display(),
                    error
                );
                continue;
            }
        };

        let changed = {
            let mut app_guard = APP.lock().await;
            if let Some(app) = app_guard.as_mut().filter(|app| app.name == app_name) {
                if app.update_method != update_method || app.auto_start != auto_start {
                    if app.auto_start && !auto_start {
                        AUTO_START_CANCELLED.store(true, AtomicOrdering::SeqCst);
                    }
                    app.update_method = update_method;
                    app.auto_start = auto_start;
                    true
                } else {
                    false
                }
            } else {
                false
            }
        };

        if changed {
            info!("Reloaded preferences from app.json for '{}'.", app_name);
            emit_app().await;
        }
    }
}

pub async fn periodically_update_app_running_status(app_handle: AppHandle) {
    let mut ticker = interval(Duration::from_secs(2));
    info!("Starting periodic app status update (2s interval).");
    let mut sys = System::new();
    loop {
        ticker.tick().await;
        if let Some(window) = app_handle.get_webview_window("main") {
            if !window.is_visible().unwrap_or(false) {
                continue;
            }
        }
        sys.refresh_processes(ProcessesToUpdate::All, true);
        let Some(app_name) = APP.lock().await.as_ref().map(|app| app.name.clone()) else {
            continue;
        };
        let new_status = is_app_running(&sys, &app_name);

        let changed = {
            let mut app_guard = APP.lock().await;
            if let Some(app) = app_guard.as_mut().filter(|app| app.name == app_name) {
                if app.running != new_status {
                    debug!(
                        "Periodic: Running status for '{}': {} -> {}",
                        app.name, app.running, new_status
                    );
                    app.running = new_status;
                    true
                } else {
                    false
                }
            } else {
                false
            }
        };
        if changed {
            info!("App status changed by periodic check. Emitting.");
            emit_app().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        backup_invalid_app_config, build_python_execution_environment, get_update_target,
        icon_mime_type, is_invalid_json_error, parse_app_preferences,
        resolve_current_version_state,
    };
    use crate::app::{Profile, UPDATE_METHOD_OPTION_AUTO, UPDATE_METHOD_OPTION_AUTO_PRE_RELEASE};
    use std::path::Path;

    fn versions(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn identifies_only_json_syntax_and_eof_errors_as_regeneratable() {
        let syntax_error =
            anyhow::Error::new(serde_json::from_str::<serde_json::Value>("{").unwrap_err());
        assert!(is_invalid_json_error(&syntax_error));

        let data_error = anyhow::Error::new(
            serde_json::from_str::<std::collections::HashMap<String, String>>(
                r#"{"preference":1}"#,
            )
            .unwrap_err(),
        );
        assert!(!is_invalid_json_error(&data_error));
    }

    #[tokio::test]
    async fn backs_up_invalid_app_json_without_changing_the_original() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "pyappify-invalid-app-json-{}-{unique}",
            std::process::id()
        ));
        tokio::fs::create_dir_all(&root).await.unwrap();
        let config_path = root.join("app.json");
        tokio::fs::write(&config_path, "{invalid").await.unwrap();

        let backup_path = backup_invalid_app_config(&config_path).await.unwrap();

        assert_eq!(
            tokio::fs::read_to_string(&config_path).await.unwrap(),
            "{invalid"
        );
        assert_eq!(
            tokio::fs::read_to_string(&backup_path).await.unwrap(),
            "{invalid"
        );
        assert_eq!(backup_path.parent(), Some(root.as_path()));
        assert!(backup_path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("app.json.invalid-"));

        tokio::fs::remove_dir_all(root).await.unwrap();
    }

    #[test]
    fn parses_preferences_written_by_the_python_api() {
        assert_eq!(
            parse_app_preferences(
                r#"{"name":"example","update_method":"AUTO_UPDATE_PRE_RELEASE","auto_start":true}"#
            )
            .unwrap(),
            (UPDATE_METHOD_OPTION_AUTO_PRE_RELEASE.to_string(), true)
        );
    }

    #[test]
    fn python_environment_contains_absolute_app_json_path() {
        let profile = Profile {
            name: "default".to_string(),
            main_script: "main.py".to_string(),
            admin: None,
            use_pythonw: None,
            show_add_defender: None,
            requirements: String::new(),
            python_path: String::new(),
            git_url: String::new(),
            requires_python: String::new(),
            pip_args: String::new(),
        };
        let env = build_python_execution_environment(
            "example",
            &profile,
            None,
            None,
            Vec::new(),
            "0.1.0".to_string(),
        );
        let app_json_path = env
            .iter()
            .find(|(name, _)| name == "PYAPPIFY_APP_JSON_PATH")
            .map(|(_, value)| Path::new(value))
            .unwrap();

        assert!(app_json_path.is_absolute());
        assert!(app_json_path.ends_with(Path::new("data/apps/example/app.json")));
        assert_eq!(
            env.iter()
                .find(|(name, _)| name == "PYAPPIFY_LOCALE")
                .map(|(_, value)| value.as_str()),
            Some("en")
        );
    }

    #[test]
    fn recognizes_supported_icon_formats_case_insensitively() {
        assert_eq!(
            icon_mime_type(Path::new("assets/app.PNG")),
            Some("image/png")
        );
        assert_eq!(
            icon_mime_type(Path::new("assets/app.jpeg")),
            Some("image/jpeg")
        );
        assert_eq!(
            icon_mime_type(Path::new("assets/app.svg")),
            Some("image/svg+xml")
        );
        assert_eq!(icon_mime_type(Path::new("assets/app.txt")), None);
    }

    #[test]
    fn resolves_current_version_tag_normally() {
        let (current_version, current_version_missing) = resolve_current_version_state(
            Some("v1.0.0".to_string()),
            &versions(&["v1.1.0", "v1.0.0"]),
            "v1.0.0".to_string(),
        );

        assert_eq!(current_version, Some("v1.0.0".to_string()));
        assert!(!current_version_missing);
    }

    #[test]
    fn does_not_save_commit_hash_as_current_version() {
        let (current_version, current_version_missing) = resolve_current_version_state(
            Some("7fa243f331892d478c4e450f6215495ca3b48258".to_string()),
            &versions(&["v1.1.0", "v1.0.0"]),
            "7fa243f331892d478c4e450f6215495ca3b48258".to_string(),
        );

        assert_eq!(current_version, None);
        assert!(current_version_missing);
    }

    #[test]
    fn preserves_previous_version_when_head_no_longer_matches_it() {
        let (current_version, current_version_missing) = resolve_current_version_state(
            Some("v1.0.0".to_string()),
            &versions(&["v1.1.0"]),
            "7fa243f331892d478c4e450f6215495ca3b48258".to_string(),
        );

        assert_eq!(current_version, Some("v1.0.0".to_string()));
        assert!(current_version_missing);
    }

    #[test]
    fn stable_auto_update_ignores_newer_prereleases() {
        let available = versions(&["v2.0.0-beta.1", "v1.9.0", "v1.8.0"]);

        assert_eq!(
            get_update_target(&available, UPDATE_METHOD_OPTION_AUTO),
            available.get(1)
        );
    }

    #[test]
    fn prerelease_auto_update_uses_newest_version() {
        let available = versions(&["v1.9.0", "v2.0.0-beta.1", "v2.0.0-alpha.2"]);

        assert_eq!(
            get_update_target(&available, UPDATE_METHOD_OPTION_AUTO_PRE_RELEASE),
            available.get(1)
        );
    }
}
