use gpttools_core::storage::Storage;
use reqwest::Method;
use tiny_http::Request;

use crate::storage_helpers::{hash_platform_key, open_storage};

pub(super) struct LocalValidationResult {
    pub(super) storage: Storage,
    pub(super) path: String,
    pub(super) body: Vec<u8>,
    pub(super) request_method: String,
    pub(super) key_id: String,
    pub(super) model_for_log: Option<String>,
    pub(super) reasoning_for_log: Option<String>,
    pub(super) method: Method,
}

pub(super) struct LocalValidationError {
    pub(super) status_code: u16,
    pub(super) message: String,
}

impl LocalValidationError {
    fn new(status_code: u16, message: impl Into<String>) -> Self {
        Self {
            status_code,
            message: message.into(),
        }
    }
}

pub(super) fn prepare_local_request(
    request: &mut Request,
    debug: bool,
) -> Result<LocalValidationResult, LocalValidationError> {
    // 中文注释：先读完整请求体再返回鉴权错误，避免客户端出现“写入未完成就断流”的假异常。
    let mut body = Vec::new();
    let _ = request.as_reader().read_to_end(&mut body);

    let Some(platform_key) = super::extract_platform_key(request) else {
        if debug {
            let remote = request
                .remote_addr()
                .map(|a| a.to_string())
                .unwrap_or_else(|| "<none>".to_string());
            let auth_scheme = request
                .headers()
                .iter()
                .find(|h| h.field.equiv("Authorization"))
                .and_then(|h| h.value.as_str().split_whitespace().next())
                .unwrap_or("<none>");
            let header_names = request
                .headers()
                .iter()
                .map(|h| h.field.as_str().as_str())
                .collect::<Vec<_>>()
                .join(",");
            eprintln!(
                "gateway auth missing: url={}, remote={}, has_auth={}, auth_scheme={}, has_x_api_key={}, headers=[{}]",
                request.url(),
                remote,
                request
                    .headers()
                    .iter()
                    .any(|h| h.field.equiv("Authorization")),
                auth_scheme,
                request.headers().iter().any(|h| h.field.equiv("x-api-key")),
                header_names,
            );
        }
        return Err(LocalValidationError::new(401, "missing api key"));
    };

    let Some(storage) = open_storage() else {
        return Err(LocalValidationError::new(500, "storage unavailable"));
    };

    let key_hash = hash_platform_key(&platform_key);
    let api_key = storage
        .find_api_key_by_hash(&key_hash)
        .map_err(|err| LocalValidationError::new(500, format!("storage read failed: {err}")))?;
    let Some(api_key) = api_key else {
        if debug {
            eprintln!(
                "gateway auth invalid: url={}, key_hash_prefix={}",
                request.url(),
                &key_hash[..8]
            );
        }
        return Err(LocalValidationError::new(403, "invalid api key"));
    };
    if api_key.status != "active" {
        if debug {
            eprintln!("gateway auth disabled: url={}, key_id={}", request.url(), api_key.id);
        }
        return Err(LocalValidationError::new(403, "api key disabled"));
    }
    // 按当前策略取消每次请求都更新 api_keys.last_used_at，减少并发写入冲突。

    let path = super::normalize_models_path(request.url());
    body = super::apply_request_overrides(
        &path,
        body,
        api_key.model_slug.as_deref(),
        api_key.reasoning_effort.as_deref(),
    );
    let request_method = request.method().as_str().to_string();
    let key_id = api_key.id.clone();
    let model_for_log = super::extract_request_model(&body).or(api_key.model_slug.clone());
    let reasoning_for_log =
        super::extract_request_reasoning_effort(&body).or(api_key.reasoning_effort.clone());
    let method = Method::from_bytes(request_method.as_bytes())
        .map_err(|_| LocalValidationError::new(405, "unsupported method"))?;

    Ok(LocalValidationResult {
        storage,
        path,
        body,
        request_method,
        key_id,
        model_for_log,
        reasoning_for_log,
        method,
    })
}
