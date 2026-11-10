//git.rs
use crate::utils::path::get_working_dir_for_repo;
use crate::{app::App, emit_info, emit_update_info, submodule};
use anyhow::{Context, Result};
use dashmap::DashMap;
use git2::{
    build::CheckoutBuilder, opts, Cred, Direction, Error as GitError, ErrorClass, ErrorCode,
    FetchOptions, ObjectType, Oid, Progress, ProxyOptions, RemoteCallbacks, Repository, Sort,
};
use once_cell::sync::Lazy;
use regex::Regex;
use std::cmp::Ordering;
use std::collections::HashSet;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use tokio::{
    sync::{Mutex, OwnedMutexGuard},
    task,
};
use tracing::{debug, info, warn};

const MAX_UPDATE_NOTE_LINES: usize = 10;

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub struct VersionHistoryEntry {
    pub version: String,
    pub previous_version: Option<String>,
    pub update_note: Vec<String>,
}

static REPO_LOCKS: Lazy<DashMap<PathBuf, Arc<Mutex<()>>>> = Lazy::new(DashMap::new);
static GIT_CONFIG_INITIALIZED: OnceLock<()> = OnceLock::new();

/// Serializes all git operations on one repository path.
async fn lock_repo(repo_path: &Path) -> OwnedMutexGuard<()> {
    let lock = REPO_LOCKS
        .entry(repo_path.to_path_buf())
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone();
    lock.lock_owned().await
}

/// Force-checks out `obj` and detaches HEAD at the commit it points to.
fn checkout_detached(repo: &Repository, obj: &git2::Object<'_>, label: &str) -> Result<Oid> {
    with_checkout_retries(label, || {
        repo.checkout_tree(obj, Some(CheckoutBuilder::new().force()))
    })?;
    let commit_oid = obj
        .peel_to_commit()
        .map_or_else(|_| obj.id(), |commit| commit.id());
    repo.set_head_detached(commit_oid)
        .with_context(|| format!("Failed to set head detached to {commit_oid} for {label}"))?;
    Ok(commit_oid)
}

fn resolve_tag_commit(repo: &Repository, tag: &str) -> Result<Oid> {
    Ok(repo
        .revparse_single(&format!("refs/tags/{tag}"))
        .with_context(|| format!("Failed to resolve version tag '{tag}'"))?
        .peel_to_commit()
        .with_context(|| format!("Failed to peel version tag '{tag}' to a commit"))?
        .id())
}

#[derive(Debug, Clone, Eq, PartialEq)]
enum Prerelease {
    Alpha(Option<u64>),
    Beta(Option<u64>),
    Rc(Option<u64>),
    Release,
}

impl Prerelease {
    fn rank(&self) -> u8 {
        match self {
            Self::Alpha(_) => 0,
            Self::Beta(_) => 1,
            Self::Rc(_) => 2,
            Self::Release => 3,
        }
    }

    fn number(&self) -> Option<u64> {
        match self {
            Self::Alpha(number) | Self::Beta(number) | Self::Rc(number) => *number,
            Self::Release => None,
        }
    }

    fn is_release(&self) -> bool {
        matches!(self, Self::Release)
    }
}

impl Ord for Prerelease {
    fn cmp(&self, other: &Self) -> Ordering {
        self.rank()
            .cmp(&other.rank())
            .then_with(|| self.number().cmp(&other.number()))
    }
}

impl PartialOrd for Prerelease {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
struct VersionKey {
    major: u64,
    minor: u64,
    patch: u64,
    prerelease: Prerelease,
}

fn parse_version_tag(tag_name: &str) -> Option<VersionKey> {
    static VERSION_REGEX: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r"^v?(\d+)\.(\d+)\.(\d+)(?:(?:-|\.)(alpha|beta|rc)(?:\.(\d+))?)?$").unwrap()
    });

    let caps = VERSION_REGEX.captures(tag_name)?;
    let major = caps.get(1)?.as_str().parse::<u64>().ok()?;
    let minor = caps.get(2)?.as_str().parse::<u64>().ok()?;
    let patch = caps.get(3)?.as_str().parse::<u64>().ok()?;
    let prerelease_number = caps
        .get(5)
        .map(|m| m.as_str().parse::<u64>())
        .transpose()
        .ok()?;
    let prerelease = match caps.get(4).map(|m| m.as_str()) {
        Some("alpha") => Prerelease::Alpha(prerelease_number),
        Some("beta") => Prerelease::Beta(prerelease_number),
        Some("rc") => Prerelease::Rc(prerelease_number),
        None => Prerelease::Release,
        _ => return None,
    };

    Some(VersionKey {
        major,
        minor,
        patch,
        prerelease,
    })
}

pub fn compare_version_tags(left: &str, right: &str) -> Option<Ordering> {
    Some(parse_version_tag(left)?.cmp(&parse_version_tag(right)?))
}

pub fn is_version_tag(tag_name: &str) -> bool {
    parse_version_tag(tag_name).is_some()
}

pub fn is_release_version(tag_name: &str) -> bool {
    parse_version_tag(tag_name)
        .map(|version| version.prerelease.is_release())
        .unwrap_or(false)
}

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

/// With `all_tags`, every remote tag is downloaded along with the refspecs;
/// otherwise only the refspecs are.
fn create_fetch_options(
    callbacks: RemoteCallbacks<'static>,
    depth: Option<u32>,
    all_tags: bool,
) -> FetchOptions<'static> {
    let mut fo = FetchOptions::new();
    fo.remote_callbacks(callbacks);
    fo.proxy_options(create_proxy_options());
    if let Some(d) = depth {
        fo.depth(d as i32);
    }
    fo.download_tags(if all_tags {
        git2::AutotagOption::All
    } else {
        git2::AutotagOption::None
    });
    fo
}

/// Fetches `refspecs` from `origin`, reporting transfer progress under
/// `progress_prefix` when given.
fn fetch_from_origin(
    repo: &Repository,
    app_name: &str,
    refspecs: &[String],
    depth: Option<u32>,
    all_tags: bool,
    progress_prefix: Option<&str>,
) -> Result<()> {
    let mut remote = repo
        .find_remote("origin")
        .context("Failed to find remote 'origin'")?;
    let remote_url = remote.url().ok().map(String::from);
    let mut callbacks = RemoteCallbacks::new();
    configure_credentials(&mut callbacks, remote_url.as_deref());
    if let Some(prefix) = progress_prefix {
        callbacks.transfer_progress(create_transfer_progress_callback(
            app_name.to_string(),
            prefix,
            false,
        ));
    }
    let mut fetch_options = create_fetch_options(callbacks, depth, all_tags);
    fetch_options.prune(git2::FetchPrune::On);
    let refspecs: Vec<&str> = refspecs.iter().map(String::as_str).collect();
    let result = remote
        .fetch(&refspecs, Some(&mut fetch_options), None)
        .with_context(|| {
            format!(
                "Failed to fetch {} from {}",
                refspecs.join(" "),
                remote_url.as_deref().unwrap_or("origin")
            )
        });
    if progress_prefix.is_some() {
        emit_update_info!(app_name, "");
    }
    result
}

fn tag_refspec(tag: &str) -> String {
    format!("+refs/tags/{0}:refs/tags/{0}", tag)
}

/// Repository config key holding the app's `keep_releases`, so every git
/// operation on the repository (and the Python package's standalone updater)
/// applies it without needing the app's configuration.
const KEEP_RELEASES_CONFIG_KEY: &str = "pyappify.keepreleases";
/// Shallow clones start at this depth and deepen until the history between
/// the kept releases is complete.
const SHALLOW_START_DEPTH: u32 = 100;
const SHALLOW_MAX_DEPTH: u32 = 25_600;

/// The number of releases the repository keeps, or `None` to keep every tag.
fn keep_releases(repo: &Repository) -> Option<usize> {
    let value = repo.config().ok()?.get_i64(KEEP_RELEASES_CONFIG_KEY).ok()?;
    usize::try_from(value).ok().filter(|keep| *keep > 0)
}

fn set_keep_releases(repo: &Repository, keep: Option<usize>) -> Result<()> {
    if keep_releases(repo) == keep {
        return Ok(());
    }
    let mut config = repo.config().context("Failed to open repository config")?;
    match keep {
        Some(keep) => config
            .set_i64(KEEP_RELEASES_CONFIG_KEY, keep as i64)
            .with_context(|| format!("Failed to set {KEEP_RELEASES_CONFIG_KEY}")),
        None => match config.remove(KEEP_RELEASES_CONFIG_KEY) {
            Err(error) if error.code() != ErrorCode::NotFound => Err(anyhow::Error::new(error)
                .context(format!("Failed to remove {KEEP_RELEASES_CONFIG_KEY}"))),
            _ => Ok(()),
        },
    }
}

/// The version tags to keep: the `keep` newest releases and the prereleases
/// newer than the oldest of them, newest first. With fewer releases than
/// `keep`, every version tag is kept.
fn select_kept_tags<'a>(tags: impl IntoIterator<Item = &'a str>, keep: usize) -> Vec<String> {
    let mut versions: Vec<(VersionKey, &str)> = tags
        .into_iter()
        .filter_map(|tag| parse_version_tag(tag).map(|key| (key, tag)))
        .collect();
    versions.sort_by(|a, b| b.0.cmp(&a.0));
    let mut kept = Vec::new();
    let mut releases = 0;
    for (key, tag) in versions {
        if releases >= keep {
            break;
        }
        if key.prerelease.is_release() {
            releases += 1;
        }
        kept.push(tag.to_string());
    }
    kept
}

/// The tags pointing at the checked-out commit.
fn head_tags(repo: &Repository) -> Vec<String> {
    let Some(head) = repo.head().ok().and_then(|head| head.target()) else {
        return Vec::new();
    };
    repo.tag_names(None)
        .map(|names| {
            names
                .iter()
                .filter_map(|tag| tag.ok().flatten())
                .filter(|tag| resolve_tag_commit(repo, tag).ok() == Some(head))
                .map(String::from)
                .collect()
        })
        .unwrap_or_default()
}

/// The local tags to keep given the remote's tags: all of them, or with
/// `keep_releases` the kept versions, the tag being checked out (`extra`) and
/// the tags of the checked-out version.
fn tags_to_keep(
    repo: &Repository,
    remote_tags: &HashSet<String>,
    extra: Option<&str>,
) -> HashSet<String> {
    let Some(keep) = keep_releases(repo) else {
        return remote_tags.clone();
    };
    let mut kept: HashSet<String> = select_kept_tags(remote_tags.iter().map(String::as_str), keep)
        .into_iter()
        .collect();
    kept.extend(
        head_tags(repo)
            .into_iter()
            .chain(extra.map(String::from))
            .filter(|tag| remote_tags.contains(tag)),
    );
    kept
}

/// Fetches the remote's tags (and branches with `include_heads`) and removes
/// local tags that are not kept, see [`tags_to_keep`]. With `keep_releases`,
/// only the kept tags are fetched and branches are skipped.
fn sync_tags(
    repo: &Repository,
    app_name: &str,
    include_heads: bool,
    progress_prefix: Option<&str>,
) -> Result<()> {
    let remote_tags = list_remote_tags(repo)?;
    if keep_releases(repo).is_some() {
        let mut kept: Vec<String> = tags_to_keep(repo, &remote_tags, None).into_iter().collect();
        kept.sort();
        let refspecs: Vec<String> = kept.iter().map(|tag| tag_refspec(tag)).collect();
        if !refspecs.is_empty() {
            fetch_from_origin(repo, app_name, &refspecs, None, false, progress_prefix)?;
        }
        return prune_local_tags(repo, app_name, &kept.into_iter().collect());
    }
    let mut refspecs = vec!["+refs/tags/*:refs/tags/*".to_string()];
    if include_heads {
        refspecs.insert(0, "+refs/heads/*:refs/remotes/origin/*".to_string());
    }
    fetch_from_origin(repo, app_name, &refspecs, None, true, progress_prefix)?;
    prune_local_tags(repo, app_name, &remote_tags)
}

/// Removes local tags the remote no longer has, or with `keep_releases` the
/// ones that are not kept. `extra` names a tag that must stay.
fn prune_tags(repo: &Repository, app_name: &str, extra: Option<&str>) -> Result<()> {
    let remote_tags = list_remote_tags(repo)?;
    prune_local_tags(repo, app_name, &tags_to_keep(repo, &remote_tags, extra))
}

/// The commits listed in the repository's `shallow` file: the commits whose
/// parents were not fetched.
fn shallow_roots(repo: &Repository) -> HashSet<Oid> {
    fs::read_to_string(repo.path().join("shallow"))
        .map(|content| {
            content
                .lines()
                .filter_map(|line| Oid::from_str(line.trim()).ok())
                .collect()
        })
        .unwrap_or_default()
}

/// Whether the history between the kept tags (newest first) is complete, so
/// update notes between them can be read: walking from the kept tags down to
/// the oldest one reaches no commit whose parents are missing.
fn kept_history_complete(repo: &Repository, kept: &[String]) -> Result<bool> {
    let roots = shallow_roots(repo);
    let Some(oldest) = kept.last() else {
        return Ok(true);
    };
    if roots.is_empty() {
        return Ok(true);
    }
    let mut revwalk = repo.revwalk().context("Failed to create revwalk")?;
    for tag in kept {
        revwalk.push(resolve_tag_commit(repo, tag)?)?;
    }
    revwalk.hide(resolve_tag_commit(repo, oldest)?)?;
    for oid in revwalk {
        match oid {
            Ok(oid) if roots.contains(&oid) => return Ok(false),
            Ok(_) => {}
            Err(_) => return Ok(false),
        }
    }
    Ok(true)
}

/// Creates the bare repository at `repo_path` with only the kept releases of
/// `url` and as much history as their update notes need. Returns `None`, with
/// nothing created, when the remote has no version tags.
fn clone_kept_releases(
    url: &str,
    repo_path: &Path,
    app_name: &str,
    keep: usize,
) -> Result<Option<Repository>> {
    init_git_options();
    let repo = Repository::init_bare(repo_path)
        .with_context(|| format!("Failed to create repository at {}", repo_path.display()))?;
    repo.remote("origin", url)
        .with_context(|| format!("Failed to add remote {url}"))?;
    set_keep_releases(&repo, Some(keep))?;
    let remote_tags = list_remote_tags(&repo)?;
    let kept = select_kept_tags(remote_tags.iter().map(String::as_str), keep);
    if kept.is_empty() {
        drop(repo);
        fs::remove_dir_all(repo_path)
            .with_context(|| format!("Failed to remove {}", repo_path.display()))?;
        return Ok(None);
    }
    emit_info!(
        app_name,
        "Fetching the {} newest version(s) of {} releases kept.",
        kept.len(),
        keep
    );
    let refspecs: Vec<String> = kept.iter().map(|tag| tag_refspec(tag)).collect();
    let mut depth = SHALLOW_START_DEPTH;
    loop {
        let roots_before = shallow_roots(&repo);
        // One tag per fetch: a shallow fetch of several tags at once can
        // stall in libgit2. After the newest tag, the others are mostly
        // already present and quick to fetch.
        for refspec in &refspecs {
            let fetch = || {
                fetch_from_origin(
                    &repo,
                    app_name,
                    std::slice::from_ref(refspec),
                    Some(depth),
                    false,
                    Some("Receiving objects"),
                )
            };
            if let Err(error) = fetch() {
                warn!("Fetching {} failed, retrying once: {:#}", refspec, error);
                fetch()?;
            }
        }
        if kept_history_complete(&repo, &kept)? {
            break;
        }
        if depth >= SHALLOW_MAX_DEPTH || shallow_roots(&repo) == roots_before {
            warn!(
                "History between the kept releases of {} is still incomplete at depth {}.",
                app_name, depth
            );
            break;
        }
        depth *= 4;
        debug!("Deepening {} to depth {}", app_name, depth);
    }
    Ok(Some(repo))
}

/// Emits throttled (0.1%) transfer progress. With `detailed`, also reports
/// received bytes and indexed objects, which is useful for initial clones.
fn create_transfer_progress_callback(
    app_name: String,
    prefix: &str,
    detailed: bool,
) -> impl FnMut(Progress<'_>) -> bool + 'static {
    let prefix = prefix.to_string();
    let mut last_percent = -1.0;
    move |progress: Progress| {
        let received_objects = progress.received_objects();
        let total_objects = progress.total_objects();
        let detail = if detailed {
            format!(
                "({}), indexing {} objects... ",
                format_bytes(progress.received_bytes()),
                progress.indexed_objects()
            )
        } else {
            String::new()
        };
        if total_objects > 0 {
            let current_percent = (received_objects as f64 * 100.0) / total_objects as f64;
            let rounded_percent = (current_percent * 10.0).round() / 10.0;
            if (rounded_percent - last_percent).abs() >= 0.1 || received_objects == total_objects {
                emit_update_info!(
                    app_name,
                    "\r{}: {:.1}% ({} / {}) {}",
                    prefix,
                    rounded_percent,
                    received_objects,
                    total_objects,
                    detail
                );
                last_percent = rounded_percent;
            }
        } else {
            emit_update_info!(
                app_name,
                "\r{}: {} received... {}",
                prefix,
                received_objects,
                detail
            );
        }
        io::stdout().flush().unwrap_or_default();
        true
    }
}

fn get_sorted_tags_by_time(repo: &Repository) -> Result<Vec<String>> {
    let tag_array = repo
        .tag_names(None)
        .with_context(|| format!("Failed to list tags from repository at {:?}", repo.path()))?;

    let mut version_tags = Vec::new();

    for tag_name_opt in tag_array.iter() {
        if let Ok(Some(tag_name)) = tag_name_opt {
            if let Some(sort_key) = parse_version_tag(tag_name) {
                version_tags.push((sort_key, tag_name.to_string()));
            }
        }
    }

    version_tags.sort_by(|a, b| b.0.cmp(&a.0));

    let sorted_tags = version_tags.into_iter().map(|(_, tag)| tag).collect();
    Ok(sorted_tags)
}

fn collect_commit_messages_between_tags(
    repo: &Repository,
    previous_tag: Option<&str>,
    target_tag: &str,
) -> Result<Vec<String>> {
    let target_oid = resolve_tag_commit(repo, target_tag)?;
    let hidden_oid = previous_tag
        .map(|tag| resolve_tag_commit(repo, tag))
        .transpose()?;
    collect_commit_messages(repo, target_oid, hidden_oid)
}

/// Collects up to `MAX_UPDATE_NOTE_LINES` unique, non-empty message lines from
/// non-merge commits reachable from `target_oid` but not from `hidden_oid`,
/// newest first.
fn collect_commit_messages(
    repo: &Repository,
    target_oid: Oid,
    hidden_oid: Option<Oid>,
) -> Result<Vec<String>> {
    let mut revwalk = repo.revwalk().context("Failed to create revwalk")?;
    revwalk
        .push(target_oid)
        .with_context(|| format!("Failed to walk from commit {target_oid}"))?;
    if let Some(hidden_oid) = hidden_oid {
        revwalk
            .hide(hidden_oid)
            .with_context(|| format!("Failed to hide commit {hidden_oid}"))?;
    }
    revwalk
        .set_sorting(Sort::TIME)
        .context("Failed to sort version update commits")?;

    let mut messages = Vec::new();
    let mut seen_messages = HashSet::new();
    'revwalk: for oid in revwalk {
        let oid = oid.context("Error iterating version update commits")?;
        let commit = repo
            .find_commit(oid)
            .with_context(|| format!("Failed to find commit {oid}"))?;
        if commit.parent_count() > 1 {
            continue;
        }
        if let Ok(full_message) = commit.message() {
            for line in full_message
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
            {
                if seen_messages.insert(line.to_string()) {
                    messages.push(line.to_string());
                    if messages.len() >= MAX_UPDATE_NOTE_LINES {
                        break 'revwalk;
                    }
                }
            }
        }
    }

    Ok(messages)
}

fn build_version_history(
    repo: &Repository,
    number_versions: usize,
    release_only: bool,
) -> Result<Vec<VersionHistoryEntry>> {
    let tags = get_sorted_tags_by_time(repo)?
        .into_iter()
        .filter(|tag| !release_only || is_release_version(tag))
        .collect::<Vec<_>>();

    tags.iter()
        .take(number_versions)
        .enumerate()
        .map(|(index, version)| {
            let previous_version = tags.get(index + 1).cloned();
            let update_note =
                collect_commit_messages_between_tags(repo, previous_version.as_deref(), version)?;
            Ok(VersionHistoryEntry {
                version: version.clone(),
                previous_version,
                update_note,
            })
        })
        .collect()
}

pub async fn get_version_history(
    repo_path: &Path,
    number_versions: usize,
    release_only: bool,
) -> Result<Vec<VersionHistoryEntry>> {
    let _guard = lock_repo(repo_path).await;
    let repo_path = repo_path.to_path_buf();

    task::spawn_blocking(move || {
        let repo = open_repository(&repo_path)?;
        build_version_history(&repo, number_versions, release_only)
    })
    .await
    .context("Task for get_version_history panicked or was cancelled")?
}

fn collect_remote_tag_names(
    remote: &mut git2::Remote<'_>,
    remote_url: Option<&str>,
) -> Result<HashSet<String>> {
    let mut callbacks = RemoteCallbacks::new();
    configure_credentials(&mut callbacks, remote_url);

    let connection = remote
        .connect_auth(
            Direction::Fetch,
            Some(callbacks),
            Some(create_proxy_options()),
        )
        .context("Failed to connect to remote for tag pruning")?;

    let mut remote_tags = HashSet::new();
    for head in connection
        .list()
        .context("Failed to list remote refs for tag pruning")?
    {
        if let Some(tag_name) = head.name().strip_prefix("refs/tags/") {
            if !tag_name.ends_with("^{}") {
                remote_tags.insert(tag_name.to_string());
            }
        }
    }

    Ok(remote_tags)
}

fn list_remote_tags(repo: &Repository) -> Result<HashSet<String>> {
    let mut remote = repo
        .find_remote("origin")
        .context("Failed to find remote 'origin' for listing tags")?;
    let remote_url = remote.url().ok().map(String::from);
    collect_remote_tag_names(&mut remote, remote_url.as_deref())
}

/// Deletes the local tags that are not in `keep`.
fn prune_local_tags(repo: &Repository, app_name: &str, keep: &HashSet<String>) -> Result<()> {
    let local_tag_array = repo
        .tag_names(None)
        .with_context(|| format!("Failed to list local tags from {:?}", repo.path()))?;
    let mut local_tags = Vec::new();
    for tag_name_opt in local_tag_array.iter() {
        if let Ok(Some(tag_name)) = tag_name_opt {
            local_tags.push(tag_name.to_string());
        }
    }

    let mut pruned_count = 0;
    for tag_name in local_tags {
        if keep.contains(&tag_name) {
            continue;
        }

        let reference_name = format!("refs/tags/{}", tag_name);
        match repo.find_reference(&reference_name) {
            Ok(mut reference) => {
                reference
                    .delete()
                    .with_context(|| format!("Failed to delete local tag {}", tag_name))?;
                pruned_count += 1;
                emit_info!(
                    app_name,
                    "Pruned local tag '{}': it is gone from the remote or older than the kept releases.",
                    tag_name
                );
            }
            Err(error)
                if error.code() == ErrorCode::NotFound
                    && error.class() == ErrorClass::Reference => {}
            Err(error) => {
                return Err(anyhow::Error::new(error)
                    .context(format!("Failed to find local tag {}", tag_name)));
            }
        }
    }

    if pruned_count > 0 {
        info!(
            "Pruned {} local tag(s) for {} that are gone from the remote or not kept.",
            pruned_count, app_name
        );
    }

    Ok(())
}

/// A network read that receives nothing for this long fails instead of
/// stalling the fetch forever (libgit2 waits indefinitely by default).
const SERVER_TIMEOUT_MS: i32 = 60_000;

fn init_git_options() {
    GIT_CONFIG_INITIALIZED.get_or_init(|| {
        unsafe {
            let _ = opts::set_verify_owner_validation(false);
            if let Err(error) = opts::set_server_timeout_in_milliseconds(SERVER_TIMEOUT_MS) {
                warn!("Failed to set the git server timeout: {}", error);
            }
        }
        debug!("git2 owner validation disabled for this process.");
    });
}

/// Opens an app repository.
///
/// App repositories are bare: `repo` holds only git data and the app's files
/// live in the `working` folder next to it, which is set as the work tree
/// here (in memory, so nothing absolute is written and no `.git` link is put
/// into `working`). Checkouts then update `working` directly: files the app
/// created there are kept, and only files removed between versions are
/// deleted. A non-bare repository is opened as is.
pub fn open_repository(repo_path: &Path) -> Result<Repository> {
    init_git_options();
    let repo = Repository::open(repo_path)
        .with_context(|| format!("Failed to open local repo at {}", repo_path.display()))?;
    if repo.is_bare() {
        let work_tree = get_working_dir_for_repo(repo_path);
        fs::create_dir_all(&work_tree)
            .with_context(|| format!("Failed to create {}", work_tree.display()))?;
        repo.set_workdir(&work_tree, false).with_context(|| {
            format!(
                "Failed to use {} as the work tree of {}",
                work_tree.display(),
                repo_path.display()
            )
        })?;
    }
    Ok(repo)
}

/// Converts an app installed with the old layout, where `repo` was a full
/// clone whose files were copied into `working`, into a bare `repo` that
/// checks out into `working` directly. Safe to rerun after an interruption.
///
/// The old index describes exactly the files that were copied into
/// `working`, so it is kept as the baseline for the next checkout.
fn migrate_to_bare_layout(app_name: &str, repo_path: &Path) -> Result<()> {
    let git_dir = repo_path.join(".git");
    let moved_git_dir = repo_path.with_file_name("repo.git.migrating");
    let old_checkout = repo_path.with_file_name("repo.checkout.old");

    if git_dir.is_dir() {
        emit_info!(
            app_name,
            "Moving {} to the shared-checkout layout (git data in repo, app files in working)...",
            repo_path.display()
        );
        // Mark it bare before it is opened again, or libgit2 would treat its
        // parent folder as the work tree.
        git2::Config::open(&git_dir.join("config"))
            .and_then(|mut config| config.set_bool("core.bare", true))
            .with_context(|| format!("Failed to mark {} as bare", git_dir.display()))?;
        fs::rename(&git_dir, &moved_git_dir).with_context(|| {
            format!(
                "Failed to move {} to {}",
                git_dir.display(),
                moved_git_dir.display()
            )
        })?;
    }
    if moved_git_dir.is_dir() {
        if repo_path.exists() {
            if old_checkout.exists() {
                fs::remove_dir_all(&old_checkout).with_context(|| {
                    format!("Failed to remove {}", old_checkout.display())
                })?;
            }
            fs::rename(repo_path, &old_checkout).with_context(|| {
                format!(
                    "Failed to move {} to {}",
                    repo_path.display(),
                    old_checkout.display()
                )
            })?;
        }
        fs::rename(&moved_git_dir, repo_path).with_context(|| {
            format!(
                "Failed to move {} to {}",
                moved_git_dir.display(),
                repo_path.display()
            )
        })?;
    }
    if old_checkout.exists() {
        match fs::remove_dir_all(&old_checkout) {
            Ok(()) => {
                emit_info!(
                    app_name,
                    "Removed the old duplicate checkout at {}.",
                    old_checkout.display()
                );
            }
            Err(error) => warn!(
                "Could not remove the old checkout at {}: {}. It can be deleted manually.",
                old_checkout.display(),
                error
            ),
        }
    }
    Ok(())
}

/// Migrates an old-layout install (see [`migrate_to_bare_layout`]) and makes
/// `working` match the checked-out commit.
pub async fn ensure_bare_layout(app_name: &str, repo_path: &Path) -> Result<()> {
    let _guard = lock_repo(repo_path).await;
    let git_dir = repo_path.join(".git");
    let moved_git_dir = repo_path.with_file_name("repo.git.migrating");
    if !git_dir.is_dir() && !moved_git_dir.is_dir() {
        return Ok(());
    }
    let app_name = app_name.to_string();
    let repo_path = repo_path.to_path_buf();
    task::spawn_blocking(move || -> Result<()> {
        migrate_to_bare_layout(&app_name, &repo_path)?;
        let repo = open_repository(&repo_path)?;
        checkout_head_into_work_tree(&repo, &app_name, "migrated repository")?;
        emit_info!(app_name, "Repository layout migration complete.");
        Ok(())
    })
    .await
    .context("Task for ensure_bare_layout panicked or was cancelled")?
}

fn checkout_head_into_work_tree(repo: &Repository, app_name: &str, label: &str) -> Result<()> {
    with_checkout_retries(label, || {
        repo.checkout_head(Some(CheckoutBuilder::new().force()))
    })?;
    submodule::update_repository_submodules(repo, app_name, label)
}

/// Makes the app's work tree match the commit checked out in its repository,
/// restoring missing or modified tracked files and leaving other files alone.
pub async fn checkout_head_to_working(app_name: &str, repo_path: &Path) -> Result<()> {
    let _guard = lock_repo(repo_path).await;
    let app_name = app_name.to_string();
    let repo_path = repo_path.to_path_buf();
    task::spawn_blocking(move || -> Result<()> {
        let repo = open_repository(&repo_path)?;
        checkout_head_into_work_tree(
            &repo,
            &app_name,
            &format!("work tree of {}", repo_path.display()),
        )
    })
    .await
    .context("Task for checkout_head_to_working panicked or was cancelled")?
}

const CHECKOUT_ATTEMPTS: u32 = 3;

/// Retries a checkout a few times, since files in the work tree can be
/// briefly locked on Windows (antivirus, a process that is still exiting).
fn with_checkout_retries(
    label: &str,
    mut checkout: impl FnMut() -> std::result::Result<(), GitError>,
) -> Result<()> {
    let mut attempt = 1;
    loop {
        match checkout() {
            Ok(()) => return Ok(()),
            Err(error) if attempt < CHECKOUT_ATTEMPTS => {
                warn!(
                    "Checkout of {} failed (attempt {}/{}): {}. Retrying.",
                    label, attempt, CHECKOUT_ATTEMPTS, error
                );
                std::thread::sleep(std::time::Duration::from_millis(500 * attempt as u64));
                attempt += 1;
            }
            Err(error) => {
                return Err(anyhow::Error::new(error)
                    .context(format!("Failed to checkout tree for {label}")))
            }
        }
    }
}

pub fn get_repository_origin_url(repo: &Repository) -> Result<Option<String>> {
    match repo.find_remote("origin") {
        Ok(remote) => Ok(remote.url().ok().map(String::from)),
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
    let _guard = lock_repo(&repo_path).await;

    let app_name_for_task = app_name.to_string();
    let repo_path_for_task = repo_path.clone();

    let result = task::spawn_blocking(move || -> Result<(Vec<String>, String)> {
        emit_info!(
            app_name_for_task,
            "Fetching all tags for repository at {}",
            repo_path_for_task.display()
        );

        let repo = open_repository(&repo_path_for_task)?;
        sync_tags(&repo, &app_name_for_task, false, None).with_context(|| {
            format!(
                "Failed to fetch tags for repository {}",
                repo_path_for_task.display()
            )
        })?;

        let sorted_tags = get_sorted_tags_by_time(&repo)?;

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
    ensure_bare_layout(&app.name, &repo_path).await?;
    let _guard = lock_repo(&repo_path).await;

    let profile = app.get_current_profile_settings();
    let url = profile.git_url.clone();
    let app_name = app.name.clone();
    info!("ensure_repository {} {}", app_name, &url);
    emit_info!(app_name, "Clone {} from {}", app_name, &url);

    if repo_path.exists() {
        emit_info!(
            app.name,
            "Repository already exists {}",
            repo_path.display()
        );
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
                set_keep_releases(&repo, app.keep_releases)?;

                emit_info!(app_name, "Fetching updates for existing repository...");
                let repo_path_for_task = repo_path.clone();
                let app_name_for_task = app_name.clone();

                task::spawn_blocking(move || -> Result<()> {
                    let repo = open_repository(&repo_path_for_task)?;
                    sync_tags(&repo, &app_name_for_task, true, Some("Fetching objects"))
                        .with_context(|| {
                            format!(
                                "Failed to fetch updates for {}",
                                repo_path_for_task.display()
                            )
                        })?;
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
    let keep = app.keep_releases;

    task::spawn_blocking(move || -> Result<()> {
        emit_info!(
            app_name_for_messages,
            "Attempting to clone {} into {}",
            url_for_clone_task,
            repo_path_for_clone_task.display()
        );
        let kept_clone = match keep {
            Some(keep) => clone_kept_releases(
                &url_for_clone_task,
                &repo_path_for_clone_task,
                &app_name_for_messages,
                keep,
            )
            .map(|repo| repo.is_some())
            .inspect_err(|_| {
                let _ = fs::remove_dir_all(&repo_path_for_clone_task);
            })
            .with_context(|| format!("Git clone failed for {}", url_for_clone_task))?,
            None => false,
        };

        if !kept_clone {
            let mut callbacks = RemoteCallbacks::new();
            configure_credentials(&mut callbacks, Some(&url_for_clone_task));
            callbacks.transfer_progress(create_transfer_progress_callback(
                app_name_for_messages.clone(),
                "Receiving objects",
                true,
            ));

            let fetch_options = create_fetch_options(callbacks, None, true);

            // Bare: `repo` keeps only git data; files are checked out into `working`.
            let mut builder = git2::build::RepoBuilder::new();
            builder.fetch_options(fetch_options);
            builder.bare(true);
            drop(
                builder
                    .clone(&url_for_clone_task, &repo_path_for_clone_task)
                    .with_context(|| format!("Git clone failed for {}", url_for_clone_task))?,
            );
        }
        let repo = open_repository(&repo_path_for_clone_task)?;
        set_keep_releases(&repo, keep)?;

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
            checkout_head_into_work_tree(
                &repo,
                &app_name_for_messages,
                &format!("repository at {}", repo_path_for_clone_task.display()),
            )?;
            return Ok(());
        }

        let latest_tag_name = sorted_tags
            .iter()
            .find(|tag| is_release_version(tag))
            .unwrap_or(&sorted_tags[0]);

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
        checkout_detached(&repo, &obj, &format!("tag {latest_tag_name}"))?;

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
    let _guard = lock_repo(repo_path).await;

    let task_repo_path = repo_path.to_path_buf();
    let tag_to_checkout = version_tag_name.to_string();
    let app_name_for_task = app_name.to_string();

    let oid = task::spawn_blocking(move || -> Result<Oid> {
        let repo = open_repository(&task_repo_path)?;

        let refspec = tag_refspec(&tag_to_checkout);
        emit_info!(
            app_name_for_task,
            "Fetching refspec: {} for repo: {}",
            refspec,
            task_repo_path.display()
        );
        fetch_from_origin(
            &repo,
            &app_name_for_task,
            &[refspec],
            None,
            keep_releases(&repo).is_none(),
            Some("Fetching objects for tag"),
        )
        .with_context(|| {
            format!(
                "Failed to fetch tag {} for repo {}",
                tag_to_checkout,
                task_repo_path.display()
            )
        })?;
        prune_tags(&repo, &app_name_for_task, Some(&tag_to_checkout))?;

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
        let commit_oid = checkout_detached(&repo, &obj, &format!("tag {tag_to_checkout}"))?;

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

pub async fn get_current_head_oid(repo_path: &Path) -> Result<Oid> {
    let _guard = lock_repo(repo_path).await;

    let task_repo_path = repo_path.to_path_buf();
    let oid = task::spawn_blocking(move || -> Result<Oid> {
        let repo = open_repository(&task_repo_path)?;
        let head_ref = repo.head().context("Failed to get repo HEAD")?;
        head_ref.target().context("HEAD has no target OID")
    })
    .await
    .context("Task for get_current_head_oid panicked or was cancelled")??;

    Ok(oid)
}

/// Resolves a revision (commit, branch or tag name) that already exists locally.
fn find_local_revision<'r>(
    repo: &'r Repository,
    repo_path: &Path,
    revision: &str,
) -> Result<git2::Object<'r>> {
    repo.revparse_single(revision)
        .or_else(|_| repo.revparse_single(&format!("refs/tags/{revision}")))
        .with_context(|| {
            format!(
                "Revision '{}' not found locally in repo {}",
                revision,
                repo_path.display()
            )
        })
}

pub async fn get_revision_oid(repo_path: &Path, revision: &str) -> Result<Oid> {
    let _guard = lock_repo(repo_path).await;

    let task_repo_path = repo_path.to_path_buf();
    let revision_to_resolve = revision.to_string();
    let oid = task::spawn_blocking(move || -> Result<Oid> {
        let repo = open_repository(&task_repo_path)?;
        let object = find_local_revision(&repo, &task_repo_path, &revision_to_resolve)?;
        Ok(object
            .peel_to_commit()
            .map_or_else(|_| object.id(), |commit| commit.id()))
    })
    .await
    .context("Task for get_revision_oid panicked or was cancelled")??;
    Ok(oid)
}

pub async fn checkout_existing_revision(
    app_name: &str,
    repo_path: &Path,
    revision: &str,
) -> Result<Oid> {
    let _guard = lock_repo(repo_path).await;

    let task_repo_path = repo_path.to_path_buf();
    let revision_to_checkout = revision.to_string();
    let app_name_for_task = app_name.to_string();

    let oid = task::spawn_blocking(move || -> Result<Oid> {
        let repo = open_repository(&task_repo_path)?;
        let obj = find_local_revision(&repo, &task_repo_path, &revision_to_checkout)?;
        let commit_oid =
            checkout_detached(&repo, &obj, &format!("revision {revision_to_checkout}"))?;

        emit_info!(
            app_name_for_task,
            "Successfully rolled back and set head to revision {} ({}) for repo {}.",
            revision_to_checkout,
            commit_oid,
            task_repo_path.display()
        );

        submodule::update_repository_submodules(
            &repo,
            &app_name_for_task,
            &format!(
                "repository at {} after rolling back to revision {}",
                task_repo_path.display(),
                revision_to_checkout
            ),
        )?;

        Ok(commit_oid)
    })
    .await
    .context("Task for checkout_existing_revision panicked or was cancelled")??;
    Ok(oid)
}

pub async fn get_commit_messages_for_version_diff(
    repo_path: &Path,
    target_version_tag_name: &str,
) -> Result<Vec<String>> {
    let _guard = lock_repo(repo_path).await;

    let repo_path_clone = repo_path.to_path_buf();
    let target_tag = target_version_tag_name.to_string();

    let messages = task::spawn_blocking(move || -> Result<Vec<String>> {
        let repo = open_repository(&repo_path_clone)?;
        let head_ref = repo.head().context("Failed to get repo HEAD")?;
        let head_oid = head_ref.target().context("HEAD has no target OID")?;

        let target_tag_ref_str = format!("refs/tags/{}", target_tag);
        if repo.find_reference(&target_tag_ref_str).is_err() {
            debug!(
                "Fetching target tag {} as it's not found locally.",
                target_tag
            );
            fetch_from_origin(
                &repo,
                "",
                &[tag_refspec(&target_tag)],
                None,
                keep_releases(&repo).is_none(),
                None,
            )
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

        let mut messages = collect_commit_messages(&repo, target_commit_oid, Some(head_oid))?;
        info!(
            "Found {} commit messages in diff from HEAD ({}) to target {} ({})",
            messages.len(),
            head_oid,
            target_tag,
            target_commit_oid
        );

        if messages.is_empty() {
            let target_commit = repo.find_commit(target_commit_oid).with_context(|| {
                format!("Failed to find target commit for OID {}", target_commit_oid)
            })?;
            if let Ok(full_message) = target_commit.message() {
                info!(
                    "Diff is empty, using target commit's message: {}",
                    full_message.lines().next().unwrap_or("")
                );
                for line in full_message.lines() {
                    let trimmed = line.trim();
                    if !trimmed.is_empty() {
                        messages.push(trimmed.to_string());
                    }
                }
            }
        }

        Ok(messages)
    })
    .await
    .context("Task for get_commit_messages panicked or was cancelled")??;
    Ok(messages)
}

#[cfg(test)]
mod tests {
    use super::{
        build_version_history, checkout_existing_revision, checkout_head_to_working,
        checkout_version_tag, compare_version_tags, ensure_bare_layout, is_release_version,
        is_version_tag, open_repository, resolve_tag_commit,
    };
    use git2::{Repository, Signature};
    use std::cmp::Ordering;

    fn commit_and_tag(repo: &Repository, root: &std::path::Path, tag: &str, message: &str) {
        let file_path = root.join("history.txt");
        std::fs::write(&file_path, format!("{tag}\n")).unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(std::path::Path::new("history.txt")).unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let signature = Signature::now("PyAppify Test", "test@example.com").unwrap();
        let parent = repo
            .head()
            .ok()
            .and_then(|head| head.target())
            .map(|oid| repo.find_commit(oid).unwrap());
        let parents = parent.iter().collect::<Vec<_>>();
        let commit_id = repo
            .commit(
                Some("HEAD"),
                &signature,
                &signature,
                message,
                &tree,
                &parents,
            )
            .unwrap();
        let commit = repo.find_commit(commit_id).unwrap();
        repo.tag_lightweight(tag, commit.as_object(), false)
            .unwrap();
    }

    fn unique_temp_dir(label: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "pyappify-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    /// Writes (`Some`) or deletes (`None`) files, commits everything and tags it.
    fn commit_files(
        repo: &Repository,
        root: &std::path::Path,
        files: &[(&str, Option<&str>)],
        tag: &str,
    ) {
        for (path, content) in files {
            let file_path = root.join(path);
            match content {
                Some(content) => {
                    std::fs::create_dir_all(file_path.parent().unwrap()).unwrap();
                    std::fs::write(file_path, content).unwrap();
                }
                None => std::fs::remove_file(file_path).unwrap(),
            }
        }
        let mut index = repo.index().unwrap();
        index
            .add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)
            .unwrap();
        index.update_all(["*"].iter(), None).unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let signature = Signature::now("PyAppify Test", "test@example.com").unwrap();
        let parent = repo
            .head()
            .ok()
            .and_then(|head| head.target())
            .map(|oid| repo.find_commit(oid).unwrap());
        let parents = parent.iter().collect::<Vec<_>>();
        let commit_id = repo
            .commit(Some("HEAD"), &signature, &signature, tag, &tree, &parents)
            .unwrap();
        repo.tag_lightweight(tag, &repo.find_object(commit_id, None).unwrap(), false)
            .unwrap();
    }

    fn bare_clone(source: &std::path::Path, repo_path: &std::path::Path) {
        drop(
            git2::build::RepoBuilder::new()
                .bare(true)
                .clone(source.to_str().unwrap(), repo_path)
                .unwrap(),
        );
    }

    #[track_caller]
    fn read(path: std::path::PathBuf) -> String {
        std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
    }

    #[tokio::test]
    async fn bare_repository_checks_out_into_working_and_keeps_app_files() {
        let root = unique_temp_dir("bare-layout");
        let source_path = root.join("source");
        let source = Repository::init(&source_path).unwrap();
        commit_files(
            &source,
            &source_path,
            &[("app.py", Some("v1")), ("pkg/old.py", Some("old"))],
            "v1.0.0",
        );
        commit_files(
            &source,
            &source_path,
            &[
                ("app.py", Some("v2")),
                ("pkg/old.py", None),
                ("pkg/new.py", Some("new")),
            ],
            "v1.1.0",
        );
        let repo_path = root.join("app").join("repo");
        let working = root.join("app").join("working");
        bare_clone(&source_path, &repo_path);

        checkout_version_tag("test", &repo_path, "v1.0.0")
            .await
            .unwrap();
        assert_eq!(read(working.join("app.py")), "v1");
        assert!(!repo_path.join("app.py").exists(), "repo keeps git data only");
        assert!(!working.join(".git").exists());

        // Files the app creates, at the top level and in a tracked folder.
        std::fs::write(working.join("settings.json"), "{}").unwrap();
        std::fs::create_dir_all(working.join("configs")).unwrap();
        std::fs::write(working.join("configs/user.json"), "{}").unwrap();
        std::fs::write(working.join("pkg/cache.bin"), "cache").unwrap();

        checkout_version_tag("test", &repo_path, "v1.1.0")
            .await
            .unwrap();
        assert_eq!(read(working.join("app.py")), "v2");
        assert_eq!(read(working.join("pkg/new.py")), "new");
        assert!(!working.join("pkg/old.py").exists(), "removed files are deleted");
        assert_eq!(read(working.join("settings.json")), "{}");
        assert_eq!(read(working.join("configs/user.json")), "{}");
        assert_eq!(read(working.join("pkg/cache.bin")), "cache");

        // A recreated or damaged working folder is restored from HEAD.
        std::fs::remove_file(working.join("app.py")).unwrap();
        std::fs::write(working.join("pkg/new.py"), "edited").unwrap();
        checkout_head_to_working("test", &repo_path).await.unwrap();
        assert_eq!(read(working.join("app.py")), "v2");
        assert_eq!(read(working.join("pkg/new.py")), "new");
        assert_eq!(read(working.join("settings.json")), "{}");

        checkout_existing_revision("test", &repo_path, "v1.0.0")
            .await
            .unwrap();
        assert_eq!(read(working.join("pkg/old.py")), "old");
        assert!(!working.join("pkg/new.py").exists());

        drop(source);
        std::fs::remove_dir_all(root).unwrap();
    }

    fn copy_without_git(source: &std::path::Path, destination: &std::path::Path) {
        std::fs::create_dir_all(destination).unwrap();
        for entry in std::fs::read_dir(source).unwrap() {
            let entry = entry.unwrap();
            if entry.file_name() == ".git" {
                continue;
            }
            let target = destination.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                copy_without_git(&entry.path(), &target);
            } else {
                std::fs::copy(entry.path(), target).unwrap();
            }
        }
    }

    #[tokio::test]
    async fn migrates_old_layout_with_submodules() {
        let root = unique_temp_dir("migrate-layout");
        let module_source_path = root.join("templates");
        let module_source = Repository::init(&module_source_path).unwrap();
        commit_files(
            &module_source,
            &module_source_path,
            &[("template.txt", Some("t1"))],
            "t1",
        );

        let source_path = root.join("source");
        let source = Repository::init(&source_path).unwrap();
        let mut module = source
            .submodule(
                module_source_path.to_str().unwrap(),
                std::path::Path::new("ok_templates"),
                true,
            )
            .unwrap();
        drop(module.clone(None).unwrap());
        module.add_finalize().unwrap();
        drop(module);
        commit_files(&source, &source_path, &[("app.py", Some("v1"))], "v1.0.0");
        commit_files(&source, &source_path, &[("app.py", Some("v2"))], "v1.1.0");

        // The old layout: a full clone in `repo`, copied into `working`.
        let repo_path = root.join("app").join("repo");
        let working = root.join("app").join("working");
        drop(Repository::clone(source_path.to_str().unwrap(), &repo_path).unwrap());
        checkout_version_tag("test", &repo_path, "v1.0.0")
            .await
            .unwrap();
        assert_eq!(read(repo_path.join("ok_templates/template.txt")), "t1");
        copy_without_git(&repo_path, &working);
        std::fs::create_dir_all(working.join("configs")).unwrap();
        std::fs::write(working.join("configs/user.json"), "{}").unwrap();

        ensure_bare_layout("test", &repo_path).await.unwrap();

        assert!(Repository::open(&repo_path).unwrap().is_bare());
        assert!(!repo_path.join("app.py").exists());
        assert!(!root.join("app").join("repo.checkout.old").exists());
        assert!(!root.join("app").join("repo.git.migrating").exists());
        assert_eq!(
            read(working.join("ok_templates/.git")).trim(),
            "gitdir: ../../repo/modules/ok_templates",
            "the link is relative so the install can be moved"
        );
        assert_eq!(
            git2::Config::open(&repo_path.join("modules/ok_templates/config"))
                .unwrap()
                .get_string("core.worktree")
                .unwrap(),
            "../../../working/ok_templates"
        );
        assert_eq!(read(working.join("ok_templates/template.txt")), "t1");
        assert_eq!(read(working.join("configs/user.json")), "{}");
        let (_, current) = {
            let repo = open_repository(&repo_path).unwrap();
            let head = repo.head().unwrap().target().unwrap();
            ((), head)
        };
        assert_eq!(current, resolve_tag_commit(&source, "v1.0.0").unwrap());
        // Running it again is a no-op.
        ensure_bare_layout("test", &repo_path).await.unwrap();

        checkout_version_tag("test", &repo_path, "v1.1.0")
            .await
            .unwrap();
        assert_eq!(read(working.join("app.py")), "v2");
        assert_eq!(read(working.join("ok_templates/template.txt")), "t1");
        assert_eq!(read(working.join("configs/user.json")), "{}");

        // A recreated working folder gets its submodule back.
        std::fs::remove_dir_all(&working).unwrap();
        checkout_head_to_working("test", &repo_path).await.unwrap();
        assert_eq!(read(working.join("app.py")), "v2");
        assert_eq!(read(working.join("ok_templates/template.txt")), "t1");

        // A fresh install (bare clone) gets the submodule cloned into working.
        let fresh_repo_path = root.join("fresh").join("repo");
        let fresh_working = root.join("fresh").join("working");
        bare_clone(&source_path, &fresh_repo_path);
        checkout_version_tag("test", &fresh_repo_path, "v1.1.0")
            .await
            .unwrap();
        assert_eq!(read(fresh_working.join("ok_templates/template.txt")), "t1");
        assert!(fresh_repo_path.join("modules/ok_templates/HEAD").is_file());
        assert!(!fresh_repo_path.join("ok_templates").exists());

        drop(source);
        drop(module_source);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn compares_release_and_prerelease_versions() {
        let ordered = [
            "1.4.2",
            "1.5.0-alpha.1",
            "1.5.0-beta",
            "1.5.0.beta.1",
            "1.5.0-beta.2",
            "1.5.0-rc.1",
            "1.5.0",
        ];

        for pair in ordered.windows(2) {
            assert_eq!(compare_version_tags(pair[0], pair[1]), Some(Ordering::Less));
        }

        assert_eq!(
            compare_version_tags("1.5.0", "1.5.0-rc.1"),
            Some(Ordering::Greater)
        );
        assert!(is_release_version("1.5.0"));
        assert!(is_release_version("v1.5.0"));
        assert!(!is_release_version("1.5.0-beta"));
        assert!(!is_release_version("v1.5.0.beta"));
        assert!(is_version_tag("v1.5.0.beta"));
        assert!(!is_version_tag("7fa243f331892d478c4e450f6215495ca3b48258"));
    }

    #[test]
    fn version_history_compares_each_version_to_the_previous_filtered_version() {
        let root = std::env::temp_dir().join(format!(
            "pyappify-version-history-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let repo = Repository::init(&root).unwrap();
        commit_and_tag(&repo, &root, "v1.0.0", "initial release");
        commit_and_tag(&repo, &root, "v1.1.0-beta.1", "beta change");
        commit_and_tag(&repo, &root, "v1.1.0", "release change");

        let releases = build_version_history(&repo, 2, true).unwrap();
        assert_eq!(releases[0].version, "v1.1.0");
        assert_eq!(releases[0].previous_version.as_deref(), Some("v1.0.0"));
        assert_eq!(
            releases[0].update_note,
            vec!["release change", "beta change"]
        );

        let all_versions = build_version_history(&repo, 2, false).unwrap();
        assert_eq!(
            all_versions[0].previous_version.as_deref(),
            Some("v1.1.0-beta.1")
        );
        assert_eq!(all_versions[0].update_note, vec!["release change"]);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn explicit_setup_tag_selects_prerelease_and_missing_tag_fails() {
        let root = std::env::temp_dir().join(format!(
            "pyappify-setup-tag-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let source_path = root.join("source");
        let source = Repository::init(&source_path).unwrap();
        commit_and_tag(&source, &source_path, "v1.0.0", "stable dependencies");
        commit_and_tag(&source, &source_path, "v1.1.0-beta.1", "minimal dependencies");
        let checkout_path = root.join("checkout");
        let checkout = Repository::clone(source_path.to_str().unwrap(), &checkout_path).unwrap();
        drop(checkout);

        checkout_version_tag("test", &checkout_path, "v1.0.0")
            .await
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(checkout_path.join("history.txt")).unwrap().trim(),
            "v1.0.0"
        );
        checkout_version_tag("test", &checkout_path, "v1.1.0-beta.1")
            .await
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(checkout_path.join("history.txt")).unwrap().trim(),
            "v1.1.0-beta.1"
        );
        assert!(checkout_version_tag("test", &checkout_path, "v9.0.0").await.is_err());
        assert_eq!(
            std::fs::read_to_string(checkout_path.join("history.txt")).unwrap().trim(),
            "v1.1.0-beta.1"
        );

        drop(source);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn kept_tags_are_the_newest_releases_and_newer_prereleases() {
        let tags = [
            "v1.0.0",
            "v1.1.0-beta.1",
            "v1.1.0",
            "v1.2.0-beta.1",
            "v1.2.0",
            "v1.3.0-beta.2",
            "latest",
        ];
        assert_eq!(
            super::select_kept_tags(tags, 2),
            ["v1.3.0-beta.2", "v1.2.0", "v1.2.0-beta.1", "v1.1.0"]
        );
        assert_eq!(
            super::select_kept_tags(tags, 1),
            ["v1.3.0-beta.2", "v1.2.0"]
        );
        assert_eq!(super::select_kept_tags(tags, 10).len(), 6);
    }

    #[tokio::test]
    async fn keep_releases_fetches_only_kept_tags_and_keeps_the_checked_out_one() {
        let root = unique_temp_dir("keep-releases");
        let source_path = root.join("source");
        let source = Repository::init(&source_path).unwrap();
        for tag in ["v1.0.0", "v1.1.0", "v1.2.0-beta.1", "v1.2.0"] {
            commit_and_tag(&source, &source_path, tag, tag);
        }
        let repo_path = root.join("repo");
        bare_clone(&source_path, &repo_path);
        let repo = open_repository(&repo_path).unwrap();
        super::checkout_detached(
            &repo,
            &repo.revparse_single("refs/tags/v1.0.0").unwrap(),
            "v1.0.0",
        )
        .unwrap();
        super::set_keep_releases(&repo, Some(1)).unwrap();
        assert_eq!(super::keep_releases(&repo), Some(1));

        commit_and_tag(&source, &source_path, "v1.3.0-beta.1", "v1.3.0-beta.1");
        super::sync_tags(&repo, "test", true, None).unwrap();
        let mut tags: Vec<String> = repo
            .tag_names(None)
            .unwrap()
            .iter()
            .filter_map(|tag| tag.ok().flatten().map(String::from))
            .collect();
        tags.sort();
        // v1.0.0 stays because it is checked out.
        assert_eq!(tags, ["v1.0.0", "v1.2.0", "v1.3.0-beta.1"]);

        super::set_keep_releases(&repo, None).unwrap();
        assert_eq!(super::keep_releases(&repo), None);
        super::sync_tags(&repo, "test", true, None).unwrap();
        assert_eq!(repo.tag_names(None).unwrap().len(), 5);

        drop(repo);
        drop(source);
        std::fs::remove_dir_all(root).unwrap();
    }

    /// Shallow fetches need a smart transport, so this clones a real update
    /// repository: `cargo test kept_release_clone -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn kept_release_clone_is_shallow_with_complete_notes() {
        let url = std::env::var("PYAPPIFY_TEST_GIT_URL")
            .unwrap_or_else(|_| "https://github.com/ok-oldking/ok-ww-update.git".to_string());
        let root = unique_temp_dir("kept-clone");
        let repo_path = root.join("repo");
        let keep = std::env::var("PYAPPIFY_TEST_KEEP")
            .ok()
            .and_then(|keep| keep.parse().ok())
            .unwrap_or(3);
        let repo = super::clone_kept_releases(&url, &repo_path, "test", keep)
            .unwrap()
            .unwrap();
        let kept: Vec<String> = super::get_sorted_tags_by_time(&repo).unwrap();
        println!("kept tags: {kept:?}");
        println!("shallow roots: {}", super::shallow_roots(&repo).len());
        let mut revwalk = repo.revwalk().unwrap();
        revwalk.push_glob("refs/tags/*").unwrap();
        println!("commits: {}", revwalk.count());
        assert!(super::kept_history_complete(&repo, &kept).unwrap());
        let history = super::build_version_history(&repo, 10, false).unwrap();
        for entry in &history {
            println!(
                "{} <- {:?}: {:?}",
                entry.version, entry.previous_version, entry.update_note
            );
        }
        let size: u64 = walk_size(&repo_path);
        println!("repo size: {:.1} MB", size as f64 / 1024.0 / 1024.0);
        drop(repo);

        // A later sync fetches only the kept tags into the shallow repository.
        let repo = open_repository(&repo_path).unwrap();
        super::sync_tags(&repo, "test", true, None).unwrap();
        println!(
            "size after sync: {:.1} MB",
            walk_size(&repo_path) as f64 / 1024.0 / 1024.0
        );
        drop(repo);
        std::fs::remove_dir_all(root).unwrap();
    }

    fn walk_size(path: &std::path::Path) -> u64 {
        std::fs::read_dir(path)
            .unwrap()
            .flatten()
            .map(|entry| {
                let meta = entry.metadata().unwrap();
                if meta.is_dir() {
                    walk_size(&entry.path())
                } else {
                    meta.len()
                }
            })
            .sum()
    }
}
