use super::*;

#[test]
fn idempotency_key_is_captured_for_safe_post_retry() {
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        "idempotency-key",
        "request-stable-123".parse().expect("idempotency key"),
    );

    let snapshot = IncomingHeaderSnapshot::from_http_headers(&headers);
    assert_eq!(snapshot.idempotency_key(), Some("request-stable-123"));
}

/// 函数 `strict_bearer_parsing_matches_auth_extraction_behavior`
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
fn strict_bearer_parsing_matches_auth_extraction_behavior() {
    assert_eq!(strict_bearer_token("Bearer abc"), Some("abc".to_string()));
    assert_eq!(strict_bearer_token("bearer abc"), None);
    assert_eq!(strict_bearer_token("Bearer   "), None);
}

/// 函数 `case_insensitive_bearer_parsing_matches_sticky_derivation_behavior`
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
fn case_insensitive_bearer_parsing_matches_sticky_derivation_behavior() {
    assert_eq!(
        case_insensitive_bearer_token("Bearer abc"),
        Some("abc".to_string())
    );
    assert_eq!(
        case_insensitive_bearer_token("bearer abc"),
        Some("abc".to_string())
    );
    assert_eq!(case_insensitive_bearer_token("basic abc"), None);
    assert_eq!(case_insensitive_bearer_token("bearer   "), None);
}

/// 函数 `goog_api_key_header_is_accepted_as_platform_key`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-05
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn goog_api_key_header_is_accepted_as_platform_key() {
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        "x-goog-api-key",
        axum::http::HeaderValue::from_static("platform-key-from-gemini"),
    );

    let snapshot = IncomingHeaderSnapshot::from_http_headers(&headers);
    assert_eq!(snapshot.platform_key(), Some("platform-key-from-gemini"));
    assert!(snapshot.has_x_api_key());
    assert_eq!(
        snapshot.sticky_key_material(),
        Some("platform-key-from-gemini")
    );
}

#[test]
fn session_id_is_preferred_for_sticky_key_material() {
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        "session_id",
        axum::http::HeaderValue::from_static("session-thread-1"),
    );
    headers.insert(
        "x-api-key",
        axum::http::HeaderValue::from_static("platform-key-1"),
    );

    let snapshot = IncomingHeaderSnapshot::from_http_headers(&headers);

    assert_eq!(snapshot.platform_key(), Some("platform-key-1"));
    assert_eq!(snapshot.session_id(), Some("session-thread-1"));
    assert_eq!(snapshot.sticky_key_material(), Some("session-thread-1"));
}

/// 函数 `codex_headers_are_captured_from_http_headers`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-11
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn codex_headers_are_captured_from_http_headers() {
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        "User-Agent",
        axum::http::HeaderValue::from_static("codex_cli_rs/0.999.0"),
    );
    headers.insert(
        "originator",
        axum::http::HeaderValue::from_static("codex_cli_rs"),
    );
    headers.insert(
        "x-session-affinity",
        axum::http::HeaderValue::from_static("affinity_123"),
    );
    headers.insert(
        "x-codex-parent-thread-id",
        axum::http::HeaderValue::from_static("thread_parent_123"),
    );
    headers.insert(
        "x-codex-installation-id",
        axum::http::HeaderValue::from_static("install_123"),
    );
    headers.insert(
        "x-codex-window-id",
        axum::http::HeaderValue::from_static("thread_child_123:7"),
    );
    headers.insert(
        "x-codex-other-limit-name",
        axum::http::HeaderValue::from_static("promo_header"),
    );
    headers.insert(
        "x-responsesapi-include-timing-metrics",
        axum::http::HeaderValue::from_static("true"),
    );
    headers.insert(
        "x-codex-inference-call-id",
        axum::http::HeaderValue::from_static("call_123"),
    );
    headers.insert(
        "x-oai-attestation",
        axum::http::HeaderValue::from_static("attest_123"),
    );

    let snapshot = IncomingHeaderSnapshot::from_http_headers(&headers);
    assert_eq!(snapshot.user_agent(), Some("codex_cli_rs/0.999.0"));
    assert_eq!(snapshot.originator(), Some("codex_cli_rs"));
    assert_eq!(snapshot.session_affinity(), Some("affinity_123"));
    assert_eq!(snapshot.parent_thread_id(), Some("thread_parent_123"));
    assert_eq!(snapshot.codex_installation_id(), Some("install_123"));
    assert_eq!(snapshot.window_id(), Some("thread_child_123:7"));
    assert_eq!(snapshot.responsesapi_include_timing_metrics(), Some("true"));
    assert_eq!(snapshot.codex_inference_call_id(), Some("call_123"));
    assert_eq!(snapshot.oai_attestation(), Some("attest_123"));
    assert!(snapshot.passthrough_codex_headers().is_empty());
}

#[test]
fn turn_metadata_session_id_is_used_when_session_header_is_missing() {
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        "x-codex-turn-metadata",
        axum::http::HeaderValue::from_static(
            r#"{"session_id":"019e779c-f433-7040-ace3-c93eab04ae31","thread_id":"019e779c-f433-7040-ace3-c93eab04ae31","turn_id":"019e779c-f43c-7520-9dbd-78b84462e524","request_kind":"turn"}"#,
        ),
    );

    let snapshot = IncomingHeaderSnapshot::from_http_headers(&headers);

    assert_eq!(
        snapshot.session_id(),
        Some("019e779c-f433-7040-ace3-c93eab04ae31")
    );
    assert_eq!(
        snapshot.sticky_key_material(),
        Some("019e779c-f433-7040-ace3-c93eab04ae31")
    );
    assert!(snapshot.turn_metadata().is_some());
}

#[test]
fn explicit_session_header_wins_over_turn_metadata_session_id() {
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        "session_id",
        axum::http::HeaderValue::from_static("session-from-header"),
    );
    headers.insert(
        "x-codex-turn-metadata",
        axum::http::HeaderValue::from_static(r#"{"session_id":"session-from-metadata"}"#),
    );

    let snapshot = IncomingHeaderSnapshot::from_http_headers(&headers);

    assert_eq!(snapshot.session_id(), Some("session-from-header"));
}

#[test]
fn invalid_turn_metadata_does_not_create_session_id() {
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        "x-codex-turn-metadata",
        axum::http::HeaderValue::from_static(r#"{"session_id":"unsafe session"}"#),
    );

    let snapshot = IncomingHeaderSnapshot::from_http_headers(&headers);

    assert_eq!(snapshot.session_id(), None);
    assert_eq!(session_id_from_turn_metadata("not-json"), None);
    assert_eq!(session_id_from_turn_metadata(r#"{"session_id":42}"#), None);
    assert_eq!(
        session_id_from_turn_metadata(r#"{"session_id":"unsafe\nsession"}"#),
        None
    );
}
