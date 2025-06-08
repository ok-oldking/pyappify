use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::vec::Vec;
use chrono::{DateTime, Utc};
use tracing::{debug, warn};
use crate::utils::path;
use crate::utils::path::get_app_working_dir_path;

pub const OK_YAML_NAME: &str = "ok.yml";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct App {
    pub name: String,
    pub url: String,
    pub current_version: Option<String>,
    pub available_versions: Vec<String>,
    #[serde(default)]
    pub running: bool,
    #[serde(default = "default_last_start_fn")]
    pub last_start: DateTime<Utc>,
    #[serde(default = "default_profile_fn")]
    pub current_profile: String,
    #[serde(default)]
    pub installed: bool, // Added installed state, defaults to false
    #[serde(default)] // Config will be AppConfig::default() after deserialization
    pub config: Config,
}

pub fn default_profile_fn() -> String {
    "default".to_string()
}

fn default_last_start_fn() -> DateTime<Utc> {
    Utc::now()
}

impl App {
    pub fn get_repo_path(&self) -> PathBuf {
        path::get_app_repo_path(&self.name)
    }

    pub fn get_current_profile_settings(&self) -> &Profile {
        self.config.get_profile(&self.current_profile)
            .or_else(|| {
                warn!(
                    "Current profile '{}' not found for app '{}'. Falling back to 'default' profile.",
                    self.current_profile, self.name
                );
                self.config.get_profile("default")
            })
            .expect("Critical: Default profile missing in AppConfig.")
    }

    pub fn read_and_set_config_from_working_dir(&mut self) {
        let working_dir = get_app_working_dir_path(&self.name);
        let ok_yaml_path = working_dir.join(OK_YAML_NAME);
        let ok_yaml_path_str = ok_yaml_path.to_string_lossy().into_owned();

        self.config = load_config_from_yaml(&ok_yaml_path_str);
        debug!(
            "Refreshed app.config for '{}' from {}",
            self.name,
            ok_yaml_path.display()
        );
    }
}


#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Config {
    pub profiles: Vec<Profile>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Profile {
    pub name: String,
    #[serde(default)]
    pub main_script: String,
    #[serde(default)]
    pub admin: bool,
    #[serde(default)]
    pub requirements: String,
    #[serde(default, rename = "PYTHONPATH")]
    pub python_path: String,
    #[serde(default)]
    pub git_url: String,
    #[serde(default)]
    pub requires_python: String,
}

impl Default for Config {
    fn default() -> Self {
        let default_profile = Profile {
            name: "default".to_string(),
            main_script: "main.py".to_string(),
            admin: false,
            requirements: "requirements.txt".to_string(),
            python_path: "".to_string(),
            git_url: "".to_string(),
            requires_python: "3.12".to_string(),
        };
        Config {
            profiles: vec![default_profile],
        }
    }
}

impl Config {
    pub fn get_profile(&self, profile_name: &str) -> Option<&Profile> {
        self.profiles.iter().find(|p| p.name == profile_name)
    }
}

pub fn load_config_from_yaml(file_path_str: &str) -> Config {
    let file_path = Path::new(file_path_str);

    if !file_path.exists() {
        println!("Config file not found, using default configuration.");
        return Config::default();
    }

    let yaml_content = match fs::read_to_string(file_path) {
        Ok(content) => content,
        Err(e) => {
            eprintln!(
                "Error reading config file: {}, using default configuration.",
                e
            );
            return Config::default();
        }
    };

    let mut parsed_config: Config = match serde_yaml::from_str(&yaml_content) {
        Ok(config) => config,
        Err(e) => {
            eprintln!("Error parsing YAML: {}, using default configuration.", e);
            return Config::default();
        }
    };

    if parsed_config.profiles.is_empty() {
        println!("Parsed YAML has no profiles, using default configuration.");
        return Config::default();
    }

    if parsed_config.get_profile("default").is_none() {
        println!("Profile 'default' not found in config, using default configuration.");
        return Config::default();
    }

    let first_profile_defaults = parsed_config.profiles[0].clone();

    for profile in parsed_config.profiles.iter_mut() {
        if profile.main_script.is_empty() {
            profile.main_script = first_profile_defaults.main_script.clone();
        }

        if profile.requirements.is_empty() {
            profile.requirements = first_profile_defaults.requirements.clone();
        }

        if profile.python_path.is_empty() {
            profile.python_path = first_profile_defaults.python_path.clone();
        }

        if profile.admin == bool::default() {
            profile.admin = first_profile_defaults.admin;
        }

        if profile.git_url.is_empty() {
            profile.git_url = first_profile_defaults.git_url.clone();
        }

        if profile.requires_python.is_empty() {
            profile.requires_python = first_profile_defaults.requires_python.clone();
        }
    }

    parsed_config
}