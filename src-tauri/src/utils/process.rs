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
            self.env_remove("Path");
            self.env_remove("PATH");
            self.env("PATH", system_default_path());
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
            self.env_remove("Path");
            self.env_remove("PATH");
            self.env("PATH", system_default_path());
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
