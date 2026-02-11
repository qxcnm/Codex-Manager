use gpttools_core::storage::{ApiKey, Storage};
use reqwest::Method;
use tiny_http::Request;

use super::local_validation::{LocalValidationError, LocalValidationResult};

pub(super) fn build_local_validation_result(
    request: &Request,
    storage: Storage,
    mut body: Vec<u8>,
    api_key: ApiKey,
) -> Result<LocalValidationResult, LocalValidationError> {
    // 按当前策略取消每次请求都更新 api_keys.last_used_at，减少并发写入冲突。
    let path = super::normalize_models_path(request.url());
    body = super::apply_request_overrides(
        &path,
        body,
        api_key.model_slug.as_deref(),
        api_key.reasoning_effort.as_deref(),
    );

    let request_method = request.method().as_str().to_string();
    let method = Method::from_bytes(request_method.as_bytes())
        .map_err(|_| LocalValidationError::new(405, "unsupported method"))?;

    let model_for_log = super::extract_request_model(&body).or(api_key.model_slug.clone());
    let reasoning_for_log =
        super::extract_request_reasoning_effort(&body).or(api_key.reasoning_effort.clone());

    Ok(LocalValidationResult {
        storage,
        path,
        body,
        request_method,
        key_id: api_key.id,
        model_for_log,
        reasoning_for_log,
        method,
    })
}
