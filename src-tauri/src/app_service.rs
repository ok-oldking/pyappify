//src/app_service.rs
use crate::{
    app::{read_embedded_app, update_app_from_yml, Profile, YML_FILE_NAME},
    emit_error_finish, emit_info, emit_success_finish, emitter, err, execute_python, git,
    python_env,
    utils::path,
    utils::process,
};
use anyhow::{anyhow, bail, Context, Result};
use chrono::Utc;
use futures::future::join_all;
use once_cell::sync::Lazy;
use runas;
use std::{collections::HashMap, fs, path::{Path, PathBuf}, sync::Arc};
use sysinfo::{Pid, ProcessesToUpdate, System};
use tauri::{AppHandle, Manager};
use tokio::sync::Mutex;
use tokio::task;
use tokio::task::JoinHandle;
use tokio::time::{interval, Duration};
use tracing::{debug, error, info, warn};
use crate::app::App;
use crate::git::ensure_repository;
use crate::utils::error::Error;
use crate::utils::file;
use crate::utils::path::{get_app_base_path, get_app_working_dir_path};

pub static APPS: Lazy<Mutex<HashMap<String, App>>> = Lazy::new(|| Mutex::new(HashMap::new()));
pub static APP_DIR_LOCKS: Lazy<Mutex<HashMap<String, Arc<Mutex<()>>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

fn is_app_running(sys: &System, app_working_dir: &Path) -> bool {
    !process::get_pids_related_to_app_dir(sys, &PathBuf::from(app_working_dir)).is_empty()
}

fn get_app_config_json_path(app_name: &str) -> PathBuf {
    get_app_base_path(app_name).join("app.json")
}

async fn save_app_config_to_json(app: &App) -> Result<()> {
    let config_path = get_app_config_json_path(&app.name);
    let json_data = serde_json::to_string_pretty(app)
        .with_context(|| format!("Failed to serialize app config for {}", app.name))?;
    if let Some(parent) = config_path.parent() {
        tokio::fs::create_dir_all(parent).await.with_context(|| {
            format!(
                "Failed to create parent directory for app.json for {}",
                app.name
            )
        })?;
    }
    tokio::fs::write(&config_path, json_data)
        .await
        .with_context(|| format!("Failed to write app.json for {}", app.name))?;
    debug!(
        "Saved app config for {} to {}",
        app.name,
        config_path.display()
    );
    Ok(())
}

async fn load_app_config_from_json(app_name: &str) -> Result<Option<App>> {
    let config_path = get_app_config_json_path(app_name);
    if !config_path.exists() {
        return Ok(None);
    }
    let json_data = tokio::fs::read_to_string(&config_path)
        .await
        .with_context(|| format!("Failed to read app.json for {}", app_name))?;

    match serde_json::from_str::<App>(&json_data) {
        Ok(mut app) => {
            if app.name != app_name {
                warn!("App name mismatch in app.json ('{}') and directory ('{}'). Correcting to directory name: '{}'.", app.name, app_name, app_name);
                app.name = app_name.to_string();
            }
            Ok(Some(app))
        }
        Err(e) => {
            error!(
                "Failed to deserialize app.json for {}: {}. Content sample: {}",
                app_name,
                e,
                json_data.chars().take(200).collect::<String>()
            );
            Err(anyhow!(
                "Failed to deserialize app.json for {}: {}",
                app_name,
                e
            ))
        }
    }
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

pub async fn get_apps_as_vec() -> Vec<App> {
    let mut apps_vec: Vec<App> = APPS.lock().await.values().cloned().collect();
    apps_vec.sort_unstable_by(|a, b| {
        b.running
            .cmp(&a.running)
            .then_with(|| b.last_start.cmp(&a.last_start))
    });
    apps_vec
}

pub(crate) async fn get_app_lock(app_name: &str) -> Arc<Mutex<()>> {
    let mut locks = APP_DIR_LOCKS.lock().await;
    locks
        .entry(app_name.to_string())
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone()
}

async fn cleanup_stale_app_directories(app_name: &str) -> Result<()> {
    if let Some(apps_dir) = get_app_base_path(app_name).parent() {
        if apps_dir.exists() {
            let mut entries = tokio::fs::read_dir(apps_dir)
                .await
                .with_context(|| format!("Failed to read apps directory: {}", apps_dir.display()))?;
            while let Some(entry) = entries.next_entry().await? {
                if entry.file_type().await?.is_dir() {
                    let dir_name = entry.file_name().to_string_lossy().into_owned();
                    if dir_name != app_name {
                        let full_path = entry.path();
                        info!(
                            "Removing stale application directory: {}",
                            full_path.display()
                        );
                        if let Err(e) = tokio::fs::remove_dir_all(&full_path).await {
                            warn!(
                                "Failed to remove stale app directory {}: {}",
                                full_path.display(),
                                e
                            );
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

async fn load_and_prepare_app_state(app_template: &App) -> Result<App> {
    let app_name = &app_template.name;
    let mut app = match load_app_config_from_json(app_name).await {
        Ok(Some(mut app_from_disk)) => {
            info!("Loaded app '{}' from app.json.", app_name);
            let mut sys = System::new();
            sys.refresh_processes(ProcessesToUpdate::All, true);
            let working_dir = get_app_working_dir_path(app_name);
            app_from_disk.running = is_app_running(&sys, &working_dir);
            let current_profile = app_from_disk.current_profile.clone();
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
        Err(e) => return Err(e.into()),
    };

    info!(
        "Loading full app details (git info, yml) for {}...",
        app.name
    );
    let repo_path = path::get_app_repo_path(&app.name);
    match git::get_tags_and_current_version(&app.name, repo_path.clone()).await {
        Ok((versions, current)) => {
            app.available_versions = versions;
            app.current_version = Some(current);
        }
        Err(e) => {
            warn!(
                "Failed to get repository versions for {}: {}",
                app.name, e
            );
        }
    };

    load_app_details(&mut app).await?;
    save_app_config_to_json(&app).await?;
    Ok(app)
}

#[tauri::command]
pub async fn load_apps() -> Result<Vec<App>, Error> {
    {
        let apps_map = APPS.lock().await;
        if !apps_map.is_empty() {
            info!("App already loaded. Triggering update from disk.");
            drop(apps_map);
            update_apps_from_disk().await?;
            return Ok(get_apps_as_vec().await);
        }
    }

    let app_template = read_embedded_app();
    cleanup_stale_app_directories(&app_template.name).await?;
    info!(
        "Loading the single, embedded application. profiles {:?}",
        app_template.profiles
    );

    let app = load_and_prepare_app_state(&app_template).await?;
    info!("Finished loading app details. {} {}", app.name, app.installed);

    APPS.lock().await.insert(app.name.clone(), app);
    update_apps_from_disk().await?;
    emit_apps().await;
    Ok(get_apps_as_vec().await)
}

async fn update_apps_from_disk() -> Result<(), Error> {
    let app_names_for_details: Vec<String> = APPS.lock().await.keys().cloned().collect();
    info!(
        "Phase 2: Loading full app details (git info, yml) for {} apps...",
        app_names_for_details.len()
    );

    let mut load_detail_tasks: Vec<JoinHandle<Result<(), Error>>> = Vec::new();
    for app_name_for_detail_load in app_names_for_details {
        let task_app_name = app_name_for_detail_load.clone();
        load_detail_tasks.push(tokio::spawn(async move {
            let app_dir_lock = get_app_lock(&task_app_name).await;
            let _guard = app_dir_lock.lock().await;
            let full_load_logic = async {
                let mut app = get_app_by_name(&task_app_name).await?;
                let repo_path = path::get_app_repo_path(&app.name);
                let (versions, current) =
                    git::get_tags_and_current_version(&app.name, repo_path.clone()).await?;
                app.available_versions = versions;
                app.current_version = Some(current);
                load_app_details(&mut app).await?;
                save_app_config_to_json(&app).await?;
                let mut apps = APPS.lock().await;
                apps.insert(task_app_name.clone(), app);
                Ok(())
            };
            full_load_logic.await
        }));
    }

    let results = join_all(load_detail_tasks).await;
    for result in results {
        match result {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                error!("App detail load task resulted in an error: {:?}", e);
            }
            Err(join_error) => {
                error!("App detail load task panicked: {:?}", join_error);
            }
        }
    }

    emit_apps().await;
    Ok(())
}

#[tauri::command]
pub async fn delete_app(app_name: &str) -> Result<(), Error> {
    info!("Attempting to delete app: {}", app_name);
    let app_dir_lock = get_app_lock(app_name).await;
    let _guard = app_dir_lock.lock().await;

    let app_base_path = get_app_base_path(app_name);
    if app_base_path.exists() {
        info!("Deleting dir: {}", app_base_path.display());
        tokio::fs::remove_dir_all(&app_base_path)
            .await
            .with_context(|| format!("Failed to delete dir {}", app_base_path.display()))?;
        info!("Deleted dir: {}", app_base_path.display());
    } else {
        info!("App dir {} not on disk.", app_base_path.display());
    }
    let mut app: App = get_app_by_name(app_name).await?;
    app.installed = false;
    save_app_config_to_json(&app).await?;
    APPS.lock().await.insert(app_name.to_string(), app);
    emit_apps().await;
    Ok(())
}

pub(crate) async fn emit_apps() {
    emitter::emit("apps", get_apps_as_vec().await);
}

#[tauri::command]
pub async fn get_update_notes(app_name: String, version: String) -> Result<Vec<String>, Error> {
    let app_lock = get_app_lock(&*app_name).await;
    let _guard = app_lock.lock().await;
    let app = get_app_by_name(&app_name).await?;
    let messages = git::get_commit_messages_for_version_diff(&app.get_repo_path(), &version).await?;
    info!("get_update_notes for {} version {} messages: {:?}", app.name, version, messages);
    Ok(messages)
}

async fn get_app_by_name(app_name: &str) -> Result<App, Error> {
    let app = APPS
        .lock()
        .await
        .get(app_name)
        .cloned()
        .ok_or_else(|| anyhow!("App '{}' not found.", app_name))?;
    Ok(app)
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
        file::copy_dir_recursive_excluding_sync(
            &task_repo_path,
            &task_working_dir_path,
            &[".git"],
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
pub async fn setup_app(app_name: &str, profile_name: &str) -> Result<PathBuf, Error> {
    let app_dir_lock = get_app_lock(app_name).await;
    let _guard = app_dir_lock.lock().await;

    let repo_path = path::get_app_repo_path(app_name);
    let app = get_app_by_name(app_name).await?;

    ensure_repository(&app).await?;

    let working_dir_path = get_app_working_dir_path(app_name);
    if !repo_path.exists() {
        err!("Repo for {} not at {}", app_name, repo_path.display());
    }

    if working_dir_path.exists() {
        info!(
            "Removing existing working dir: {}",
            working_dir_path.display()
        );
        tokio::fs::remove_dir_all(&working_dir_path)
            .await
            .with_context(|| format!("Failed to remove dir {}", working_dir_path.display()))?;
    }
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

    let venv_python_exe = task::spawn_blocking({
        let wd_path = working_dir_path.clone();
        let py_spec = python_version_spec.to_string();
        let app_name_clone = app_name.to_string();
        move || python_env::setup_python_venv(app_name_clone, &wd_path, &py_spec).map_err(|e| anyhow!(e))
    })
        .await??;

    if !requirements.is_empty() {
        python_env::install_requirements(
            app_name,
            &venv_python_exe,
            requirements,
            &working_dir_path,
            pip_args,
        ).await?;
    } else {
        info!(
            "No reqs in profile '{}' of {}. Skipping sync.",
            final_profile_name_to_set, YML_FILE_NAME
        );
    }

    let mut apps_map = APPS.lock().await;
    if let Some(app) = apps_map.get_mut(app_name) {
        load_app_details(app).await?;
        app.installed = true;
        app.current_profile = final_profile_name_to_set.clone();
        let app_to_save = app.clone();
        drop(apps_map);

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
        emit_apps().await;
    } else {
        warn!(
            "App {} not found in APPS map after setup, cannot mark as installed or set profile.",
            app_name
        );
    }

    emit_success_finish!(app_name);
    Ok(venv_python_exe)
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

#[tauri::command]
pub async fn update_to_version(app_name: &str, version: &str) -> Result<(), Error> {
    info!("Updating {} to version {}", app_name, version);
    let app_dir_lock = get_app_lock(app_name).await;
    let _lock_guard = app_dir_lock.lock().await;

    let working_dir_path = path::get_app_working_dir_path(app_name);

    let old_requirements_spec = {
        let apps = APPS.lock().await;
        apps.get(app_name)
            .map(|app| app.get_current_profile_settings().requirements.clone())
            .unwrap_or_default()
    };
    let old_content = get_relevant_content(&old_requirements_spec, &working_dir_path);

    let repo_path = path::get_app_repo_path(app_name);
    let commit_oid = git::checkout_version_tag(app_name, &repo_path, version).await?;
    emit_info!(
        app_name,
        "Checked out commit {} for version {}",
        commit_oid,
        version
    );
    update_working_from_repo(app_name).await?;
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
            emit_info!(app_name, "Requirements spec changed from '{}' to '{}'. Syncing dependencies.", old_requirements_spec, new_requirements_spec);
        } else {
            let file_type = if new_requirements_spec.ends_with(".txt") {
                &new_requirements_spec
            } else {
                "pyproject.toml"
            };
            emit_info!(app_name, "Content of '{}' changed. Syncing dependencies.", file_type);
        }
        let venv_python_exe = working_dir_path.join(".venv").join(if cfg!(windows) { "Scripts\\python.exe" } else { "bin/python" });
        if venv_python_exe.exists() {
            python_env::install_requirements(
                app_name,
                &venv_python_exe,
                &new_requirements_spec,
                &working_dir_path,
                &new_pip_args,
            ).await?;
        } else {
            warn!("Venv python not found at {}. Skipping dependency sync.", venv_python_exe.display());
        }
    } else {
        emit_info!(app_name, "Requirements are up to date. Skipping dependency sync.");
    }

    {
        let mut apps = APPS.lock().await;
        if let Some(app) = apps.get_mut(app_name) {
            load_app_details(app).await?;
            app.current_version = Some(version.to_string());
            let app_to_save = app.clone();
            drop(apps);
            save_app_config_to_json(&app_to_save).await?;
        }
    }

    emit_info!(app_name, "Updated {} to version {}", app_name, version);
    emit_success_finish!(app_name);
    emit_apps().await;
    Ok(())
}

fn build_python_execution_environment(
    profile: &Profile,
    current_version: Option<String>,
) -> Vec<(String, String)> {
    let mut envs = Vec::new();
    if !profile.python_path.is_empty() {
        envs.push(("PYTHONPATH".to_string(), profile.python_path.clone()));
    }
    if let Some(version) = current_version {
        envs.push(("PYAPPIFY_APP_VERSION".to_string(), version));
    }
    envs.push(("PYAPPIFY_APP_PROFILE".to_string(), profile.name.clone()));
    envs.push(("PYAPPIFY_PID".to_string(), std::process::id().to_string()));
    envs.push(("PYAPPIFY_UPGRADEABLE".to_string(), 1.to_string()));
    envs.push((
        "PYAPPIFY_VERSION".to_string(),
        env!("CARGO_PKG_VERSION").to_string(),
    ));
    if let Ok(exe_path) = std::env::current_exe() {
        envs.push((
            "PYAPPIFY_EXECUTABLE".to_string(),
            exe_path.to_string_lossy().to_string(),
        ));
    }
    envs
}

async fn check_running_on_start(
    app_name: &str,
    working_dir: &Path,
) -> Result<()> {
    let start_time = tokio::time::Instant::now();
    let timeout = Duration::from_secs(10);
    let mut interval = tokio::time::interval(Duration::from_secs(1));
    let mut sys = System::new();

    info!(
        "Monitoring for app '{}' to start for up to 10 seconds...",
        app_name
    );

    while tokio::time::Instant::now().duration_since(start_time) < timeout {
        interval.tick().await;
        sys.refresh_processes(ProcessesToUpdate::All, true);
        let pids = process::get_pids_related_to_app_dir(&sys, &working_dir.to_path_buf());
        if !pids.is_empty() {
            info!(
                "App '{}' detected as running with a visible window. Updating status and minimizing main window.",
                app_name
            );

            let mut apps_map = APPS.lock().await;
            if let Some(app) = apps_map.get_mut(app_name) {
                app.running = true;
            }
            drop(apps_map);

            emit_apps().await;
            return Ok(());
        }
    }

    warn!(
        "App '{}' did not appear to be running with a visible window within 10 seconds.",
        app_name
    );
    sys.refresh_processes(ProcessesToUpdate::All, true);
    let is_running_after_timeout = is_app_running(&sys, working_dir);
    let mut apps_map = APPS.lock().await;
    if let Some(app) = apps_map.get_mut(app_name) {
        if app.running != is_running_after_timeout {
            app.running = is_running_after_timeout;
            drop(apps_map);
            emit_apps().await;
        }
    }

    Ok(())
}

#[tauri::command]
pub async fn start_app(app_name: String) -> Result<(), Error> {
    info!("Attempting to start app: {}", app_name);
    let app_dir_lock = get_app_lock(&app_name).await;
    let _guard = app_dir_lock.lock().await;

    let (profile_to_run_with, working_dir, current_version) = {
        let mut apps_map = APPS.lock().await;
        if let Some(app) = apps_map.get_mut(&app_name) {
            app.last_start = Utc::now();
            let profile_settings = app.get_current_profile_settings().clone();
            let current_version = app.current_version.clone();
            let app_to_save = app.clone();
            drop(apps_map);

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

    let venv_path = working_dir.join(".venv");
    let python_exe_in_venv = venv_path.join(if cfg!(windows) {
        "Scripts\\python.exe"
    } else {
        "bin/python"
    });

    if !venv_path.exists() || !python_exe_in_venv.exists() {
        emit_error_finish!(app_name);
        return Err(anyhow!(
            "Python .venv not at '{}' for '{}'. Try setup.",
            venv_path.display(),
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

    let envs = build_python_execution_environment(&profile_to_run_with, current_version);
    execute_python::run_python_script(
        app_name.as_str(),
        &venv_path,
        profile_to_run_with.main_script.as_str(),
        &working_dir,
        profile_to_run_with.is_admin(),
        envs,
    )
        .await?;

    check_running_on_start(&app_name, &working_dir).await?;

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
        Err(e) => Err(anyhow::Error::from(e))
            .context(format!("Failed to launch elevated kill for PID {}", pid_str)),
    }
}

async fn kill_app_processes(app_name: &str, working_dir: &Path) -> Result<bool> {
    let app_name_clone = app_name.to_string();
    let working_dir_clone = working_dir.to_path_buf();

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
    let app_dir_lock = get_app_lock(&app_name).await;
    let _guard = app_dir_lock.lock().await;

    let working_dir = get_app_working_dir_path(&app_name);
    let any_pids_were_targeted = kill_app_processes(&app_name, &working_dir).await?;

    if any_pids_were_targeted {
        info!("Processes targeted for '{}'. Waiting 1s.", app_name);
        tokio::time::sleep(Duration::from_millis(1000)).await;
    } else {
        info!("No active processes for '{}'.", app_name);
    }

    let mut sys_final = System::new();
    sys_final.refresh_processes(ProcessesToUpdate::All, true);
    let currently_running_final = is_app_running(&sys_final, &working_dir);
    let mut status_changed = false;

    {
        let mut apps_map = APPS.lock().await;
        if let Some(app) = apps_map.get_mut(&app_name) {
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
                "App '{}' not in APPS map during stop_app final update.",
                app_name
            );
        }
    }

    if status_changed {
        emit_apps().await;
    }
    if currently_running_final && any_pids_were_targeted {
        warn!("App '{}' may still be running.", app_name);
    }
    Ok(())
}

pub async fn periodically_update_all_apps_running_status(app_handle: AppHandle) {
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
        let apps_to_check_data: Vec<(String, PathBuf)> = APPS
            .lock()
            .await
            .keys()
            .map(|name| (name.clone(), get_app_working_dir_path(name)))
            .collect();

        if apps_to_check_data.is_empty() {
            continue;
        }

        let mut status_updates_list: Vec<(String, bool)> = Vec::new();
        for (app_name, app_working_dir) in &apps_to_check_data {
            status_updates_list.push((app_name.clone(), is_app_running(&sys, app_working_dir)));
        }

        let mut changed_any_status = false;
        if !status_updates_list.is_empty() {
            let mut apps_map = APPS.lock().await;
            for (app_name, new_status) in status_updates_list {
                if let Some(app_in_map) = apps_map.get_mut(&app_name) {
                    if app_in_map.running != new_status {
                        debug!(
                            "Periodic: Running status for '{}': {} -> {}",
                            app_in_map.name, app_in_map.running, new_status
                        );
                        app_in_map.running = new_status;
                        changed_any_status = true;
                    }
                }
            }
        }
        if changed_any_status {
            info!("App status changed by periodic check. Emitting.");
            emit_apps().await;
        }
    }
}
