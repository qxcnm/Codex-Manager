use codexmanager_core::auth::{DEFAULT_CLIENT_ID, DEFAULT_ISSUER};
use reqwest::blocking::Client;
use reqwest::Proxy;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{OnceLock, RwLock};
use std::time::Duration;

static UPSTREAM_CLIENT: OnceLock<RwLock<Client>> = OnceLock::new();
static UPSTREAM_CLIENT_POOL: OnceLock<RwLock<UpstreamClientPool>> = OnceLock::new();
static RUNTIME_CONFIG_LOADED: OnceLock<()> = OnceLock::new();
static REQUEST_GATE_WAIT_TIMEOUT_MS: AtomicU64 =
    AtomicU64::new(DEFAULT_REQUEST_GATE_WAIT_TIMEOUT_MS);
static TRACE_BODY_PREVIEW_MAX_BYTES: AtomicUsize =
    AtomicUsize::new(DEFAULT_TRACE_BODY_PREVIEW_MAX_BYTES);
static FRONT_PROXY_MAX_BODY_BYTES: AtomicUsize =
    AtomicUsize::new(DEFAULT_FRONT_PROXY_MAX_BODY_BYTES);
static UPSTREAM_CONNECT_TIMEOUT_SECS: AtomicU64 =
    AtomicU64::new(DEFAULT_UPSTREAM_CONNECT_TIMEOUT_SECS);
static UPSTREAM_TOTAL_TIMEOUT_MS: AtomicU64 = AtomicU64::new(DEFAULT_UPSTREAM_TOTAL_TIMEOUT_MS);
static UPSTREAM_STREAM_TIMEOUT_MS: AtomicU64 = AtomicU64::new(DEFAULT_UPSTREAM_STREAM_TIMEOUT_MS);
static ACCOUNT_MAX_INFLIGHT: AtomicUsize = AtomicUsize::new(DEFAULT_ACCOUNT_MAX_INFLIGHT);
static CPA_NO_COOKIE_HEADER_MODE: AtomicBool = AtomicBool::new(DEFAULT_CPA_NO_COOKIE_HEADER_MODE);
static STRICT_REQUEST_PARAM_ALLOWLIST: AtomicBool =
    AtomicBool::new(DEFAULT_STRICT_REQUEST_PARAM_ALLOWLIST);
static UPSTREAM_COOKIE: OnceLock<RwLock<Option<String>>> = OnceLock::new();
static UPSTREAM_PROXY_URL: OnceLock<RwLock<Option<String>>> = OnceLock::new();
static FREE_ACCOUNT_MAX_MODEL: OnceLock<RwLock<String>> = OnceLock::new();
static TOKEN_EXCHANGE_CLIENT_ID: OnceLock<RwLock<String>> = OnceLock::new();
static TOKEN_EXCHANGE_ISSUER: OnceLock<RwLock<String>> = OnceLock::new();

pub(crate) const DEFAULT_GATEWAY_DEBUG: bool = false;
const DEFAULT_UPSTREAM_CONNECT_TIMEOUT_SECS: u64 = 15;
const DEFAULT_UPSTREAM_TOTAL_TIMEOUT_MS: u64 = 120_000;
const DEFAULT_UPSTREAM_STREAM_TIMEOUT_MS: u64 = 1_800_000;
// 中文注释：默认把单账号并发收紧到 1，避免多个长连接 Codex 会话同时压到同一账号上。
const DEFAULT_ACCOUNT_MAX_INFLIGHT: usize = 1;
const DEFAULT_CPA_NO_COOKIE_HEADER_MODE: bool = false;
const DEFAULT_STRICT_REQUEST_PARAM_ALLOWLIST: bool = true;
const DEFAULT_REQUEST_GATE_WAIT_TIMEOUT_MS: u64 = 300;
const DEFAULT_TRACE_BODY_PREVIEW_MAX_BYTES: usize = 0;
const DEFAULT_FRONT_PROXY_MAX_BODY_BYTES: usize = 16 * 1024 * 1024;
const DEFAULT_FREE_ACCOUNT_MAX_MODEL: &str = "gpt-5.2";
const MAX_UPSTREAM_PROXY_POOL_SIZE: usize = 5;

const ENV_REQUEST_GATE_WAIT_TIMEOUT_MS: &str = "CODEXMANAGER_REQUEST_GATE_WAIT_TIMEOUT_MS";
const ENV_TRACE_BODY_PREVIEW_MAX_BYTES: &str = "CODEXMANAGER_TRACE_BODY_PREVIEW_MAX_BYTES";
const ENV_FRONT_PROXY_MAX_BODY_BYTES: &str = "CODEXMANAGER_FRONT_PROXY_MAX_BODY_BYTES";
const ENV_UPSTREAM_CONNECT_TIMEOUT_SECS: &str = "CODEXMANAGER_UPSTREAM_CONNECT_TIMEOUT_SECS";
const ENV_UPSTREAM_TOTAL_TIMEOUT_MS: &str = "CODEXMANAGER_UPSTREAM_TOTAL_TIMEOUT_MS";
const ENV_UPSTREAM_STREAM_TIMEOUT_MS: &str = "CODEXMANAGER_UPSTREAM_STREAM_TIMEOUT_MS";
const ENV_ACCOUNT_MAX_INFLIGHT: &str = "CODEXMANAGER_ACCOUNT_MAX_INFLIGHT";
const ENV_CPA_NO_COOKIE_HEADER_MODE: &str = "CODEXMANAGER_CPA_NO_COOKIE_HEADER_MODE";
const ENV_STRICT_REQUEST_PARAM_ALLOWLIST: &str = "CODEXMANAGER_STRICT_REQUEST_PARAM_ALLOWLIST";
const ENV_TOKEN_EXCHANGE_CLIENT_ID: &str = "CODEXMANAGER_CLIENT_ID";
const ENV_TOKEN_EXCHANGE_ISSUER: &str = "CODEXMANAGER_ISSUER";
const ENV_PROXY_LIST: &str = "CODEXMANAGER_PROXY_LIST";
const ENV_UPSTREAM_PROXY_URL: &str = "CODEXMANAGER_UPSTREAM_PROXY_URL";
const ENV_FREE_ACCOUNT_MAX_MODEL: &str = "CODEXMANAGER_FREE_ACCOUNT_MAX_MODEL";

#[derive(Default, Clone)]
struct UpstreamClientPool {
    proxies: Vec<String>,
    clients: Vec<Client>,
}

impl UpstreamClientPool {
    fn client_for_account(&self, account_id: &str) -> Option<&Client> {
        let idx = stable_proxy_index(account_id, self.clients.len())?;
        self.clients.get(idx)
    }

    fn proxy_for_account(&self, account_id: &str) -> Option<&str> {
        let idx = stable_proxy_index(account_id, self.proxies.len())?;
        self.proxies.get(idx).map(String::as_str)
    }
}

pub(crate) fn upstream_client() -> Client {
    ensure_runtime_config_loaded();
    crate::lock_utils::read_recover(upstream_client_lock(), "upstream_client").clone()
}

pub(crate) fn fresh_upstream_client() -> Client {
    ensure_runtime_config_loaded();
    build_upstream_client()
}

pub(crate) fn upstream_client_for_account(account_id: &str) -> Client {
    ensure_runtime_config_loaded();
    let cached =
        crate::lock_utils::read_recover(upstream_client_pool_lock(), "upstream_client_pool")
            .client_for_account(account_id)
            .cloned();
    cached.unwrap_or_else(upstream_client)
}

pub(crate) fn fresh_upstream_client_for_account(account_id: &str) -> Client {
    ensure_runtime_config_loaded();
    let pool = crate::lock_utils::read_recover(upstream_client_pool_lock(), "upstream_client_pool");
    if let Some(proxy_url) = pool.proxy_for_account(account_id) {
        return build_upstream_client_with_proxy(Some(proxy_url));
    }
    build_upstream_client()
}

fn upstream_connect_timeout_cached() -> Duration {
    Duration::from_secs(UPSTREAM_CONNECT_TIMEOUT_SECS.load(Ordering::Relaxed))
}

fn build_upstream_client() -> Client {
    let proxy_url = current_upstream_proxy_url();
    build_upstream_client_with_proxy(proxy_url.as_deref())
}

fn build_upstream_client_with_proxy(proxy_url: Option<&str>) -> Client {
    let mut builder = Client::builder()
        // 中文注释：显式关闭总超时，避免长时流式响应在客户端层被误判超时中断。
        .timeout(None::<Duration>)
        // 中文注释：连接阶段设置超时，避免网络异常时线程长期卡死占满并发槽位。
        .connect_timeout(upstream_connect_timeout_cached())
        .pool_max_idle_per_host(32)
        .pool_idle_timeout(Some(Duration::from_secs(90)))
        .tcp_keepalive(Some(Duration::from_secs(30)));
    if let Some(proxy_url) = proxy_url {
        let proxy = match Proxy::all(proxy_url) {
            Ok(proxy) => proxy,
            Err(err) => {
                log::warn!(
                    "event=gateway_proxy_pool_invalid_proxy proxy={} err={}",
                    proxy_url,
                    err
                );
                return build_upstream_client();
            }
        };
        builder = builder.proxy(proxy);
    }
    builder.build().unwrap_or_else(|err| {
        log::warn!("event=gateway_upstream_client_build_failed err={}", err);
        Client::new()
    })
}

pub(crate) fn upstream_total_timeout() -> Option<Duration> {
    ensure_runtime_config_loaded();
    let timeout_ms = UPSTREAM_TOTAL_TIMEOUT_MS.load(Ordering::Relaxed);
    if timeout_ms == 0 {
        None
    } else {
        Some(Duration::from_millis(timeout_ms))
    }
}

pub(crate) fn upstream_stream_timeout() -> Option<Duration> {
    ensure_runtime_config_loaded();
    let timeout_ms = UPSTREAM_STREAM_TIMEOUT_MS.load(Ordering::Relaxed);
    if timeout_ms == 0 {
        None
    } else {
        Some(Duration::from_millis(timeout_ms))
    }
}

pub(crate) fn current_upstream_stream_timeout_ms() -> u64 {
    ensure_runtime_config_loaded();
    UPSTREAM_STREAM_TIMEOUT_MS.load(Ordering::Relaxed)
}

pub(crate) fn account_max_inflight_limit() -> usize {
    ensure_runtime_config_loaded();
    ACCOUNT_MAX_INFLIGHT.load(Ordering::Relaxed)
}

pub(super) fn cpa_no_cookie_header_mode_enabled() -> bool {
    ensure_runtime_config_loaded();
    CPA_NO_COOKIE_HEADER_MODE.load(Ordering::Relaxed)
}

pub(crate) fn strict_request_param_allowlist_enabled() -> bool {
    ensure_runtime_config_loaded();
    STRICT_REQUEST_PARAM_ALLOWLIST.load(Ordering::Relaxed)
}

pub(super) fn set_cpa_no_cookie_header_mode_enabled(enabled: bool) {
    ensure_runtime_config_loaded();
    CPA_NO_COOKIE_HEADER_MODE.store(enabled, Ordering::Relaxed);
}

pub(crate) fn request_gate_wait_timeout() -> Duration {
    ensure_runtime_config_loaded();
    Duration::from_millis(REQUEST_GATE_WAIT_TIMEOUT_MS.load(Ordering::Relaxed))
}

pub(crate) fn trace_body_preview_max_bytes() -> usize {
    ensure_runtime_config_loaded();
    TRACE_BODY_PREVIEW_MAX_BYTES.load(Ordering::Relaxed)
}

pub(crate) fn front_proxy_max_body_bytes() -> usize {
    ensure_runtime_config_loaded();
    FRONT_PROXY_MAX_BODY_BYTES.load(Ordering::Relaxed)
}

pub(super) fn upstream_cookie() -> Option<String> {
    ensure_runtime_config_loaded();
    crate::lock_utils::read_recover(upstream_cookie_cell(), "upstream_cookie").clone()
}

pub(super) fn upstream_proxy_url() -> Option<String> {
    ensure_runtime_config_loaded();
    current_upstream_proxy_url()
}

pub(crate) fn current_free_account_max_model() -> String {
    ensure_runtime_config_loaded();
    crate::lock_utils::read_recover(free_account_max_model_cell(), "free_account_max_model").clone()
}

pub(crate) fn set_free_account_max_model(model: &str) -> Result<String, String> {
    ensure_runtime_config_loaded();
    let normalized = normalize_model_slug(model)?;
    std::env::set_var(ENV_FREE_ACCOUNT_MAX_MODEL, normalized.as_str());
    let mut cached =
        crate::lock_utils::write_recover(free_account_max_model_cell(), "free_account_max_model");
    *cached = normalized.clone();
    Ok(normalized)
}

pub(super) fn set_upstream_proxy_url(proxy_url: Option<&str>) -> Result<Option<String>, String> {
    ensure_runtime_config_loaded();
    let normalized = normalize_upstream_proxy_url(proxy_url)?;

    if let Some(value) = normalized.as_deref() {
        std::env::set_var(ENV_UPSTREAM_PROXY_URL, value);
    } else {
        std::env::remove_var(ENV_UPSTREAM_PROXY_URL);
    }

    let mut cached_proxy_url =
        crate::lock_utils::write_recover(upstream_proxy_url_cell(), "upstream_proxy_url");
    *cached_proxy_url = normalized.clone();
    drop(cached_proxy_url);
    refresh_upstream_clients_from_runtime_config();
    Ok(normalized)
}

pub(crate) fn set_upstream_stream_timeout_ms(timeout_ms: u64) -> u64 {
    ensure_runtime_config_loaded();
    UPSTREAM_STREAM_TIMEOUT_MS.store(timeout_ms, Ordering::Relaxed);
    std::env::set_var(ENV_UPSTREAM_STREAM_TIMEOUT_MS, timeout_ms.to_string());
    timeout_ms
}

pub(super) fn token_exchange_client_id() -> String {
    ensure_runtime_config_loaded();
    crate::lock_utils::read_recover(token_exchange_client_id_cell(), "token_exchange_client_id")
        .clone()
}

pub(super) fn token_exchange_default_issuer() -> String {
    ensure_runtime_config_loaded();
    crate::lock_utils::read_recover(token_exchange_issuer_cell(), "token_exchange_issuer").clone()
}

pub(super) fn reload_from_env() {
    REQUEST_GATE_WAIT_TIMEOUT_MS.store(
        env_u64_or(
            ENV_REQUEST_GATE_WAIT_TIMEOUT_MS,
            DEFAULT_REQUEST_GATE_WAIT_TIMEOUT_MS,
        ),
        Ordering::Relaxed,
    );
    TRACE_BODY_PREVIEW_MAX_BYTES.store(
        env_usize_or(
            ENV_TRACE_BODY_PREVIEW_MAX_BYTES,
            DEFAULT_TRACE_BODY_PREVIEW_MAX_BYTES,
        ),
        Ordering::Relaxed,
    );
    FRONT_PROXY_MAX_BODY_BYTES.store(
        env_usize_or(
            ENV_FRONT_PROXY_MAX_BODY_BYTES,
            DEFAULT_FRONT_PROXY_MAX_BODY_BYTES,
        ),
        Ordering::Relaxed,
    );
    UPSTREAM_CONNECT_TIMEOUT_SECS.store(
        env_u64_or(
            ENV_UPSTREAM_CONNECT_TIMEOUT_SECS,
            DEFAULT_UPSTREAM_CONNECT_TIMEOUT_SECS,
        ),
        Ordering::Relaxed,
    );
    UPSTREAM_TOTAL_TIMEOUT_MS.store(
        env_u64_or(
            ENV_UPSTREAM_TOTAL_TIMEOUT_MS,
            DEFAULT_UPSTREAM_TOTAL_TIMEOUT_MS,
        ),
        Ordering::Relaxed,
    );
    UPSTREAM_STREAM_TIMEOUT_MS.store(
        env_u64_or(
            ENV_UPSTREAM_STREAM_TIMEOUT_MS,
            DEFAULT_UPSTREAM_STREAM_TIMEOUT_MS,
        ),
        Ordering::Relaxed,
    );
    ACCOUNT_MAX_INFLIGHT.store(
        env_usize_or(ENV_ACCOUNT_MAX_INFLIGHT, DEFAULT_ACCOUNT_MAX_INFLIGHT),
        Ordering::Relaxed,
    );
    CPA_NO_COOKIE_HEADER_MODE.store(
        env_bool_or(
            ENV_CPA_NO_COOKIE_HEADER_MODE,
            DEFAULT_CPA_NO_COOKIE_HEADER_MODE,
        ),
        Ordering::Relaxed,
    );
    STRICT_REQUEST_PARAM_ALLOWLIST.store(
        env_bool_or(
            ENV_STRICT_REQUEST_PARAM_ALLOWLIST,
            DEFAULT_STRICT_REQUEST_PARAM_ALLOWLIST,
        ),
        Ordering::Relaxed,
    );

    let cookie = env_non_empty(ENV_UPSTREAM_COOKIE);
    let mut cached_cookie =
        crate::lock_utils::write_recover(upstream_cookie_cell(), "upstream_cookie");
    *cached_cookie = cookie;

    let client_id = env_non_empty(ENV_TOKEN_EXCHANGE_CLIENT_ID)
        .unwrap_or_else(|| DEFAULT_CLIENT_ID.to_string());
    let mut cached_client_id = crate::lock_utils::write_recover(
        token_exchange_client_id_cell(),
        "token_exchange_client_id",
    );
    *cached_client_id = client_id;

    let issuer =
        env_non_empty(ENV_TOKEN_EXCHANGE_ISSUER).unwrap_or_else(|| DEFAULT_ISSUER.to_string());
    let mut cached_issuer =
        crate::lock_utils::write_recover(token_exchange_issuer_cell(), "token_exchange_issuer");
    *cached_issuer = issuer;

    let proxy_url = env_non_empty(ENV_UPSTREAM_PROXY_URL);
    let converted_proxy = match normalize_upstream_proxy_url(proxy_url.as_deref()) {
        Ok(normalized) => normalized,
        Err(err) => {
            log::warn!(
                "event=gateway_invalid_upstream_proxy_url source=env var={} err={}",
                ENV_UPSTREAM_PROXY_URL,
                err
            );
            None
        }
    };
    let mut cached_proxy_url =
        crate::lock_utils::write_recover(upstream_proxy_url_cell(), "upstream_proxy_url");
    *cached_proxy_url = converted_proxy;
    drop(cached_proxy_url);

    let free_account_max_model = env_non_empty(ENV_FREE_ACCOUNT_MAX_MODEL)
        .and_then(|value| normalize_model_slug(value.as_str()).ok())
        .unwrap_or_else(|| DEFAULT_FREE_ACCOUNT_MAX_MODEL.to_string());
    let mut cached_free_account_max_model =
        crate::lock_utils::write_recover(free_account_max_model_cell(), "free_account_max_model");
    *cached_free_account_max_model = free_account_max_model;
    drop(cached_free_account_max_model);

    refresh_upstream_clients_from_runtime_config();
}

const ENV_UPSTREAM_COOKIE: &str = "CODEXMANAGER_UPSTREAM_COOKIE";

fn ensure_runtime_config_loaded() {
    let _ = RUNTIME_CONFIG_LOADED.get_or_init(|| reload_from_env());
}

fn upstream_client_lock() -> &'static RwLock<Client> {
    UPSTREAM_CLIENT.get_or_init(|| RwLock::new(build_upstream_client()))
}

fn upstream_client_pool_lock() -> &'static RwLock<UpstreamClientPool> {
    UPSTREAM_CLIENT_POOL.get_or_init(|| RwLock::new(build_upstream_client_pool()))
}

fn refresh_upstream_clients_from_runtime_config() {
    let client = build_upstream_client();
    let mut client_lock =
        crate::lock_utils::write_recover(upstream_client_lock(), "upstream_client");
    *client_lock = client;
    drop(client_lock);

    let pool = build_upstream_client_pool();
    let mut pool_lock =
        crate::lock_utils::write_recover(upstream_client_pool_lock(), "upstream_client_pool");
    *pool_lock = pool;
}

fn build_upstream_client_pool() -> UpstreamClientPool {
    if current_upstream_proxy_url().is_some() {
        return UpstreamClientPool::default();
    }
    let raw_proxies = parse_proxy_list_env();
    if raw_proxies.is_empty() {
        return UpstreamClientPool::default();
    }
    let mut proxies = Vec::with_capacity(raw_proxies.len());
    let mut clients = Vec::with_capacity(raw_proxies.len());
    for proxy in raw_proxies.into_iter() {
        if let Err(err) = Proxy::all(proxy.as_str()) {
            log::warn!(
                "event=gateway_proxy_pool_invalid_proxy proxy={} err={}",
                proxy,
                err
            );
            continue;
        }
        let client = build_upstream_client_with_proxy(Some(proxy.as_str()));
        proxies.push(proxy);
        clients.push(client);
    }
    if clients.is_empty() {
        UpstreamClientPool::default()
    } else {
        log::info!(
            "event=gateway_proxy_pool_initialized size={}",
            clients.len()
        );
        UpstreamClientPool { proxies, clients }
    }
}

fn upstream_cookie_cell() -> &'static RwLock<Option<String>> {
    UPSTREAM_COOKIE.get_or_init(|| RwLock::new(None))
}

fn upstream_proxy_url_cell() -> &'static RwLock<Option<String>> {
    UPSTREAM_PROXY_URL.get_or_init(|| RwLock::new(None))
}

fn free_account_max_model_cell() -> &'static RwLock<String> {
    FREE_ACCOUNT_MAX_MODEL.get_or_init(|| RwLock::new(DEFAULT_FREE_ACCOUNT_MAX_MODEL.to_string()))
}

fn current_upstream_proxy_url() -> Option<String> {
    crate::lock_utils::read_recover(upstream_proxy_url_cell(), "upstream_proxy_url").clone()
}

fn token_exchange_client_id_cell() -> &'static RwLock<String> {
    TOKEN_EXCHANGE_CLIENT_ID.get_or_init(|| RwLock::new(DEFAULT_CLIENT_ID.to_string()))
}

fn token_exchange_issuer_cell() -> &'static RwLock<String> {
    TOKEN_EXCHANGE_ISSUER.get_or_init(|| RwLock::new(DEFAULT_ISSUER.to_string()))
}

fn env_non_empty(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn env_u64_or(name: &str, default: u64) -> u64 {
    env_non_empty(name)
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(default)
}

fn env_usize_or(name: &str, default: usize) -> usize {
    env_non_empty(name)
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(default)
}

fn env_bool_or(name: &str, default: bool) -> bool {
    let Some(value) = env_non_empty(name) else {
        return default;
    };
    match value.to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => true,
        "0" | "false" | "no" | "off" => false,
        _ => default,
    }
}

fn normalize_model_slug(raw: &str) -> Result<String, String> {
    let normalized = raw.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return Err("freeAccountMaxModel is required".to_string());
    }
    if normalized == "gpt-5.4-pro" {
        return Ok("gpt-5.4".to_string());
    }
    if normalized
        .chars()
        .any(|ch| !(ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '/' | ':')))
    {
        return Err("freeAccountMaxModel contains unsupported characters".to_string());
    }
    Ok(normalized)
}

fn rewrite_socks_proxy_url(proxy_url: &str) -> String {
    let mut normalized = proxy_url.trim().to_string();
    if let Some(rest) = normalized.strip_prefix("http://socks") {
        normalized = format!("socks{rest}");
    } else if let Some(rest) = normalized.strip_prefix("https://socks") {
        normalized = format!("socks{rest}");
    }
    if normalized.starts_with("socks5://") {
        normalized = normalized.replacen("socks5://", "socks5h://", 1);
    } else if normalized.starts_with("socks://") {
        normalized = normalized.replacen("socks://", "socks5h://", 1);
    }
    normalized
}

fn normalize_upstream_proxy_url(proxy_url: Option<&str>) -> Result<Option<String>, String> {
    let mut normalized = proxy_url
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    if let Some(value) = normalized.as_mut() {
        *value = rewrite_socks_proxy_url(value);
        Proxy::all(value.as_str()).map_err(|err| format!("invalid proxy url: {err}"))?;
    }
    Ok(normalized)
}

fn parse_proxy_list_env() -> Vec<String> {
    let Some(raw) = env_non_empty(ENV_PROXY_LIST) else {
        return Vec::new();
    };
    raw.split(|ch| ch == ',' || ch == ';' || ch == '\n' || ch == '\r')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .take(MAX_UPSTREAM_PROXY_POOL_SIZE)
        .map(rewrite_socks_proxy_url)
        .collect()
}

fn stable_proxy_index(account_id: &str, size: usize) -> Option<usize> {
    if size == 0 {
        return None;
    }
    if size == 1 {
        return Some(0);
    }
    let hash = stable_account_hash(account_id);
    Some((hash as usize) % size)
}

fn stable_account_hash(account_id: &str) -> u64 {
    // 中文注释：FNV-1a 保证跨进程稳定，不受 std 默认随机种子影响。
    let mut hash = 14695981039346656037_u64;
    for byte in account_id.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(1099511628211_u64);
    }
    hash
}

#[cfg(test)]
#[path = "tests/runtime_config_tests.rs"]
mod tests;
