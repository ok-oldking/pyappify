// src/config_manager.rs
use crate::python_env::get_supported_python_versions;
use crate::utils::error::Error;
use crate::utils::path::get_config_dir;
use crate::utils::path::get_pip_cache_dir;
use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::Manager;
use tracing::{error, info, warn};

const PIP_CACHE_DIR_CONFIG_KEY: &str = "Pip Cache Directory";
const PIP_CACHE_DIR_OPTION_APP_INSTALL: &str = "App Install Directory";
const PIP_CACHE_DIR_OPTION_SYSTEM_DEFAULT: &str = "System Default";
const DEFAULT_PYTHON_VERSION_CONFIG_KEY: &str = "Default Python Version";

const PIP_INDEX_URL_CONFIG_KEY: &str = "Pip Index URL";
const PIP_INDEX_URL_OPTION_SYSTEM_DEFAULT: &str = "";
const PIP_INDEX_URL_OPTION_PYPI: &str = "https://pypi.org/simple/";
const PIP_INDEX_URL_OPTION_TSINGHUA: &str = "https://pypi.tuna.tsinghua.edu.cn/simple";
const PIP_INDEX_URL_OPTION_ALIYUN: &str = "https://mirrors.aliyun.com/pypi/simple/";
const PIP_INDEX_URL_OPTION_USTC: &str = "https://mirrors.ustc.edu.cn/pypi/simple/";
const PIP_INDEX_URL_OPTION_HUAWEI: &str = "https://repo.huaweicloud.com/repository/pypi/simple/";
const PIP_INDEX_URL_OPTION_TENCENT: &str = "https://mirrors.cloud.tencent.com/pypi/simple/";

const UPDATE_METHOD_CONFIG_KEY: &str = "Update Method";
pub const UPDATE_METHOD_OPTION_MANUAL: &str = "MANUAL_UPDATE";
pub const UPDATE_METHOD_OPTION_AUTO: &str = "AUTO_UPDATE";
pub const UPDATE_METHOD_OPTION_IGNORE: &str = "IGNORE_UPDATE";

const I18N_CONFIG_KEY: &str = "Language";
const I18N_OPTION_EN: &str = "en";
const I18N_OPTION_ZH_CN: &str = "zh-CN";
const I18N_OPTION_ZH_TW: &str = "zh-TW";
const I18N_OPTION_ES: &str = "es";
const I18N_OPTION_JA: &str = "ja";
const I18N_OPTION_KO: &str = "ko";

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(untagged)]
pub enum ConfigValue {
    String(String),
    Integer(i32),
}

impl std::fmt::Display for ConfigValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigValue::String(s) => write!(f, "{}", s),
            ConfigValue::Integer(i) => write!(f, "{}", i),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ConfigItem {
    pub name: String,
    pub description: String,
    pub value: ConfigValue,
    pub default_value: ConfigValue,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<ConfigValue>>,
}

impl ConfigItem {
    fn validate_and_normalize(&mut self) {
        if let Some(options) = &self.options {
            if !options.is_empty() && !options.contains(&self.value) {
                warn!(
                    "Value '{}' for config '{}' not in options. Resetting to default '{}'.",
                    self.value, self.name, self.default_value
                );
                self.value = self.default_value.clone();
            }
        }
        match (&self.value, &self.default_value) {
            (ConfigValue::String(_), ConfigValue::String(_))
            | (ConfigValue::Integer(_), ConfigValue::Integer(_)) => {}
            _ => {
                error!(
                    "Mismatch between value type and default_value type for '{}'. Resetting to default.",
                     self.name
                );
                self.value = self.default_value.clone();
            }
        }
    }
}

#[derive(Debug)]
pub struct AppConfig {
    items: HashMap<String, ConfigItem>,
    config_path: PathBuf,
}

fn get_default_lang_from_locale() -> &'static str {
    let locale = get_default_locale();
    if locale == "zh-CN" {
        I18N_OPTION_ZH_CN
    } else if locale == "zh-TW" || locale == "zh-HK" {
        I18N_OPTION_ZH_TW
    } else if locale.starts_with("es") {
        I18N_OPTION_ES
    } else if locale.starts_with("ja") {
        I18N_OPTION_JA
    } else if locale.starts_with("ko") {
        I18N_OPTION_KO
    } else {
        I18N_OPTION_EN
    }
}

impl AppConfig {
    pub fn new() -> Self {
        let config_dir = get_config_dir();
        let config_file_path = config_dir.join("app_config.json");

        let mut instance = Self {
            items: Self::get_default_config_items(),
            config_path: config_file_path,
        };

        instance.load_from_file();
        instance.merge_and_validate_defaults();
        instance.save_to_file();
        instance.update_pip_cache_env_var_from_config();
        instance.update_pip_index_url_env_var_from_config();
        instance
    }

    fn get_default_config_items() -> HashMap<String, ConfigItem> {
        let mut items = HashMap::new();

        let default_lang = get_default_lang_from_locale();

        items.insert(
            I18N_CONFIG_KEY.to_string(),
            ConfigItem {
                name: I18N_CONFIG_KEY.to_string(),
                description: "The display language of the application.".to_string(),
                value: ConfigValue::String(default_lang.to_string()),
                default_value: ConfigValue::String(default_lang.to_string()),
                options: Some(vec![
                    ConfigValue::String(I18N_OPTION_EN.to_string()),
                    ConfigValue::String(I18N_OPTION_ZH_CN.to_string()),
                    ConfigValue::String(I18N_OPTION_ZH_TW.to_string()),
                    ConfigValue::String(I18N_OPTION_ES.to_string()),
                    ConfigValue::String(I18N_OPTION_JA.to_string()),
                    ConfigValue::String(I18N_OPTION_KO.to_string()),
                ]),
            },
        );

        items.insert(
            PIP_CACHE_DIR_CONFIG_KEY.to_string(),
            ConfigItem {
                name: PIP_CACHE_DIR_CONFIG_KEY.to_string(),
                description: "Specifies pip's package cache location. 'App Install Directory' uses a cache within the app's data folder. 'System Default' uses pip's standard cache location.".to_string(),
                value: ConfigValue::String(PIP_CACHE_DIR_OPTION_APP_INSTALL.to_string()),
                default_value: ConfigValue::String(PIP_CACHE_DIR_OPTION_APP_INSTALL.to_string()),
                options: Some(vec![
                    ConfigValue::String(PIP_CACHE_DIR_OPTION_SYSTEM_DEFAULT.to_string()),
                    ConfigValue::String(PIP_CACHE_DIR_OPTION_APP_INSTALL.to_string()),
                ]),
            },
        );

        let supported_python_versions = get_supported_python_versions();
        let python_version_options: Vec<ConfigValue> = supported_python_versions
            .into_iter()
            .map(ConfigValue::String)
            .collect();

        let default_python_version_str = "3.12".to_string();

        items.insert(
            DEFAULT_PYTHON_VERSION_CONFIG_KEY.to_string(),
            ConfigItem {
                name: DEFAULT_PYTHON_VERSION_CONFIG_KEY.to_string(),
                description: "The default Python version to be used.".to_string(),
                value: ConfigValue::String(default_python_version_str.clone()),
                default_value: ConfigValue::String(default_python_version_str),
                options: if python_version_options.is_empty() {
                    None
                } else {
                    Some(python_version_options)
                },
            },
        );

        let locale = get_default_locale();
        info!("System locale is: {}", locale);
        let default_pip_url = if locale == "zh_CN" {
            PIP_INDEX_URL_OPTION_ALIYUN.to_string()
        } else {
            PIP_INDEX_URL_OPTION_SYSTEM_DEFAULT.to_string()
        };

        items.insert(
            PIP_INDEX_URL_CONFIG_KEY.to_string(),
            ConfigItem {
                name: PIP_INDEX_URL_CONFIG_KEY.to_string(),
                description: "Specifies the pip index URL. Select the empty option to use the system's default pip configuration (equivalent to not setting an index URL).".to_string(),
                value: ConfigValue::String(default_pip_url.clone()),
                default_value: ConfigValue::String(default_pip_url),
                options: Some(vec![
                    ConfigValue::String(PIP_INDEX_URL_OPTION_SYSTEM_DEFAULT.to_string()),
                    ConfigValue::String(PIP_INDEX_URL_OPTION_PYPI.to_string()),
                    ConfigValue::String(PIP_INDEX_URL_OPTION_TSINGHUA.to_string()),
                    ConfigValue::String(PIP_INDEX_URL_OPTION_ALIYUN.to_string()),
                    ConfigValue::String(PIP_INDEX_URL_OPTION_USTC.to_string()),
                    ConfigValue::String(PIP_INDEX_URL_OPTION_HUAWEI.to_string()),
                    ConfigValue::String(PIP_INDEX_URL_OPTION_TENCENT.to_string()),
                ]),
            },
        );

        items.insert(
            UPDATE_METHOD_CONFIG_KEY.to_string(),
            ConfigItem {
                name: UPDATE_METHOD_CONFIG_KEY.to_string(),
                description: "Controls the app's update behavior. 'MANUAL_UPDATE' requires user action, 'AUTO_UPDATE' updates automatically, and 'IGNORE_UPDATE' disables update checks.".to_string(),
                value: ConfigValue::String(UPDATE_METHOD_OPTION_AUTO.to_string()),
                default_value: ConfigValue::String(UPDATE_METHOD_OPTION_AUTO.to_string()),
                options: Some(vec![
                    ConfigValue::String(UPDATE_METHOD_OPTION_MANUAL.to_string()),
                    ConfigValue::String(UPDATE_METHOD_OPTION_AUTO.to_string()),
                    ConfigValue::String(UPDATE_METHOD_OPTION_IGNORE.to_string()),
                ]),
            },
        );

        items
    }

    fn merge_and_validate_defaults(&mut self) {
        let default_items_from_code = Self::get_default_config_items();

        for (name, default_item_definition) in default_items_from_code {
            match self.items.entry(name.clone()) {
                std::collections::hash_map::Entry::Occupied(mut entry) => {
                    let item = entry.get_mut();
                    item.description = default_item_definition.description;
                    item.default_value = default_item_definition.default_value;
                    item.options = default_item_definition.options;
                    item.validate_and_normalize();
                }
                std::collections::hash_map::Entry::Vacant(entry) => {
                    info!(
                        "Adding new default config item (not found in current items map): {}",
                        name
                    );
                    entry.insert(default_item_definition);
                }
            }
        }

        let default_keys_from_code: Vec<_> =
            Self::get_default_config_items().keys().cloned().collect();
        self.items.retain(|name, _| {
            if default_keys_from_code.contains(name) {
                true
            } else {
                warn!("Removing obsolete config item '{}' from runtime config as it's no longer defined in code.", name);
                false
            }
        });
    }

    fn load_from_file(&mut self) {
        if !self.config_path.exists() {
            info!("Config file {:?} not found. Proceeding with current (default) configuration values.", self.config_path);
            return;
        }

        match fs::read_to_string(&self.config_path) {
            Ok(content) => match serde_json::from_str::<HashMap<String, ConfigValue>>(&content) {
                Ok(loaded_values) => {
                    for (name, loaded_value) in loaded_values {
                        if let Some(item) = self.items.get_mut(&name) {
                            item.value = loaded_value;
                        } else {
                            warn!("Loaded unknown config key '{}' from file. It will be ignored and removed upon next save.", name);
                        }
                    }
                    info!(
                        "Config values loaded and merged successfully from {:?}",
                        self.config_path
                    );
                }
                Err(e) => {
                    error!("Failed to parse config file {:?} as a value map: {}. Using current (likely default) values for items. File might be corrupted or in an old format.", self.config_path, e);
                }
            },
            Err(e) => {
                error!(
                    "Failed to read config file {:?}: {}. Using current (likely default) values for items.",
                    self.config_path, e
                );
            }
        }
    }

    pub fn save_to_file(&self) {
        let values_to_save: HashMap<String, ConfigValue> = self
            .items
            .iter()
            .map(|(name, item)| (name.clone(), item.value.clone()))
            .collect();

        match serde_json::to_string_pretty(&values_to_save) {
            Ok(content) => {
                if let Some(parent) = self.config_path.parent() {
                    if !parent.exists() {
                        if let Err(e) = fs::create_dir_all(parent) {
                            error!(
                                "Failed to create parent directory {:?} for config file: {}",
                                parent, e
                            );
                            return;
                        }
                    }
                }

                if let Err(e) = fs::write(&self.config_path, content) {
                    error!(
                        "Failed to write config file to {:?}: {}",
                        self.config_path, e
                    );
                } else {
                    info!(
                        "Config (values only) saved successfully to {:?}",
                        self.config_path
                    );
                }
            }
            Err(e) => {
                error!(
                    "Failed to serialize config values to JSON for saving: {}",
                    e
                );
            }
        }
    }

    pub fn get_item_value(&self, name: &str) -> Option<ConfigValue> {
        self.items.get(name).map(|item| item.value.clone())
    }

    pub fn get_all_items_vec(&self) -> Vec<ConfigItem> {
        let mut items_vec: Vec<_> = self.items.values().cloned().collect();
        items_vec.sort_by(|a, b| a.name.cmp(&b.name));
        items_vec
    }

    pub fn update_item_value(&mut self, name: &str, new_value: ConfigValue) {
        if name == I18N_CONFIG_KEY {
            rust_i18n::set_locale(&*new_value.to_string());
            info!("Updated rust_i18n to '{}' when saving configuration.", new_value);
        }
        match self.items.get_mut(name) {
            Some(item) => {
                match (&new_value, &item.default_value) {
                    (ConfigValue::String(_), ConfigValue::String(_))
                    | (ConfigValue::Integer(_), ConfigValue::Integer(_)) => {}
                    _ => {
                        error!(
                            "Type mismatch for config item '{}'. Expected type compatible with default value's type ('{}'), got '{}'. Update rejected.",
                            name, item.default_value, new_value
                        );
                        return;
                    }
                }

                item.value = new_value;
                item.validate_and_normalize();
                self.save_to_file();

                if name == PIP_CACHE_DIR_CONFIG_KEY {
                    self.update_pip_cache_env_var_from_config();
                } else if name == PIP_INDEX_URL_CONFIG_KEY {
                    self.update_pip_index_url_env_var_from_config();
                }
                info!("Updated config item '{}' and saved configuration.", name);
            }
            None => {
                error!("Attempted to update non-existent config item: {}", name);
            }
        }
    }

    fn update_pip_cache_env_var_from_config(&self) {
        match self.get_item_value(PIP_CACHE_DIR_CONFIG_KEY) {
            Some(ConfigValue::String(value)) => {
                if value == PIP_CACHE_DIR_OPTION_APP_INSTALL {
                    let cache_dir_path = get_pip_cache_dir();
                    env::set_var("PIP_CACHE_DIR", cache_dir_path.as_os_str());
                    info!(
                        "Set PIP_CACHE_DIR to application specific cache: {:?}",
                        cache_dir_path
                    );
                } else if value == PIP_CACHE_DIR_OPTION_SYSTEM_DEFAULT {
                    env::remove_var("PIP_CACHE_DIR");
                    info!("Unset PIP_CACHE_DIR, using system default pip cache.");
                } else {
                    warn!(
                        "Unknown value '{}' for config item '{}'. PIP_CACHE_DIR not changed.",
                        value, PIP_CACHE_DIR_CONFIG_KEY
                    );
                }
            }
            Some(_) => {
                error!(
                    "Config item '{}' is not a string. PIP_CACHE_DIR not changed.",
                    PIP_CACHE_DIR_CONFIG_KEY
                );
            }
            None => {
                warn!(
                    "Config item '{}' not found. PIP_CACHE_DIR not changed.",
                    PIP_CACHE_DIR_CONFIG_KEY
                );
            }
        }
    }

    fn update_pip_index_url_env_var_from_config(&self) {
        match self.get_item_value(PIP_INDEX_URL_CONFIG_KEY) {
            Some(ConfigValue::String(value)) => {
                if value == PIP_INDEX_URL_OPTION_SYSTEM_DEFAULT || value.is_empty() {
                    env::remove_var("PIP_INDEX_URL");
                    info!("Unset PIP_INDEX_URL, using system default pip index.");
                } else {
                    env::set_var("PIP_INDEX_URL", &value);
                    info!("Set PIP_INDEX_URL to: {}", value);
                }
            }
            Some(_) => {
                error!(
                    "Config item '{}' is not a string. PIP_INDEX_URL not changed.",
                    PIP_INDEX_URL_CONFIG_KEY
                );
            }
            None => {
                warn!(
                    "Config item '{}' not found. PIP_INDEX_URL not changed.",
                    PIP_INDEX_URL_CONFIG_KEY
                );
            }
        }
    }

    pub fn get_effective_pip_cache_dir(&self) -> Option<PathBuf> {
        match self.get_item_value(PIP_CACHE_DIR_CONFIG_KEY) {
            Some(ConfigValue::String(value)) => {
                if value == PIP_CACHE_DIR_OPTION_APP_INSTALL {
                    Some(get_pip_cache_dir())
                } else if value == PIP_CACHE_DIR_OPTION_SYSTEM_DEFAULT {
                    None
                } else {
                    warn!(
                        "Unknown value '{}' for config item '{}'. Using system default pip cache.",
                        value, PIP_CACHE_DIR_CONFIG_KEY
                    );
                    None
                }
            }
            Some(_) => {
                error!(
                    "Config item '{}' is not a string. Using system default pip cache.",
                    PIP_CACHE_DIR_CONFIG_KEY
                );
                None
            }
            None => {
                warn!(
                    "Config item '{}' not found. Using system default pip cache.",
                    PIP_CACHE_DIR_CONFIG_KEY
                );
                None
            }
        }
    }

    pub fn get_effective_pip_index_url(&self) -> Option<String> {
        match self.get_item_value(PIP_INDEX_URL_CONFIG_KEY) {
            Some(ConfigValue::String(value)) => {
                if value == PIP_INDEX_URL_OPTION_SYSTEM_DEFAULT || value.is_empty() {
                    None
                } else {
                    Some(value)
                }
            }
            Some(_) => {
                error!(
                    "Config item '{}' is not a string. Using system default pip index.",
                    PIP_INDEX_URL_CONFIG_KEY
                );
                None
            }
            None => {
                warn!(
                    "Config item '{}' not found. Using system default pip index.",
                    PIP_INDEX_URL_CONFIG_KEY
                );
                None
            }
        }
    }

    pub fn get_effective_update_method(&self) -> &str {
        match self.get_item_value(UPDATE_METHOD_CONFIG_KEY) {
            Some(ConfigValue::String(value)) => match value.as_str() {
                UPDATE_METHOD_OPTION_AUTO => UPDATE_METHOD_OPTION_AUTO,
                UPDATE_METHOD_OPTION_IGNORE => UPDATE_METHOD_OPTION_IGNORE,
                _ => UPDATE_METHOD_OPTION_MANUAL,
            },
            _ => UPDATE_METHOD_OPTION_MANUAL,
        }
    }

    pub fn get_effective_lang(&self) -> &'static str {
        match self.get_item_value(I18N_CONFIG_KEY) {
            Some(ConfigValue::String(value)) => match value.as_str() {
                I18N_OPTION_EN => I18N_OPTION_EN,
                I18N_OPTION_ZH_CN => I18N_OPTION_ZH_CN,
                I18N_OPTION_ZH_TW => I18N_OPTION_ZH_TW,
                I18N_OPTION_ES => I18N_OPTION_ES,
                I18N_OPTION_JA => I18N_OPTION_JA,
                I18N_OPTION_KO => I18N_OPTION_KO,
                _ => get_default_lang_from_locale(),
            },
            _ => get_default_lang_from_locale(),
        }
    }
}

pub type ConfigState = Arc<Mutex<AppConfig>>;

pub static GLOBAL_CONFIG_STATE: OnceCell<ConfigState> = OnceCell::new();

#[tauri::command]
pub fn get_config_payload(state: tauri::State<'_, ConfigState>) -> Result<Vec<ConfigItem>, String> {
    let config_manager = state.lock().unwrap();
    Ok(config_manager.get_all_items_vec())
}

#[tauri::command]
pub fn update_config_item(
    name: String,
    value: serde_json::Value,
    state: tauri::State<'_, ConfigState>,
) -> Result<(), Error> {
    let mut config_manager = state.lock().unwrap();

    let config_value: ConfigValue = serde_json::from_value(value.clone())?;

    config_manager.update_item_value(&name, config_value);
    Ok(())
}

pub fn get_default_locale() -> String {
    sys_locale::get_locale().map_or("en-US".to_string(), |locale| locale.replace('_', "-"))
}

#[tauri::command]
pub fn save_configuration(state: tauri::State<'_, ConfigState>) -> Result<(), String> {
    let config_manager = state.lock().unwrap();
    config_manager.save_to_file();
    Ok(())
}

pub fn init_config_manager(app_handle: &tauri::AppHandle) {
    let config = AppConfig::new();
    rust_i18n::set_locale(config.get_effective_lang());
    let config_state_arc = Arc::new(Mutex::new(config));
    app_handle.manage(config_state_arc.clone());

    if GLOBAL_CONFIG_STATE.set(config_state_arc).is_err() {
        warn!("GLOBAL_CONFIG_STATE was already initialized. This should not happen if init_config_manager is called only once.");
    }
    info!("AppConfig state initialized, managed by Tauri, and set globally.");
}
