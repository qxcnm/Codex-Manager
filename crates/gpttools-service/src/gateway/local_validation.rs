use gpttools_core::storage::Storage;
use reqwest::Method;
use tiny_http::Request;

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
    pub(super) fn new(status_code: u16, message: impl Into<String>) -> Self {
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
    let body = super::local_validation_io::read_request_body(request);
    let platform_key = super::local_validation_io::extract_platform_key_or_error(request, debug)?;

    let storage = super::local_validation_auth::open_storage_or_error()?;
    let api_key =
        super::local_validation_auth::load_active_api_key(&storage, &platform_key, request.url(), debug)?;

    super::local_validation_request::build_local_validation_result(request, storage, body, api_key)
}
