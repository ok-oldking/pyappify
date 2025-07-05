// app.rs
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::vec::Vec;
use anyhow::{anyhow, Context};
use chrono::{DateTime, Utc};
use tracing::{debug, error, info, warn};
use crate::utils::path;
use crate::utils::path::{get_app_base_path, get_app_working_dir_path};
use crate::utils::defender::is_defender_excluded;

pub const YML_FILE_NAME: &str = "pyappify.yml";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct App {
    pub name: String,
    #[serde(default)]
    pub current_version: Option<String>,
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
    #[serde(default)]
    pub profiles: Vec<Profile>,
    #[serde(skip)]
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
        debug!("get_current_profile_settings {} {}", self.current_profile, self.profiles.len());
        self.get_profile(&self.current_profile)
            .expect("Critical: Default profile missing in AppConfig.")
    }

    pub fn get_profile(&self, profile_name: &str) -> Option<&Profile> {
        self.profiles.iter().find(|p| p.name == profile_name)
            .or_else(|| self.profiles.first())
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
    pub requires_defender_whitelist: Option<bool>,
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

    pub fn requires_defender_whitelist(&self) -> bool {
        self.requires_defender_whitelist.unwrap_or(false)
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
            if profile.requires_defender_whitelist.is_none() {
                profile.requires_defender_whitelist = first_profile.requires_defender_whitelist;
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
    let working_pyappify = get_app_working_dir_path(app.name.as_str());
    let working_pyappify_contents = fs::read_to_string(working_pyappify);
    if let Ok(contents) = working_pyappify_contents {
        if let Ok(new_app) = serde_yaml::from_str(&contents) {
            app = new_app;
        } else {
            error!("error!: Failed to parse working dir pyappify.yml");
        }
    }
    apply_profile_inheritance(&mut app);
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

    app.name = parsed_app.name;
    app.profiles = parsed_app.profiles;

    if app.get_profile(&app.current_profile).is_none() {
        if let Some(first_profile) = app.profiles.first() {
            app.current_profile = first_profile.name.clone();
        }
    }
}

fn get_app_config_json_path(app_name: &str) -> PathBuf {
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

pub(crate) async fn load_app_config_from_json(app_name: &str) -> anyhow::Result<Option<App>> {
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

            let profile = app.get_current_profile_settings();
            if profile.requires_defender_whitelist() {
                let app_base_path = get_app_base_path(&app.name);
                let app_base_path_str = app_base_path.display().to_string();
                match is_defender_excluded(&app_base_path_str).await {
                    Ok(excluded) => {
                        if !excluded {
                            app.show_add_defender = true;
                        }
                    }
                    Err(e) => {
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
