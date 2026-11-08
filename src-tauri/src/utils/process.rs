use std::path::Path;
use std::process::Command as StdCommand;
use sysinfo::{Pid, Process, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
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

/// Refreshes the process list with only executable paths, which is all the app
/// matching needs. Skipping CPU, memory, command line and environment data makes
/// a full scan much cheaper than `refresh_processes(All, true)`.
pub fn refresh_process_exes(sys: &mut System) {
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_exe(UpdateKind::OnlyIfNotSet),
    );
}

/// Takes a fresh process snapshot and returns the processes whose executable
/// lives under `app_dir_canonical`. Blocking; call from a blocking context.
pub fn scan_pids_related_to_app_dir(app_dir_canonical: &Path) -> Vec<Pid> {
    let mut sys = System::new();
    refresh_process_exes(&mut sys);
    get_pids_related_to_app_dir(&sys, app_dir_canonical)
}

pub fn get_pids_related_to_app_dir(sys: &System, app_dir_canonical: &Path) -> Vec<Pid> {
    let mut related_pids = Vec::new();
    for (pid, process) in sys.processes() {
        if is_process_related_to_app_dir(process, app_dir_canonical) {
            related_pids.push(*pid);
        }
    }
    related_pids
}
