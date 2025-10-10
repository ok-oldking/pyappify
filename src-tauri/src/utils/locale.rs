// i18n.rs
use crate::config_manager::GLOBAL_CONFIG_STATE;

pub fn get_locale() -> &'static str {
    if let Some(config_state) = GLOBAL_CONFIG_STATE.get() {
        let config_guard = config_state.lock().unwrap();
        config_guard.get_effective_lang()
    } else {
        "en"
    }
}


