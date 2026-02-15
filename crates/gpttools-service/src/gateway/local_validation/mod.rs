use gpttools_core::storage::Storage;
use reqwest::Method;
use tiny_http::Request;

mod auth;
mod io;
mod request;

pub(super) struct LocalValidationResult {
    pub(super) trace_id: String,
    pub(super) storage: Storage,
    pub(super) path: String,
    pub(super) body: Vec<u8>,
    pub(super) is_stream: bool,
    pub(super) protocol_type: String,
    pub(super) response_adapter: super::ResponseAdapter,
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
    pub(super) fn new(status_code: u16, message: impl Into<String>) -> Self {
        Self {
            status_code,
            message: message.into(),
        }
    }
}

pub(super) fn prepare_local_request(
    request: &mut Request,
    trace_id: String,
    debug: bool,
) -> Result<LocalValidationResult, LocalValidationError> {
    let body = io::read_request_body(request);
    let platform_key = io::extract_platform_key_or_error(request, debug)?;

    let storage = auth::open_storage_or_error()?;
    let api_key = auth::load_active_api_key(&storage, &platform_key, request.url(), debug)?;

    request::build_local_validation_result(request, trace_id, storage, body, api_key)
}
