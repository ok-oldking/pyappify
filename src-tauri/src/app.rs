// src/app.rs
use crate::config_manager::GLOBAL_CONFIG_STATE;
use crate::utils::defender::is_defender_excluded;
use crate::utils::path;
use crate::utils::path::get_app_base_path;
use anyhow::{anyhow, Context};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::vec::Vec;
use tokio::io::AsyncWriteExt;
use tracing::{debug, error, info, warn};

pub const YML_FILE_NAME: &str = "pyappify.yml";
pub const UPDATE_METHOD_OPTION_MANUAL: &str = "MANUAL_UPDATE";
pub const UPDATE_METHOD_OPTION_AUTO: &str = "AUTO_UPDATE";
pub const UPDATE_METHOD_OPTION_AUTO_PRE_RELEASE: &str = "AUTO_UPDATE_PRE_RELEASE";
static APP_JSON_TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn default_update_method_fn() -> String {
    UPDATE_METHOD_OPTION_AUTO.to_string()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AppUpdateState {
    #[default]
    Idle,
    Updating,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct App {
    pub name: String,
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub current_version: Option<String>,
    #[serde(default, skip_serializing)]
    pub current_version_missing: bool,
    #[serde(default)]
    pub app_starting_version: Option<String>,
    #[serde(default)]
    pub update_note: Vec<String>,
    #[serde(default)]
    pub available_versions: Vec<String>,
    #[serde(default)]
    pub running: bool,
    #[serde(default = "default_last_start_fn")]
    pub last_start: DateTime<Utc>,
    #[serde(default)]
    pub current_profile: String,
    #[serde(default)]
    pub installed: bool,
    #[serde(default = "default_update_method_fn")]
    pub update_method: String,
    #[serde(default)]
    pub auto_start: bool,
    #[serde(default)]
    pub update_state: AppUpdateState,
    #[serde(default)]
    pub update_target_version: Option<String>,
    #[serde(default)]
    pub update_error: Option<String>,
    #[serde(default)]
    pub profiles: Vec<Profile>,
    #[serde(default)]
    pub show_add_defender: bool,
}

fn default_last_start_fn() -> DateTime<Utc> {
    Utc::now()
}

impl App {
    pub fn get_repo_path(&self) -> PathBuf {
        path::get_app_repo_path(&self.name)
    }

    pub fn get_current_profile_settings(&self) -> &Profile {
        debug!(
            "get_current_profile_settings {} {}",
            self.current_profile,
            self.profiles.len()
        );
        self.get_profile(&self.current_profile)
            .expect("Critical: Default profile missing in AppConfig.")
    }

    pub fn get_profile(&self, profile_name: &str) -> Option<&Profile> {
        self.profiles
            .iter()
            .find(|p| p.name == profile_name)
            .or_else(|| self.profiles.first())
    }

    pub fn effective_update_method(&self) -> &str {
        match self.update_method.as_str() {
            UPDATE_METHOD_OPTION_AUTO => UPDATE_METHOD_OPTION_AUTO,
            UPDATE_METHOD_OPTION_AUTO_PRE_RELEASE => UPDATE_METHOD_OPTION_AUTO_PRE_RELEASE,
            _ => UPDATE_METHOD_OPTION_MANUAL,
        }
    }

    fn normalize_preferences(&mut self) {
        if !matches!(
            self.update_method.as_str(),
            UPDATE_METHOD_OPTION_MANUAL
                | UPDATE_METHOD_OPTION_AUTO
                | UPDATE_METHOD_OPTION_AUTO_PRE_RELEASE
        ) {
            warn!(
                "Unknown update method '{}' for app '{}'. Resetting to '{}'.",
                self.update_method, self.name, UPDATE_METHOD_OPTION_AUTO
            );
            self.update_method = default_update_method_fn();
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Profile {
    pub name: String,
    #[serde(default)]
    pub main_script: String,
    #[serde(default)]
    pub admin: Option<bool>,
    #[serde(default)]
    pub use_pythonw: Option<bool>,
    #[serde(default)]
    pub show_add_defender: Option<bool>,
    #[serde(default)]
    pub requirements: String,
    #[serde(default, rename = "PYTHONPATH")]
    pub python_path: String,
    #[serde(default)]
    pub git_url: String,
    #[serde(default)]
    pub requires_python: String,
    #[serde(default)]
    pub pip_args: String,
}

impl Profile {
    pub fn is_admin(&self) -> bool {
        self.admin.unwrap_or(false)
    }

    pub fn use_pythonw(&self) -> bool {
        self.use_pythonw.unwrap_or(false)
    }

    pub fn show_add_defender(&self) -> bool {
        self.show_add_defender.unwrap_or(false)
    }
}

fn apply_profile_inheritance(config: &mut App) {
    if let Some(first_profile) = config.profiles.first().cloned() {
        for profile in config.profiles.iter_mut().skip(1) {
            if profile.main_script.is_empty() {
                profile.main_script = first_profile.main_script.clone();
            }
            if profile.requirements.is_empty() {
                profile.requirements = first_profile.requirements.clone();
            }
            if profile.python_path.is_empty() {
                profile.python_path = first_profile.python_path.clone();
            }
            if profile.git_url.is_empty() {
                profile.git_url = first_profile.git_url.clone();
            }
            if profile.requires_python.is_empty() {
                profile.requires_python = first_profile.requires_python.clone();
            }
            if profile.admin.is_none() {
                profile.admin = first_profile.admin;
            }
            if profile.use_pythonw.is_none() {
                profile.use_pythonw = first_profile.use_pythonw;
            }
            if profile.show_add_defender.is_none() {
                profile.show_add_defender = first_profile.show_add_defender;
            }
            if profile.pip_args.is_empty() {
                profile.pip_args = first_profile.pip_args.clone();
            }
        }
    }
}

pub fn read_embedded_app() -> App {
    let yml_content = fs::read_to_string("pyappify.yml")
        .unwrap_or_else(|_| include_str!("../assets/pyappify.yml").to_string());
    let mut app: App = serde_yaml::from_str(&yml_content).expect("Failed to parse pyappify.yml");
    apply_profile_inheritance(&mut app);
    app.normalize_preferences();
    if app.current_profile.is_empty() {
        app.current_profile = app.profiles.first().unwrap().name.clone();
        info!(
            "app current_profile is empty, set to first profile: {}",
            &app.current_profile
        );
    }
    app
}
pub fn update_app_from_yml(app: &mut App, file_path_str: &str) {
    let file_path = Path::new(file_path_str);

    if !file_path.exists() {
        return;
    }

    info!("update_app_from_yml: {}", file_path.display());

    let yaml_content = match fs::read_to_string(file_path) {
        Ok(content) => content,
        Err(e) => {
            warn!(
                "Error reading config file {}: {}. Not updating app '{}'.",
                file_path.display(),
                e,
                app.name
            );
            return;
        }
    };

    let mut parsed_app: App = match serde_yaml::from_str(&yaml_content) {
        Ok(app_from_yml) => app_from_yml,
        Err(e) => {
            warn!(
                "Error parsing YAML from {}: {}. Not updating app '{}'.",
                file_path.display(),
                e,
                app.name
            );
            return;
        }
    };

    apply_profile_inheritance(&mut parsed_app);

    app.icon = parsed_app.icon;
    app.profiles = parsed_app.profiles;

    if app.get_profile(&app.current_profile).is_none() {
        if let Some(first_profile) = app.profiles.first() {
            app.current_profile = first_profile.name.clone();
        }
    }
}

pub(crate) fn get_app_config_json_path(app_name: &str) -> PathBuf {
    get_app_base_path(app_name).join("app.json")
}

pub(crate) async fn save_app_config_to_json(app: &App) -> anyhow::Result<()> {
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
    write_file_atomically(&config_path, json_data.as_bytes())
        .await
        .with_context(|| format!("Failed to write app.json for {}", app.name))?;
    debug!(
        "Saved app config for {} to {}",
        app.name,
        config_path.display()
    );
    Ok(())
}

async fn write_file_atomically(path: &Path, contents: &[u8]) -> anyhow::Result<()> {
    let sequence = APP_JSON_TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temp_path = path.with_file_name(format!(".app.json.{}.{sequence}.tmp", std::process::id()));

    let write_result = async {
        let mut temp_file = tokio::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp_path)
            .await?;
        temp_file.write_all(contents).await?;
        temp_file.flush().await?;
        temp_file.sync_all().await?;
        drop(temp_file);

        replace_file(&temp_path, path)?;
        Ok::<(), std::io::Error>(())
    }
    .await;

    if write_result.is_err() {
        let _ = tokio::fs::remove_file(&temp_path).await;
    }
    write_result?;
    Ok(())
}

#[cfg(windows)]
fn replace_file(source: &Path, destination: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    let source_wide: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination_wide: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    let moved = unsafe {
        MoveFileExW(
            source_wide.as_ptr(),
            destination_wide.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };

    if moved == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn replace_file(source: &Path, destination: &Path) -> std::io::Result<()> {
    fs::rename(source, destination)
}

pub(crate) async fn load_app_config_from_json(app_name: &str) -> anyhow::Result<Option<App>> {
    let config_path = get_app_config_json_path(app_name);
    if !config_path.exists() {
        return Ok(None);
    }
    let json_data = tokio::fs::read_to_string(&config_path)
        .await
        .with_context(|| format!("Failed to read app.json for {}", app_name))?;

    let json_value: serde_json::Value = serde_json::from_str(&json_data)
        .with_context(|| format!("Failed to parse app.json as JSON for {}", app_name))?;
    let has_update_method = json_value.get("update_method").is_some();
    let has_auto_start = json_value.get("auto_start").is_some();

    match serde_json::from_value::<App>(json_value) {
        Ok(mut app) => {
            if app.name != app_name {
                warn!("App name mismatch in app.json ('{}') and directory ('{}'). Correcting to directory name: '{}'.", app.name, app_name, app_name);
                app.name = app_name.to_string();
            }

            if !has_update_method || !has_auto_start {
                if let Some(config_state) = GLOBAL_CONFIG_STATE.get() {
                    let (legacy_update_method, legacy_auto_start) =
                        config_state.lock().unwrap().legacy_app_preferences();
                    let mut migrated = false;
                    if !has_update_method {
                        if let Some(update_method) = legacy_update_method {
                            app.update_method = update_method;
                            migrated = true;
                        }
                    }
                    if !has_auto_start {
                        if let Some(auto_start) = legacy_auto_start {
                            app.auto_start = auto_start;
                            migrated = true;
                        }
                    }
                    if migrated {
                        info!(
                            "Migrated legacy global preferences into app.json for '{}'.",
                            app_name
                        );
                    }
                }
            }

            app.normalize_preferences();

            let profile = app.get_current_profile_settings();
            debug!("app {} current profile: {:?}", app.name, profile);
            if profile.show_add_defender() {
                match is_defender_excluded().await {
                    Ok(excluded) => {
                        app.show_add_defender = !excluded;
                    }
                    Err(e) => {
                        app.show_add_defender = false;
                        warn!("Could not check defender exclusion for {}: {}", app.name, e);
                    }
                }
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

#[cfg(test)]
mod tests {
    use super::{
        write_file_atomically, App, AppUpdateState, UPDATE_METHOD_OPTION_AUTO,
        UPDATE_METHOD_OPTION_MANUAL,
    };

    #[tokio::test]
    async fn atomically_replaces_existing_app_json() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "pyappify-atomic-app-json-{}-{unique}",
            std::process::id()
        ));
        tokio::fs::create_dir_all(&root).await.unwrap();
        let config_path = root.join("app.json");
        tokio::fs::write(&config_path, br#"{"name":"old"}"#)
            .await
            .unwrap();

        write_file_atomically(&config_path, br#"{"name":"new"}"#)
            .await
            .unwrap();

        assert_eq!(
            tokio::fs::read_to_string(&config_path).await.unwrap(),
            r#"{"name":"new"}"#
        );
        assert_eq!(
            std::fs::read_dir(&root)
                .unwrap()
                .filter_map(Result::ok)
                .count(),
            1
        );
        tokio::fs::remove_dir_all(root).await.unwrap();
    }

    #[test]
    fn icon_defaults_to_empty_when_omitted_from_yaml() {
        let app: App = serde_yaml::from_str("name: example\n").unwrap();
        assert!(app.icon.is_empty());
    }

    #[test]
    fn reads_relative_icon_path_from_yaml() {
        let app: App = serde_yaml::from_str("name: example\nicon: assets/icon.png\n").unwrap();
        assert_eq!(app.icon, "assets/icon.png");
    }

    #[test]
    fn app_preferences_have_per_app_defaults_when_omitted() {
        let app: App = serde_json::from_str(r#"{"name":"example"}"#).unwrap();

        assert_eq!(app.update_method, UPDATE_METHOD_OPTION_AUTO);
        assert!(!app.auto_start);
        assert_eq!(app.update_state, AppUpdateState::Idle);
        assert_eq!(app.update_target_version, None);
        assert_eq!(app.update_error, None);

        let saved = serde_json::to_value(app).unwrap();
        assert_eq!(saved["update_method"], UPDATE_METHOD_OPTION_AUTO);
        assert_eq!(saved["auto_start"], false);
        assert_eq!(saved["update_state"], "idle");
    }

    #[test]
    fn unknown_update_method_is_treated_as_manual() {
        let app: App = serde_json::from_str(
            r#"{"name":"example","update_method":"UNKNOWN","auto_start":true}"#,
        )
        .unwrap();

        assert_eq!(app.effective_update_method(), UPDATE_METHOD_OPTION_MANUAL);
        assert!(app.auto_start);
    }
}
