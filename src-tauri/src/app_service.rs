//src/app_service.rs
use crate::{
    app::{read_embedded_app, update_app_from_yml, YML_FILE_NAME},
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
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};
use sysinfo::{Pid, ProcessesToUpdate, System};
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

#[tauri::command]
pub async fn load_apps() -> Result<Vec<App>, Error> {
    {
        let apps_map = APPS.lock().await;
        if !apps_map.is_empty() {
            info!("App already loaded. Triggering update from disk.");
            drop(apps_map);
            update_apps_from_disk().await?;
            let apps_list: Vec<App> = APPS.lock().await.values().cloned().collect();
            return Ok(apps_list)
        }
    }

    let app_template = read_embedded_app();
    let app_name = app_template.name.clone();
    info!(
        "Loading the single, embedded application. profiles {:?}",
        app_template.profiles
    );

    let mut app = match load_app_config_from_json(&app_name).await {
        Ok(Some(mut app_from_disk)) => {
            info!("Loaded app '{}' from app.json.", app_name);
            let mut sys = System::new();
            sys.refresh_processes(ProcessesToUpdate::All, true);
            let working_dir = get_app_working_dir_path(&app_name);
            app_from_disk.running = is_app_running(&sys, &working_dir);
            let current_profile = app_from_disk.current_profile.clone();
            app_from_disk.profiles = app_template.profiles;
            app_from_disk.current_profile = current_profile;
            app_from_disk
        }
        Ok(None) => {
            info!(
                "app.json for '{}' not found. Creating from embedded template.",
                app_name
            );
            save_app_config_to_json(&app_template).await?;
            app_template
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
            app.available_versions = Vec::new();
            app.current_version = None;
        }
    };

    load_app_details(&mut app).await?;
    save_app_config_to_json(&app).await?;
    info!("Finished loading app details. {} {}", app.name, app.installed);
    APPS.lock().await.insert(app.name.clone(), app);
    update_apps_from_disk().await?;
    emit_apps().await;
    let apps_list: Vec<App> = APPS.lock().await.values().cloned().collect();
    Ok(apps_list)
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
    let mut app: App = get_app_by_name(&app_name).await?;
    app.installed = false;
    save_app_config_to_json(&app).await?;
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
    Ok(git::get_commit_messages_for_version_diff(&app.get_repo_path(), &version).await?)
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
    let temp_app_config = &temp_app_for_config;

    let final_profile_name_to_set: String;
    let profile_settings_for_setup = match temp_app_config.get_profile(profile_name) {
        Some(profile) => {
            final_profile_name_to_set = profile_name.to_string();
            profile
        }
        None => {
            if profile_name != "default" {
                warn!(
                    "Profile '{}' not found for setup in app '{}'. Falling back to 'default' profile.",
                    profile_name, app_name
                );
            }
            final_profile_name_to_set = "default".to_string();
            temp_app_config.get_profile("default").ok_or_else(|| {
                anyhow!(
                    "Profile '{}' (and fallback 'default') not found in {} for app {}",
                    profile_name,
                    YML_FILE_NAME,
                    app_name
                )
            })?
        }
    };
    let requirements_relative_path = &profile_settings_for_setup.requirements;
    let python_version_spec = &profile_settings_for_setup.requires_python;
    let app_name_clone = app_name.to_string();

    let venv_python_exe = task::spawn_blocking({
        let wd_path = working_dir_path.clone();
        let py_spec = python_version_spec.to_string();
        move || python_env::setup_python_venv(app_name_clone, &wd_path, &py_spec).map_err(|e| anyhow!(e))
    })
        .await??;

    if !requirements_relative_path.is_empty() {
        let full_req_path = working_dir_path.join(requirements_relative_path);
        if full_req_path.exists() {
            info!(
                "Requirements '{}' for profile '{}' found. Syncing.",
                requirements_relative_path, final_profile_name_to_set
            );
            python_env::install_requirements(
                app_name,
                &venv_python_exe,
                &full_req_path,
                &working_dir_path,
            )
                .await?;
        } else {
            emit_error_finish!(app_name);
            err!(
                "Reqs '{}' (profile '{}') in {} not at {}. Skipping.",
                requirements_relative_path,
                final_profile_name_to_set,
                YML_FILE_NAME,
                full_req_path.display()
            );
        }
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
        info!("App config json saved successfully after setup {} installed {}", app_to_save.name, app_to_save.installed);
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

#[tauri::command]
pub async fn update_to_version(app_name: &str, version: &str) -> Result<(), Error> {
    info!("Updating {} to version {}", app_name, version);
    let app_dir_lock = get_app_lock(app_name).await;
    let _lock_guard = app_dir_lock.lock().await;

    let repo_path = path::get_app_repo_path(app_name);
    let working_dir_path = get_app_working_dir_path(app_name);

    let old_req_info: Option<(PathBuf, String)> = {
        let apps_guard = APPS.lock().await;
        if let Some(app) = apps_guard.get(app_name) {
            let profile_settings = app.get_current_profile_settings();
            if !profile_settings.requirements.is_empty() {
                let old_req_path = working_dir_path.join(&profile_settings.requirements);
                if old_req_path.exists() {
                    tokio::fs::read_to_string(&old_req_path)
                        .await
                        .ok()
                        .map(|content| (old_req_path, content))
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        }
    };
    let old_req_path = old_req_info.as_ref().map(|(p, _)| p.clone());
    let old_req_content = old_req_info.map(|(_, c)| c);

    let commit_oid = git::checkout_version_tag(app_name, &repo_path, version)
        .await
        .map_err(|e| anyhow!(e))?;
    emit_info!(
        app_name,
        "Checked out commit {} for version {}",
        commit_oid,
        version
    );

    update_working_from_repo(app_name).await?;
    debug!("Updated working dir for app {}", app_name);

    let yml_path_new = working_dir_path.join(YML_FILE_NAME);
    let yml_path_str_new = yml_path_new.to_string_lossy().into_owned();

    let mut temp_app_for_config = read_embedded_app();
    temp_app_for_config.name = app_name.to_string();
    update_app_from_yml(&mut temp_app_for_config, &yml_path_str_new);
    let new_app_config_from_yml = &temp_app_for_config;

    let new_default_profile = new_app_config_from_yml
        .get_profile("default")
        .ok_or_else(|| {
            anyhow!(
                "Default profile missing in new {} for {}",
                YML_FILE_NAME,
                app_name
            )
        })?;
    let new_requirements_relative_path = &new_default_profile.requirements;

    let new_req_path_for_sync = if !new_requirements_relative_path.is_empty() {
        Some(working_dir_path.join(new_requirements_relative_path))
    } else {
        None
    };
    let new_req_content = if let Some(ref p) = new_req_path_for_sync {
        if p.exists() {
            tokio::fs::read_to_string(p).await.ok()
        } else {
            None
        }
    } else {
        None
    };

    let mut needs_pip_sync = false;
    if new_req_path_for_sync.is_some() && new_req_content.is_some() {
        if old_req_path.as_ref() != new_req_path_for_sync.as_ref()
            || old_req_content != new_req_content
        {
            emit_info!(
                app_name,
                "Reqs file/path changed (now '{}'). Syncing.",
                new_requirements_relative_path
            );
            needs_pip_sync = true;
        } else {
            emit_info!(
                app_name,
                "Reqs file ('{}') unchanged. Skipping sync.",
                new_requirements_relative_path
            );
        }
    } else if old_req_path.is_some() && old_req_content.is_some() {
        emit_info!(
            app_name,
            "Reqs file removed/empty for {}. Not re-syncing.",
            app_name
        );
    } else if new_req_path_for_sync.is_some()
        && new_req_content.is_none()
        && !new_requirements_relative_path.is_empty()
    {
        warn!(
            "New reqs file '{}' specified but empty/not found. Skipping sync.",
            new_requirements_relative_path
        );
    } else {
        emit_info!(app_name, "No significant reqs file changes. Skipping sync.");
    }

    if needs_pip_sync {
        if let Some(ref sync_path) = new_req_path_for_sync {
            if sync_path.exists() {
                let venv_python_exe = working_dir_path.join(".venv").join(if cfg!(windows) {
                    "Scripts\\python.exe"
                } else {
                    "bin/python"
                });
                if !venv_python_exe.exists() {
                    return Err(anyhow!(
                        "Python exe not at {} for {}. Venv corrupted.",
                        venv_python_exe.display(),
                        app_name
                    )
                        .into());
                }
                python_env::install_requirements(
                    app_name,
                    &venv_python_exe,
                    &sync_path,
                    &working_dir_path,
                )
                    .await?;
            } else {
                warn!(
                    "Reqs file {} expected for sync but not found. Skipping.",
                    sync_path.display()
                );
            }
        }
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
            drop(apps_map); // Drop lock before async I/O

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

    let main_script_relative = &profile_to_run_with.main_script;
    if main_script_relative.is_empty() {
        return Err(anyhow!(
            "Main script empty for profile '{}' in app '{}'.",
            profile_to_run_with.name,
            app_name
        )
            .into());
    }

    let script_path = find_main_script(&app_name, &working_dir, main_script_relative)?;

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
        main_script_relative
    );

    let mut envs = Vec::new();
    if !profile_to_run_with.python_path.is_empty() {
        envs.push((
            "PYTHONPATH".to_string(),
            profile_to_run_with.python_path.clone(),
        ));
    }
    if let Some(version) = current_version {
        envs.push(("PYAPPIFY_APP_VERSION".to_string(), version));
    }
    envs.push((
        "PYAPPIFY_APP_PROFILE".to_string(),
        profile_to_run_with.name.clone(),
    ));

    envs.push(("PYAPPIFY_PID".to_string(), std::process::id().to_string()));
    envs.push(("PYAPPIFY_UPGRADEABLE".to_string(), 1.to_string()));

    execute_python::run_python_script(
        app_name.as_str(),
        &venv_path,
        &script_path,
        &working_dir,
        profile_to_run_with.is_admin(),
        envs,
    )
        .await?;

    let mut sys = System::new();
    sys.refresh_processes(ProcessesToUpdate::All, true);
    let currently_running = is_app_running(&sys, &working_dir);
    let mut status_changed = false;

    {
        let mut apps_map = APPS.lock().await;
        if let Some(app) = apps_map.get_mut(&app_name) {
            if app.running != currently_running {
                debug!(
                    "Updating running status for '{}' after start: {} -> {}",
                    app_name, app.running, currently_running
                );
                app.running = currently_running;
                status_changed = true;
            }
        } else {
            warn!("App '{}' not in APPS map after start_app.", app_name);
        }
    }

    if status_changed {
        emit_apps().await;
    } else {
        let apps_map_check = APPS.lock().await;
        if apps_map_check.contains_key(&app_name) {
            drop(apps_map_check);
            emit_apps().await;
        }
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
        Err(e) => Err(anyhow::Error::from(e))
            .context(format!("Failed to launch elevated kill for PID {}", pid_str)),
    }
}

#[tauri::command]
pub async fn stop_app(app_name: String) -> Result<(), Error> {
    info!("Attempting to stop app: {}", app_name);
    let app_dir_lock = get_app_lock(&app_name).await;
    let _guard = app_dir_lock.lock().await;
    let working_dir = get_app_working_dir_path(&app_name);

    let app_name_clone_for_task = app_name.clone();
    let working_dir_clone_for_task = working_dir.clone();

    let any_pids_were_targeted: bool = task::spawn_blocking(move || -> Result<bool> {
        let mut sys_task = System::new();
        sys_task.refresh_processes(ProcessesToUpdate::All, true);
        debug!(
            "Scanning processes to stop for '{}' in '{}'",
            app_name_clone_for_task,
            working_dir_clone_for_task.display()
        );
        let pids_to_kill =
            process::get_pids_related_to_app_dir(&sys_task, &working_dir_clone_for_task);
        let targeted_any = !pids_to_kill.is_empty();

        for pid_to_kill in pids_to_kill {
            if let Some(process_to_kill) = sys_task.process(pid_to_kill) {
                info!(
                    "Killing {:?} (PID {}) for app '{}'",
                    process_to_kill.name(),
                    pid_to_kill.as_u32(),
                    app_name_clone_for_task
                );
                if process_to_kill.kill() {
                    info!("Kill signal sent to PID {}.", pid_to_kill.as_u32());
                } else {
                    sys_task.refresh_processes(ProcessesToUpdate::Some(&[pid_to_kill]), true);
                    if sys_task.process(pid_to_kill).is_none() {
                        info!(
                            "PID {} for '{}' exited post-kill failure report.",
                            pid_to_kill.as_u32(),
                            app_name_clone_for_task
                        );
                    } else {
                        warn!(
                            "Standard kill failed for PID {} ('{}'). Attempting elevated.",
                            pid_to_kill.as_u32(),
                            app_name_clone_for_task
                        );
                        if let Err(e) =
                            try_kill_with_elevation(pid_to_kill, &app_name_clone_for_task)
                        {
                            error!(
                                "Elevated kill for PID {} ('{}') failed: {:?}",
                                pid_to_kill.as_u32(),
                                app_name_clone_for_task,
                                e
                            );
                        }
                    }
                }
            } else {
                info!(
                    "PID {} for '{}' already exited.",
                    pid_to_kill.as_u32(),
                    app_name_clone_for_task
                );
            }
        }
        Ok(targeted_any)
    })
        .await??;

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

pub async fn periodically_update_all_apps_running_status() {
    let mut ticker = interval(Duration::from_secs(2));
    info!("Starting periodic app status update (2s interval).");
    let mut sys = System::new();
    loop {
        ticker.tick().await;
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

fn find_main_script(
    app_name: &str,
    working_dir: &Path,
    main_script_relative: &str,
) -> Result<PathBuf, Error> {
    let priority = [main_script_relative, "webui.py", "app.py"];
    for script_name in priority.iter() {
        let script_path = working_dir.join(script_name);
        if script_path.exists() {
            return Ok(script_path);
        }
    }
    emit_error_finish!(app_name);
    Err(err!(
        "Main script '{}' not at '{}' for '{}'",
        main_script_relative,
        main_script_relative,
        app_name
    ))
}