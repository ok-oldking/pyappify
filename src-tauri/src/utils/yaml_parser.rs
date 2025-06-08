// src/utils/yaml_parser.rs
use serde::{Deserialize, Serialize};
// use std::collections::HashMap; // No longer needed for profiles
use std::fs;
use std::path::Path;
use std::vec::Vec;
// Explicitly using Vec

// Define the structs to match the YAML structure
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Config {
    pub requires_python: String,
    pub profiles: Vec<Profile>, // Changed from HashMap<String, Profile> to Vec<Profile>
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Profile {
    pub name: String, // Added name field
    #[serde(default)]
    pub main_script: String,
    #[serde(default)] // Defaults to false if missing in YAML for this profile
    pub admin: bool,
    #[serde(default)] // Defaults to "" (empty string) if missing in YAML for this profile
    pub requirements: String,
    #[serde(default, rename = "PYTHONPATH")]
    pub python_path: String,
}

// Implement Default for the Config struct
impl Default for Config {
    fn default() -> Self {
        let default_profile = Profile {
            name: "default".to_string(), // Set name for default profile
            main_script: "main.py".to_string(),
            admin: false,
            requirements: "requirements.txt".to_string(),
            python_path: "".to_string(),
        };
        Config {
            requires_python: "3.12".to_string(), // A sensible default for python version
            profiles: vec![default_profile],     // Store profiles in a Vec
        }
    }
}

impl Config {
    // Implement get_profile(profile_name) for the Config struct
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

    // Ensure there is at least one profile.
    // If profiles list is empty after parsing a valid YAML (e.g. `profiles: []`),
    // then Config::default() might be more appropriate if a "first profile" is always expected.
    // However, the `get_profile("default")` check below implicitly handles this by returning Config::default().
    if parsed_config.profiles.is_empty() {
        // This case will typically be caught by the get_profile("default").is_none() check if
        // that specific profile is mandatory. If any first profile is acceptable,
        // and an empty profiles list is valid YAML, then no defaults can be applied.
        // For now, let's assume the existing "default" profile check handles this.
        println!("Parsed YAML has no profiles, using default configuration.");
        return Config::default(); // Or return parsed_config if an empty profile list is acceptable.
    }

    // The user's original code checked for a profile named "default".
    // If it's not present, it returns Config::default().
    // Config::default() *does* have a profile, and it's the first one.
    // So, `parsed_config.profiles` will not be empty if we proceed past this check.
    if parsed_config.get_profile("default").is_none() {
        println!("Profile 'default' not found in config, using default configuration.");
        return Config::default();
    }

    // At this point, parsed_config.profiles is guaranteed to have at least one element.
    // Clone the values from the first profile to use as defaults.
    // The first profile itself would have already had its `#[serde(default)]` applied.
    let first_profile_defaults = parsed_config.profiles[0].clone();

    for profile in parsed_config.profiles.iter_mut() {
        // If main_script is empty (its default value), use the first profile's main_script.
        if profile.main_script.is_empty() {
            profile.main_script = first_profile_defaults.main_script.clone();
        }

        // If requirements is empty (its default value), use the first profile's requirements.
        if profile.requirements.is_empty() {
            profile.requirements = first_profile_defaults.requirements.clone();
        }

        // If python_path is empty (its default value), use the first profile's python_path.
        if profile.python_path.is_empty() {
            profile.python_path = first_profile_defaults.python_path.clone();
        }

        // If admin is false (its default value), use the first profile's admin value.
        // bool::default() is false.
        if profile.admin == bool::default() {
            profile.admin = first_profile_defaults.admin;
        }
    }

    parsed_config
}
