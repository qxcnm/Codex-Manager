use codexmanager_core::storage::{Account, Storage, Token};
use reqwest::header::HeaderMap;
use reqwest::header::CONTENT_TYPE;
use reqwest::Client;
use reqwest::Method;
use reqwest::StatusCode;
use std::future::Future;
#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{OnceLock, RwLock};
use std::time::Duration;
use tokio::runtime::{Builder, Runtime};

const REQUEST_ID_HEADER: &str = "x-request-id";
const OAI_REQUEST_ID_HEADER: &str = "x-oai-request-id";
const CF_RAY_HEADER: &str = "cf-ray";
const AUTH_ERROR_HEADER: &str = "x-openai-authorization-error";
static MODEL_PICKER_RUNTIME: OnceLock<Runtime> = OnceLock::new();
static MODEL_PICKER_CLIENT: OnceLock<RwLock<ModelPickerClientEntry>> = OnceLock::new();
#[cfg(test)]
static MODEL_PICKER_CLIENT_BUILD_COUNT: AtomicUsize = AtomicUsize::new(0);
const MODEL_PICKER_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const MODEL_PICKER_TOTAL_TIMEOUT: Duration = Duration::from_secs(120);
const MODEL_PICKER_RESPONSE_READ_TIMEOUT: Duration = Duration::from_secs(30);
const ACCOUNT_MODELS_PROBE_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Clone, Debug, PartialEq, Eq)]
struct ModelPickerClientConfig {
    user_agent: String,
    proxy_url: Option<String>,
}

struct ModelPickerClientEntry {
    config: ModelPickerClientConfig,
    client: Client,
}

fn append_client_version_query(url: &str) -> String {
    if url.contains("client_version=") {
        return url.to_string();
    }
    let separator = if url.contains('?') { '&' } else { '?' };
    format!(
        "{url}{separator}client_version={}",
        crate::gateway::current_codex_user_agent_version()
    )
}

/// 函数 `build_models_request_headers`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - bearer: 参数 bearer
/// - user_agent: 参数 user_agent
/// - originator: 参数 originator
/// - residency_requirement: 参数 residency_requirement
/// - include_account_header: 参数 include_account_header
/// - account_header_value: 参数 account_header_value
///
/// # 返回
/// 返回函数执行结果
fn build_models_request_headers(
    bearer: &str,
    user_agent: &str,
    originator: &str,
    residency_requirement: Option<&str>,
    include_account_header: bool,
    account_header_value: Option<&str>,
) -> Vec<(String, String)> {
    let mut headers = Vec::with_capacity(6);
    headers.push(("Accept".to_string(), "application/json".to_string()));
    headers.push(("User-Agent".to_string(), user_agent.to_string()));
    headers.push(("originator".to_string(), originator.to_string()));
    if let Some(residency_requirement) = residency_requirement
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        headers.push((
            crate::gateway::runtime_config::RESIDENCY_HEADER_NAME.to_string(),
            residency_requirement.to_string(),
        ));
    }
    headers.push((
        "Authorization".to_string(),
        if bearer.trim().starts_with("AgentAssertion ") {
            bearer.trim().to_string()
        } else {
            format!("Bearer {}", bearer.trim())
        },
    ));
    if include_account_header {
        if let Some(account_id) = account_header_value
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            headers.push(("ChatGPT-Account-ID".to_string(), account_id.to_string()));
        }
    }
    headers
}

fn build_models_request_url(upstream_base: &str, path: &str) -> String {
    let (url, _url_alt) = super::super::compute_upstream_url(upstream_base, path);
    append_client_version_query(&url)
}

/// 函数 `extract_response_header`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - headers: 参数 headers
/// - name: 参数 name
///
/// # 返回
/// 返回函数执行结果
fn extract_response_header(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

/// 函数 `summarize_models_error_response`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - status: 参数 status
/// - headers: 参数 headers
/// - body: 参数 body
/// - force_html_error: 参数 force_html_error
///
/// # 返回
/// 返回函数执行结果
fn summarize_models_error_response(
    status: StatusCode,
    headers: &HeaderMap,
    body: &str,
    force_html_error: bool,
) -> String {
    let request_id = extract_response_header(headers, REQUEST_ID_HEADER)
        .or_else(|| extract_response_header(headers, OAI_REQUEST_ID_HEADER));
    let cf_ray = extract_response_header(headers, CF_RAY_HEADER);
    let auth_error = extract_response_header(headers, AUTH_ERROR_HEADER);
    let identity_error_code = crate::gateway::extract_identity_error_code_from_headers(headers);
    let body_hint = if force_html_error {
        super::super::http_bridge::summarize_upstream_error_hint_from_body(403, body.as_bytes())
    } else {
        super::super::http_bridge::summarize_upstream_error_hint_from_body(
            status.as_u16(),
            body.as_bytes(),
        )
    }
    .or_else(|| {
        let trimmed = body.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    })
    .unwrap_or_else(|| "unknown error".to_string());

    let mut details = Vec::new();
    if let Some(request_id) = request_id {
        details.push(format!("request id: {request_id}"));
    }
    if let Some(cf_ray) = cf_ray {
        details.push(format!("cf-ray: {cf_ray}"));
    }
    if let Some(auth_error) = auth_error {
        details.push(format!("auth error: {auth_error}"));
    }
    if let Some(identity_error_code) = identity_error_code {
        details.push(format!("identity_error_code: {identity_error_code}"));
    }

    if details.is_empty() {
        format!("models upstream failed: status={} body={body_hint}", status)
    } else {
        format!(
            "models upstream failed: status={} body={body_hint}, {}",
            status,
            details.join(", ")
        )
    }
}

/// 函数 `model_picker_runtime`
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
fn model_picker_runtime() -> &'static Runtime {
    MODEL_PICKER_RUNTIME.get_or_init(|| {
        Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .thread_name("model-picker-http")
            .build()
            .unwrap_or_else(|err| panic!("build model picker runtime failed: {err}"))
    })
}

/// 函数 `run_model_picker_future`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - future: 参数 future
///
/// # 返回
/// 返回函数执行结果
fn run_model_picker_future<F>(future: F) -> F::Output
where
    F: Future,
{
    model_picker_runtime().block_on(future)
}

fn current_model_picker_client_config() -> ModelPickerClientConfig {
    ModelPickerClientConfig {
        user_agent: crate::gateway::current_codex_user_agent(),
        proxy_url: crate::gateway::current_upstream_proxy_url(),
    }
}

fn model_picker_client_lock() -> &'static RwLock<ModelPickerClientEntry> {
    MODEL_PICKER_CLIENT.get_or_init(|| {
        let config = current_model_picker_client_config();
        RwLock::new(ModelPickerClientEntry {
            client: build_model_picker_client_for_config(&config),
            config,
        })
    })
}

fn model_picker_client() -> Client {
    let config = current_model_picker_client_config();
    {
        let cached =
            crate::lock_utils::read_recover(model_picker_client_lock(), "model_picker_client");
        if cached.config == config {
            return cached.client.clone();
        }
    }

    let client = build_model_picker_client_for_config(&config);
    let mut cached =
        crate::lock_utils::write_recover(model_picker_client_lock(), "model_picker_client");
    if cached.config == config {
        return cached.client.clone();
    }
    cached.config = config;
    cached.client = client.clone();
    client
}

/// 函数 `build_model_picker_client`
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
#[cfg(test)]
fn build_model_picker_client() -> Client {
    let config = current_model_picker_client_config();
    build_model_picker_client_for_config(&config)
}

fn refresh_model_picker_client() -> Client {
    let config = current_model_picker_client_config();
    let client = build_model_picker_client_for_config(&config);
    let mut cached =
        crate::lock_utils::write_recover(model_picker_client_lock(), "model_picker_client");
    cached.config = config;
    cached.client = client.clone();
    client
}

fn build_model_picker_client_for_config(config: &ModelPickerClientConfig) -> Client {
    #[cfg(test)]
    MODEL_PICKER_CLIENT_BUILD_COUNT.fetch_add(1, Ordering::SeqCst);

    let builder = Client::builder()
        .connect_timeout(MODEL_PICKER_CONNECT_TIMEOUT)
        .timeout(MODEL_PICKER_TOTAL_TIMEOUT)
        .pool_max_idle_per_host(8)
        .pool_idle_timeout(Some(Duration::from_secs(60)))
        .user_agent(config.user_agent.as_str());
    let builder = crate::gateway::apply_async_upstream_proxy(
        builder,
        config.proxy_url.as_deref(),
        "model_picker_proxy_invalid",
    );
    builder.build().unwrap_or_else(|err| {
        log::warn!("event=model_picker_client_build_failed err={}", err);
        Client::new()
    })
}

#[cfg(test)]
fn reset_model_picker_client_build_count_for_test() {
    MODEL_PICKER_CLIENT_BUILD_COUNT.store(0, Ordering::SeqCst);
}

#[cfg(test)]
fn model_picker_client_build_count_for_test() -> usize {
    MODEL_PICKER_CLIENT_BUILD_COUNT.load(Ordering::SeqCst)
}

/// 函数 `read_response_text`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - resp: 参数 resp
/// - timeout: 参数 timeout
///
/// # 返回
/// 返回函数执行结果
async fn read_response_text(resp: reqwest::Response, timeout: Duration) -> Result<String, String> {
    match tokio::time::timeout(timeout, resp.text()).await {
        Ok(Ok(body)) => Ok(body),
        Ok(Err(err)) => Err(err.to_string()),
        Err(_) => Err(format!(
            "response read timed out after {}ms",
            timeout.as_millis()
        )),
    }
}

/// 函数 `read_response_bytes`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - resp: 参数 resp
/// - timeout: 参数 timeout
///
/// # 返回
/// 返回函数执行结果
async fn read_response_bytes(
    resp: reqwest::Response,
    timeout: Duration,
) -> Result<Vec<u8>, String> {
    match tokio::time::timeout(timeout, resp.bytes()).await {
        Ok(Ok(body)) => Ok(body.to_vec()),
        Ok(Err(err)) => Err(err.to_string()),
        Err(_) => Err(format!(
            "response read timed out after {}ms",
            timeout.as_millis()
        )),
    }
}

/// 函数 `send_models_request`
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
pub(super) fn send_models_request(
    storage: &Storage,
    method: &Method,
    upstream_base: &str,
    path: &str,
    account: &Account,
    token: &mut Token,
) -> Result<Vec<u8>, String> {
    run_model_picker_future(send_models_request_async(
        storage,
        method,
        upstream_base,
        path,
        account,
        token,
        None,
    ))
}

pub(super) fn send_models_probe_request(
    storage: &Storage,
    method: &Method,
    upstream_base: &str,
    path: &str,
    account: &Account,
    token: &mut Token,
) -> Result<Vec<u8>, String> {
    run_model_picker_future(send_models_request_async(
        storage,
        method,
        upstream_base,
        path,
        account,
        token,
        Some(ACCOUNT_MODELS_PROBE_TIMEOUT),
    ))
}

/// 函数 `send_models_request_async`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - storage: 参数 storage
/// - method: 参数 method
/// - upstream_base: 参数 upstream_base
/// - path: 参数 path
/// - account: 参数 account
/// - token: 参数 token
///
/// # 返回
/// 返回函数执行结果
async fn send_models_request_async(
    storage: &Storage,
    method: &Method,
    upstream_base: &str,
    path: &str,
    account: &Account,
    token: &mut Token,
    request_timeout: Option<Duration>,
) -> Result<Vec<u8>, String> {
    let url = build_models_request_url(upstream_base, path);
    // 中文注释：OpenAI 基线要求 api_key_access_token，
    // 不这样区分会导致模型列表请求在 OpenAI 上游稳定 401。
    let bearer = if super::super::is_openai_api_base(upstream_base) {
        super::super::resolve_openai_bearer_token(storage, account, token)?
    } else if let Some(assertion) =
        crate::account_agent_identity::build_agent_identity_authorization(storage, &account.id)?
    {
        assertion
    } else {
        token.access_token.clone()
    };
    let account_header_value = account
        .chatgpt_account_id
        .as_deref()
        .or_else(|| account.workspace_id.as_deref())
        .map(str::to_string);
    let include_account_header = !super::super::is_openai_api_base(upstream_base);
    let client = model_picker_client();
    let build_request = |http: &Client| {
        let mut builder = http.request(method.clone(), &url);
        for (name, value) in build_models_request_headers(
            bearer.as_str(),
            crate::gateway::current_codex_user_agent().as_str(),
            crate::gateway::current_wire_originator().as_str(),
            crate::gateway::current_residency_requirement().as_deref(),
            include_account_header,
            account_header_value.as_deref(),
        ) {
            builder = builder.header(name, value);
        }
        if let Some(timeout) = request_timeout {
            builder = builder.timeout(timeout);
        }
        builder
    };

    let response = match build_request(&client).send().await {
        Ok(resp) => resp,
        Err(first_err) => {
            let fresh = refresh_model_picker_client();
            match build_request(&fresh).send().await {
                Ok(resp) => resp,
                Err(second_err) => {
                    return Err(format!(
                        "models upstream request failed: {}; retry_after_fresh_client: {}",
                        first_err, second_err
                    ));
                }
            }
        }
    };
    if !response.status().is_success() {
        let status = response.status();
        let headers = response.headers().clone();
        let body = read_response_text(response, MODEL_PICKER_RESPONSE_READ_TIMEOUT).await?;
        return Err(summarize_models_error_response(
            status, &headers, &body, false,
        ));
    }
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if super::super::is_html_content_type(content_type) {
        let status = response.status();
        let headers = response.headers().clone();
        let body = read_response_text(response, MODEL_PICKER_RESPONSE_READ_TIMEOUT).await?;
        return Err(summarize_models_error_response(
            status, &headers, &body, true,
        ));
    }

    read_response_bytes(response, MODEL_PICKER_RESPONSE_READ_TIMEOUT).await
}

#[cfg(test)]
#[path = "request_tests.rs"]
mod tests;
