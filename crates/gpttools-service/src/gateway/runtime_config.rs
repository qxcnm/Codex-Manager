use reqwest::blocking::Client;
use std::sync::OnceLock;
use std::time::Duration;

static UPSTREAM_CLIENT: OnceLock<Client> = OnceLock::new();

pub(crate) const DEFAULT_MODELS_CLIENT_VERSION: &str = "0.98.0";
pub(crate) const DEFAULT_GATEWAY_DEBUG: bool = false;
const DEFAULT_UPSTREAM_CONNECT_TIMEOUT_SECS: u64 = 15;
const DEFAULT_ACCOUNT_MAX_INFLIGHT: usize = 0;
const DEFAULT_REQUEST_GATE_WAIT_TIMEOUT_MS: u64 = 300;
const DEFAULT_TRACE_BODY_PREVIEW_MAX_BYTES: usize = 0;
const DEFAULT_FRONT_PROXY_MAX_BODY_BYTES: usize = 16 * 1024 * 1024;

const ENV_REQUEST_GATE_WAIT_TIMEOUT_MS: &str = "GPTTOOLS_REQUEST_GATE_WAIT_TIMEOUT_MS";
const ENV_TRACE_BODY_PREVIEW_MAX_BYTES: &str = "GPTTOOLS_TRACE_BODY_PREVIEW_MAX_BYTES";
const ENV_FRONT_PROXY_MAX_BODY_BYTES: &str = "GPTTOOLS_FRONT_PROXY_MAX_BODY_BYTES";
const ENV_UPSTREAM_CONNECT_TIMEOUT_SECS: &str = "GPTTOOLS_UPSTREAM_CONNECT_TIMEOUT_SECS";
const ENV_ACCOUNT_MAX_INFLIGHT: &str = "GPTTOOLS_ACCOUNT_MAX_INFLIGHT";

pub(crate) fn upstream_client() -> &'static Client {
    UPSTREAM_CLIENT.get_or_init(|| {
        Client::builder()
            // 中文注释：显式关闭总超时，避免长时流式响应在客户端层被误判超时中断。
            .timeout(None::<Duration>)
            // 中文注释：连接阶段设置超时，避免网络异常时线程长期卡死占满并发槽位。
            .connect_timeout(upstream_connect_timeout())
            .pool_max_idle_per_host(32)
            .pool_idle_timeout(Some(Duration::from_secs(90)))
            .tcp_keepalive(Some(Duration::from_secs(30)))
            .build()
            .unwrap_or_else(|_| Client::new())
    })
}

fn upstream_connect_timeout() -> Duration {
    Duration::from_secs(env_u64_or(
        ENV_UPSTREAM_CONNECT_TIMEOUT_SECS,
        DEFAULT_UPSTREAM_CONNECT_TIMEOUT_SECS,
    ))
}

pub(crate) fn account_max_inflight_limit() -> usize {
    env_usize_or(ENV_ACCOUNT_MAX_INFLIGHT, DEFAULT_ACCOUNT_MAX_INFLIGHT)
}

pub(crate) fn request_gate_wait_timeout() -> Duration {
    Duration::from_millis(env_u64_or(
        ENV_REQUEST_GATE_WAIT_TIMEOUT_MS,
        DEFAULT_REQUEST_GATE_WAIT_TIMEOUT_MS,
    ))
}

pub(crate) fn trace_body_preview_max_bytes() -> usize {
    env_usize_or(
        ENV_TRACE_BODY_PREVIEW_MAX_BYTES,
        DEFAULT_TRACE_BODY_PREVIEW_MAX_BYTES,
    )
}

pub(crate) fn front_proxy_max_body_bytes() -> usize {
    env_usize_or(
        ENV_FRONT_PROXY_MAX_BODY_BYTES,
        DEFAULT_FRONT_PROXY_MAX_BODY_BYTES,
    )
}

fn env_u64_or(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(default)
}

fn env_usize_or(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(default)
}
