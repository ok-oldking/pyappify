//git.rs
use crate::{emit_info, emit_update_info};
use anyhow::{Context, Result};
use git2::{
    build::CheckoutBuilder, Cred, Error as GitError, ErrorClass, ErrorCode, FetchOptions,
    ObjectType, Oid, Progress, ProxyOptions, RemoteCallbacks, Repository, Sort,
};
use std::collections::HashSet;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use tokio::task;
use tracing::{debug, info, warn};

use crate::app::App;
use crate::submodule;

fn configure_credentials(callbacks: &mut RemoteCallbacks<'static>, url: Option<&str>) {
    if let Some(url_str) = url {
        let url_for_closure = url_str.to_string();
        let remote_url_lower = url_str.trim().to_lowercase();

        if remote_url_lower.starts_with("git@") || remote_url_lower.starts_with("ssh://") {
            callbacks.credentials(move |_url, username_from_url, _allowed_types| {
                Cred::ssh_key_from_agent(username_from_url.unwrap_or("git")).map_err(|e| {
                    GitError::new(
                        ErrorCode::Auth,
                        ErrorClass::Ssh,
                        format!("SSH agent auth failed for {}: {}", url_for_closure, e),
                    )
                })
            });
        } else if remote_url_lower.starts_with("https://") {
            callbacks.credentials(move |_url, _username_from_url, _allowed_types| {
                Cred::default().map_err(|e| {
                    GitError::new(
                        ErrorCode::Auth,
                        ErrorClass::Http,
                        format!("Default creds failed for {}: {}", url_for_closure, e),
                    )
                })
            });
        }
    } else {
        callbacks.credentials(|_url, username_from_url, allowed_types| {
            if allowed_types.contains(git2::CredentialType::SSH_KEY) {
                Cred::ssh_key_from_agent(username_from_url.unwrap_or("git"))
            } else if allowed_types.contains(git2::CredentialType::DEFAULT) {
                Cred::default()
            } else {
                Err(GitError::new(
                    ErrorCode::Auth,
                    ErrorClass::Ssh,
                    "No suitable credential type found and remote URL not available for specific hints.",
                ))
            }
        });
    }
}

fn create_proxy_options() -> ProxyOptions<'static> {
    let mut proxy_opts = ProxyOptions::new();
    proxy_opts.auto();
    proxy_opts
}

fn create_fetch_options(
    callbacks: RemoteCallbacks<'static>,
    depth: Option<u32>,
) -> FetchOptions<'static> {
    let mut fo = FetchOptions::new();
    fo.remote_callbacks(callbacks);
    fo.proxy_options(create_proxy_options());
    if let Some(d) = depth {
        fo.depth(d as i32);
    }
    fo.download_tags(git2::AutotagOption::All);
    fo
}

fn create_transfer_progress_callback(
    app_name: String,
    prefix: String,
) -> impl FnMut(Progress<'_>) -> bool + 'static {
    let mut last_percent = -1.0;
    move |progress: Progress| {
        let received_objects = progress.received_objects();
        let total_objects = progress.total_objects();
        if total_objects > 0 {
            let current_percent = (received_objects as f64 * 100.0) / total_objects as f64;
            let rounded_percent = (current_percent * 10.0).round() / 10.0;
            if (rounded_percent - last_percent).abs() >= 0.1 || received_objects == total_objects {
                emit_update_info!(
                    app_name,
                    "\r{}: {:.1}% ({} / {}) ",
                    prefix,
                    rounded_percent,
                    received_objects,
                    total_objects
                );
                last_percent = rounded_percent;
            }
        } else {
            emit_update_info!(app_name, "\r{}: {} received... ", prefix, received_objects);
        }
        io::stdout().flush().unwrap_or_default();
        true
    }
}

fn get_sorted_tags_by_time(repo: &Repository) -> Result<Vec<String>> {
    let tag_array = repo
        .tag_names(None)
        .with_context(|| format!("Failed to list tags from repository at {:?}", repo.path()))?;

    let mut tags_with_time: Vec<(i64, String)> = Vec::new();

    for tag_name_opt in tag_array.iter() {
        if let Some(tag_name) = tag_name_opt {
            let ref_name = format!("refs/tags/{}", tag_name);
            if let Ok(obj) = repo.revparse_single(&ref_name) {
                if let Ok(commit) = obj.peel_to_commit() {
                    tags_with_time.push((commit.time().seconds(), tag_name.to_string()));
                } else {
                    warn!("Could not peel tag '{}' to a commit.", tag_name);
                }
            } else {
                warn!("Could not resolve tag '{}' to an object.", tag_name);
            }
        }
    }

    tags_with_time.sort_by_key(|k| std::cmp::Reverse(k.0));

    let sorted_tags = tags_with_time.into_iter().map(|(_, name)| name).collect();
    Ok(sorted_tags)
}

pub fn open_repository(repo_path: &Path) -> Result<Repository> {
    Repository::open(repo_path)
        .with_context(|| format!("Failed to open local repo at {}", repo_path.display()))
}

pub fn get_repository_origin_url(repo: &Repository) -> Result<Option<String>> {
    match repo.find_remote("origin") {
        Ok(remote) => Ok(remote.url().map(String::from)),
        Err(e) => {
            if e.code() == ErrorCode::NotFound && e.class() == ErrorClass::Config {
                Ok(None)
            } else {
                Err(anyhow::Error::new(e).context("Failed to find remote 'origin'"))
            }
        }
    }
}

#[tauri::command]
pub async fn get_tags_and_current_version(
    app_name: &str,
    repo_path: PathBuf,
) -> Result<(Vec<String>, String)> {
    let app_name_for_task = app_name.to_string();
    let repo_path_for_task = repo_path.clone();

    let result = task::spawn_blocking(move || -> Result<(Vec<String>, String)> {
        emit_info!(
            app_name_for_task,
            "Fetching all tags for repository at {}",
            repo_path_for_task.display()
        );

        let repo = open_repository(&repo_path_for_task)?;

        let mut remote = repo.find_remote("origin").with_context(|| {
            format!(
                "Failed to find remote 'origin' in repository {}",
                repo_path_for_task.display()
            )
        })?;

        let remote_url = remote.url().map(String::from);

        let mut remote_callbacks = RemoteCallbacks::new();
        configure_credentials(&mut remote_callbacks, remote_url.as_deref());

        let mut fetch_options = create_fetch_options(remote_callbacks, None);

        remote
            .fetch(
                &["+refs/tags/*:refs/tags/*"],
                Some(&mut fetch_options),
                None,
            )
            .with_context(|| {
                format!(
                    "Failed to fetch tags for repository {}",
                    repo_path_for_task.display()
                )
            })?;

        let remote_tags: HashSet<String> = remote
            .list()
            .context("Failed to list remote references")?
            .iter()
            .filter_map(|reference| {
                let name = reference.name();
                if name.starts_with("refs/tags/") {
                    Some(name.trim_start_matches("refs/tags/").to_string())
                } else {
                    None
                }
            })
            .collect();

        let mut local_tags_to_delete = Vec::new();
        repo.tag_names(None)
            .context("Failed to list local tag names")?
            .iter()
            .filter_map(|name| name)
            .for_each(|local_tag_name| {
                if !remote_tags.contains(local_tag_name) {
                    local_tags_to_delete.push(local_tag_name.to_string());
                }
            });

        for tag_name in local_tags_to_delete {
            repo.tag_delete(&tag_name)
                .with_context(|| format!("Failed to delete local tag '{}'", tag_name))?;
            info!("Deleted local tag: {}", tag_name);
        }

        let mut sorted_tags = get_sorted_tags_by_time(&repo)?;

        let head_ref = repo.head().context("Failed to get repo HEAD")?;
        let head_oid = head_ref.target().context("HEAD has no target OID")?;

        let mut current_version_tag: Option<String> = None;
        for tag_name in &sorted_tags {
            let tag_ref_name = format!("refs/tags/{}", tag_name);
            if let Ok(reference) = repo.find_reference(&tag_ref_name) {
                if let Ok(obj) = reference.peel(ObjectType::Commit) {
                    if obj.id() == head_oid {
                        current_version_tag = Some(tag_name.clone());
                        break;
                    }
                } else if let Ok(obj) = reference.peel(ObjectType::Tag) {
                    if let Some(annotated_tag) = obj.as_tag() {
                        if annotated_tag.target_id() == head_oid {
                            current_version_tag = Some(tag_name.clone());
                            break;
                        }
                    }
                }
            }
        }
        let current_version = current_version_tag.unwrap_or_else(|| head_oid.to_string());

        let lts_commit_oid = repo
            .revparse_single("refs/tags/lts")
            .ok()
            .and_then(|obj| obj.peel_to_commit().ok())
            .map(|commit| commit.id());

        if let Some(lts_oid) = lts_commit_oid {
            let lts_version_index = sorted_tags.iter().position(|tag_name| {
                repo.revparse_single(&format!("refs/tags/{}", tag_name))
                    .ok()
                    .and_then(|obj| obj.peel_to_commit().ok())
                    .map_or(false, |commit| commit.id() == lts_oid)
            });

            if let Some(index) = lts_version_index {
                sorted_tags.truncate(index + 1);
            }
        }

        Ok((sorted_tags, current_version))
    })
        .await
        .context("Task for get_tags_and_current_version panicked or was cancelled")??;

    Ok(result)
}

fn format_bytes(bytes: usize) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    } else if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{} bytes", bytes)
    }
}

pub async fn ensure_repository(app: &App) -> Result<()> {
    let repo_path = app.get_repo_path();
    let profile = app.get_current_profile_settings();
    let url = profile.git_url.clone();
    let app_name = app.name.clone();
    info!("ensure_repository {} {}", app_name, &url);
    emit_info!(app_name, "Clone {} from {}", app_name, &url);

    if repo_path.exists() {
        emit_info!(app.name, "Repository already exists {}", repo_path.display());
        match open_repository(&repo_path) {
            Ok(repo) => {
                let current_url = get_repository_origin_url(&repo)?;
                if current_url.as_deref() != Some(url.as_str()) {
                    emit_info!(
                        app_name,
                        "Updating remote origin URL for '{}' to '{}'.",
                        app_name,
                        url
                    );
                    repo.remote_set_url("origin", &url).with_context(|| {
                        format!("Failed to set remote url for {}", repo_path.display())
                    })?;
                }

                emit_info!(app_name, "Fetching updates for existing repository...");
                let repo_path_for_task = repo_path.clone();
                let url_for_task = url.clone();
                let app_name_for_task = app_name.clone();

                task::spawn_blocking(move || -> Result<()> {
                    let repo = open_repository(&repo_path_for_task)?;
                    let mut remote = repo.find_remote("origin")?;
                    let mut callbacks = RemoteCallbacks::new();
                    configure_credentials(&mut callbacks, Some(&url_for_task));

                    let app_name_for_progress = app_name_for_task.clone();
                    callbacks.transfer_progress(create_transfer_progress_callback(
                        app_name_for_progress,
                        "Fetching objects".to_string(),
                    ));

                    let mut fetch_options = create_fetch_options(callbacks, None);
                    let fetch_result = remote
                        .fetch(&["+refs/tags/*:refs/tags/*"], Some(&mut fetch_options), None)
                        .with_context(|| {
                            format!(
                                "Failed to fetch updates for {}",
                                repo_path_for_task.display()
                            )
                        });

                    emit_update_info!(app_name_for_task, "");
                    println!();
                    fetch_result?;
                    emit_info!(app_name_for_task, "Fetch complete.");
                    Ok(())
                })
                    .await
                    .context("Task for fetching updates panicked")??;
                return Ok(());
            }
            Err(e) => {
                warn!(
                    "Directory at {} exists but is not a valid git repository ({}). Removing and re-cloning.",
                    repo_path.display(),
                    e
                );
                fs::remove_dir_all(&repo_path).with_context(|| {
                    format!(
                        "Failed to remove invalid repository directory at {}",
                        repo_path.display()
                    )
                })?;
            }
        }
    }

    let repo_path_for_clone_task = repo_path.to_path_buf();
    let url_for_clone_task = url.to_string();
    let app_name_for_messages = app_name.to_string();

    task::spawn_blocking(move || -> Result<()> {
        let mut callbacks = RemoteCallbacks::new();
        configure_credentials(&mut callbacks, Some(&url_for_clone_task));
        let app_name_for_progress_clone = app_name_for_messages.clone();
        callbacks.transfer_progress({
            let mut last_percent = -1.0;
            move |progress: Progress| {
                let received_objects = progress.received_objects();
                let total_objects = progress.total_objects();
                let indexed_objects = progress.indexed_objects();
                let received_bytes = progress.received_bytes();
                if total_objects > 0 {
                    let current_percent = (received_objects as f64 * 100.0) / total_objects as f64;
                    let rounded_percent = (current_percent * 10.0).round() / 10.0;
                    if (rounded_percent - last_percent).abs() >= 0.1 {
                        emit_update_info!(
                            app_name_for_progress_clone,
                            "\rReceiving objects: {:.1}% ({} / {}) ({}), indexing {} objects... ",
                            rounded_percent,
                            received_objects,
                            total_objects,
                            format_bytes(received_bytes),
                            indexed_objects
                        );
                        last_percent = rounded_percent;
                    }
                } else {
                    emit_update_info!(
                        app_name_for_progress_clone,
                        "\rReceiving objects: {} received ({} bytes), indexing {} objects... ",
                        received_objects,
                        received_bytes,
                        indexed_objects
                    );
                }
                io::stdout().flush().unwrap_or_default();
                true
            }
        });

        let mut fetch_options = create_fetch_options(callbacks, None);
        fetch_options.download_tags(git2::AutotagOption::All);

        let mut builder = git2::build::RepoBuilder::new();
        builder.fetch_options(fetch_options);
        builder.bare(false);

        emit_info!(
            app_name_for_messages,
            "Attempting to clone {} into {}",
            url_for_clone_task,
            repo_path_for_clone_task.display()
        );
        let repo = builder
            .clone(&url_for_clone_task, &repo_path_for_clone_task)
            .with_context(|| format!("Git clone failed for {}", url_for_clone_task))?;

        emit_info!(
            app_name_for_messages,
            "Clone successful. Checking for latest version tag..."
        );

        let sorted_tags = get_sorted_tags_by_time(&repo)?;

        if sorted_tags.is_empty() {
            emit_info!(
                app_name_for_messages,
                "No tags found. Repository will remain on default branch."
            );
            submodule::update_repository_submodules(
                &repo,
                &app_name_for_messages,
                &format!("repository at {}", repo_path_for_clone_task.display()),
            )?;
            return Ok(());
        }

        let latest_tag_name = &sorted_tags[0];

        emit_info!(
            app_name_for_messages,
            "Latest tag found: {}. Attempting checkout.",
            latest_tag_name,
        );

        let obj = repo
            .revparse_single(&format!("refs/tags/{}", latest_tag_name))
            .with_context(|| {
                format!(
                    "Tag '{}' not found locally after clone for checkout",
                    latest_tag_name
                )
            })?;

        repo.checkout_tree(&obj, Some(CheckoutBuilder::new().force()))
            .with_context(|| format!("Failed to checkout tree for tag {}", latest_tag_name))?;

        let commit_oid = obj
            .peel_to_commit()
            .map_or_else(|_| obj.id(), |commit| commit.id());

        repo.set_head_detached(commit_oid).with_context(|| {
            format!(
                "Failed to set head detached to {} for tag {}",
                commit_oid, latest_tag_name
            )
        })?;

        emit_info!(
            app_name_for_messages,
            "Successfully checked out tag {}.",
            latest_tag_name
        );

        submodule::update_repository_submodules(
            &repo,
            &app_name_for_messages,
            &format!(
                "repository at {} after checking out tag {}",
                repo_path_for_clone_task.display(),
                latest_tag_name
            ),
        )?;
        Ok(())
    })
        .await
        .context("Task for ensure_repository panicked or was cancelled")??;
    Ok(())
}

pub async fn checkout_version_tag(
    app_name: &str,
    repo_path: &Path,
    version_tag_name: &str,
) -> Result<Oid> {
    let task_repo_path = repo_path.to_path_buf();
    let tag_to_checkout = version_tag_name.to_string();
    let app_name_for_task = app_name.to_string();

    let oid = task::spawn_blocking(move || -> Result<Oid> {
        let repo = open_repository(&task_repo_path)?;

        let mut remote = repo
            .find_remote("origin")
            .context("Failed to find remote 'origin'")?;

        let mut callbacks = RemoteCallbacks::new();
        configure_credentials(&mut callbacks, remote.url());

        callbacks.transfer_progress(create_transfer_progress_callback(
            app_name_for_task.clone(),
            "Fetching objects for tag".to_string(),
        ));

        let mut fetch_options = create_fetch_options(callbacks, None);

        let refspec = format!("+refs/tags/{0}:refs/tags/{0}", tag_to_checkout);
        emit_info!(
            app_name_for_task,
            "Fetching refspec: {} for repo: {}",
            refspec,
            task_repo_path.display()
        );
        let fetch_result = remote
            .fetch(&[refspec.as_str()], Some(&mut fetch_options), None)
            .with_context(|| {
                format!(
                    "Failed to fetch tag {} for repo {}",
                    tag_to_checkout,
                    task_repo_path.display()
                )
            });
        emit_update_info!(app_name_for_task, "");
        println!();
        fetch_result?;

        debug!("Fetch successful for tag {}", tag_to_checkout);

        let obj = repo
            .revparse_single(&format!("refs/tags/{}", tag_to_checkout))
            .with_context(|| {
                format!(
                    "Tag '{}' not found locally after fetch in repo : {}",
                    tag_to_checkout,
                    task_repo_path.display()
                )
            })?;

        debug!("Revparsed tag {} to object {}", tag_to_checkout, obj.id());

        repo.checkout_tree(&obj, Some(CheckoutBuilder::new().force()))
            .with_context(|| format!("Failed to checkout tree for tag {}", tag_to_checkout))?;
        debug!("Checkout tree successful for tag {}", tag_to_checkout);

        let commit_oid = obj
            .peel_to_commit()
            .map_or_else(|_| obj.id(), |commit| commit.id());

        repo.set_head_detached(commit_oid)
            .with_context(|| format!("Failed to set head detached to {}", commit_oid))?;

        emit_info!(
            app_name_for_task,
            "Successfully checked out and set head to tag {} ({}) for repo {}.",
            tag_to_checkout,
            commit_oid,
            task_repo_path.display()
        );

        submodule::update_repository_submodules(
            &repo,
            &app_name_for_task,
            &format!(
                "repository at {} after checking out tag {}",
                task_repo_path.display(),
                tag_to_checkout
            ),
        )?;

        Ok(commit_oid)
    })
        .await
        .context("Task for checkout_version_tag panicked or was cancelled")??;
    Ok(oid)
}

pub async fn get_commit_messages_for_version_diff(
    repo_path: &Path,
    target_version_tag_name: &str,
) -> Result<Vec<String>> {
    let repo_path_clone = repo_path.to_path_buf();
    let target_tag = target_version_tag_name.to_string();

    let messages = task::spawn_blocking(move || -> Result<Vec<String>> {
        let repo = open_repository(&repo_path_clone)?;
        let mut remote = repo
            .find_remote("origin")
            .context("Failed to find remote 'origin'")?;

        let mut callbacks = RemoteCallbacks::new();
        configure_credentials(&mut callbacks, remote.url());

        let mut fetch_options = create_fetch_options(callbacks, None);

        let head_ref = repo.head().context("Failed to get repo HEAD")?;
        let head_oid = head_ref.target().context("HEAD has no target OID")?;

        let target_tag_ref_str = format!("refs/tags/{}", target_tag);
        if repo.find_reference(&target_tag_ref_str).is_err() {
            let target_refspec = format!("+refs/tags/{0}:refs/tags/{0}", target_tag);
            debug!(
                "Fetching target tag {} as it's not found locally.",
                target_tag
            );
            remote
                .fetch(&[target_refspec.as_str()], Some(&mut fetch_options), None)
                .with_context(|| format!("Failed to fetch target version tag {}", target_tag))?;
        }

        let target_obj = repo.revparse_single(&target_tag_ref_str).with_context(|| {
            format!(
                "Target version tag '{}' not found locally after potential fetch",
                target_tag
            )
        })?;

        let target_commit_oid = target_obj
            .peel_to_commit()
            .with_context(|| format!("Failed to peel tag '{}' to a commit object", target_tag))?
            .id();

        let target_commit = repo.find_commit(target_commit_oid).with_context(|| {
            format!(
                "Failed to find commit for target_commit_oid {}",
                target_commit_oid
            )
        })?;
        let target_commit_summary = target_commit.summary().map(|s| s.to_string());

        if head_oid == target_commit_oid {
            return Ok(Vec::new());
        }

        let is_target_ancestor_of_head = repo
            .graph_descendant_of(head_oid, target_commit_oid)
            .unwrap_or(false);

        if is_target_ancestor_of_head {
            if let Some(summary) = target_commit_summary {
                info!(
                    "HEAD is ahead of target {}. Returning target version's commit message: {}",
                    target_tag, summary
                );
                return Ok(vec![summary]);
            } else {
                info!(
                    "HEAD is ahead of target {}. Target version commit {} has no summary.",
                    target_tag, target_commit_oid
                );
                return Ok(Vec::new());
            }
        } else {
            let mut revwalk = repo.revwalk().context("Failed to create revwalk")?;
            revwalk
                .push(target_commit_oid)
                .with_context(|| format!("Failed to push OID {} to revwalk", target_commit_oid))?;
            revwalk
                .hide(head_oid)
                .with_context(|| format!("Failed to hide OID {} from revwalk", head_oid))?;
            revwalk
                .set_sorting(Sort::TIME)
                .context("Failed to set revwalk sorting")?;

            let mut messages = Vec::new();
            for oid_result in revwalk {
                let oid = oid_result.context("Error iterating revwalk")?;
                let commit = repo
                    .find_commit(oid)
                    .with_context(|| format!("Failed to find commit for OID {}", oid))?;
                if let Some(message) = commit.summary() {
                    info!("Processing commit {}: {}", oid, message);
                    messages.push(message.to_string());
                    if messages.len() >= 10 {
                        break;
                    }
                }
            }
            info!(
                "Found {} commit messages leading up to target {} (not in HEAD {} history)",
                messages.len(),
                target_tag,
                head_oid
            );
            Ok(messages)
        }
    })
        .await
        .context("Task for get_commit_messages panicked or was cancelled")??;
    Ok(messages)
}