use lazy_static::lazy_static;
use std::env;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

const BASE_DIR: &str = "data";
const APPS_DIR: &str = "apps";
pub const PYTHON_ROOT_DIR: &str = "python";
const WORKING_DIR_NAME: &str = "working";

lazy_static! {
    static ref CWD: PathBuf = env::current_dir().expect("Failed to get current directory");
}
pub fn get_log_dir() -> PathBuf {
    PathBuf::from(BASE_DIR).join("logs")
}
fn get_base_dir() -> PathBuf {
    CWD.join(BASE_DIR)
}
pub fn get_python_dir(app_name: &str) -> PathBuf {
    get_app_base_path(app_name).join(PYTHON_ROOT_DIR)
}

pub fn get_cwd() -> PathBuf {
    CWD.clone()
}

pub fn get_python_exe(app_name: &str, use_pythonw: bool) -> PathBuf {
    let python_dir = get_python_dir(app_name);
    if use_pythonw {
        python_dir.join("pythonw.exe")
    } else {
        python_dir.join("python.exe")
    }
}

pub fn get_apps_dir() -> PathBuf {
    get_base_dir().join(APPS_DIR)
}
pub fn get_app_repo_path(app_name: &str) -> PathBuf {
    get_app_base_path(app_name).join("repo")
}

pub fn get_app_base_path(app_name: &str) -> PathBuf {
    get_apps_dir().join(app_name)
}

pub fn get_app_working_dir_path(app_name: &str) -> PathBuf {
    get_app_base_path(app_name).join(WORKING_DIR_NAME)
}
pub fn get_pip_cache_dir() -> PathBuf {
    CWD.join("cache").join("pip")
}

pub fn get_config_dir() -> PathBuf {
    get_base_dir().join("config")
}

pub fn get_start_dir(app_handle: AppHandle) -> PathBuf {
    app_handle
        .path()
        .config_dir()
        .map(|path| path.join("Microsoft\\Windows\\Start Menu\\Programs"))
        .unwrap()
}

fn strip_extended_path_prefix(path_str: &str) -> String {
    if path_str.starts_with("\\\\?\\") {
        path_str[4..].to_string()
    } else {
        path_str.to_string()
    }
}

pub fn path_to_abs(path: &Path) -> String {
    if let Ok(absolute_path_buf) = path.canonicalize() {
        if let Some(s_ref) = absolute_path_buf.to_str() {
            return strip_extended_path_prefix(s_ref);
        }
    } else {
        if let Ok(current_dir) = env::current_dir() {
            let absolute_path_buf = current_dir.join(path);
            if let Some(s_ref) = absolute_path_buf.to_str() {
                return strip_extended_path_prefix(s_ref);
            }
        }
    }

    let path_cow = path.to_string_lossy();
    strip_extended_path_prefix(&path_cow)
}
