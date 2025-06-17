// app.rs
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::vec::Vec;
use chrono::{DateTime, Utc};
use tracing::{debug, info, warn};
use crate::utils::path;
use crate::utils::path::get_app_working_dir_path;

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

    pub fn read_and_set_config_from_working_dir(&mut self) {
        let working_dir = get_app_working_dir_path(&self.name);
        let yml_path = working_dir.join(YML_FILE_NAME);
        let yml_path_str = yml_path.to_string_lossy();

        update_app_from_yml(self, &yml_path_str);
        debug!(
            "Attempted to refresh app config for '{}' from {}",
            self.name,
            yml_path.display()
        );
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
    pub requirements: String,
    #[serde(default, rename = "PYTHONPATH")]
    pub python_path: String,
    #[serde(default)]
    pub git_url: String,
    #[serde(default)]
    pub requires_python: String,
}

impl Profile {
    pub fn is_admin(&self) -> bool {
        self.admin.unwrap_or(false)
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
        }
    }
}

pub fn read_embedded_app() -> App {
    let yaml_content = include_str!("../assets/pyappify.yml");
    let mut app: App = serde_yaml::from_str(yaml_content).expect("Failed to parse embedded pyappify.yml");
    apply_profile_inheritance(&mut app);
    if app.current_profile.is_empty() {
        app.current_profile = app.profiles.first().unwrap().name.clone();
        info!("app current_profile is empty set to first {}", &app.current_profile);
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