#[cfg(windows)]
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;
use sysinfo::{Pid, Process, System};
use tokio::process::Command as TokioCommand;

pub const PYTHON_ENVS_TO_REMOVE: [&str; 13] = [
    "PYTHONHOME",
    "PYTHONSTARTUP",
    "VIRTUAL_ENV",
    "PYTHONPATH",
    "PYTHONUSERBASE",
    "PYTHONCASEOK",
    "PYTHONHASHSEED",
    "PYTHONOPTIMIZE",
    "PYTHONVERBOSE",
    "PYTHONDEBUG",
    "PYTHONWARNINGS",
    "PYTHONIOENCODING",
    "PYTHONINSPECT",
];
#[cfg(windows)]
fn system_default_path() -> String {
    let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string());
    [
        format!("{}\\system32", system_root),
        system_root.clone(),
        format!("{}\\System32\\Wbem", system_root),
        format!("{}\\System32\\WindowsPowerShell\\v1.0\\", system_root),
        format!("{}\\System32\\OpenSSH\\", system_root),
    ]
    .join(";")
}

#[cfg(windows)]
fn inherited_path_or_default() -> OsString {
    for key in ["PATH", "Path"] {
        if let Some(value) = std::env::var_os(key) {
            if !value.is_empty() {
                return value;
            }
        }
    }

    OsString::from(system_default_path())
}

pub trait RemovePythonEnvsExt {
    fn clear_python_envs(&mut self) -> &mut Self;
}
impl RemovePythonEnvsExt for StdCommand {
    fn clear_python_envs(&mut self) -> &mut Self {
        for env in PYTHON_ENVS_TO_REMOVE {
            self.env_remove(env);
        }
        #[cfg(windows)]
        {
            // 保留启动器继承到的完整 PATH，避免子进程缺少 Qt、驱动或工具路径。
            self.env("PATH", inherited_path_or_default());
        }
        self.env("PYTHONNOUSERSITE", "1");
        self
    }
}
impl RemovePythonEnvsExt for TokioCommand {
    fn clear_python_envs(&mut self) -> &mut Self {
        for env in PYTHON_ENVS_TO_REMOVE {
            self.env_remove(env);
        }
        #[cfg(windows)]
        {
            // 保留启动器继承到的完整 PATH，避免子进程缺少 Qt、驱动或工具路径。
            self.env("PATH", inherited_path_or_default());
        }
        self.env("PYTHONNOUSERSITE", "1");
        self
    }
}
pub fn is_process_related_to_app_dir(process: &Process, app_dir_canonical: &Path) -> bool {
    if let Some(exe_path) = process.exe() {
        if exe_path.starts_with(app_dir_canonical) {
            return true;
        }
    }
    false
}

pub fn get_pids_related_to_app_dir(sys: &System, app_dir_canonical: &PathBuf) -> Vec<Pid> {
    let mut related_pids = Vec::new();
    for (pid, process) in sys.processes() {
        if is_process_related_to_app_dir(process, app_dir_canonical) {
            related_pids.push(*pid);
        }
    }
    related_pids
}
