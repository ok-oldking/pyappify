use crate::app::App;
use crate::python_env::get_supported_python_versions;
use crate::{git, utils::error::Error, utils::path};
use anyhow::{anyhow, Context, Result};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use std::path::Path;
use tracing::{error, info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppInstallDetails {
    pub name: String,
    pub url: String,
    pub python_versions: Vec<String>,
}

#[tauri::command]
pub fn get_app_install_details_by_url(url: &str) -> Result<AppInstallDetails, Error> {
    let trimmed_url = url.trim();
    let path_str = if trimmed_url.starts_with("http") || trimmed_url.starts_with("git@") {
        if let Ok(parsed_url) = Url::parse(trimmed_url) {
            parsed_url.path().to_string()
        } else if let Some(colon_pos) = trimmed_url.find(':') {
            if !trimmed_url.starts_with("ssh://") || trimmed_url[colon_pos + 1..].contains('/') {
                trimmed_url[colon_pos + 1..].to_string()
            } else {
                Url::parse(trimmed_url)
                    .with_context(|| format!("Failed to parse SSH URL part for {}", url))?
                    .path()
                    .to_string()
            }
        } else {
            return Err(anyhow::anyhow!("Could not parse path from URL: {}", url).into());
        }
    } else {
        trimmed_url.to_string()
    };
    let app_name = Path::new(&path_str)
        .file_name()
        .and_then(|s| s.to_str())
        .map(|s| s.strip_suffix(".git").unwrap_or(s).to_string())
        .ok_or_else(|| anyhow!("Could not extract app name from URL path: {}", path_str))?;
    Ok(AppInstallDetails {
        name: app_name,
        url: url.to_string(),
        python_versions: get_supported_python_versions(),
    })
}

pub async fn clone_app(url: String) -> Result<App> {
    let details = get_app_install_details_by_url(&url)?;
    let app_name = details.name;
    let repo_path = path::get_app_repo_path(&app_name);
    let base_path = path::get_app_base_path(&app_name);
    let app_dir_lock = crate::app_service::get_app_lock(&app_name).await;
    let _guard = app_dir_lock.lock().await;

    if base_path.exists() {
        warn!(
            "App directory '{}' already exists. Attempting to load and refresh it.",
            base_path.display()
        );
        match crate::app_service::load_app_details(app_name.clone()).await {
            Ok(app) => {
                info!("Existing app '{}' details refreshed.", app.name);
                return Ok(app);
            }
            Err(load_err) => {
                error!("Existing directory {} failed to load/refresh: {}. Deleting and attempting fresh clone.", base_path.display(), load_err);
                tokio::fs::remove_dir_all(&base_path)
                    .await
                    .with_context(|| {
                        format!("Failed to delete conflicting dir {}", base_path.display())
                    })?;
                info!("Conflicting directory deleted. Proceeding with fresh clone.");
            }
        }
    }

    info!("Cloning app '{}' from {}", app_name, &url);
    if let Some(parent) = repo_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("Failed to create parent dir {}", parent.display()))?;
    }

    git::clone_repository(app_name.as_str(), &url, &repo_path).await?;

    info!(
        "Successfully cloned app '{}'. Initial checkout is likely default branch.",
        app_name
    );

    let app = crate::app_service::load_app_details(app_name.clone()).await?;

    info!(
        "Registered newly cloned app '{}' (Version: {:?}, Available: {} tags, Running: {}) into managed list.",
        app.name,
        app.current_version,
        app.available_versions.len(),
        app.running
    );
    crate::app_service::emit_apps().await;
    Ok(app)
}
