use super::{
    extract_error_hint_from_body, limit_upstream_error_hint, output_text_limit_bytes,
    reload_from_env, summarize_upstream_error_hint, UpstreamResponseBridgeResult,
    UPSTREAM_ERROR_HINT_LIMIT_BYTES,
};

struct EnvGuard {
    key: &'static str,
    original: Option<std::ffi::OsString>,
}

impl EnvGuard {
    fn clear(key: &'static str) -> Self {
        let original = std::env::var_os(key);
        std::env::remove_var(key);
        Self { key, original }
    }

    fn set(key: &'static str, value: &str) -> Self {
        let original = std::env::var_os(key);
        std::env::set_var(key, value);
        Self { key, original }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        if let Some(value) = &self.original {
            std::env::set_var(self.key, value);
        } else {
            std::env::remove_var(self.key);
        }
    }
}

/// 函数 `summarize_upstream_error_hint_recognizes_challenge_html`
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
#[test]
fn summarize_upstream_error_hint_recognizes_challenge_html() {
    assert_eq!(
        summarize_upstream_error_hint(403, "<html><title>Just a moment...</title>"),
        "Cloudflare 安全验证页（title=Just a moment...）"
    );
}

/// 函数 `summarize_upstream_error_hint_recognizes_generic_html`
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
#[test]
fn summarize_upstream_error_hint_recognizes_generic_html() {
    assert_eq!(
        summarize_upstream_error_hint(502, "<!doctype html><html><body>error</body></html>"),
        "<!doctype html><html><body>error</body></html>"
    );
}

/// 函数 `summarize_upstream_error_hint_recognizes_unsupported_model`
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
#[test]
fn summarize_upstream_error_hint_recognizes_unsupported_model() {
    assert_eq!(
        summarize_upstream_error_hint(
            400,
            "code=model_not_found type=invalid_request_error The model 'gpt-5.4' does not exist"
        ),
        "code=model_not_found type=invalid_request_error The model 'gpt-5.4' does not exist"
    );
    assert_eq!(
        summarize_upstream_error_hint(400, "unsupported model"),
        "unsupported model"
    );
}

/// 函数 `extract_error_hint_from_body_summarizes_html_body`
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
#[test]
fn extract_error_hint_from_body_summarizes_html_body() {
    assert_eq!(
        extract_error_hint_from_body(403, b"<html><body>Cloudflare</body></html>").as_deref(),
        Some("Cloudflare 安全验证页")
    );
}

/// 函数 `extract_error_hint_from_body_prefers_json_message`
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
#[test]
fn extract_error_hint_from_body_prefers_json_message() {
    let body = br#"{"error":{"message":"forbidden","type":"permission_error"}}"#;
    assert_eq!(
        extract_error_hint_from_body(403, body).as_deref(),
        Some("type=permission_error forbidden")
    );
}

/// 函数 `extract_error_hint_from_body_summarizes_unsupported_model_json`
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
#[test]
fn extract_error_hint_from_body_summarizes_unsupported_model_json() {
    let body = br#"{"error":{"message":"The model 'gpt-5.4' does not exist","type":"invalid_request_error","code":"model_not_found"}}"#;
    assert_eq!(
        extract_error_hint_from_body(400, body).as_deref(),
        Some("code=model_not_found type=invalid_request_error The model 'gpt-5.4' does not exist")
    );
}

/// 函数 `extract_error_hint_from_body_summarizes_unsupported_model_detail_json`
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
#[test]
fn extract_error_hint_from_body_summarizes_unsupported_model_detail_json() {
    let body = br#"{"detail":"The 'gpt-5.4' model is not supported when using Codex with a ChatGPT account."}"#;
    assert_eq!(
        extract_error_hint_from_body(400, body).as_deref(),
        Some("The 'gpt-5.4' model is not supported when using Codex with a ChatGPT account.")
    );
}

/// 函数 `limit_upstream_error_hint_truncates_large_body`
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
#[test]
fn limit_upstream_error_hint_truncates_large_body() {
    let raw = "x".repeat(UPSTREAM_ERROR_HINT_LIMIT_BYTES + 32);
    let text = limit_upstream_error_hint(&raw);
    assert!(text.ends_with("...[truncated]"));
    assert!(text.len() > UPSTREAM_ERROR_HINT_LIMIT_BYTES);
}

/// 函数 `bridge_error_message_reports_stream_incomplete_in_chinese`
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
#[test]
fn bridge_error_message_reports_stream_incomplete_in_chinese() {
    let bridge = UpstreamResponseBridgeResult {
        stream_terminal_seen: false,
        ..UpstreamResponseBridgeResult::default()
    };
    assert_eq!(
        bridge.error_message(true).as_deref(),
        Some("连接中断（可能是网络波动或客户端主动取消）")
    );
}

#[test]
fn completed_stream_with_trailing_client_disconnect_remains_client_cancelled() {
    let bridge = UpstreamResponseBridgeResult {
        stream_terminal_seen: true,
        stream_terminal_delivered: true,
        delivery_error: Some("broken pipe: downstream HTTP body closed".to_string()),
        ..UpstreamResponseBridgeResult::default()
    };
    assert!(!bridge.is_ok(true));
}

#[test]
fn client_disconnect_before_terminal_event_remains_failed() {
    let bridge = UpstreamResponseBridgeResult {
        stream_terminal_seen: true,
        delivery_error: Some("broken pipe: downstream HTTP body closed".to_string()),
        ..UpstreamResponseBridgeResult::default()
    };
    assert!(!bridge.is_ok(true));
}

#[test]
fn output_text_limit_defaults_to_unbounded_when_env_missing() {
    let _guard = crate::test_env_guard();
    let _env_guard = EnvGuard::clear("CODEXMANAGER_HTTP_BRIDGE_OUTPUT_TEXT_LIMIT_BYTES");

    reload_from_env();

    assert_eq!(output_text_limit_bytes(), 0);
}

#[test]
fn output_text_limit_invalid_value_falls_back_to_unbounded() {
    let _guard = crate::test_env_guard();
    let _env_guard = EnvGuard::set("CODEXMANAGER_HTTP_BRIDGE_OUTPUT_TEXT_LIMIT_BYTES", "abc");

    reload_from_env();

    assert_eq!(output_text_limit_bytes(), 0);
}
