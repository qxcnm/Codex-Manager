use bytes::Bytes;
use codexmanager_core::storage::ConversationBinding;
use reqwest::Method;
use tiny_http::Request;

use super::conversation_binding::RouteConversationSource;

mod auth;
mod io;
mod request;

pub(super) struct LocalValidationResult {
    pub(super) trace_id: String,
    pub(super) incoming_headers: super::IncomingHeaderSnapshot,
    pub(super) storage: crate::storage_helpers::StorageHandle,
    pub(super) original_path: String,
    pub(super) passthrough_path: String,
    pub(super) path: String,
    pub(super) passthrough_body: Bytes,
    pub(super) body: Bytes,
    pub(super) is_stream: bool,
    pub(super) has_prompt_cache_key: bool,
    pub(super) request_shape: Option<String>,
    pub(super) protocol_type: String,
    pub(super) rotation_strategy: String,
    pub(super) aggregate_api_id: Option<String>,
    pub(super) account_plan_filter: Option<String>,
    pub(super) allowed_platforms: Vec<String>,
    pub(super) concurrency_limit: Option<i64>,
    pub(super) response_adapter: super::ResponseAdapter,
    pub(super) gemini_stream_output_mode: Option<super::GeminiStreamOutputMode>,
    pub(super) tool_name_restore_map: super::ToolNameRestoreMap,
    pub(super) request_method: String,
    pub(super) key_id: String,
    pub(super) platform_key_hash: String,
    pub(super) local_conversation_id: Option<String>,
    pub(super) route_conversation_id: Option<String>,
    pub(super) route_conversation_source: Option<RouteConversationSource>,
    pub(super) conversation_binding: Option<ConversationBinding>,
    pub(super) client_model_for_log: Option<String>,
    pub(super) model_for_log: Option<String>,
    pub(super) model_source_for_log: Option<String>,
    pub(super) client_reasoning_for_log: Option<String>,
    pub(super) reasoning_for_log: Option<String>,
    pub(super) reasoning_source_for_log: Option<String>,
    pub(super) service_tier_for_log: Option<String>,
    pub(super) effective_service_tier_for_log: Option<String>,
    pub(super) service_tier_source_for_log: Option<String>,
    pub(super) gateway_mode_for_log: Option<String>,
    pub(super) method: Method,
}

pub(super) struct LocalValidationError {
    pub(super) status_code: u16,
    pub(super) message: String,
}

impl LocalValidationError {
    /// 函数 `new`
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
    pub(super) fn new(status_code: u16, message: impl Into<String>) -> Self {
        Self {
            status_code,
            message: message.into(),
        }
    }
}

/// 函数 `prepare_local_request`
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
pub(super) fn prepare_local_request(
    request: &mut Request,
    trace_id: String,
    debug: bool,
) -> Result<LocalValidationResult, LocalValidationError> {
    let body = io::read_request_body(request)?;
    let incoming_headers = super::IncomingHeaderSnapshot::from_request(request);
    let platform_key = io::extract_platform_key_or_error(request, &incoming_headers, debug)?;

    let storage = auth::open_storage_or_error()?;
    let api_key = auth::load_active_api_key(&storage, &platform_key, request.url(), debug)?;

    request::build_local_validation_result(
        request,
        trace_id,
        incoming_headers,
        storage,
        body,
        api_key,
    )
}
