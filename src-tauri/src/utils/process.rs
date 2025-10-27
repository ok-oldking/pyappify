use std::path::{Path, PathBuf};
use sysinfo::{Pid, Process, System};
use std::process::Command as StdCommand;
use tokio::process::Command as TokioCommand;

pub const PYTHON_ENVS_TO_REMOVE: [&str; 6] = ["PYTHONHOME", "PYTHONSTARTUP", "VIRTUAL_ENV", "Path", "PYTHONPATH", "PYTHONUSERBASE"];
pub trait RemovePythonEnvsExt {
    fn clear_python_envs(&mut self) -> &mut Self;
}
impl RemovePythonEnvsExt for StdCommand {
    fn clear_python_envs(&mut self) -> &mut Self {
        for env in PYTHON_ENVS_TO_REMOVE {
            self.env(env, "");
        }
        self
    }
}
impl RemovePythonEnvsExt for TokioCommand {
    fn clear_python_envs(&mut self) -> &mut Self {
        for env in PYTHON_ENVS_TO_REMOVE {
            self.env(env, "");
        }
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
