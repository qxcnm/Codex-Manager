mod request_router;
mod types;

pub(super) use self::request_router::{
    adapt_openai_responses_to_anthropic_messages, adapt_request_for_protocol,
};
pub(super) use self::types::{
    AdaptedGatewayRequest, GeminiStreamOutputMode, ResponseAdapter, ToolNameRestoreMap,
};

pub(super) fn build_gemini_error_body(message: &str) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "error": { "code": 500, "message": message, "status": "INTERNAL" }
    }))
    .unwrap_or_else(|_| {
        b"{\"error\":{\"code\":500,\"message\":\"unknown error\",\"status\":\"INTERNAL\"}}".to_vec()
    })
}
