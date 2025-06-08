use std::path::{Path, PathBuf};
use sysinfo::{Pid, Process, System};

pub fn is_process_related_to_app_dir(process: &Process, app_dir_canonical: &Path) -> bool {
    if let Some(exe_path) = process.exe() {
        if exe_path.starts_with(app_dir_canonical) {
            // debug!(
            //     "Process {:?} (PID {}) matched by EXE path: {}",
            //     process.name(),
            //     process.pid().as_u32(),
            //     exe_path.display()
            // );
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
