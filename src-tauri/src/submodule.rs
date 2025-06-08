use crate::emit_info;
use anyhow::{Context, Result};
use git2::Repository;

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
        for mut submodule in submodules {
            submodule.update(true, None).with_context(|| {
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
