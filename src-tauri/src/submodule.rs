use crate::emit_info;
use crate::utils::path::relative_path;
use anyhow::{Context, Result};
use git2::{build::CheckoutBuilder, Repository, SubmoduleUpdateOptions};
use std::fs;
use std::path::Path;
use tracing::{info, warn};

pub fn update_repository_submodules(
    repo: &Repository,
    app_name: &str,
    context_message: &str,
) -> Result<()> {
    let submodules = repo
        .submodules()
        .with_context(|| format!("Failed to load submodules for {}", context_message))?;

    if !submodules.is_empty() {
        emit_info!(
            app_name,
            "Found {} submodules for {}. Updating them...",
            submodules.len(),
            context_message
        );
        link_submodule_work_trees(repo)
            .with_context(|| format!("Failed to link submodules for {}", context_message))?;
        for mut submodule in submodules {
            // Forced like the app's own checkout, so missing or modified files
            // are restored (libgit2's default checkout leaves them as they are).
            let mut checkout = CheckoutBuilder::new();
            checkout.force();
            let mut options = SubmoduleUpdateOptions::new();
            options.checkout(checkout);
            submodule
                .update(true, Some(&mut options))
                .with_context(|| {
                    format!(
                        "Failed to update submodule '{}' for {}",
                        submodule.name().unwrap_or("<unknown>"),
                        context_message
                    )
                })?;
            emit_info!(
                app_name,
                "Successfully updated submodule: {} for {}",
                submodule.name().unwrap_or("<unknown>"),
                context_message
            );
        }
        emit_info!(app_name, "All submodules updated for {}.", context_message);
    } else {
        emit_info!(
            app_name,
            "No submodules found to update for {}.",
            context_message
        );
    }
    Ok(())
}

/// Points each submodule folder in the work tree at its git data in
/// `<git dir>/modules/<name>`, as `git submodule absorbgitdirs` does.
///
/// The app's work tree is separate from its git directory, so a submodule
/// folder can lack its `.git` link: after migrating from the old layout (whose
/// copies skipped `.git`), or after the working directory was recreated.
/// Without the link libgit2 would try to clone into the non-empty folder.
/// A folder whose git data is gone is removed so the update clones it again.
pub fn link_submodule_work_trees(repo: &Repository) -> Result<()> {
    let Some(work_tree) = repo.workdir() else {
        return Ok(());
    };
    let work_tree = std::path::absolute(work_tree)?;
    let git_dir = std::path::absolute(repo.path())?;
    for submodule in repo.submodules()? {
        let Ok(name) = submodule.name() else {
            continue;
        };
        let module_work_tree = work_tree.join(submodule.path());
        if module_work_tree.join(".git").exists() {
            continue;
        }
        let module_git_dir = git_dir.join("modules").join(name);
        if !module_git_dir.join("HEAD").is_file() {
            if is_non_empty_dir(&module_work_tree) {
                warn!(
                    "Submodule '{}' has files at {} but no git data. Removing them so it is cloned again.",
                    name,
                    module_work_tree.display()
                );
                fs::remove_dir_all(&module_work_tree)
                    .with_context(|| format!("Failed to remove {}", module_work_tree.display()))?;
            }
            continue;
        }

        fs::create_dir_all(&module_work_tree)
            .with_context(|| format!("Failed to create {}", module_work_tree.display()))?;
        let git_dir_link = slash_path(&relative_path(&module_work_tree, &module_git_dir));
        fs::write(
            module_work_tree.join(".git"),
            format!("gitdir: {git_dir_link}\n"),
        )
        .with_context(|| format!("Failed to link submodule '{}'", name))?;
        let mut config = git2::Config::open(&module_git_dir.join("config"))
            .with_context(|| format!("Failed to open config of submodule '{}'", name))?;
        config
            .set_str(
                "core.worktree",
                &slash_path(&relative_path(&module_git_dir, &module_work_tree)),
            )
            .with_context(|| format!("Failed to set work tree of submodule '{}'", name))?;
        info!(
            "Linked submodule '{}' at {} to {}",
            name,
            module_work_tree.display(),
            module_git_dir.display()
        );
    }
    Ok(())
}

fn is_non_empty_dir(path: &Path) -> bool {
    fs::read_dir(path)
        .map(|mut entries| entries.next().is_some())
        .unwrap_or(false)
}

fn slash_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}
