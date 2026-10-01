use super::{
    max_text_input_chars, parse_request_metadata, parse_request_metadata_from_value,
    validate_text_input_limit_for_path, validate_text_input_limit_for_value,
    DEFAULT_MAX_TEXT_INPUT_CHARS,
};

#[test]
fn request_metadata_from_value_matches_byte_parser() {
    let value = serde_json::json!({
        "model": "gpt-5.4",
        "reasoning": { "effort": "high" },
        "service_tier": "ultrafast",
        "stream": true,
        "prompt_cache_key": "thread-1",
        "previous_response_id": "resp-1",
        "input": "hello"
    });
    let body = serde_json::to_vec(&value).expect("serialize body");

    let from_body = parse_request_metadata(&body);
    let from_value = parse_request_metadata_from_value(&value);

    assert_eq!(from_value.model, from_body.model);
    assert_eq!(from_value.reasoning_effort, from_body.reasoning_effort);
    assert_eq!(from_value.service_tier, from_body.service_tier);
    assert_eq!(from_value.is_stream, from_body.is_stream);
    assert_eq!(from_value.stream_specified, from_body.stream_specified);
    assert_eq!(
        from_value.has_prompt_cache_key,
        from_body.has_prompt_cache_key
    );
    assert_eq!(from_value.prompt_cache_key, from_body.prompt_cache_key);
    assert_eq!(
        from_value.has_previous_response_id,
        from_body.has_previous_response_id
    );
    assert_eq!(from_value.request_shape, from_body.request_shape);
}

#[test]
fn responses_text_limit_allows_small_payloads() {
    let body = serde_json::json!({
        "instructions": "system",
        "input": [
            {
                "role": "user",
                "content": [
                    { "type": "input_text", "text": "hello" },
                    { "type": "input_text", "text": "world" }
                ]
            }
        ]
    });
    let body = serde_json::to_vec(&body).expect("serialize body");

    let result = validate_text_input_limit_for_path("/v1/responses", &body);

    assert!(result.is_ok());
}

#[test]
fn responses_text_limit_rejects_oversized_payloads() {
    let body = serde_json::json!({
        "input": "x".repeat(DEFAULT_MAX_TEXT_INPUT_CHARS + 1),
    });
    let body = serde_json::to_vec(&body).expect("serialize body");

    let err = validate_text_input_limit_for_path("/v1/responses", &body)
        .expect_err("oversized body should be rejected");

    assert_eq!(err.max_chars, DEFAULT_MAX_TEXT_INPUT_CHARS);
    assert_eq!(err.actual_chars, DEFAULT_MAX_TEXT_INPUT_CHARS + 1);
    assert!(err
        .message()
        .contains("Input exceeds the maximum length of 1048576 characters."));
}

#[test]
fn responses_text_limit_can_validate_preparsed_value() {
    let value = serde_json::json!({
        "input": "x".repeat(DEFAULT_MAX_TEXT_INPUT_CHARS + 1),
    });

    let err = validate_text_input_limit_for_value("/v1/responses", &value)
        .expect_err("oversized body should be rejected");

    assert_eq!(err.max_chars, DEFAULT_MAX_TEXT_INPUT_CHARS);
    assert_eq!(err.actual_chars, DEFAULT_MAX_TEXT_INPUT_CHARS + 1);
}

#[test]
fn chat_completions_text_limit_counts_message_content_and_instructions() {
    let first = "x".repeat(DEFAULT_MAX_TEXT_INPUT_CHARS / 2);
    let second = "y".repeat(DEFAULT_MAX_TEXT_INPUT_CHARS / 2 + 1);
    let body = serde_json::json!({
        "instructions": first,
        "messages": [
            {
                "role": "user",
                "content": [
                    { "type": "text", "text": second }
                ]
            }
        ]
    });
    let body = serde_json::to_vec(&body).expect("serialize body");

    let err = validate_text_input_limit_for_path("/v1/chat/completions", &body)
        .expect_err("combined text length should be rejected");

    assert_eq!(err.actual_chars, DEFAULT_MAX_TEXT_INPUT_CHARS + 1);
}

#[test]
fn non_inference_path_skips_text_limit_validation() {
    let body = serde_json::json!({
        "input": "x".repeat(DEFAULT_MAX_TEXT_INPUT_CHARS + 100),
    });
    let body = serde_json::to_vec(&body).expect("serialize body");

    let result = validate_text_input_limit_for_path("/v1/models", &body);

    assert!(result.is_ok());
}

#[test]
fn legacy_completions_path_no_longer_participates_in_text_limit_validation() {
    let body = serde_json::json!({
        "prompt": "x".repeat(DEFAULT_MAX_TEXT_INPUT_CHARS + 100),
    });
    let body = serde_json::to_vec(&body).expect("serialize body");

    let result = validate_text_input_limit_for_path("/v1/completions", &body);

    assert!(result.is_ok());
}

/// 函数 `max_text_input_chars_honours_env_override`
///
/// 作者: gaohongshun
///
/// 时间: 2026-10-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn max_text_input_chars_honours_env_override() {
    let _guard = crate::test_env_guard();
    let name = "CODEXMANAGER_MAX_TEXT_INPUT_CHARS";
    std::env::remove_var(name);
    assert_eq!(max_text_input_chars(), DEFAULT_MAX_TEXT_INPUT_CHARS);
    std::env::set_var(name, "2097152");
    assert_eq!(max_text_input_chars(), 2_097_152);
    assert!(validate_text_input_limit_for_value(
        "/v1/chat/completions",
        &serde_json::json!({ "messages": [ { "role": "user", "content": "x".repeat(DEFAULT_MAX_TEXT_INPUT_CHARS + 1) } ] })
    )
    .is_ok(), "a payload above the default but below the override must pass");
    std::env::set_var(name, "0");
    assert_eq!(max_text_input_chars(), DEFAULT_MAX_TEXT_INPUT_CHARS);
    std::env::set_var(name, "not-a-number");
    assert_eq!(max_text_input_chars(), DEFAULT_MAX_TEXT_INPUT_CHARS);
    std::env::remove_var(name);
}
