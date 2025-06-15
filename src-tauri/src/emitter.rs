// src-tauri/src/emit.rs
use once_cell::sync::OnceCell;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Wry};
use tracing::{debug, error};

static GLOBAL_APP_HANDLE: OnceCell<AppHandle<Wry>> = OnceCell::new();

pub fn init_app_handle(handle: AppHandle<Wry>) {
    if GLOBAL_APP_HANDLE.set(handle).is_err() {
        error!("Global AppHandle was already initialized. This call was ignored.");
    }
}

#[derive(Clone, Serialize)]
struct MessagePayload<'a> {
    app_name: String,
    message: &'a str,
    #[serde(default)]
    update: bool,
    #[serde(default)]
    finished: bool,
    #[serde(default)]
    error: bool,
}

fn get_app_handle() -> Option<&'static AppHandle<Wry>> {
    GLOBAL_APP_HANDLE.get()
}

pub fn emit<S: Serialize + Clone>(event_name: &str, payload: S) {
    if let Some(handle) = get_app_handle() {
        if let Err(e) = handle.emit(event_name, payload) {
            error!("Failed to emit event '{}': {}", event_name, e);
        }
    } else {
        debug!("AppHandle not initialized. Cannot emit event '{}'.", event_name);
    }
}

#[doc(hidden)]
pub(crate) fn emit_log_impl(
    app_name: String,
    original_message: &str,
    is_update_param: bool,
    is_error: bool,
) {
    if is_error && original_message.is_empty() {
        error!(
            "Attempted to emit an empty error message for app: {} (update: {})",
            app_name, is_update_param
        );
        return;
    }

    let mut actual_message = original_message;
    let mut final_is_update = is_update_param;

    if original_message.ends_with('\r') {
        final_is_update = true;
        actual_message = original_message
            .strip_suffix('\r')
            .unwrap_or(original_message);
    }

    emit(
        "app-log",
        MessagePayload {
            app_name: app_name.clone(),
            message: actual_message,
            update: final_is_update,
            finished: false,
            error: is_error,
        },
    );

    let prefix = if final_is_update { "UPDATE " } else { "" };
    let log_type = if is_error { "ERROR" } else { "INFO" };

    if is_error {
        error!("{}{} [{}]: {}", prefix, log_type, app_name, actual_message);
    } else {
        println!("{}{} [{}]: {}", prefix, log_type, app_name, actual_message);
    }
}

#[doc(hidden)]
pub(crate) fn emit_finish_impl(app_name: String, is_error: bool) {
    emit(
        "app-log",
        MessagePayload {
            app_name: app_name.clone(),
            message: "",
            update: false,
            finished: true,
            error: is_error,
        },
    );
    let status = if is_error { "FAILED" } else { "COMPLETED" };
    println!("FINISHED [{}]: Process {}.", app_name, status);
}

#[macro_export]
macro_rules! emit_info {
    ($app_name:expr, $fmt:literal $(, $($args:tt)*)?) => {
        $crate::emitter::emit_log_impl($app_name.to_string(), &::std::format!($fmt $(, $($args)*)?), false, false);
    };
    ($app_name:expr, $message:expr) => {
        $crate::emitter::emit_log_impl($app_name.to_string(), &::std::format!("{}", $message), false, false);
    };
}

#[macro_export]
macro_rules! emit_error {
    ($app_name:expr, $fmt:literal $(, $($args:tt)*)?) => {
        $crate::emitter::emit_log_impl($app_name.to_string(), &::std::format!($fmt $(, $($args)*)?), false, true);
    };
    ($app_name:expr, $message:expr) => {
        $crate::emitter::emit_log_impl($app_name.to_string(), &::std::format!("{}", $message), false, true);
    };
}

#[macro_export]
macro_rules! emit_update_info {
    ($app_name:expr, $fmt:literal $(, $($args:tt)*)?) => {
        $crate::emitter::emit_log_impl($app_name.to_string(), &::std::format!($fmt $(, $($args)*)?), true, false);
    };
    ($app_name:expr, $message:expr) => {
        $crate::emitter::emit_log_impl($app_name.to_string(), &::std::format!("{}", $message), true, false);
    };
}

#[macro_export]
macro_rules! emit_update_error {
    ($app_name:expr, $fmt:literal $(, $($args:tt)*)?) => {
        $crate::emitter::emit_log_impl($app_name.to_string(), &::std::format!($fmt $(, $($args)*)?), true, true);
    };
    ($app_name:expr, $message:expr) => {
        $crate::emitter::emit_log_impl($app_name.to_string(), &::std::format!("{}", $message), true, true);
    };
}

#[macro_export]
macro_rules! emit_success_finish {
    ($app_name:expr) => {
        $crate::emitter::emit_finish_impl($app_name.to_string(), false);
    };
}

#[macro_export]
macro_rules! emit_error_finish {
    ($app_name:expr) => {
        $crate::emitter::emit_finish_impl($app_name.to_string(), true);
    };
}

pub fn emit_custom_event<S: Serialize + Clone>(event_name: &str, payload: S) {
    emit(event_name, payload);
}
