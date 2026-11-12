// src/pip_mirror.rs
//! The automatic pip index choice.
//!
//! When the user leaves "Pip Index URL" empty and pip has no index of its own
//! configured, the mirrors are measured in parallel and the fastest one that
//! answers with 200 is used; Aliyun wins near-ties. The result is cached for a
//! week in the config directory, apart from the user's own choice.
use crate::emit_info;
use crate::utils::command::new_cmd;
use crate::utils::path::get_config_dir;
use crate::utils::process::RemovePythonEnvsExt;
use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tracing::warn;

pub const PYPI_URL: &str = "https://pypi.org/simple/";
pub const ALIYUN_URL: &str = "https://mirrors.aliyun.com/pypi/simple/";
pub const HUAWEI_URL: &str = "https://repo.huaweicloud.com/repository/pypi/simple/";
pub const TENCENT_URL: &str = "https://mirrors.cloud.tencent.com/pypi/simple/";
pub const MIRROR_URLS: [&str; 4] = [PYPI_URL, ALIYUN_URL, HUAWEI_URL, TENCENT_URL];

/// Set to "0" to turn the automatic choice off.
pub const DISABLE_ENV: &str = "PYAPPIFY_AUTO_PIP_INDEX";
const CACHE_FILE_NAME: &str = "pip_mirror_auto.json";
const CACHE_MAX_AGE_SECS: u64 = 7 * 24 * 3600;
const PROBE_TIMEOUT: Duration = Duration::from_secs(3);
const PIP_CONFIG_TIMEOUT: Duration = Duration::from_secs(30);
// Mirrors this close to the fastest count as a tie, and a tie goes to Aliyun.
const TIE_RATIO: f64 = 1.2;
const TIE_SLACK: Duration = Duration::from_millis(30);

/// pip output that means the package index could not be reached or refused
/// us. "(from versions: none)" is how pip reports an index that answered with
/// an error page, as a mirror blocking the user's IP does.
const NETWORK_ERROR_MARKERS: [&str; 17] = [
    "NewConnectionError",
    "ConnectTimeoutError",
    "ReadTimeoutError",
    "ProxyError",
    "SSLError",
    "ConnectionResetError",
    "RemoteDisconnected",
    "Max retries exceeded",
    "Failed to establish a new connection",
    "Read timed out",
    "getaddrinfo failed",
    "Temporary failure in name resolution",
    "Name or service not known",
    "Could not fetch URL",
    "HTTP error 403",
    "HTTP error 5",
    "(from versions: none)",
];

static ORIGINAL_PIP_INDEX_URL: OnceCell<Option<String>> = OnceCell::new();

/// Record the user's own PIP_INDEX_URL before the launcher's config changes it.
pub fn remember_original_pip_index_url() {
    ORIGINAL_PIP_INDEX_URL.get_or_init(|| env::var("PIP_INDEX_URL").ok().filter(|v| !v.is_empty()));
}

pub fn original_pip_index_url() -> Option<String> {
    ORIGINAL_PIP_INDEX_URL.get().cloned().flatten()
}

pub fn is_network_error(pip_output: &str) -> bool {
    NETWORK_ERROR_MARKERS
        .iter()
        .any(|marker| pip_output.contains(marker))
}

/// Choose from measured timings; None keeps pip's default (PyPI).
pub fn pick(timings: &[(&str, Option<Duration>)]) -> Option<String> {
    let answered: Vec<(&str, Duration)> = timings
        .iter()
        .filter_map(|(url, time)| time.map(|time| (*url, time)))
        .collect();
    let fastest = answered.iter().map(|(_, time)| *time).min()?;
    let limit = fastest.mul_f64(TIE_RATIO) + TIE_SLACK;
    if answered
        .iter()
        .any(|(url, time)| *url == ALIYUN_URL && *time <= limit)
    {
        return Some(ALIYUN_URL.to_string());
    }
    let (winner, _) = answered.iter().min_by_key(|(_, time)| *time)?;
    if *winner == PYPI_URL {
        None
    } else {
        Some(winner.to_string())
    }
}

async fn probe(client: &reqwest::Client, url: &str) -> Option<Duration> {
    let start = Instant::now();
    let response = client
        .get(format!("{url}pip/"))
        .header(reqwest::header::USER_AGENT, "pip")
        .send()
        .await
        .ok()?;
    if response.status() != reqwest::StatusCode::OK {
        return None;
    }
    Some(start.elapsed())
}

/// Probe every mirror in parallel.
pub async fn measure() -> Vec<(&'static str, Option<Duration>)> {
    let client = match reqwest::Client::builder().timeout(PROBE_TIMEOUT).build() {
        Ok(client) => client,
        Err(error) => {
            warn!("Could not build an HTTP client to measure pip mirrors: {error}");
            return MIRROR_URLS.iter().map(|url| (*url, None)).collect();
        }
    };
    futures_util::future::join_all(MIRROR_URLS.iter().map(|url| {
        let client = client.clone();
        async move { (*url, probe(&client, url).await) }
    }))
    .await
}

fn has_index_line(pip_config_list: &str) -> bool {
    pip_config_list.lines().any(|line| {
        line.split('=')
            .next()
            .is_some_and(|key| key.trim().ends_with(".index-url"))
    })
}

/// True when the user's environment or pip's own config already names an index.
pub async fn pip_has_index_config(python_exe: &Path) -> bool {
    if original_pip_index_url().is_some() {
        return true;
    }
    let mut command = new_cmd(python_exe);
    command
        .args(["-m", "pip", "config", "list"])
        .clear_python_envs()
        .env_remove("PIP_INDEX_URL")
        .stdin(Stdio::null())
        .stderr(Stdio::null());
    match tokio::time::timeout(PIP_CONFIG_TIMEOUT, command.output()).await {
        Ok(Ok(output)) => has_index_line(&String::from_utf8_lossy(&output.stdout)),
        _ => false,
    }
}

#[derive(Serialize, Deserialize)]
struct Cache {
    url: String,
    checked_at: u64,
}

fn cache_path() -> PathBuf {
    get_config_dir().join(CACHE_FILE_NAME)
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// The cached choice ("" for pip's default) if it is still fresh and valid.
fn read_cache(path: &Path, now: u64) -> Option<String> {
    let cache: Cache = serde_json::from_str(&fs::read_to_string(path).ok()?).ok()?;
    let fresh = cache.checked_at <= now && now - cache.checked_at < CACHE_MAX_AGE_SECS;
    let known = cache.url.is_empty() || MIRROR_URLS.contains(&cache.url.as_str());
    (fresh && known).then_some(cache.url)
}

fn write_cache(path: &Path, url: &str, now: u64) {
    let cache = Cache {
        url: url.to_string(),
        checked_at: now,
    };
    let result = path
        .parent()
        .map_or(Ok(()), fs::create_dir_all)
        .and_then(|_| fs::write(path, serde_json::to_string_pretty(&cache).unwrap_or_default()));
    if let Err(error) = result {
        warn!("Could not cache the pip mirror choice at {}: {error}", path.display());
    }
}

/// The fastest mirror, cached for a week; None means pip's default index.
pub async fn auto_url(app_name: &str) -> Option<String> {
    if env::var(DISABLE_ENV).as_deref() == Ok("0") {
        return None;
    }
    let path = cache_path();
    let now = now_secs();
    if let Some(cached) = read_cache(&path, now) {
        return (!cached.is_empty()).then_some(cached);
    }
    emit_info!(app_name, "Measuring pip mirror speed...");
    let timings = measure().await;
    for (url, time) in &timings {
        let result = time.map_or("failed".to_string(), |time| format!("{} ms", time.as_millis()));
        emit_info!(app_name, "  {}: {}", url, result);
    }
    let url = pick(&timings);
    emit_info!(
        app_name,
        "Using {}",
        url.as_deref().unwrap_or("pip's default index")
    );
    write_cache(&path, url.as_deref().unwrap_or(""), now);
    url
}

/// Drop the cached choice so the next install measures again.
pub fn forget_auto_url() {
    let _ = fs::remove_file(cache_path());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(value: u64) -> Option<Duration> {
        Some(Duration::from_millis(value))
    }

    #[test]
    fn picks_aliyun_on_a_tie_and_pip_default_when_pypi_wins() {
        // Measured on a Chinese network: Aliyun and Huawei tie, Aliyun wins the tie.
        assert_eq!(
            pick(&[(PYPI_URL, ms(720)), (ALIYUN_URL, ms(43)), (HUAWEI_URL, ms(40)), (TENCENT_URL, ms(300))]),
            Some(ALIYUN_URL.to_string())
        );
        assert_eq!(
            pick(&[(PYPI_URL, ms(720)), (ALIYUN_URL, ms(500)), (HUAWEI_URL, ms(40))]),
            Some(HUAWEI_URL.to_string())
        );
        assert_eq!(
            pick(&[(PYPI_URL, ms(720)), (ALIYUN_URL, None), (HUAWEI_URL, ms(40))]),
            Some(HUAWEI_URL.to_string())
        );
        assert_eq!(pick(&[(PYPI_URL, ms(50)), (ALIYUN_URL, ms(400))]), None);
        assert_eq!(pick(&[(PYPI_URL, None), (ALIYUN_URL, None)]), None);
        assert_eq!(pick(&[]), None);
    }

    #[test]
    fn cache_expires_after_a_week_and_rejects_unknown_urls() {
        let dir = env::temp_dir().join(format!("pyappify-pip-mirror-{}", std::process::id()));
        let path = dir.join(CACHE_FILE_NAME);
        assert_eq!(read_cache(&path, 1000), None);
        write_cache(&path, ALIYUN_URL, 1000);
        assert_eq!(read_cache(&path, 1000 + 60), Some(ALIYUN_URL.to_string()));
        assert_eq!(read_cache(&path, 1000 + CACHE_MAX_AGE_SECS), None);
        assert_eq!(read_cache(&path, 999), None);
        write_cache(&path, "", 1000);
        assert_eq!(read_cache(&path, 1000), Some(String::new()));
        write_cache(&path, "https://pypi.tuna.tsinghua.edu.cn/simple", 1000);
        assert_eq!(read_cache(&path, 1000), None);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn detects_index_in_pip_config_list() {
        assert!(has_index_line("global.index-url='https://example.invalid/'\n"));
        assert!(has_index_line(":env:.index-url='https://example.invalid/'\r\n"));
        assert!(!has_index_line("global.timeout='60'\nglobal.extra-index-url='x'\n"));
        assert!(!has_index_line(""));
    }

    #[test]
    fn recognizes_network_errors_in_pip_output() {
        assert!(is_network_error(
            "WARNING: Retrying (Retry(total=4)) after connection broken by \
             'NewConnectionError(...: Failed to establish a new connection)': /simple/six/"
        ));
        assert!(is_network_error(
            "ERROR: Could not find a version that satisfies the requirement six (from versions: none)"
        ));
        assert!(!is_network_error("error: subprocess-exited-with-error"));
    }
}
