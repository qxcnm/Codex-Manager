use codexmanager_core::auth::DEFAULT_ORIGINATOR;
use codexmanager_core::auth::{DEFAULT_CLIENT_ID, DEFAULT_ISSUER};
use codexmanager_core::storage::Storage;
use reqwest::blocking::Client;
use reqwest::Proxy;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{OnceLock, RwLock};
use std::time::Duration;

static UPSTREAM_CLIENT: OnceLock<RwLock<Client>> = OnceLock::new();
static ASYNC_UPSTREAM_CLIENT: OnceLock<RwLock<reqwest::Client>> = OnceLock::new();
static RETRY_UPSTREAM_CLIENT: OnceLock<RwLock<Client>> = OnceLock::new();
static ASYNC_RETRY_UPSTREAM_CLIENT: OnceLock<RwLock<reqwest::Client>> = OnceLock::new();
static DIRECT_UPSTREAM_CLIENT: OnceLock<RwLock<Client>> = OnceLock::new();
static UPSTREAM_CLIENT_POOL: OnceLock<RwLock<UpstreamClientPool>> = OnceLock::new();
static ACCOUNT_CANDIDATE_CLIENTS: OnceLock<
    RwLock<HashMap<AccountCandidateClientKey, AccountCandidateClients>>,
> = OnceLock::new();
static ACCOUNT_PROXY_CLIENTS: OnceLock<RwLock<HashMap<String, AccountProxyClientCacheEntry>>> =
    OnceLock::new();
static AGGREGATE_CANDIDATE_CLIENTS: OnceLock<RwLock<HashMap<AggregateCandidateClientKey, Client>>> =
    OnceLock::new();
#[cfg(test)]
static UPSTREAM_CLIENT_BUILD_COUNT: AtomicUsize = AtomicUsize::new(0);
#[cfg(test)]
static ASYNC_UPSTREAM_CLIENT_BUILD_COUNT: AtomicUsize = AtomicUsize::new(0);
#[cfg(test)]
static DIRECT_UPSTREAM_CLIENT_USE_COUNT: AtomicUsize = AtomicUsize::new(0);
static RUNTIME_CONFIG_LOADED: OnceLock<()> = OnceLock::new();
static REQUEST_GATE_WAIT_TIMEOUT_MS: AtomicU64 =
    AtomicU64::new(DEFAULT_REQUEST_GATE_WAIT_TIMEOUT_MS);
static TRACE_BODY_PREVIEW_MAX_BYTES: AtomicUsize =
    AtomicUsize::new(DEFAULT_TRACE_BODY_PREVIEW_MAX_BYTES);
static FRONT_PROXY_MAX_BODY_BYTES: AtomicUsize =
    AtomicUsize::new(DEFAULT_FRONT_PROXY_MAX_BODY_BYTES);
static FRONT_PROXY_ZSTD_MAX_BODY_BYTES: AtomicUsize =
    AtomicUsize::new(DEFAULT_FRONT_PROXY_ZSTD_MAX_BODY_BYTES);
static UPSTREAM_CONNECT_TIMEOUT_SECS: AtomicU64 =
    AtomicU64::new(DEFAULT_UPSTREAM_CONNECT_TIMEOUT_SECS);
static UPSTREAM_TOTAL_TIMEOUT_MS: AtomicU64 = AtomicU64::new(DEFAULT_UPSTREAM_TOTAL_TIMEOUT_MS);
static UPSTREAM_STREAM_TIMEOUT_MS: AtomicU64 = AtomicU64::new(DEFAULT_UPSTREAM_STREAM_TIMEOUT_MS);
static SSE_KEEPALIVE_ENABLED: AtomicBool = AtomicBool::new(DEFAULT_SSE_KEEPALIVE_ENABLED);
static SSE_KEEPALIVE_INTERVAL_MS: AtomicU64 = AtomicU64::new(DEFAULT_SSE_KEEPALIVE_INTERVAL_MS);
static ACCOUNT_MAX_INFLIGHT: AtomicUsize = AtomicUsize::new(DEFAULT_ACCOUNT_MAX_INFLIGHT);
static THREAD_AWARE_ACCOUNT_DISTRIBUTION: AtomicBool =
    AtomicBool::new(DEFAULT_THREAD_AWARE_ACCOUNT_DISTRIBUTION);
static STRICT_REQUEST_PARAM_ALLOWLIST: AtomicBool =
    AtomicBool::new(DEFAULT_STRICT_REQUEST_PARAM_ALLOWLIST);
static ENABLE_REQUEST_COMPRESSION: AtomicBool = AtomicBool::new(DEFAULT_ENABLE_REQUEST_COMPRESSION);
static USE_WEBSOCKET_UPSTREAM: AtomicBool = AtomicBool::new(DEFAULT_USE_WEBSOCKET_UPSTREAM);
static CODEX_IMAGE_GENERATION_ENABLED: AtomicBool =
    AtomicBool::new(DEFAULT_CODEX_IMAGE_GENERATION_ENABLED);
static UPSTREAM_PROXY_URL: OnceLock<RwLock<Option<String>>> = OnceLock::new();
static UPSTREAM_PROXY_BYPASS_HOSTS: OnceLock<RwLock<Vec<String>>> = OnceLock::new();
static FREE_ACCOUNT_MAX_MODEL: OnceLock<RwLock<String>> = OnceLock::new();
static COMPACT_MODEL: OnceLock<RwLock<String>> = OnceLock::new();
static COMPACT_API_PATH: OnceLock<RwLock<String>> = OnceLock::new();
static MODEL_FORWARD_RULES: OnceLock<RwLock<Vec<ModelForwardRule>>> = OnceLock::new();
static COMPACT_MODEL_FORWARD_RULES: OnceLock<RwLock<Vec<ModelForwardRule>>> = OnceLock::new();
static CODEX_IMAGE_MAIN_MODEL: OnceLock<RwLock<String>> = OnceLock::new();
static CODEX_IMAGE_TOOL_MODEL: OnceLock<RwLock<String>> = OnceLock::new();
static ORIGINATOR: OnceLock<RwLock<String>> = OnceLock::new();
static CODEX_USER_AGENT_VERSION: OnceLock<RwLock<String>> = OnceLock::new();
static RESIDENCY_REQUIREMENT: OnceLock<RwLock<Option<String>>> = OnceLock::new();
static TOKEN_EXCHANGE_CLIENT_ID: OnceLock<RwLock<String>> = OnceLock::new();
static TOKEN_EXCHANGE_ISSUER: OnceLock<RwLock<String>> = OnceLock::new();

pub(crate) const DEFAULT_GATEWAY_DEBUG: bool = false;
const DEFAULT_UPSTREAM_CONNECT_TIMEOUT_SECS: u64 = 15;
const DEFAULT_UPSTREAM_TOTAL_TIMEOUT_MS: u64 = 0;
const DEFAULT_UPSTREAM_STREAM_TIMEOUT_MS: u64 = 300_000;
const DEFAULT_SSE_KEEPALIVE_ENABLED: bool = true;
const DEFAULT_SSE_KEEPALIVE_INTERVAL_MS: u64 = 15_000;
const DEFAULT_ACCOUNT_MAX_INFLIGHT: usize = 0;
const DEFAULT_THREAD_AWARE_ACCOUNT_DISTRIBUTION: bool = true;
const DEFAULT_STRICT_REQUEST_PARAM_ALLOWLIST: bool = false;
const DEFAULT_ENABLE_REQUEST_COMPRESSION: bool = true;
const DEFAULT_USE_WEBSOCKET_UPSTREAM: bool = false;
const DEFAULT_CODEX_IMAGE_GENERATION_ENABLED: bool = true;
const DEFAULT_REQUEST_GATE_WAIT_TIMEOUT_MS: u64 = 0;
const DEFAULT_TRACE_BODY_PREVIEW_MAX_BYTES: usize = 0;
const DEFAULT_FRONT_PROXY_MAX_BODY_BYTES: usize = 0;
const DEFAULT_FRONT_PROXY_ZSTD_MAX_BODY_BYTES: usize = 256 * 1024 * 1024;
const DEFAULT_FREE_ACCOUNT_MAX_MODEL: &str = "auto";
const DEFAULT_COMPACT_MODEL: &str = "auto";
const DEFAULT_COMPACT_API_PATH: &str = "/v1/responses/compact";
const DEFAULT_MODEL_FORWARD_RULES: &str = "";
const DEFAULT_COMPACT_MODEL_FORWARD_RULES: &str = "";
const DEFAULT_CODEX_IMAGE_MAIN_MODEL: &str = "gpt-5.4-mini";
const DEFAULT_CODEX_IMAGE_TOOL_MODEL: &str = "gpt-image-2";
const DEFAULT_CODEX_USER_AGENT_VERSION: &str = "0.130.0";
const MAX_UPSTREAM_PROXY_POOL_SIZE: usize = 5;
const MAX_CANDIDATE_CLIENT_CACHE_ENTRIES: usize = 512;

const ENV_REQUEST_GATE_WAIT_TIMEOUT_MS: &str = "CODEXMANAGER_REQUEST_GATE_WAIT_TIMEOUT_MS";
const ENV_TRACE_BODY_PREVIEW_MAX_BYTES: &str = "CODEXMANAGER_TRACE_BODY_PREVIEW_MAX_BYTES";
const ENV_FRONT_PROXY_MAX_BODY_BYTES: &str = "CODEXMANAGER_FRONT_PROXY_MAX_BODY_BYTES";
const ENV_FRONT_PROXY_ZSTD_MAX_BODY_BYTES: &str = "CODEXMANAGER_FRONT_PROXY_ZSTD_MAX_BODY_BYTES";
const ENV_UPSTREAM_CONNECT_TIMEOUT_SECS: &str = "CODEXMANAGER_UPSTREAM_CONNECT_TIMEOUT_SECS";
const ENV_UPSTREAM_TOTAL_TIMEOUT_MS: &str = "CODEXMANAGER_UPSTREAM_TOTAL_TIMEOUT_MS";
const ENV_UPSTREAM_STREAM_TIMEOUT_MS: &str = "CODEXMANAGER_UPSTREAM_STREAM_TIMEOUT_MS";
const ENV_SSE_KEEPALIVE_ENABLED: &str = "CODEXMANAGER_SSE_KEEPALIVE_ENABLED";
const ENV_SSE_KEEPALIVE_INTERVAL_MS: &str = "CODEXMANAGER_SSE_KEEPALIVE_INTERVAL_MS";
const ENV_ACCOUNT_MAX_INFLIGHT: &str = "CODEXMANAGER_ACCOUNT_MAX_INFLIGHT";
const ENV_STRICT_REQUEST_PARAM_ALLOWLIST: &str = "CODEXMANAGER_STRICT_REQUEST_PARAM_ALLOWLIST";
const ENV_ENABLE_REQUEST_COMPRESSION: &str = "CODEXMANAGER_ENABLE_REQUEST_COMPRESSION";
const ENV_USE_WEBSOCKET_UPSTREAM: &str = "CODEXMANAGER_USE_WEBSOCKET_UPSTREAM";
const ENV_CODEX_IMAGE_GENERATION_ENABLED: &str = "CODEXMANAGER_CODEX_IMAGE_GENERATION_ENABLED";
const ENV_CODEX_IMAGE_MAIN_MODEL: &str = "CODEXMANAGER_CODEX_IMAGE_MAIN_MODEL";
const ENV_CODEX_IMAGE_TOOL_MODEL: &str = "CODEXMANAGER_CODEX_IMAGE_TOOL_MODEL";
const ENV_TOKEN_EXCHANGE_CLIENT_ID: &str = "CODEXMANAGER_CLIENT_ID";
const ENV_TOKEN_EXCHANGE_ISSUER: &str = "CODEXMANAGER_ISSUER";
const ENV_PROXY_LIST: &str = "CODEXMANAGER_PROXY_LIST";
const ENV_UPSTREAM_PROXY_URL: &str = "CODEXMANAGER_UPSTREAM_PROXY_URL";
const ENV_UPSTREAM_PROXY_BYPASS_HOSTS: &str = "CODEXMANAGER_UPSTREAM_PROXY_BYPASS_HOSTS";
const ENV_FREE_ACCOUNT_MAX_MODEL: &str = "CODEXMANAGER_FREE_ACCOUNT_MAX_MODEL";
const ENV_COMPACT_MODEL: &str = "CODEXMANAGER_COMPACT_MODEL";
const ENV_COMPACT_API_PATH: &str = "CODEXMANAGER_COMPACT_API_PATH";
const ENV_MODEL_FORWARD_RULES: &str = "CODEXMANAGER_MODEL_FORWARD_RULES";
const ENV_COMPACT_MODEL_FORWARD_RULES: &str = "CODEXMANAGER_COMPACT_MODEL_FORWARD_RULES";
const ENV_ORIGINATOR: &str = "CODEXMANAGER_ORIGINATOR";
const ENV_RESIDENCY_REQUIREMENT: &str = "CODEXMANAGER_RESIDENCY_REQUIREMENT";
pub(crate) const RESIDENCY_HEADER_NAME: &str = "x-openai-internal-codex-residency";

#[derive(Default, Clone)]
struct UpstreamClientPool {
    proxies: Vec<String>,
    retry_clients: Vec<Client>,
    async_retry_clients: Vec<reqwest::Client>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct AccountCandidateClientKey {
    account_id: String,
    proxy_profile: Option<String>,
}

impl AccountCandidateClientKey {
    fn new(account_id: &str, proxy_profile: Option<String>) -> Self {
        Self {
            account_id: account_id.trim().to_string(),
            proxy_profile,
        }
    }
}

#[derive(Clone)]
struct AccountCandidateClients {
    blocking: Client,
    async_client: reqwest::Client,
}

#[derive(Clone)]
enum AccountProxyClientCacheEntry {
    NotConfigured,
    Invalid {
        proxy_url: String,
        error: String,
    },
    Ready {
        proxy_url: String,
        blocking_client: Client,
        async_client: reqwest::Client,
    },
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct AggregateCandidateClientKey {
    aggregate_api_id: String,
    url: String,
    proxy_profile: Option<String>,
}

impl AggregateCandidateClientKey {
    fn new(
        aggregate_api_id: &str,
        url: &str,
        proxy_profile: Option<String>,
    ) -> Result<Self, String> {
        let aggregate_api_id = aggregate_api_id.trim();
        if aggregate_api_id.is_empty() {
            return Err("aggregate api id is required".to_string());
        }
        let url = url.trim();
        if url.is_empty() {
            return Err("aggregate api url is required".to_string());
        }
        Ok(Self {
            aggregate_api_id: aggregate_api_id.to_string(),
            url: url.to_string(),
            proxy_profile,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ModelForwardRule {
    pub from_pattern: String,
    pub to_model: String,
}

impl UpstreamClientPool {
    fn retry_client_for_account(&self, account_id: &str) -> Option<&Client> {
        let idx = stable_proxy_index(account_id, self.retry_clients.len())?;
        self.retry_clients.get(idx)
    }

    fn async_retry_client_for_account(&self, account_id: &str) -> Option<&reqwest::Client> {
        let idx = stable_proxy_index(account_id, self.async_retry_clients.len())?;
        self.async_retry_clients.get(idx)
    }

    /// 函数 `proxy_for_account`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - account_id: 参数 account_id
    ///
    /// # 返回
    /// 返回函数执行结果
    fn proxy_for_account(&self, account_id: &str) -> Option<&str> {
        let idx = stable_proxy_index(account_id, self.proxies.len())?;
        self.proxies.get(idx).map(String::as_str)
    }
}

/// 函数 `upstream_client`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn upstream_client() -> Client {
    ensure_runtime_config_loaded();
    crate::lock_utils::read_recover(upstream_client_lock(), "upstream_client").clone()
}

pub(crate) fn upstream_client_for_aggregate_url(url: &str) -> Client {
    ensure_runtime_config_loaded();
    if aggregate_api_should_bypass_upstream_proxy(url) {
        return direct_upstream_client();
    }
    upstream_client()
}

/// 函数 `upstream_client_for_account`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn upstream_client_for_account(account_id: &str) -> Result<Client, String> {
    ensure_runtime_config_loaded();
    let account_id = account_id.trim();
    if account_id.is_empty() {
        return Ok(upstream_client());
    }
    match account_proxy_client_cache_entry(account_id) {
        AccountProxyClientCacheEntry::Ready {
            blocking_client, ..
        } => Ok(blocking_client),
        AccountProxyClientCacheEntry::Invalid { proxy_url, error } => Err(format!(
            "account explicit proxy for {account_id} is invalid and fail-closed: {proxy_url}. {error}"
        )),
        AccountProxyClientCacheEntry::NotConfigured => {
            Ok(account_candidate_clients_for_account(account_id).blocking)
        }
    }
}

pub(crate) fn prepare_upstream_client_for_account(account_id: &str) -> Result<(), String> {
    ensure_runtime_config_loaded();
    let account_id = account_id.trim();
    if account_id.is_empty() {
        return Err("account id is required".to_string());
    }
    upstream_client_for_account(account_id).map(|_| ())
}

/// 函数 `fresh_upstream_client_for_account`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn fresh_upstream_client_for_account(account_id: &str) -> Result<Client, String> {
    ensure_runtime_config_loaded();
    match account_proxy_client_cache_entry(account_id) {
        AccountProxyClientCacheEntry::Ready { proxy_url, .. } => {
            build_blocking_client_with_proxy_strict(Some(proxy_url.as_str()))
        }
        AccountProxyClientCacheEntry::Invalid { proxy_url, error } => Err(format!(
            "account explicit proxy for {account_id} is invalid and fail-closed: {proxy_url}. {error}"
        )),
        AccountProxyClientCacheEntry::NotConfigured => {
            let cached =
                crate::lock_utils::read_recover(upstream_client_pool_lock(), "upstream_client_pool")
                    .retry_client_for_account(account_id)
                    .cloned();
            Ok(cached.unwrap_or_else(retry_upstream_client))
        }
    }
}

pub(crate) fn async_upstream_client_for_account(
    account_id: &str,
) -> Result<reqwest::Client, String> {
    ensure_runtime_config_loaded();
    let account_id = account_id.trim();
    if account_id.is_empty() {
        return Ok(async_upstream_client());
    }
    match account_proxy_client_cache_entry(account_id) {
        AccountProxyClientCacheEntry::Ready { async_client, .. } => Ok(async_client),
        AccountProxyClientCacheEntry::Invalid { proxy_url, error } => Err(format!(
            "account explicit proxy for {account_id} is invalid and fail-closed: {proxy_url}. {error}"
        )),
        AccountProxyClientCacheEntry::NotConfigured => {
            Ok(account_candidate_clients_for_account(account_id).async_client)
        }
    }
}

fn async_upstream_client() -> reqwest::Client {
    crate::lock_utils::read_recover(async_upstream_client_lock(), "async_upstream_client").clone()
}

pub(crate) fn fresh_async_upstream_client_for_account(
    account_id: &str,
) -> Result<reqwest::Client, String> {
    ensure_runtime_config_loaded();
    match account_proxy_client_cache_entry(account_id) {
        AccountProxyClientCacheEntry::Ready { proxy_url, .. } => {
            build_async_client_with_proxy_strict(Some(proxy_url.as_str()))
        }
        AccountProxyClientCacheEntry::Invalid { proxy_url, error } => Err(format!(
            "account explicit proxy for {account_id} is invalid and fail-closed: {proxy_url}. {error}"
        )),
        AccountProxyClientCacheEntry::NotConfigured => {
            let cached = crate::lock_utils::read_recover(
                upstream_client_pool_lock(),
                "upstream_client_pool",
            )
            .async_retry_client_for_account(account_id)
            .cloned();
            Ok(cached.unwrap_or_else(async_retry_upstream_client))
        }
    }
}

/// 函数 `account_test_proxy_url_for_account`
///
/// 作者: gaohongshun
///
/// 时间: 2026-08-26
///
/// # 参数
/// - account_id: 参数 account_id
///
/// # 返回
/// 返回账号测试请求应使用的代理地址（显式账号代理 → 代理池 → 全局代理），
/// 显式代理配置无效时 fail-closed。
pub(crate) fn account_test_proxy_url_for_account(
    account_id: &str,
) -> Result<Option<String>, String> {
    ensure_runtime_config_loaded();
    match account_proxy_client_cache_entry(account_id) {
        AccountProxyClientCacheEntry::Ready { proxy_url, .. } => return Ok(Some(proxy_url)),
        AccountProxyClientCacheEntry::Invalid {
            proxy_url: _,
            error,
        } => {
            // 不回显 proxy_url：显式代理地址可能内嵌账号密码（http://user:pass@host）。
            return Err(format!(
                "account explicit proxy for {account_id} is invalid and fail-closed. {error}"
            ));
        }
        AccountProxyClientCacheEntry::NotConfigured => {}
    }
    let pool = crate::lock_utils::read_recover(upstream_client_pool_lock(), "upstream_client_pool");
    if let Some(proxy_url) = pool.proxy_for_account(account_id) {
        return Ok(Some(proxy_url.to_string()));
    }
    Ok(current_upstream_proxy_url())
}

/// 函数 `build_account_test_client_with_timeouts`
///
/// 作者: gaohongshun
///
/// 时间: 2026-08-26
///
/// # 参数
/// - proxy_url: 参数 proxy_url
/// - overall_timeout: 参数 overall_timeout
///
/// # 返回
/// 返回带整体超时的阻塞式上游客户端，用于有界生命周期的账号测试请求。
pub(crate) fn build_account_test_client_with_timeouts(
    proxy_url: Option<&str>,
    overall_timeout: Duration,
) -> Result<Client, String> {
    let mut builder = Client::builder()
        .timeout(overall_timeout)
        .connect_timeout(upstream_connect_timeout_cached())
        .pool_max_idle_per_host(32)
        .pool_idle_timeout(Some(Duration::from_secs(90)))
        .tcp_keepalive(Some(Duration::from_secs(30)));
    if let Some(proxy_url) = proxy_url.map(str::trim).filter(|value| !value.is_empty()) {
        let proxy = Proxy::all(proxy_url).map_err(|err| format!("invalid proxy url: {err}"))?;
        builder = builder.proxy(proxy);
    }
    builder
        .build()
        .map_err(|err| format!("build account test client failed: {err}"))
}

#[cfg(test)]
pub(crate) fn upstream_proxy_url_for_account(account_id: &str) -> Option<String> {
    ensure_runtime_config_loaded();
    match account_proxy_client_cache_entry(account_id) {
        AccountProxyClientCacheEntry::Ready { proxy_url, .. }
        | AccountProxyClientCacheEntry::Invalid { proxy_url, .. } => return Some(proxy_url),
        AccountProxyClientCacheEntry::NotConfigured => {}
    }
    let pool = crate::lock_utils::read_recover(upstream_client_pool_lock(), "upstream_client_pool");
    if let Some(proxy_url) = pool.proxy_for_account(account_id) {
        return Some(proxy_url.to_string());
    }
    current_upstream_proxy_url()
}

pub(crate) fn websocket_proxy_url_for_account(
    account_id: &str,
    target_url: &str,
) -> Result<Option<String>, String> {
    ensure_runtime_config_loaded();
    match account_proxy_client_cache_entry(account_id) {
        AccountProxyClientCacheEntry::Ready { proxy_url, .. } => return Ok(Some(proxy_url)),
        AccountProxyClientCacheEntry::Invalid { proxy_url, error } => {
            return Err(format!(
                "account explicit proxy for {account_id} is invalid and fail-closed: {proxy_url}. {error}"
            ));
        }
        AccountProxyClientCacheEntry::NotConfigured => {}
    }

    let pool = crate::lock_utils::read_recover(upstream_client_pool_lock(), "upstream_client_pool");
    if let Some(proxy_url) = pool.proxy_for_account(account_id) {
        return Ok(Some(proxy_url.to_string()));
    }
    drop(pool);
    if let Some(proxy_url) = current_upstream_proxy_url() {
        return Ok(Some(proxy_url));
    }

    environment_proxy_url_for_target(target_url)
}

fn environment_proxy_url_for_target(target_url: &str) -> Result<Option<String>, String> {
    let target = reqwest::Url::parse(target_url.trim())
        .map_err(|err| format!("invalid websocket target url {target_url}: {err}"))?;
    if environment_no_proxy_matches(&target) {
        return Ok(None);
    }

    let proxy_url = match target.scheme() {
        "wss" | "https" => {
            first_environment_proxy(&["https_proxy", "HTTPS_PROXY", "all_proxy", "ALL_PROXY"])
        }
        "ws" | "http" => {
            first_environment_proxy(&["http_proxy", "HTTP_PROXY", "all_proxy", "ALL_PROXY"])
        }
        _ => None,
    };
    if proxy_url.is_some() {
        return normalize_upstream_proxy_url(proxy_url.as_deref())
            .map_err(|err| format!("invalid websocket environment proxy: {err}"));
    }
    system_proxy_url_for_target(target_url)
}

fn system_proxy_url_for_target(target_url: &str) -> Result<Option<String>, String> {
    let matcher = hyper_util::client::proxy::matcher::Matcher::from_system();
    proxy_url_from_matcher_for_target(target_url, &matcher)
}

fn proxy_url_from_matcher_for_target(
    target_url: &str,
    matcher: &hyper_util::client::proxy::matcher::Matcher,
) -> Result<Option<String>, String> {
    let mut target = reqwest::Url::parse(target_url.trim())
        .map_err(|err| format!("invalid websocket target url {target_url}: {err}"))?;
    let http_scheme = match target.scheme() {
        "wss" => "https",
        "ws" => "http",
        "https" => "https",
        "http" => "http",
        scheme => return Err(format!("unsupported websocket target scheme: {scheme}")),
    };
    target
        .set_scheme(http_scheme)
        .map_err(|_| format!("replace websocket target scheme failed: {target_url}"))?;
    let target_uri = target
        .as_str()
        .parse::<axum::http::Uri>()
        .map_err(|err| format!("invalid websocket target uri {target_url}: {err}"))?;
    let Some(intercept) = matcher.intercept(&target_uri) else {
        return Ok(None);
    };
    let proxy_url = intercept.uri().to_string();
    normalize_upstream_proxy_url(Some(proxy_url.as_str()))
        .map_err(|err| format!("invalid websocket system proxy: {err}"))
}

fn first_environment_proxy(keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| env_non_empty(key))
}

fn environment_no_proxy_matches(target: &reqwest::Url) -> bool {
    let Some(host) = target.host_str().map(|value| value.to_ascii_lowercase()) else {
        return false;
    };
    let target_port = target.port_or_known_default();
    let Some(raw) = env_non_empty("no_proxy").or_else(|| env_non_empty("NO_PROXY")) else {
        return false;
    };

    raw.split(',').any(|entry| {
        let entry = entry.trim();
        if entry == "*" {
            return true;
        }
        if entry.is_empty() {
            return false;
        }

        let (pattern, port) = split_no_proxy_host_port(entry);
        if port.is_some() && port != target_port {
            return false;
        }
        let pattern = pattern
            .trim()
            .trim_start_matches("*.")
            .trim_start_matches('.')
            .trim_end_matches('.')
            .to_ascii_lowercase();
        !pattern.is_empty() && (host == pattern || host.ends_with(format!(".{pattern}").as_str()))
    })
}

fn split_no_proxy_host_port(entry: &str) -> (&str, Option<u16>) {
    if let Some(rest) = entry.strip_prefix('[') {
        if let Some((host, suffix)) = rest.split_once(']') {
            let port = suffix
                .strip_prefix(':')
                .and_then(|value| value.parse().ok());
            return (host, port);
        }
    }
    if entry.matches(':').count() == 1 {
        if let Some((host, port)) = entry.rsplit_once(':') {
            if let Ok(port) = port.parse::<u16>() {
                return (host, Some(port));
            }
        }
    }
    (entry, None)
}

pub(crate) fn upstream_client_for_aggregate_api_candidate(
    aggregate_api_id: &str,
    url: &str,
) -> Client {
    ensure_runtime_config_loaded();
    let Ok(key) = aggregate_candidate_client_key(aggregate_api_id, url) else {
        return upstream_client();
    };
    aggregate_candidate_client_for_key(key)
}

pub(crate) fn prepare_upstream_client_for_aggregate_api_candidate(
    aggregate_api_id: &str,
    url: &str,
) -> Result<(), String> {
    ensure_runtime_config_loaded();
    let key = aggregate_candidate_client_key(aggregate_api_id, url)?;
    let _ = aggregate_candidate_client_for_key(key);
    Ok(())
}

fn account_candidate_clients_for_account(account_id: &str) -> AccountCandidateClients {
    let proxy_profile = account_candidate_proxy_profile(account_id);
    let key = AccountCandidateClientKey::new(account_id, proxy_profile);
    if let Some(clients) = crate::lock_utils::read_recover(
        account_candidate_clients_lock(),
        "account_candidate_clients",
    )
    .get(&key)
    .cloned()
    {
        return clients;
    }

    let clients = AccountCandidateClients {
        blocking: build_upstream_client_with_proxy(key.proxy_profile.as_deref()),
        async_client: build_async_upstream_client_with_proxy(key.proxy_profile.as_deref()),
    };
    let mut cache = crate::lock_utils::write_recover(
        account_candidate_clients_lock(),
        "account_candidate_clients",
    );
    if let Some(existing) = cache.get(&key).cloned() {
        return existing;
    }
    if cache.len() >= MAX_CANDIDATE_CLIENT_CACHE_ENTRIES {
        cache.clear();
    }
    cache.insert(key, clients.clone());
    clients
}

fn account_candidate_proxy_profile(account_id: &str) -> Option<String> {
    let pool = crate::lock_utils::read_recover(upstream_client_pool_lock(), "upstream_client_pool");
    if let Some(proxy_url) = pool.proxy_for_account(account_id) {
        return Some(proxy_url.to_string());
    }
    drop(pool);
    current_upstream_proxy_url()
}

fn aggregate_candidate_client_key(
    aggregate_api_id: &str,
    url: &str,
) -> Result<AggregateCandidateClientKey, String> {
    let proxy_profile = if aggregate_api_should_bypass_upstream_proxy(url) {
        None
    } else {
        current_upstream_proxy_url()
    };
    AggregateCandidateClientKey::new(aggregate_api_id, url, proxy_profile)
}

fn aggregate_candidate_client_for_key(key: AggregateCandidateClientKey) -> Client {
    if let Some(client) = crate::lock_utils::read_recover(
        aggregate_candidate_clients_lock(),
        "aggregate_candidate_clients",
    )
    .get(&key)
    .cloned()
    {
        return client;
    }

    let client = build_upstream_client_with_proxy(key.proxy_profile.as_deref());
    let mut cache = crate::lock_utils::write_recover(
        aggregate_candidate_clients_lock(),
        "aggregate_candidate_clients",
    );
    if let Some(existing) = cache.get(&key).cloned() {
        return existing;
    }
    if cache.len() >= MAX_CANDIDATE_CLIENT_CACHE_ENTRIES {
        cache.clear();
    }
    cache.insert(key, client.clone());
    client
}

/// 函数 `upstream_connect_timeout_cached`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 返回函数执行结果
fn upstream_connect_timeout_cached() -> Duration {
    Duration::from_secs(UPSTREAM_CONNECT_TIMEOUT_SECS.load(Ordering::Relaxed))
}

pub(crate) fn current_upstream_connect_timeout() -> Duration {
    ensure_runtime_config_loaded();
    upstream_connect_timeout_cached()
}

/// 函数 `build_upstream_client`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 返回函数执行结果
fn build_upstream_client() -> Client {
    let proxy_url = current_upstream_proxy_url();
    build_upstream_client_with_proxy(proxy_url.as_deref())
}

fn build_async_upstream_client() -> reqwest::Client {
    let proxy_url = current_upstream_proxy_url();
    build_async_upstream_client_with_proxy(proxy_url.as_deref())
}

fn build_direct_upstream_client() -> Client {
    Client::builder()
        .no_proxy()
        .timeout(None::<Duration>)
        .connect_timeout(upstream_connect_timeout_cached())
        .pool_max_idle_per_host(32)
        .pool_idle_timeout(Some(Duration::from_secs(90)))
        .tcp_keepalive(Some(Duration::from_secs(30)))
        .build()
        .unwrap_or_else(|err| {
            log::warn!(
                "event=gateway_direct_upstream_client_build_failed err={}",
                err
            );
            Client::new()
        })
}

pub(crate) fn apply_async_upstream_proxy(
    mut builder: reqwest::ClientBuilder,
    proxy_url: Option<&str>,
    invalid_event: &str,
) -> reqwest::ClientBuilder {
    if let Some(proxy_url) = proxy_url.map(str::trim).filter(|value| !value.is_empty()) {
        match Proxy::all(proxy_url) {
            Ok(proxy) => {
                builder = builder.proxy(proxy);
            }
            Err(err) => {
                log::warn!("event={} proxy={} err={}", invalid_event, proxy_url, err);
            }
        }
    }
    builder
}

fn build_blocking_client_with_proxy_strict(proxy_url: Option<&str>) -> Result<Client, String> {
    let mut builder = Client::builder()
        .timeout(None::<Duration>)
        .connect_timeout(upstream_connect_timeout_cached())
        .pool_max_idle_per_host(32)
        .pool_idle_timeout(Some(Duration::from_secs(90)))
        .tcp_keepalive(Some(Duration::from_secs(30)));
    if let Some(proxy_url) = proxy_url.map(str::trim).filter(|value| !value.is_empty()) {
        let proxy = Proxy::all(proxy_url).map_err(|err| format!("invalid proxy url: {err}"))?;
        builder = builder.proxy(proxy);
    }
    builder
        .build()
        .map_err(|err| format!("build upstream client failed: {err}"))
}

fn build_async_client_with_proxy_strict(
    proxy_url: Option<&str>,
) -> Result<reqwest::Client, String> {
    let mut builder = reqwest::Client::builder()
        .connect_timeout(upstream_connect_timeout_cached())
        .pool_max_idle_per_host(32)
        .pool_idle_timeout(Some(Duration::from_secs(90)))
        .tcp_keepalive(Some(Duration::from_secs(30)));
    if let Some(proxy_url) = proxy_url.map(str::trim).filter(|value| !value.is_empty()) {
        let proxy = Proxy::all(proxy_url).map_err(|err| format!("invalid proxy url: {err}"))?;
        builder = builder.proxy(proxy);
    }
    builder
        .build()
        .map_err(|err| format!("build async upstream client failed: {err}"))
}

/// 函数 `build_upstream_client_with_proxy`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - proxy_url: 参数 proxy_url
///
/// # 返回
/// 返回函数执行结果
fn build_upstream_client_with_proxy(proxy_url: Option<&str>) -> Client {
    #[cfg(test)]
    UPSTREAM_CLIENT_BUILD_COUNT.fetch_add(1, Ordering::SeqCst);

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

fn build_async_upstream_client_with_proxy(proxy_url: Option<&str>) -> reqwest::Client {
    #[cfg(test)]
    ASYNC_UPSTREAM_CLIENT_BUILD_COUNT.fetch_add(1, Ordering::SeqCst);

    let mut builder = reqwest::Client::builder()
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
                return build_async_upstream_client();
            }
        };
        builder = builder.proxy(proxy);
    }
    builder.build().unwrap_or_else(|err| {
        log::warn!(
            "event=gateway_async_upstream_client_build_failed err={}",
            err
        );
        reqwest::Client::new()
    })
}

/// 函数 `upstream_total_timeout`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn upstream_total_timeout() -> Option<Duration> {
    ensure_runtime_config_loaded();
    let timeout_ms = UPSTREAM_TOTAL_TIMEOUT_MS.load(Ordering::Relaxed);
    if timeout_ms == 0 {
        None
    } else {
        Some(Duration::from_millis(timeout_ms))
    }
}

/// 函数 `upstream_stream_timeout`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn upstream_stream_timeout() -> Option<Duration> {
    ensure_runtime_config_loaded();
    let timeout_ms = UPSTREAM_STREAM_TIMEOUT_MS.load(Ordering::Relaxed);
    if timeout_ms == 0 {
        None
    } else {
        Some(Duration::from_millis(timeout_ms))
    }
}

/// 函数 `current_upstream_stream_timeout_ms`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn current_upstream_stream_timeout_ms() -> u64 {
    ensure_runtime_config_loaded();
    UPSTREAM_STREAM_TIMEOUT_MS.load(Ordering::Relaxed)
}

pub(crate) fn current_upstream_total_timeout_ms() -> u64 {
    ensure_runtime_config_loaded();
    UPSTREAM_TOTAL_TIMEOUT_MS.load(Ordering::Relaxed)
}

pub(super) fn current_sse_keepalive_enabled() -> bool {
    ensure_runtime_config_loaded();
    SSE_KEEPALIVE_ENABLED.load(Ordering::Relaxed)
}

pub(super) fn sse_keepalive_enabled_is_env_overridden() -> bool {
    env_non_empty(ENV_SSE_KEEPALIVE_ENABLED).is_some()
}

pub(super) fn set_sse_keepalive_enabled(enabled: bool) -> bool {
    ensure_runtime_config_loaded();
    if sse_keepalive_enabled_is_env_overridden() {
        return SSE_KEEPALIVE_ENABLED.load(Ordering::Relaxed);
    }
    SSE_KEEPALIVE_ENABLED.store(enabled, Ordering::Relaxed);
    enabled
}

pub(super) fn current_sse_keepalive_interval_ms() -> u64 {
    ensure_runtime_config_loaded();
    SSE_KEEPALIVE_INTERVAL_MS.load(Ordering::Relaxed).max(1)
}

pub(super) fn sse_keepalive_interval_is_env_overridden() -> bool {
    env_non_empty(ENV_SSE_KEEPALIVE_INTERVAL_MS).is_some()
}

pub(super) fn set_sse_keepalive_interval_ms(interval_ms: u64) -> Result<u64, String> {
    ensure_runtime_config_loaded();
    if interval_ms == 0 {
        return Err("SSE keepalive interval must be greater than 0".to_string());
    }
    if sse_keepalive_interval_is_env_overridden() {
        return Ok(SSE_KEEPALIVE_INTERVAL_MS.load(Ordering::Relaxed).max(1));
    }
    SSE_KEEPALIVE_INTERVAL_MS.store(interval_ms, Ordering::Relaxed);
    Ok(interval_ms)
}

/// 函数 `request_compression_enabled`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
pub(crate) fn use_websocket_upstream() -> bool {
    ensure_runtime_config_loaded();
    USE_WEBSOCKET_UPSTREAM.load(Ordering::Relaxed)
}

pub(crate) fn request_compression_enabled() -> bool {
    ensure_runtime_config_loaded();
    ENABLE_REQUEST_COMPRESSION.load(Ordering::Relaxed)
}

pub(crate) fn codex_image_generation_enabled() -> bool {
    ensure_runtime_config_loaded();
    CODEX_IMAGE_GENERATION_ENABLED.load(Ordering::Relaxed)
}

pub(crate) fn current_codex_image_main_model() -> String {
    ensure_runtime_config_loaded();
    crate::lock_utils::read_recover(codex_image_main_model_cell(), "codex_image_main_model").clone()
}

pub(crate) fn current_codex_image_tool_model() -> String {
    ensure_runtime_config_loaded();
    crate::lock_utils::read_recover(codex_image_tool_model_cell(), "codex_image_tool_model").clone()
}

/// 函数 `account_max_inflight_limit`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn account_max_inflight_limit() -> usize {
    ensure_runtime_config_loaded();
    ACCOUNT_MAX_INFLIGHT.load(Ordering::Relaxed)
}

/// 函数 `set_account_max_inflight_limit`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn set_account_max_inflight_limit(limit: usize) -> usize {
    ensure_runtime_config_loaded();
    ACCOUNT_MAX_INFLIGHT.store(limit, Ordering::Relaxed);
    limit
}

pub(crate) fn thread_aware_account_distribution_enabled() -> bool {
    ensure_runtime_config_loaded();
    THREAD_AWARE_ACCOUNT_DISTRIBUTION.load(Ordering::Relaxed)
}

pub(crate) fn set_thread_aware_account_distribution_enabled(enabled: bool) -> bool {
    ensure_runtime_config_loaded();
    THREAD_AWARE_ACCOUNT_DISTRIBUTION.store(enabled, Ordering::Relaxed);
    enabled
}

/// 函数 `strict_request_param_allowlist_enabled`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn strict_request_param_allowlist_enabled() -> bool {
    ensure_runtime_config_loaded();
    STRICT_REQUEST_PARAM_ALLOWLIST.load(Ordering::Relaxed)
}

/// 函数 `request_gate_wait_timeout`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn request_gate_wait_timeout() -> Option<Duration> {
    ensure_runtime_config_loaded();
    let timeout_ms = REQUEST_GATE_WAIT_TIMEOUT_MS.load(Ordering::Relaxed);
    if timeout_ms == 0 {
        None
    } else {
        Some(Duration::from_millis(timeout_ms))
    }
}

/// 函数 `trace_body_preview_max_bytes`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn trace_body_preview_max_bytes() -> usize {
    ensure_runtime_config_loaded();
    TRACE_BODY_PREVIEW_MAX_BYTES.load(Ordering::Relaxed)
}

/// 函数 `front_proxy_max_body_bytes`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn front_proxy_max_body_bytes() -> usize {
    ensure_runtime_config_loaded();
    FRONT_PROXY_MAX_BODY_BYTES.load(Ordering::Relaxed)
}

pub(crate) fn front_proxy_zstd_max_body_bytes() -> usize {
    ensure_runtime_config_loaded();
    FRONT_PROXY_ZSTD_MAX_BODY_BYTES.load(Ordering::Relaxed)
}

/// 函数 `upstream_proxy_url`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - super: 参数 super
///
/// # 返回
/// 返回函数执行结果
pub(super) fn upstream_proxy_url() -> Option<String> {
    ensure_runtime_config_loaded();
    current_upstream_proxy_url()
}

pub(super) fn upstream_proxy_bypass_hosts() -> String {
    ensure_runtime_config_loaded();
    crate::lock_utils::read_recover(
        upstream_proxy_bypass_hosts_cell(),
        "upstream_proxy_bypass_hosts",
    )
    .join("\n")
}

/// 函数 `current_free_account_max_model`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn current_free_account_max_model() -> String {
    ensure_runtime_config_loaded();
    crate::lock_utils::read_recover(free_account_max_model_cell(), "free_account_max_model").clone()
}

/// 函数 `current_compact_model`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 返回当前上下文压缩模型配置
pub(crate) fn current_compact_model() -> String {
    ensure_runtime_config_loaded();
    crate::lock_utils::read_recover(compact_model_cell(), "compact_model").clone()
}

/// 函数 `current_compact_model_override`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 返回压缩请求需要强制使用的模型；`auto` 表示不改写
pub(crate) fn current_compact_model_override() -> Option<String> {
    let current = current_compact_model();
    (!current.eq_ignore_ascii_case("auto")).then_some(current)
}

pub(crate) fn current_compact_api_path() -> String {
    ensure_runtime_config_loaded();
    crate::lock_utils::read_recover(compact_api_path_cell(), "compact_api_path").clone()
}

pub(crate) fn compact_api_path_uses_chat_completions() -> bool {
    current_compact_api_path().eq_ignore_ascii_case("/v1/chat/completions")
}

/// 函数 `current_model_forward_rules`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-05
///
/// # 参数
/// 无
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn current_model_forward_rules() -> String {
    ensure_runtime_config_loaded();
    serialize_model_forward_rules(&crate::lock_utils::read_recover(
        model_forward_rules_cell(),
        "model_forward_rules",
    ))
}

pub(crate) fn current_compact_model_forward_rules() -> String {
    ensure_runtime_config_loaded();
    serialize_model_forward_rules(&crate::lock_utils::read_recover(
        compact_model_forward_rules_cell(),
        "compact_model_forward_rules",
    ))
}

/// 函数 `resolve_forwarded_model`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-05
///
/// # 参数
/// - model: 参数 model
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn resolve_forwarded_model(model: &str) -> Option<String> {
    ensure_runtime_config_loaded();
    let normalized_model = normalize_model_forward_lookup_model(model)?;
    if let Some(forwarded) = resolve_forwarded_model_from_rules(
        &crate::lock_utils::read_recover(model_forward_rules_cell(), "model_forward_rules"),
        normalized_model.as_str(),
    ) {
        return Some(forwarded);
    }

    resolve_builtin_forwarded_model(normalized_model.as_str())
}

/// 函数 `resolve_builtin_forwarded_model`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-16
///
/// # 参数
/// - model: 参数 model
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn resolve_builtin_forwarded_model(model: &str) -> Option<String> {
    let _ = model;
    None
}

/// 函数 `current_originator`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn current_originator() -> String {
    ensure_runtime_config_loaded();
    crate::lock_utils::read_recover(originator_cell(), "originator").clone()
}

/// 函数 `default_originator`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-11
///
/// # 参数
/// 无
///
/// # 返回
/// 返回 Codex 默认 originator
pub(crate) fn default_originator() -> &'static str {
    DEFAULT_ORIGINATOR
}

/// 函数 `current_wire_originator`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn current_wire_originator() -> String {
    current_originator()
}

/// 函数 `set_originator`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn set_originator(originator: &str) -> Result<String, String> {
    ensure_runtime_config_loaded();
    let normalized = normalize_originator(originator)?;
    std::env::set_var(ENV_ORIGINATOR, normalized.as_str());
    let mut cached = crate::lock_utils::write_recover(originator_cell(), "originator");
    *cached = normalized.clone();
    Ok(normalized)
}

/// 函数 `current_codex_user_agent_version`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn current_codex_user_agent_version() -> String {
    ensure_runtime_config_loaded();
    crate::lock_utils::read_recover(codex_user_agent_version_cell(), "codex_user_agent_version")
        .clone()
}

/// 函数 `default_codex_user_agent_version`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-11
///
/// # 参数
/// 无
///
/// # 返回
/// 返回 Codex 默认 User-Agent 版本
pub(crate) fn default_codex_user_agent_version() -> &'static str {
    DEFAULT_CODEX_USER_AGENT_VERSION
}

/// 函数 `set_codex_user_agent_version`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn set_codex_user_agent_version(version: &str) -> Result<String, String> {
    ensure_runtime_config_loaded();
    let normalized = normalize_codex_user_agent_version(version)?;
    let mut cached = crate::lock_utils::write_recover(
        codex_user_agent_version_cell(),
        "codex_user_agent_version",
    );
    *cached = normalized.clone();
    Ok(normalized)
}

/// 函数 `current_codex_user_agent`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn current_codex_user_agent() -> String {
    ensure_runtime_config_loaded();
    let originator = current_wire_originator();
    let version = current_codex_user_agent_version();
    let os_info = os_info::get();
    format!(
        "{}/{} ({} {}; {}) {}",
        originator,
        version,
        os_info.os_type(),
        os_info.version(),
        os_info.architecture().unwrap_or("unknown"),
        current_codex_terminal_user_agent_token()
    )
}

/// 函数 `current_residency_requirement`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn current_residency_requirement() -> Option<String> {
    ensure_runtime_config_loaded();
    crate::lock_utils::read_recover(residency_requirement_cell(), "residency_requirement").clone()
}

/// 函数 `set_residency_requirement`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn set_residency_requirement(value: Option<&str>) -> Result<Option<String>, String> {
    ensure_runtime_config_loaded();
    let normalized = normalize_residency_requirement(value)?;
    if let Some(value) = normalized.as_deref() {
        std::env::set_var(ENV_RESIDENCY_REQUIREMENT, value);
    } else {
        std::env::remove_var(ENV_RESIDENCY_REQUIREMENT);
    }
    let mut cached =
        crate::lock_utils::write_recover(residency_requirement_cell(), "residency_requirement");
    *cached = normalized.clone();
    Ok(normalized)
}

/// 函数 `set_free_account_max_model`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn set_free_account_max_model(model: &str) -> Result<String, String> {
    ensure_runtime_config_loaded();
    let normalized = normalize_model_slug(model)?;
    std::env::set_var(ENV_FREE_ACCOUNT_MAX_MODEL, normalized.as_str());
    let mut cached =
        crate::lock_utils::write_recover(free_account_max_model_cell(), "free_account_max_model");
    *cached = normalized.clone();
    Ok(normalized)
}

/// 函数 `set_model_forward_rules`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-05
///
/// # 参数
/// - raw: 参数 raw
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn set_model_forward_rules(raw: &str) -> Result<String, String> {
    ensure_runtime_config_loaded();
    let normalized = normalize_model_forward_rules(raw)?;
    let parsed = parse_model_forward_rules(normalized.as_str())?;
    if normalized.is_empty() {
        std::env::remove_var(ENV_MODEL_FORWARD_RULES);
    } else {
        std::env::set_var(ENV_MODEL_FORWARD_RULES, normalized.as_str());
    }
    let mut cached =
        crate::lock_utils::write_recover(model_forward_rules_cell(), "model_forward_rules");
    *cached = parsed;
    Ok(normalized)
}

pub(crate) fn set_compact_model_forward_rules(raw: &str) -> Result<String, String> {
    ensure_runtime_config_loaded();
    let normalized = normalize_model_forward_rules(raw)?;
    let parsed = parse_model_forward_rules(normalized.as_str())?;
    if normalized.is_empty() {
        std::env::remove_var(ENV_COMPACT_MODEL_FORWARD_RULES);
    } else {
        std::env::set_var(ENV_COMPACT_MODEL_FORWARD_RULES, normalized.as_str());
    }
    let mut cached = crate::lock_utils::write_recover(
        compact_model_forward_rules_cell(),
        "compact_model_forward_rules",
    );
    *cached = parsed;
    Ok(normalized)
}

/// 函数 `set_request_compression_enabled`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn set_request_compression_enabled(enabled: bool) -> bool {
    ensure_runtime_config_loaded();
    ENABLE_REQUEST_COMPRESSION.store(enabled, Ordering::Relaxed);
    std::env::set_var(
        ENV_ENABLE_REQUEST_COMPRESSION,
        if enabled { "1" } else { "0" },
    );
    enabled
}

/// 函数 `set_upstream_proxy_url`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - super: 参数 super
///
/// # 返回
/// 返回函数执行结果
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

pub(super) fn set_upstream_proxy_bypass_hosts(raw: Option<&str>) -> String {
    ensure_runtime_config_loaded();
    let normalized = normalize_upstream_proxy_bypass_hosts(raw);

    if normalized.is_empty() {
        std::env::remove_var(ENV_UPSTREAM_PROXY_BYPASS_HOSTS);
    } else {
        std::env::set_var(ENV_UPSTREAM_PROXY_BYPASS_HOSTS, normalized.as_str());
    }

    let mut cached_bypass_hosts = crate::lock_utils::write_recover(
        upstream_proxy_bypass_hosts_cell(),
        "upstream_proxy_bypass_hosts",
    );
    *cached_bypass_hosts = parse_upstream_proxy_bypass_hosts(normalized.as_str());
    normalized
}

/// 函数 `set_upstream_stream_timeout_ms`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn set_upstream_stream_timeout_ms(timeout_ms: u64) -> u64 {
    ensure_runtime_config_loaded();
    UPSTREAM_STREAM_TIMEOUT_MS.store(timeout_ms, Ordering::Relaxed);
    std::env::set_var(ENV_UPSTREAM_STREAM_TIMEOUT_MS, timeout_ms.to_string());
    timeout_ms
}

pub(crate) fn set_upstream_total_timeout_ms(timeout_ms: u64) -> u64 {
    ensure_runtime_config_loaded();
    UPSTREAM_TOTAL_TIMEOUT_MS.store(timeout_ms, Ordering::Relaxed);
    std::env::set_var(ENV_UPSTREAM_TOTAL_TIMEOUT_MS, timeout_ms.to_string());
    timeout_ms
}

/// 函数 `token_exchange_client_id`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - super: 参数 super
///
/// # 返回
/// 返回函数执行结果
pub(super) fn token_exchange_client_id() -> String {
    ensure_runtime_config_loaded();
    crate::lock_utils::read_recover(token_exchange_client_id_cell(), "token_exchange_client_id")
        .clone()
}

/// 函数 `token_exchange_default_issuer`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - super: 参数 super
///
/// # 返回
/// 返回函数执行结果
pub(super) fn token_exchange_default_issuer() -> String {
    ensure_runtime_config_loaded();
    crate::lock_utils::read_recover(token_exchange_issuer_cell(), "token_exchange_issuer").clone()
}

/// 函数 `reload_from_env`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - super: 参数 super
///
/// # 返回
/// 无
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
    let zstd_max_body_bytes = env_usize_or(
        ENV_FRONT_PROXY_ZSTD_MAX_BODY_BYTES,
        DEFAULT_FRONT_PROXY_ZSTD_MAX_BODY_BYTES,
    );
    FRONT_PROXY_ZSTD_MAX_BODY_BYTES.store(
        if zstd_max_body_bytes == 0 {
            DEFAULT_FRONT_PROXY_ZSTD_MAX_BODY_BYTES
        } else {
            zstd_max_body_bytes
        },
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
    SSE_KEEPALIVE_ENABLED.store(
        env_bool_or(ENV_SSE_KEEPALIVE_ENABLED, DEFAULT_SSE_KEEPALIVE_ENABLED),
        Ordering::Relaxed,
    );
    SSE_KEEPALIVE_INTERVAL_MS.store(
        env_u64_or(
            ENV_SSE_KEEPALIVE_INTERVAL_MS,
            DEFAULT_SSE_KEEPALIVE_INTERVAL_MS,
        ),
        Ordering::Relaxed,
    );
    ACCOUNT_MAX_INFLIGHT.store(
        env_usize_or(ENV_ACCOUNT_MAX_INFLIGHT, DEFAULT_ACCOUNT_MAX_INFLIGHT),
        Ordering::Relaxed,
    );
    STRICT_REQUEST_PARAM_ALLOWLIST.store(
        env_bool_or(
            ENV_STRICT_REQUEST_PARAM_ALLOWLIST,
            DEFAULT_STRICT_REQUEST_PARAM_ALLOWLIST,
        ),
        Ordering::Relaxed,
    );
    ENABLE_REQUEST_COMPRESSION.store(
        env_bool_or(
            ENV_ENABLE_REQUEST_COMPRESSION,
            DEFAULT_ENABLE_REQUEST_COMPRESSION,
        ),
        Ordering::Relaxed,
    );
    USE_WEBSOCKET_UPSTREAM.store(
        env_bool_or(ENV_USE_WEBSOCKET_UPSTREAM, DEFAULT_USE_WEBSOCKET_UPSTREAM),
        Ordering::Relaxed,
    );
    CODEX_IMAGE_GENERATION_ENABLED.store(
        env_bool_or(
            ENV_CODEX_IMAGE_GENERATION_ENABLED,
            DEFAULT_CODEX_IMAGE_GENERATION_ENABLED,
        ),
        Ordering::Relaxed,
    );

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

    let bypass_hosts = normalize_upstream_proxy_bypass_hosts(
        env_non_empty(ENV_UPSTREAM_PROXY_BYPASS_HOSTS).as_deref(),
    );
    let mut cached_bypass_hosts = crate::lock_utils::write_recover(
        upstream_proxy_bypass_hosts_cell(),
        "upstream_proxy_bypass_hosts",
    );
    *cached_bypass_hosts = parse_upstream_proxy_bypass_hosts(bypass_hosts.as_str());
    drop(cached_bypass_hosts);

    let free_account_max_model = env_non_empty(ENV_FREE_ACCOUNT_MAX_MODEL)
        .and_then(|value| normalize_model_slug(value.as_str()).ok())
        .unwrap_or_else(|| DEFAULT_FREE_ACCOUNT_MAX_MODEL.to_string());
    let mut cached_free_account_max_model =
        crate::lock_utils::write_recover(free_account_max_model_cell(), "free_account_max_model");
    *cached_free_account_max_model = free_account_max_model;
    drop(cached_free_account_max_model);

    let compact_model = env_non_empty(ENV_COMPACT_MODEL)
        .and_then(|value| normalize_model_slug_with_error(value.as_str(), "compactModel").ok())
        .unwrap_or_else(|| DEFAULT_COMPACT_MODEL.to_string());
    let mut cached_compact_model =
        crate::lock_utils::write_recover(compact_model_cell(), "compact_model");
    *cached_compact_model = compact_model;
    drop(cached_compact_model);

    let compact_api_path = env_non_empty(ENV_COMPACT_API_PATH)
        .map(|value| normalize_compact_api_path(value.as_str()))
        .transpose()
        .unwrap_or_else(|err| {
            log::warn!(
                "event=gateway_invalid_compact_api_path source=env var={} err={}",
                ENV_COMPACT_API_PATH,
                err
            );
            None
        })
        .unwrap_or_else(|| DEFAULT_COMPACT_API_PATH.to_string());
    let mut cached_compact_api_path =
        crate::lock_utils::write_recover(compact_api_path_cell(), "compact_api_path");
    *cached_compact_api_path = compact_api_path;
    drop(cached_compact_api_path);

    let model_forward_rules = env_non_empty(ENV_MODEL_FORWARD_RULES)
        .map(|value| parse_model_forward_rules(value.as_str()))
        .transpose()
        .unwrap_or_else(|err| {
            log::warn!(
                "event=gateway_invalid_model_forward_rules source=env var={} err={}",
                ENV_MODEL_FORWARD_RULES,
                err
            );
            None
        })
        .unwrap_or_default();
    let mut cached_model_forward_rules =
        crate::lock_utils::write_recover(model_forward_rules_cell(), "model_forward_rules");
    *cached_model_forward_rules = model_forward_rules;
    drop(cached_model_forward_rules);

    let compact_model_forward_rules = env_non_empty(ENV_COMPACT_MODEL_FORWARD_RULES)
        .map(|value| parse_model_forward_rules(value.as_str()))
        .transpose()
        .unwrap_or_else(|err| {
            log::warn!(
                "event=gateway_invalid_compact_model_forward_rules source=env var={} err={}",
                ENV_COMPACT_MODEL_FORWARD_RULES,
                err
            );
            None
        })
        .unwrap_or_default();
    let mut cached_compact_model_forward_rules = crate::lock_utils::write_recover(
        compact_model_forward_rules_cell(),
        "compact_model_forward_rules",
    );
    *cached_compact_model_forward_rules = compact_model_forward_rules;
    drop(cached_compact_model_forward_rules);

    let codex_image_main_model = env_non_empty(ENV_CODEX_IMAGE_MAIN_MODEL)
        .and_then(|value| normalize_model_slug(value.as_str()).ok())
        .unwrap_or_else(|| DEFAULT_CODEX_IMAGE_MAIN_MODEL.to_string());
    let mut cached_codex_image_main_model =
        crate::lock_utils::write_recover(codex_image_main_model_cell(), "codex_image_main_model");
    *cached_codex_image_main_model = codex_image_main_model;
    drop(cached_codex_image_main_model);

    let codex_image_tool_model = env_non_empty(ENV_CODEX_IMAGE_TOOL_MODEL)
        .and_then(|value| normalize_model_slug(value.as_str()).ok())
        .unwrap_or_else(|| DEFAULT_CODEX_IMAGE_TOOL_MODEL.to_string());
    let mut cached_codex_image_tool_model =
        crate::lock_utils::write_recover(codex_image_tool_model_cell(), "codex_image_tool_model");
    *cached_codex_image_tool_model = codex_image_tool_model;
    drop(cached_codex_image_tool_model);

    let originator = env_non_empty(ENV_ORIGINATOR)
        .and_then(|value| normalize_originator(value.as_str()).ok())
        .unwrap_or_else(|| DEFAULT_ORIGINATOR.to_string());
    let mut cached_originator = crate::lock_utils::write_recover(originator_cell(), "originator");
    *cached_originator = originator;
    drop(cached_originator);

    let mut cached_user_agent_version = crate::lock_utils::write_recover(
        codex_user_agent_version_cell(),
        "codex_user_agent_version",
    );
    *cached_user_agent_version = DEFAULT_CODEX_USER_AGENT_VERSION.to_string();
    drop(cached_user_agent_version);

    let residency_requirement = env_non_empty(ENV_RESIDENCY_REQUIREMENT)
        .and_then(|value| normalize_residency_requirement(Some(value.as_str())).ok())
        .flatten();
    let mut cached_residency =
        crate::lock_utils::write_recover(residency_requirement_cell(), "residency_requirement");
    *cached_residency = residency_requirement;
    drop(cached_residency);

    refresh_upstream_clients_from_runtime_config();
}

/// 函数 `ensure_runtime_config_loaded`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
fn ensure_runtime_config_loaded() {
    let _ = RUNTIME_CONFIG_LOADED.get_or_init(|| reload_from_env());
}

/// 函数 `upstream_client_lock`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 返回函数执行结果
fn upstream_client_lock() -> &'static RwLock<Client> {
    UPSTREAM_CLIENT.get_or_init(|| RwLock::new(build_upstream_client()))
}

fn async_upstream_client_lock() -> &'static RwLock<reqwest::Client> {
    ASYNC_UPSTREAM_CLIENT.get_or_init(|| RwLock::new(build_async_upstream_client()))
}

fn retry_upstream_client() -> Client {
    crate::lock_utils::read_recover(retry_upstream_client_lock(), "retry_upstream_client").clone()
}

fn retry_upstream_client_lock() -> &'static RwLock<Client> {
    RETRY_UPSTREAM_CLIENT.get_or_init(|| RwLock::new(build_upstream_client()))
}

fn direct_upstream_client() -> Client {
    #[cfg(test)]
    DIRECT_UPSTREAM_CLIENT_USE_COUNT.fetch_add(1, Ordering::SeqCst);

    crate::lock_utils::read_recover(direct_upstream_client_lock(), "direct_upstream_client").clone()
}

fn direct_upstream_client_lock() -> &'static RwLock<Client> {
    DIRECT_UPSTREAM_CLIENT.get_or_init(|| RwLock::new(build_direct_upstream_client()))
}

fn async_retry_upstream_client() -> reqwest::Client {
    crate::lock_utils::read_recover(
        async_retry_upstream_client_lock(),
        "async_retry_upstream_client",
    )
    .clone()
}

fn async_retry_upstream_client_lock() -> &'static RwLock<reqwest::Client> {
    ASYNC_RETRY_UPSTREAM_CLIENT.get_or_init(|| RwLock::new(build_async_upstream_client()))
}

/// 函数 `upstream_client_pool_lock`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 返回函数执行结果
fn upstream_client_pool_lock() -> &'static RwLock<UpstreamClientPool> {
    UPSTREAM_CLIENT_POOL.get_or_init(|| RwLock::new(build_upstream_client_pool()))
}

fn account_candidate_clients_lock(
) -> &'static RwLock<HashMap<AccountCandidateClientKey, AccountCandidateClients>> {
    ACCOUNT_CANDIDATE_CLIENTS.get_or_init(|| RwLock::new(HashMap::new()))
}

fn account_proxy_clients_lock() -> &'static RwLock<HashMap<String, AccountProxyClientCacheEntry>> {
    ACCOUNT_PROXY_CLIENTS.get_or_init(|| RwLock::new(HashMap::new()))
}

pub(crate) fn invalidate_account_proxy_client_cache(account_id: &str) {
    let normalized = account_id.trim();
    if normalized.is_empty() {
        return;
    }
    crate::lock_utils::write_recover(account_proxy_clients_lock(), "account_proxy_client_cache")
        .remove(normalized);
    crate::lock_utils::write_recover(
        account_candidate_clients_lock(),
        "account_candidate_clients",
    )
    .retain(|key, _| key.account_id != normalized);
}

fn clear_account_proxy_client_cache() {
    crate::lock_utils::write_recover(account_proxy_clients_lock(), "account_proxy_client_cache")
        .clear();
}

fn aggregate_candidate_clients_lock(
) -> &'static RwLock<HashMap<AggregateCandidateClientKey, Client>> {
    AGGREGATE_CANDIDATE_CLIENTS.get_or_init(|| RwLock::new(HashMap::new()))
}

/// 函数 `refresh_upstream_clients_from_runtime_config`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
fn refresh_upstream_clients_from_runtime_config() {
    let client = build_upstream_client();
    let mut client_lock =
        crate::lock_utils::write_recover(upstream_client_lock(), "upstream_client");
    *client_lock = client;
    drop(client_lock);

    let async_client = build_async_upstream_client();
    let mut async_client_lock =
        crate::lock_utils::write_recover(async_upstream_client_lock(), "async_upstream_client");
    *async_client_lock = async_client;
    drop(async_client_lock);

    let retry_client = build_upstream_client();
    let mut retry_client_lock =
        crate::lock_utils::write_recover(retry_upstream_client_lock(), "retry_upstream_client");
    *retry_client_lock = retry_client;
    drop(retry_client_lock);

    let async_retry_client = build_async_upstream_client();
    let mut async_retry_client_lock = crate::lock_utils::write_recover(
        async_retry_upstream_client_lock(),
        "async_retry_upstream_client",
    );
    *async_retry_client_lock = async_retry_client;
    drop(async_retry_client_lock);

    let direct_client = build_direct_upstream_client();
    let mut direct_client_lock =
        crate::lock_utils::write_recover(direct_upstream_client_lock(), "direct_upstream_client");
    *direct_client_lock = direct_client;
    drop(direct_client_lock);

    let pool = build_upstream_client_pool();
    let mut pool_lock =
        crate::lock_utils::write_recover(upstream_client_pool_lock(), "upstream_client_pool");
    *pool_lock = pool;
    drop(pool_lock);

    clear_candidate_client_caches();
}

fn clear_candidate_client_caches() {
    clear_account_proxy_client_cache();
    crate::lock_utils::write_recover(
        account_candidate_clients_lock(),
        "account_candidate_clients",
    )
    .clear();
    crate::lock_utils::write_recover(
        aggregate_candidate_clients_lock(),
        "aggregate_candidate_clients",
    )
    .clear();
}

/// 函数 `build_upstream_client_pool`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 返回函数执行结果
fn build_upstream_client_pool() -> UpstreamClientPool {
    if current_upstream_proxy_url().is_some() {
        return UpstreamClientPool::default();
    }
    let raw_proxies = parse_proxy_list_env();
    if raw_proxies.is_empty() {
        return UpstreamClientPool::default();
    }
    let mut proxies = Vec::with_capacity(raw_proxies.len());
    let mut retry_clients = Vec::with_capacity(raw_proxies.len());
    let mut async_retry_clients = Vec::with_capacity(raw_proxies.len());
    for proxy in raw_proxies.into_iter() {
        if let Err(err) = Proxy::all(proxy.as_str()) {
            log::warn!(
                "event=gateway_proxy_pool_invalid_proxy proxy={} err={}",
                proxy,
                err
            );
            continue;
        }
        let retry_client = build_upstream_client_with_proxy(Some(proxy.as_str()));
        let async_retry_client = build_async_upstream_client_with_proxy(Some(proxy.as_str()));
        proxies.push(proxy);
        retry_clients.push(retry_client);
        async_retry_clients.push(async_retry_client);
    }
    if retry_clients.is_empty() {
        UpstreamClientPool::default()
    } else {
        log::info!(
            "event=gateway_proxy_pool_initialized size={}",
            retry_clients.len()
        );
        UpstreamClientPool {
            proxies,
            retry_clients,
            async_retry_clients,
        }
    }
}

#[cfg(test)]
fn reset_upstream_client_build_count_for_test() {
    UPSTREAM_CLIENT_BUILD_COUNT.store(0, Ordering::SeqCst);
}

#[cfg(test)]
fn upstream_client_build_count_for_test() -> usize {
    UPSTREAM_CLIENT_BUILD_COUNT.load(Ordering::SeqCst)
}

#[cfg(test)]
fn reset_async_upstream_client_build_count_for_test() {
    ASYNC_UPSTREAM_CLIENT_BUILD_COUNT.store(0, Ordering::SeqCst);
}

#[cfg(test)]
fn async_upstream_client_build_count_for_test() -> usize {
    ASYNC_UPSTREAM_CLIENT_BUILD_COUNT.load(Ordering::SeqCst)
}

#[cfg(test)]
fn reset_direct_upstream_client_use_count_for_test() {
    DIRECT_UPSTREAM_CLIENT_USE_COUNT.store(0, Ordering::SeqCst);
}

#[cfg(test)]
fn direct_upstream_client_use_count_for_test() -> usize {
    DIRECT_UPSTREAM_CLIENT_USE_COUNT.load(Ordering::SeqCst)
}

/// 函数 `upstream_proxy_url_cell`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 返回函数执行结果
fn upstream_proxy_url_cell() -> &'static RwLock<Option<String>> {
    UPSTREAM_PROXY_URL.get_or_init(|| RwLock::new(None))
}

fn upstream_proxy_bypass_hosts_cell() -> &'static RwLock<Vec<String>> {
    UPSTREAM_PROXY_BYPASS_HOSTS.get_or_init(|| RwLock::new(Vec::new()))
}

/// 函数 `free_account_max_model_cell`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 返回函数执行结果
fn free_account_max_model_cell() -> &'static RwLock<String> {
    FREE_ACCOUNT_MAX_MODEL.get_or_init(|| RwLock::new(DEFAULT_FREE_ACCOUNT_MAX_MODEL.to_string()))
}

/// 函数 `compact_model_cell`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 返回函数执行结果
fn compact_model_cell() -> &'static RwLock<String> {
    COMPACT_MODEL.get_or_init(|| RwLock::new(DEFAULT_COMPACT_MODEL.to_string()))
}

fn compact_api_path_cell() -> &'static RwLock<String> {
    COMPACT_API_PATH.get_or_init(|| RwLock::new(DEFAULT_COMPACT_API_PATH.to_string()))
}

/// 函数 `model_forward_rules_cell`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-05
///
/// # 参数
/// 无
///
/// # 返回
/// 返回函数执行结果
fn model_forward_rules_cell() -> &'static RwLock<Vec<ModelForwardRule>> {
    MODEL_FORWARD_RULES.get_or_init(|| {
        let initial = parse_model_forward_rules(DEFAULT_MODEL_FORWARD_RULES).unwrap_or_default();
        RwLock::new(initial)
    })
}

fn compact_model_forward_rules_cell() -> &'static RwLock<Vec<ModelForwardRule>> {
    COMPACT_MODEL_FORWARD_RULES.get_or_init(|| {
        let initial =
            parse_model_forward_rules(DEFAULT_COMPACT_MODEL_FORWARD_RULES).unwrap_or_default();
        RwLock::new(initial)
    })
}

fn codex_image_main_model_cell() -> &'static RwLock<String> {
    CODEX_IMAGE_MAIN_MODEL.get_or_init(|| RwLock::new(DEFAULT_CODEX_IMAGE_MAIN_MODEL.to_string()))
}

fn codex_image_tool_model_cell() -> &'static RwLock<String> {
    CODEX_IMAGE_TOOL_MODEL.get_or_init(|| RwLock::new(DEFAULT_CODEX_IMAGE_TOOL_MODEL.to_string()))
}

/// 函数 `originator_cell`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 返回函数执行结果
fn originator_cell() -> &'static RwLock<String> {
    ORIGINATOR.get_or_init(|| RwLock::new(DEFAULT_ORIGINATOR.to_string()))
}

/// 函数 `codex_user_agent_version_cell`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 返回函数执行结果
fn codex_user_agent_version_cell() -> &'static RwLock<String> {
    CODEX_USER_AGENT_VERSION
        .get_or_init(|| RwLock::new(DEFAULT_CODEX_USER_AGENT_VERSION.to_string()))
}

/// 函数 `residency_requirement_cell`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 返回函数执行结果
fn residency_requirement_cell() -> &'static RwLock<Option<String>> {
    RESIDENCY_REQUIREMENT.get_or_init(|| RwLock::new(None))
}

/// 函数 `current_upstream_proxy_url`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 返回函数执行结果
fn current_upstream_proxy_url() -> Option<String> {
    crate::lock_utils::read_recover(upstream_proxy_url_cell(), "upstream_proxy_url").clone()
}

fn account_proxy_client_cache_entry(account_id: &str) -> AccountProxyClientCacheEntry {
    let normalized = account_id.trim();
    if normalized.is_empty() {
        return AccountProxyClientCacheEntry::NotConfigured;
    }

    if let Some(entry) =
        crate::lock_utils::read_recover(account_proxy_clients_lock(), "account_proxy_client_cache")
            .get(normalized)
            .cloned()
    {
        return entry;
    }

    let entry = load_account_proxy_client_cache_entry(normalized);
    crate::lock_utils::write_recover(account_proxy_clients_lock(), "account_proxy_client_cache")
        .insert(normalized.to_string(), entry.clone());
    entry
}

fn load_account_proxy_client_cache_entry(account_id: &str) -> AccountProxyClientCacheEntry {
    let Some(storage) = crate::storage_helpers::open_storage() else {
        return AccountProxyClientCacheEntry::NotConfigured;
    };
    load_account_proxy_client_cache_entry_from_storage(&storage, account_id)
}

fn load_account_proxy_client_cache_entry_from_storage(
    storage: &Storage,
    account_id: &str,
) -> AccountProxyClientCacheEntry {
    let proxy_url =
        match crate::account_proxy::resolve_account_proxy_mode_from_storage(storage, account_id) {
            crate::account_proxy::AccountProxyMode::Disabled => {
                return AccountProxyClientCacheEntry::NotConfigured;
            }
            crate::account_proxy::AccountProxyMode::Invalid {
                proxy_url, error, ..
            } => {
                return AccountProxyClientCacheEntry::Invalid {
                    proxy_url: proxy_url.unwrap_or_default(),
                    error,
                };
            }
            crate::account_proxy::AccountProxyMode::Explicit { proxy_url, .. } => proxy_url,
        };

    let normalized_proxy_url = match normalize_upstream_proxy_url(Some(proxy_url.as_str())) {
        Ok(Some(proxy_url)) => proxy_url,
        Ok(None) => return AccountProxyClientCacheEntry::NotConfigured,
        Err(error) => {
            return AccountProxyClientCacheEntry::Invalid { proxy_url, error };
        }
    };
    let blocking_client =
        match build_blocking_client_with_proxy_strict(Some(normalized_proxy_url.as_str())) {
            Ok(client) => client,
            Err(error) => {
                return AccountProxyClientCacheEntry::Invalid {
                    proxy_url: normalized_proxy_url,
                    error,
                };
            }
        };
    let async_client =
        match build_async_client_with_proxy_strict(Some(normalized_proxy_url.as_str())) {
            Ok(client) => client,
            Err(error) => {
                return AccountProxyClientCacheEntry::Invalid {
                    proxy_url: normalized_proxy_url,
                    error,
                };
            }
        };

    AccountProxyClientCacheEntry::Ready {
        proxy_url: normalized_proxy_url,
        blocking_client,
        async_client,
    }
}

/// 函数 `token_exchange_client_id_cell`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 返回函数执行结果
fn token_exchange_client_id_cell() -> &'static RwLock<String> {
    TOKEN_EXCHANGE_CLIENT_ID.get_or_init(|| RwLock::new(DEFAULT_CLIENT_ID.to_string()))
}

/// 函数 `token_exchange_issuer_cell`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 返回函数执行结果
fn token_exchange_issuer_cell() -> &'static RwLock<String> {
    TOKEN_EXCHANGE_ISSUER.get_or_init(|| RwLock::new(DEFAULT_ISSUER.to_string()))
}

/// 函数 `env_non_empty`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - name: 参数 name
///
/// # 返回
/// 返回函数执行结果
fn env_non_empty(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// 函数 `env_u64_or`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - name: 参数 name
/// - default: 参数 default
///
/// # 返回
/// 返回函数执行结果
fn env_u64_or(name: &str, default: u64) -> u64 {
    env_non_empty(name)
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(default)
}

/// 函数 `env_usize_or`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - name: 参数 name
/// - default: 参数 default
///
/// # 返回
/// 返回函数执行结果
fn env_usize_or(name: &str, default: usize) -> usize {
    env_non_empty(name)
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(default)
}

/// 函数 `env_bool_or`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - name: 参数 name
/// - default: 参数 default
///
/// # 返回
/// 返回函数执行结果
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

/// 函数 `normalize_model_forward_pattern`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-05
///
/// # 参数
/// - raw: 参数 raw
///
/// # 返回
/// 返回函数执行结果
fn normalize_model_forward_pattern(raw: &str) -> Result<String, String> {
    let normalized = raw.trim();
    if normalized.is_empty() {
        return Err("modelForwardRules pattern is required".to_string());
    }
    if normalized.chars().all(|ch| ch == '*') {
        return Err("modelForwardRules pattern cannot be wildcard-only".to_string());
    }
    if normalized
        .chars()
        .any(|ch| !(ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '/' | ':' | '*')))
    {
        return Err("modelForwardRules pattern contains unsupported characters".to_string());
    }
    Ok(normalized.to_string())
}

/// 函数 `normalize_forward_target_model`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-05
///
/// # 参数
/// - raw: 参数 raw
///
/// # 返回
/// 返回函数执行结果
fn normalize_forward_target_model(raw: &str) -> Result<String, String> {
    let normalized = raw.trim();
    if normalized.is_empty() {
        return Err("modelForwardRules target model is required".to_string());
    }
    if normalized.eq_ignore_ascii_case("auto") {
        return Err("modelForwardRules target model cannot be auto".to_string());
    }
    if normalized
        .chars()
        .any(|ch| !(ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '/' | ':')))
    {
        return Err("modelForwardRules target model contains unsupported characters".to_string());
    }
    Ok(normalized.to_string())
}

/// 函数 `parse_model_forward_rule_line`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-05
///
/// # 参数
/// - line: 参数 line
///
/// # 返回
/// 返回函数执行结果
fn parse_model_forward_rule_line(line: &str) -> Option<(&str, &str)> {
    line.split_once("=>").or_else(|| line.split_once('='))
}

/// 函数 `normalize_model_forward_rules`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-05
///
/// # 参数
/// - raw: 参数 raw
///
/// # 返回
/// 返回函数执行结果
fn normalize_model_forward_rules(raw: &str) -> Result<String, String> {
    let mut lines = Vec::new();
    for (idx, line) in raw.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some((pattern, target)) = parse_model_forward_rule_line(trimmed) else {
            return Err(format!(
                "modelForwardRules line {} must use pattern=target",
                idx + 1
            ));
        };
        let normalized_pattern = normalize_model_forward_pattern(pattern)?;
        let normalized_target = normalize_forward_target_model(target)?;
        lines.push(format!("{normalized_pattern}={normalized_target}"));
    }
    Ok(lines.join("\n"))
}

/// 函数 `parse_model_forward_rules`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-05
///
/// # 参数
/// - raw: 参数 raw
///
/// # 返回
/// 返回函数执行结果
fn parse_model_forward_rules(raw: &str) -> Result<Vec<ModelForwardRule>, String> {
    let normalized = normalize_model_forward_rules(raw)?;
    let mut rules = Vec::new();
    for line in normalized.lines() {
        let Some((from_pattern, to_model)) = line.split_once('=') else {
            continue;
        };
        rules.push(ModelForwardRule {
            from_pattern: from_pattern.to_string(),
            to_model: to_model.to_string(),
        });
    }
    Ok(rules)
}

/// 函数 `serialize_model_forward_rules`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-05
///
/// # 参数
/// - rules: 参数 rules
///
/// # 返回
/// 返回函数执行结果
fn serialize_model_forward_rules(rules: &[ModelForwardRule]) -> String {
    rules
        .iter()
        .map(|rule| format!("{}={}", rule.from_pattern, rule.to_model))
        .collect::<Vec<_>>()
        .join("\n")
}

fn resolve_forwarded_model_from_rules(
    rules: &[ModelForwardRule],
    normalized_model: &str,
) -> Option<String> {
    rules
        .iter()
        .find(|rule| wildcard_pattern_matches(rule.from_pattern.as_str(), normalized_model))
        .map(|rule| rule.to_model.clone())
}

/// 函数 `wildcard_pattern_matches`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-05
///
/// # 参数
/// - pattern: 参数 pattern
/// - value: 参数 value
///
/// # 返回
/// 返回函数执行结果
fn wildcard_pattern_matches(pattern: &str, value: &str) -> bool {
    if !pattern.contains('*') {
        return pattern.eq_ignore_ascii_case(value);
    }
    let normalized_pattern = pattern.to_ascii_lowercase();
    let normalized_value = value.to_ascii_lowercase();
    let starts_with_wildcard = normalized_pattern.starts_with('*');
    let ends_with_wildcard = normalized_pattern.ends_with('*');
    let segments = normalized_pattern
        .split('*')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    if segments.is_empty() {
        return false;
    }

    let mut cursor = 0usize;
    for (idx, segment) in segments.iter().enumerate() {
        let Some(found) = normalized_value[cursor..].find(*segment) else {
            return false;
        };
        let absolute = cursor + found;
        if idx == 0 && !starts_with_wildcard && absolute != 0 {
            return false;
        }
        cursor = absolute + segment.len();
    }

    if !ends_with_wildcard {
        return cursor == normalized_value.len();
    }
    true
}

fn normalize_model_forward_lookup_model(raw: &str) -> Option<String> {
    let normalized = raw.trim();
    if normalized.is_empty() {
        return None;
    }
    if normalized
        .chars()
        .any(|ch| !(ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '/' | ':')))
    {
        return None;
    }
    Some(normalized.to_string())
}

/// 函数 `normalize_model_slug`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - raw: 参数 raw
///
/// # 返回
/// 返回函数执行结果
fn normalize_model_slug(raw: &str) -> Result<String, String> {
    normalize_model_slug_with_error(raw, "freeAccountMaxModel")
}

fn normalize_model_slug_with_error(raw: &str, field_name: &str) -> Result<String, String> {
    let normalized = raw.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return Err(format!("{field_name} is required"));
    }
    if normalized == "auto" {
        return Ok(normalized);
    }
    if normalized == "gpt-5.4-pro" {
        return Ok("gpt-5.4".to_string());
    }
    if normalized
        .chars()
        .any(|ch| !(ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '/' | ':')))
    {
        return Err(format!("{field_name} contains unsupported characters"));
    }
    Ok(normalized)
}

fn normalize_compact_api_path(raw: &str) -> Result<String, String> {
    let normalized = raw.trim();
    if normalized.is_empty() {
        return Err("compactApiPath is required".to_string());
    }
    let canonical = normalized.split('?').next().unwrap_or(normalized).trim();
    match canonical {
        "/v1/responses/compact" | "/v1/chat/completions" => Ok(canonical.to_string()),
        _ => {
            Err("compactApiPath must be /v1/responses/compact or /v1/chat/completions".to_string())
        }
    }
}

/// 函数 `normalize_originator`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - raw: 参数 raw
///
/// # 返回
/// 返回函数执行结果
fn normalize_originator(raw: &str) -> Result<String, String> {
    let normalized = raw.trim();
    if normalized.is_empty() {
        return Err("originator is required".to_string());
    }
    if normalized.chars().any(|ch| ch.is_ascii_control()) {
        return Err("originator contains control characters".to_string());
    }
    Ok(normalized.to_string())
}

/// 函数 `normalize_codex_user_agent_version`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - raw: 参数 raw
///
/// # 返回
/// 返回函数执行结果
fn normalize_codex_user_agent_version(raw: &str) -> Result<String, String> {
    let normalized = raw.trim();
    if normalized.is_empty() {
        return Err("codexUserAgentVersion is required".to_string());
    }
    if normalized.chars().any(|ch| ch.is_ascii_control()) {
        return Err("codexUserAgentVersion contains control characters".to_string());
    }
    if normalized
        .chars()
        .any(|ch| !(ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_' | '+')))
    {
        return Err("codexUserAgentVersion contains unsupported characters".to_string());
    }
    Ok(normalized.to_string())
}

/// 函数 `current_codex_terminal_user_agent_token`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 返回函数执行结果
fn current_codex_terminal_user_agent_token() -> String {
    if let Some(program) = env_non_empty("TERM_PROGRAM") {
        let version = env_non_empty("TERM_PROGRAM_VERSION");
        return sanitize_header_value(format_terminal_user_agent(program, version));
    }
    if std::env::var_os("WEZTERM_VERSION").is_some() {
        return sanitize_header_value(format_terminal_user_agent(
            "WezTerm".to_string(),
            env_non_empty("WEZTERM_VERSION"),
        ));
    }
    if std::env::var_os("ITERM_SESSION_ID").is_some()
        || std::env::var_os("ITERM_PROFILE").is_some()
        || std::env::var_os("ITERM_PROFILE_NAME").is_some()
    {
        return sanitize_header_value("iTerm.app".to_string());
    }
    if std::env::var_os("TERM_SESSION_ID").is_some() {
        return sanitize_header_value("Apple_Terminal".to_string());
    }
    if std::env::var_os("KITTY_WINDOW_ID").is_some()
        || std::env::var("TERM")
            .map(|term| term.contains("kitty"))
            .unwrap_or(false)
    {
        return sanitize_header_value("kitty".to_string());
    }
    if std::env::var_os("ALACRITTY_SOCKET").is_some()
        || std::env::var("TERM")
            .map(|term| term == "alacritty")
            .unwrap_or(false)
    {
        return sanitize_header_value("Alacritty".to_string());
    }
    if std::env::var_os("KONSOLE_VERSION").is_some() {
        return sanitize_header_value(format_terminal_user_agent(
            "Konsole".to_string(),
            env_non_empty("KONSOLE_VERSION"),
        ));
    }
    if std::env::var_os("GNOME_TERMINAL_SCREEN").is_some() {
        return sanitize_header_value("gnome-terminal".to_string());
    }
    if std::env::var_os("VTE_VERSION").is_some() {
        return sanitize_header_value(format_terminal_user_agent(
            "VTE".to_string(),
            env_non_empty("VTE_VERSION"),
        ));
    }
    if std::env::var_os("WT_SESSION").is_some() {
        return "WindowsTerminal".to_string();
    }
    if let Some(term) = env_non_empty("TERM") {
        return sanitize_header_value(term);
    }
    "unknown".to_string()
}

fn format_terminal_user_agent(name: String, version: Option<String>) -> String {
    match version.as_ref().filter(|value| !value.is_empty()) {
        Some(version) => format!("{name}/{version}"),
        None => name,
    }
}

/// 函数 `sanitize_header_value`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - raw: 参数 raw
///
/// # 返回
/// 返回函数执行结果
fn sanitize_header_value(raw: String) -> String {
    let sanitized: String = raw
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '/') {
                ch
            } else {
                '_'
            }
        })
        .collect();
    if sanitized.trim().is_empty() {
        return "unknown".to_string();
    }
    sanitized
}

/// 函数 `normalize_residency_requirement`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - raw: 参数 raw
///
/// # 返回
/// 返回函数执行结果
fn normalize_residency_requirement(raw: Option<&str>) -> Result<Option<String>, String> {
    let Some(value) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    match value.to_ascii_lowercase().as_str() {
        "us" => Ok(Some("us".to_string())),
        _ => Err("residencyRequirement only supports 'us' or empty".to_string()),
    }
}

/// 函数 `rewrite_socks_proxy_url`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - proxy_url: 参数 proxy_url
///
/// # 返回
/// 返回函数执行结果
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

/// 函数 `normalize_upstream_proxy_url`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - proxy_url: 参数 proxy_url
///
/// # 返回
/// 返回函数执行结果
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

fn parse_upstream_proxy_bypass_hosts(raw: &str) -> Vec<String> {
    raw.split(|ch| matches!(ch, ',' | ';' | '\n' | '\r'))
        .filter_map(normalize_upstream_proxy_bypass_host)
        .fold(Vec::new(), |mut hosts, host| {
            if !hosts.contains(&host) {
                hosts.push(host);
            }
            hosts
        })
}

fn normalize_upstream_proxy_bypass_hosts(raw: Option<&str>) -> String {
    raw.map(parse_upstream_proxy_bypass_hosts)
        .unwrap_or_default()
        .join("\n")
}

fn normalize_upstream_proxy_bypass_host(raw: &str) -> Option<String> {
    let mut value = raw
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .to_ascii_lowercase();
    if value.is_empty() {
        return None;
    }
    if let Some(fragment_start) = value.find('#') {
        value.truncate(fragment_start);
        value = value.trim().to_string();
    }
    if value.is_empty() {
        return None;
    }

    if let Some(rest) = value.strip_prefix("*.") {
        let normalized = normalize_exact_bypass_host(rest)?;
        return Some(format!("*.{normalized}"));
    }

    normalize_exact_bypass_host(value.as_str())
}

fn normalize_exact_bypass_host(raw: &str) -> Option<String> {
    let candidate = raw.trim().trim_end_matches('.');
    if candidate.is_empty() {
        return None;
    }
    if let Ok(parsed) = reqwest::Url::parse(candidate) {
        return parsed
            .host_str()
            .map(|host| host.trim_end_matches('.').to_ascii_lowercase());
    }

    let without_path = candidate.split('/').next().unwrap_or(candidate);
    let parse_target = format!("https://{without_path}");
    if let Ok(parsed) = reqwest::Url::parse(parse_target.as_str()) {
        return parsed
            .host_str()
            .map(|host| host.trim_end_matches('.').to_ascii_lowercase());
    }

    without_path
        .split(':')
        .next()
        .map(str::trim)
        .filter(|host| !host.is_empty())
        .map(|host| host.trim_end_matches('.').to_ascii_lowercase())
}

/// 函数 `parse_proxy_list_env`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 返回函数执行结果
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

pub(crate) fn aggregate_api_should_bypass_upstream_proxy(url: &str) -> bool {
    let Ok(parsed) = reqwest::Url::parse(url.trim()) else {
        return false;
    };
    let Some(host) = parsed.host_str().map(|value| value.to_ascii_lowercase()) else {
        return false;
    };
    crate::lock_utils::read_recover(
        upstream_proxy_bypass_hosts_cell(),
        "upstream_proxy_bypass_hosts",
    )
    .iter()
    .any(|pattern| bypass_host_pattern_matches(pattern, host.as_str()))
}

fn bypass_host_pattern_matches(pattern: &str, host: &str) -> bool {
    if let Some(suffix) = pattern.strip_prefix("*.") {
        return host.ends_with(&format!(".{suffix}"));
    }
    host == pattern
}

/// 函数 `stable_proxy_index`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - account_id: 参数 account_id
/// - size: 参数 size
///
/// # 返回
/// 返回函数执行结果
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

/// 函数 `stable_account_hash`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - account_id: 参数 account_id
///
/// # 返回
/// 返回函数执行结果
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
