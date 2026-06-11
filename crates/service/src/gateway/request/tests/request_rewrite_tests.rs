use super::{
    apply_request_overrides, apply_request_overrides_with_forced_prompt_cache_key,
    apply_request_overrides_with_prompt_cache_key, apply_request_overrides_with_service_tier,
    apply_request_overrides_with_service_tier_and_forced_prompt_cache_key_scope,
    apply_request_overrides_with_service_tier_and_prompt_cache_key_scope, compute_upstream_url,
};
use serde_json::json;
use std::time::{SystemTime, UNIX_EPOCH};

const STRICT_REQUEST_PARAM_ALLOWLIST_ENV: &str = "CODEXMANAGER_STRICT_REQUEST_PARAM_ALLOWLIST";
const CODEX_IMAGE_GENERATION_AUTO_INJECT_TOOL_ENV: &str =
    "CODEXMANAGER_CODEX_IMAGE_GENERATION_AUTO_INJECT_TOOL";
const COMPACT_MODEL_ENV: &str = "CODEXMANAGER_COMPACT_MODEL";
const COMPACT_API_PATH_ENV: &str = "CODEXMANAGER_COMPACT_API_PATH";
const CODEXMANAGER_DB_PATH_ENV: &str = "CODEXMANAGER_DB_PATH";

struct RuntimeEnvGuard {
    name: &'static str,
    previous_value: Option<String>,
}

impl RuntimeEnvGuard {
    fn set(name: &'static str, value: &str) -> Self {
        let previous_value = std::env::var(name).ok();
        std::env::set_var(name, value);
        crate::gateway::reload_runtime_config_from_env();
        Self {
            name,
            previous_value,
        }
    }

    fn clear(name: &'static str) -> Self {
        let previous_value = std::env::var(name).ok();
        std::env::remove_var(name);
        crate::gateway::reload_runtime_config_from_env();
        Self {
            name,
            previous_value,
        }
    }
}

impl Drop for RuntimeEnvGuard {
    fn drop(&mut self) {
        match self.previous_value.as_deref() {
            Some(value) => std::env::set_var(self.name, value),
            None => std::env::remove_var(self.name),
        }
        crate::gateway::reload_runtime_config_from_env();
    }
}

fn isolated_db_path(label: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time after unix epoch")
        .as_nanos();
    std::env::temp_dir()
        .join(format!("codexmanager-{label}-{nanos}"))
        .join("codexmanager.db")
        .to_string_lossy()
        .to_string()
}

fn apply_codex_compat_request_overrides(
    path: &str,
    body: Vec<u8>,
    model_slug: Option<&str>,
    reasoning_effort: Option<&str>,
    upstream_base_url: Option<&str>,
) -> Vec<u8> {
    apply_request_overrides_with_service_tier_and_prompt_cache_key_scope(
        path,
        body,
        model_slug,
        reasoning_effort,
        None,
        upstream_base_url,
        None,
        true,
    )
}

fn apply_codex_compat_request_overrides_with_prompt_cache_key(
    path: &str,
    body: Vec<u8>,
    model_slug: Option<&str>,
    reasoning_effort: Option<&str>,
    upstream_base_url: Option<&str>,
    prompt_cache_key: Option<&str>,
) -> Vec<u8> {
    apply_request_overrides_with_service_tier_and_prompt_cache_key_scope(
        path,
        body,
        model_slug,
        reasoning_effort,
        None,
        upstream_base_url,
        prompt_cache_key,
        true,
    )
}

/// 函数 `chat_completions_stream_enforces_include_usage`
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
fn chat_completions_stream_enforces_include_usage() {
    let body = json!({
        "model": "gpt-4o",
        "stream": true,
        "messages": [{ "role": "user", "content": "hi" }]
    });
    let out = apply_request_overrides(
        "/v1/chat/completions",
        serde_json::to_vec(&body).expect("serialize request body"),
        None,
        None,
        None,
    );
    let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output body");
    assert_eq!(
        value
            .get("stream_options")
            .and_then(|v| v.get("include_usage"))
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
}

/// 函数 `chat_completions_stream_preserves_options_while_enabling_usage`
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
fn chat_completions_stream_preserves_options_while_enabling_usage() {
    let body = json!({
        "model": "gpt-4o",
        "stream": true,
        "stream_options": { "include_usage": false, "foo": "bar" },
        "messages": [{ "role": "user", "content": "hi" }]
    });
    let out = apply_request_overrides(
        "/v1/chat/completions",
        serde_json::to_vec(&body).expect("serialize request body"),
        None,
        None,
        None,
    );
    let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output body");
    assert_eq!(
        value
            .get("stream_options")
            .and_then(|v| v.get("include_usage"))
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        value
            .get("stream_options")
            .and_then(|v| v.get("foo"))
            .and_then(serde_json::Value::as_str),
        Some("bar")
    );
}

/// 函数 `chat_completions_uses_reasoning_effort_and_drops_non_official_keys`
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
fn chat_completions_uses_reasoning_effort_and_drops_non_official_keys() {
    let _env_guard = crate::test_env_guard();
    let _strict_guard = RuntimeEnvGuard::set(STRICT_REQUEST_PARAM_ALLOWLIST_ENV, "1");
    let body = json!({
        "model": "gpt-4.1",
        "messages": [{ "role": "user", "content": "hi" }],
        "reasoning": { "effort": "high" },
        "metadata": { "source": "test" },
        "unknown_field": "drop-me"
    });
    let out = apply_request_overrides(
        "/v1/chat/completions",
        serde_json::to_vec(&body).expect("serialize request body"),
        None,
        Some("medium"),
        None,
    );
    let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output body");
    assert_eq!(
        value
            .get("reasoning_effort")
            .and_then(serde_json::Value::as_str),
        Some("medium")
    );
    assert!(value.get("reasoning").is_none());
    assert!(value.get("unknown_field").is_none());
    assert!(value.get("metadata").is_some());
}

/// 函数 `chat_completions_accepts_responses_style_payload`
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
fn chat_completions_accepts_responses_style_payload() {
    let body = json!({
        "model": "gpt-4.1",
        "instructions": "act as reviewer",
        "input": [
            {
                "type": "message",
                "role": "user",
                "content": [
                    { "type": "input_text", "text": "hello" }
                ]
            }
        ],
        "reasoning": { "effort": "high" },
        "stream": true
    });
    let out = apply_request_overrides(
        "/v1/chat/completions",
        serde_json::to_vec(&body).expect("serialize request body"),
        None,
        None,
        None,
    );
    let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output body");
    assert!(value.get("instructions").is_none());
    assert!(value.get("input").is_none());
    assert_eq!(
        value
            .get("messages")
            .and_then(|v| v.get(0))
            .and_then(|v| v.get("role"))
            .and_then(serde_json::Value::as_str),
        Some("system")
    );
    assert_eq!(
        value
            .get("messages")
            .and_then(|v| v.get(1))
            .and_then(|v| v.get("content"))
            .and_then(serde_json::Value::as_str),
        Some("hello")
    );
    assert_eq!(
        value
            .get("reasoning_effort")
            .and_then(serde_json::Value::as_str),
        Some("high")
    );
    assert_eq!(
        value
            .get("stream_options")
            .and_then(|v| v.get("include_usage"))
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
}

/// 函数 `chat_completions_normalizes_responses_function_tools`
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
fn chat_completions_normalizes_responses_function_tools() {
    let body = json!({
        "model": "gpt-4.1",
        "input": "ping",
        "tools": [
            {
                "type": "function",
                "name": "ping",
                "description": "ping tool",
                "parameters": { "type": "object", "properties": {} },
                "strict": true
            }
        ],
        "tool_choice": {
            "type": "function",
            "name": "ping"
        }
    });
    let out = apply_request_overrides(
        "/v1/chat/completions",
        serde_json::to_vec(&body).expect("serialize request body"),
        None,
        None,
        None,
    );
    let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output body");
    assert_eq!(
        value
            .get("tools")
            .and_then(|v| v.get(0))
            .and_then(|v| v.get("function"))
            .and_then(|v| v.get("name"))
            .and_then(serde_json::Value::as_str),
        Some("ping")
    );
    assert_eq!(
        value
            .get("tool_choice")
            .and_then(|v| v.get("function"))
            .and_then(|v| v.get("name"))
            .and_then(serde_json::Value::as_str),
        Some("ping")
    );
}

/// 函数 `responses_overrides_model_and_reasoning_effort`
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
fn responses_overrides_model_and_reasoning_effort() {
    let _guard = crate::test_env_guard();
    let body = json!({
        "model": "gpt-5.3-codex",
        "reasoning": { "effort": "high" },
        "input": [{ "type": "message", "role": "user", "content": [{ "type": "input_text", "text": "hi" }] }]
    });
    let out = apply_codex_compat_request_overrides(
        "/v1/responses",
        serde_json::to_vec(&body).expect("serialize request body"),
        Some("gpt-5.3-codex"),
        Some("medium"),
        Some("https://chatgpt.com/backend-api/codex"),
    );
    let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output body");
    assert_eq!(
        value.get("model").and_then(serde_json::Value::as_str),
        Some("gpt-5.3-codex")
    );
    assert_eq!(
        value
            .get("reasoning")
            .and_then(|v| v.get("effort"))
            .and_then(serde_json::Value::as_str),
        Some("medium")
    );
    assert!(value
        .get("instructions")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|value| !value.trim().is_empty()));
}

/// 函数 `responses_input_string_normalized_to_list`
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
fn responses_input_string_normalized_to_list() {
    let _guard = crate::test_env_guard();
    let body = json!({
        "model": "gpt-5.3-codex",
        "input": "hello"
    });
    let out = apply_codex_compat_request_overrides(
        "/v1/responses",
        serde_json::to_vec(&body).expect("serialize request body"),
        None,
        None,
        Some("https://chatgpt.com/backend-api/codex"),
    );
    let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output body");
    assert_eq!(
        value
            .get("input")
            .and_then(|v| v.get(0))
            .and_then(|v| v.get("type"))
            .and_then(serde_json::Value::as_str),
        Some("message")
    );
}

/// 函数 `responses_stream_and_store_are_forced_for_codex_backend`
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
fn responses_stream_and_store_are_forced_for_codex_backend() {
    let _guard = crate::test_env_guard();
    let body = json!({
        "model": "gpt-5.3-codex",
        "input": "hello",
        "stream": false,
        "store": true
    });
    let out = apply_codex_compat_request_overrides(
        "/v1/responses",
        serde_json::to_vec(&body).expect("serialize request body"),
        None,
        None,
        Some("https://chatgpt.com/backend-api/codex"),
    );
    let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output body");
    assert_eq!(
        value.get("stream").and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        value.get("store").and_then(serde_json::Value::as_bool),
        Some(false)
    );
}

#[test]
fn responses_default_path_preserves_native_codex_body_shape() {
    let _guard = crate::test_env_guard();
    let body = json!({
        "model": "gpt-5.3-codex",
        "input": "hello",
        "stream": false,
        "store": true,
        "reasoning": { "effort": "medium" }
    });
    let out = apply_request_overrides(
        "/v1/responses",
        serde_json::to_vec(&body).expect("serialize request body"),
        None,
        None,
        Some("https://chatgpt.com/backend-api/codex"),
    );
    let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output body");
    assert_eq!(
        value.get("input").and_then(serde_json::Value::as_str),
        Some("hello")
    );
    assert_eq!(
        value.get("stream").and_then(serde_json::Value::as_bool),
        Some(false)
    );
    assert_eq!(
        value.get("store").and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert!(value.get("instructions").is_none());
    assert!(value.get("tool_choice").is_none());
    assert!(value.get("parallel_tool_calls").is_none());
    assert!(value.get("include").is_none());
}

#[test]
fn responses_default_path_preserves_image_generation_tool() {
    let _guard = crate::test_env_guard();
    let body = json!({
        "model": "gpt-5.4",
        "input": "画一张极简风格的猫",
        "tools": [{
            "type": "image_generation",
            "output_format": "png"
        }],
        "tool_choice": {
            "type": "image_generation"
        },
        "stream": true
    });
    let out = apply_request_overrides(
        "/v1/responses",
        serde_json::to_vec(&body).expect("serialize request body"),
        None,
        None,
        Some("https://chatgpt.com/backend-api/codex"),
    );
    let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output body");

    assert_eq!(
        value
            .get("tools")
            .and_then(serde_json::Value::as_array)
            .and_then(|tools| tools.first())
            .and_then(|tool| tool.get("type"))
            .and_then(serde_json::Value::as_str),
        Some("image_generation")
    );
    assert_eq!(
        value
            .get("tools")
            .and_then(serde_json::Value::as_array)
            .and_then(|tools| tools.first())
            .and_then(|tool| tool.get("output_format"))
            .and_then(serde_json::Value::as_str),
        Some("png")
    );
    assert_eq!(
        value
            .get("tool_choice")
            .and_then(|tool_choice| tool_choice.get("type"))
            .and_then(serde_json::Value::as_str),
        Some("image_generation")
    );
}

#[test]
fn responses_default_path_auto_injects_image_generation_tool_for_codex_backend() {
    let _guard = crate::test_env_guard();
    let _inject_guard = RuntimeEnvGuard::set(CODEX_IMAGE_GENERATION_AUTO_INJECT_TOOL_ENV, "1");
    let body = json!({
        "model": "gpt-5.4",
        "input": "帮我生成一个现场作业中台 logo",
        "stream": true
    });
    let out = apply_request_overrides(
        "/v1/responses",
        serde_json::to_vec(&body).expect("serialize request body"),
        None,
        None,
        Some("https://chatgpt.com/backend-api/codex"),
    );
    let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output body");

    let tools = value
        .get("tools")
        .and_then(serde_json::Value::as_array)
        .expect("tools array");
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0]["type"], "image_generation");
    assert_eq!(tools[0]["output_format"], "png");
    assert!(tools[0].get("model").is_none());
}

#[test]
fn responses_default_path_does_not_auto_inject_image_generation_tool_by_default() {
    let _guard = crate::test_env_guard();
    let _inject_guard = RuntimeEnvGuard::clear(CODEX_IMAGE_GENERATION_AUTO_INJECT_TOOL_ENV);
    let body = json!({
        "model": "gpt-5.4",
        "input": "帮我生成一个现场作业中台 logo",
        "stream": true
    });

    let out = apply_request_overrides(
        "/v1/responses",
        serde_json::to_vec(&body).expect("serialize request body"),
        None,
        None,
        Some("https://chatgpt.com/backend-api/codex"),
    );

    let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output body");
    assert!(value.get("tools").is_none());
}

#[test]
fn responses_default_path_auto_inject_appends_without_duplicating_image_generation_tool() {
    let _guard = crate::test_env_guard();
    let _inject_guard = RuntimeEnvGuard::set(CODEX_IMAGE_GENERATION_AUTO_INJECT_TOOL_ENV, "1");
    let body = json!({
        "model": "gpt-5.4",
        "input": "hello",
        "tools": [{
            "type": "web_search"
        }]
    });
    let out = apply_request_overrides(
        "/v1/responses",
        serde_json::to_vec(&body).expect("serialize request body"),
        None,
        None,
        Some("https://chatgpt.com/backend-api/codex"),
    );
    let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output body");

    let tools = value
        .get("tools")
        .and_then(serde_json::Value::as_array)
        .expect("tools array");
    assert_eq!(tools.len(), 2);
    assert_eq!(tools[0]["type"], "web_search");
    assert_eq!(tools[1]["type"], "image_generation");
    assert_eq!(tools[1]["output_format"], "png");
}

#[test]
fn responses_default_path_skips_auto_image_generation_tool_when_disabled_or_spark_model() {
    let _guard = crate::test_env_guard();
    {
        let _inject_guard = RuntimeEnvGuard::set(CODEX_IMAGE_GENERATION_AUTO_INJECT_TOOL_ENV, "0");
        let body = json!({
            "model": "gpt-5.4",
            "input": "hello"
        });
        let out = apply_request_overrides(
            "/v1/responses",
            serde_json::to_vec(&body).expect("serialize request body"),
            None,
            None,
            Some("https://chatgpt.com/backend-api/codex"),
        );
        let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output body");
        assert!(value.get("tools").is_none());
    }
    {
        let _inject_guard = RuntimeEnvGuard::set(CODEX_IMAGE_GENERATION_AUTO_INJECT_TOOL_ENV, "1");
        let body = json!({
            "model": "gpt-5.4-spark",
            "input": "hello"
        });
        let out = apply_request_overrides(
            "/v1/responses",
            serde_json::to_vec(&body).expect("serialize request body"),
            None,
            None,
            Some("https://chatgpt.com/backend-api/codex"),
        );
        let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output body");
        assert!(value.get("tools").is_none());
    }
}

#[test]
fn responses_default_path_skips_request_rewrite_layer_prompt_cache_inference() {
    let _guard = crate::test_env_guard();
    let body = json!({
        "model": "gpt-5.3-codex",
        "input": "hello"
    });
    let out = apply_request_overrides_with_prompt_cache_key(
        "/v1/responses",
        serde_json::to_vec(&body).expect("serialize request body"),
        None,
        None,
        Some("https://chatgpt.com/backend-api/codex"),
        Some("thread_123"),
    );
    let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output body");
    assert!(value.get("prompt_cache_key").is_none());
}

#[test]
fn responses_compat_scope_disabled_preserves_native_codex_body_shape() {
    let _guard = crate::test_env_guard();
    let body = json!({
        "model": "gpt-5.3-codex",
        "input": "hello",
        "stream": false,
        "store": true,
        "reasoning": { "effort": "medium" },
        "stream_passthrough": true
    });
    let out = apply_request_overrides_with_service_tier_and_prompt_cache_key_scope(
        "/v1/responses",
        serde_json::to_vec(&body).expect("serialize request body"),
        None,
        None,
        None,
        Some("https://chatgpt.com/backend-api/codex"),
        Some("thread_123"),
        false,
    );
    let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output body");
    assert_eq!(
        value.get("input").and_then(serde_json::Value::as_str),
        Some("hello")
    );
    assert_eq!(
        value.get("stream").and_then(serde_json::Value::as_bool),
        Some(false)
    );
    assert_eq!(
        value.get("store").and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert!(value.get("instructions").is_none());
    assert!(value.get("prompt_cache_key").is_none());
    assert!(value.get("stream_passthrough").is_none());
}

#[test]
fn responses_compat_scope_enabled_formats_openai_api_body_for_codex_backend() {
    let _guard = crate::test_env_guard();
    let body = json!({
        "model": "gpt-5.4",
        "input": "hello",
        "stream": false,
        "store": true,
        "reasoning": { "effort": "medium" }
    });
    let out = apply_request_overrides_with_service_tier_and_prompt_cache_key_scope(
        "/v1/responses",
        serde_json::to_vec(&body).expect("serialize request body"),
        None,
        None,
        None,
        Some("https://chatgpt.com/backend-api/codex"),
        Some("thread_123"),
        true,
    );
    let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output body");

    assert_eq!(
        value
            .get("input")
            .and_then(|v| v.get(0))
            .and_then(|v| v.get("type"))
            .and_then(serde_json::Value::as_str),
        Some("message")
    );
    assert_eq!(
        value.get("stream").and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        value.get("store").and_then(serde_json::Value::as_bool),
        Some(false)
    );
    assert_eq!(
        value.get("tool_choice").and_then(serde_json::Value::as_str),
        Some("auto")
    );
    assert_eq!(
        value
            .get("parallel_tool_calls")
            .and_then(serde_json::Value::as_bool),
        Some(false)
    );
    assert_eq!(
        value
            .get("prompt_cache_key")
            .and_then(serde_json::Value::as_str),
        Some("thread_123")
    );
    assert!(value
        .get("instructions")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|value| !value.trim().is_empty()));
    assert!(value
        .get("include")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|include| include
            .iter()
            .any(|item| item.as_str() == Some("reasoning.encrypted_content"))));
}

#[test]
fn responses_compat_scope_enabled_defaults_omitted_stream_to_upstream_sse() {
    let _guard = crate::test_env_guard();
    let body = json!({
        "model": "gpt-5.4",
        "input": "hello"
    });
    let out = apply_request_overrides_with_service_tier_and_prompt_cache_key_scope(
        "/v1/responses",
        serde_json::to_vec(&body).expect("serialize request body"),
        None,
        None,
        None,
        Some("https://chatgpt.com/backend-api/codex"),
        None,
        true,
    );
    let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output body");

    assert_eq!(
        value.get("stream").and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert!(value
        .get("input")
        .and_then(serde_json::Value::as_array)
        .is_some());
    assert!(value
        .get("instructions")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|value| !value.trim().is_empty()));
}

#[test]
fn responses_forced_prompt_cache_scope_disabled_does_not_apply_compat_body_rewrite() {
    let _guard = crate::test_env_guard();
    let body = json!({
        "model": "gpt-5.3-codex",
        "input": "hello",
        "stream": false,
        "store": true
    });
    let out = apply_request_overrides_with_service_tier_and_forced_prompt_cache_key_scope(
        "/v1/responses",
        serde_json::to_vec(&body).expect("serialize request body"),
        None,
        None,
        None,
        Some("https://chatgpt.com/backend-api/codex"),
        Some("thread_123"),
        false,
    );
    let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output body");

    assert_eq!(
        value.get("stream").and_then(serde_json::Value::as_bool),
        Some(false)
    );
    assert_eq!(
        value.get("store").and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        value
            .get("prompt_cache_key")
            .and_then(serde_json::Value::as_str),
        Some("thread_123")
    );
    assert!(value.get("instructions").is_none());
    assert!(value.get("tool_choice").is_none());
    assert!(value.get("include").is_none());
}

#[test]
fn responses_codex_backend_hoists_leading_developer_message_to_instructions() {
    let _guard = crate::test_env_guard();
    let body = json!({
        "model": "gpt-5.4",
        "input": [
            {
                "role": "developer",
                "content": "You are OpenCode"
            },
            {
                "role": "user",
                "content": [
                    { "type": "input_text", "text": "请只回复：测试" }
                ]
            }
        ],
        "stream": true
    });
    let out = apply_request_overrides(
        "/v1/responses",
        serde_json::to_vec(&body).expect("serialize request body"),
        None,
        None,
        Some("https://chatgpt.com/backend-api/codex"),
    );
    let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output body");
    assert_eq!(
        value
            .get("instructions")
            .and_then(serde_json::Value::as_str),
        Some("You are OpenCode")
    );
    let input = value
        .get("input")
        .and_then(serde_json::Value::as_array)
        .expect("input array");
    assert_eq!(input.len(), 1);
    assert_eq!(
        input[0].get("role").and_then(serde_json::Value::as_str),
        Some("user")
    );
}

#[test]
fn responses_codex_backend_hoists_leading_system_message_to_instructions() {
    let _guard = crate::test_env_guard();
    let body = json!({
        "model": "gpt-5.4",
        "input": [
            {
                "role": "system",
                "content": [
                    { "type": "text", "text": "You are Cherry" }
                ]
            },
            {
                "role": "user",
                "content": [
                    { "type": "input_text", "text": "hello" }
                ]
            }
        ]
    });
    let out = apply_request_overrides(
        "/v1/responses",
        serde_json::to_vec(&body).expect("serialize request body"),
        None,
        None,
        Some("https://chatgpt.com/backend-api/codex"),
    );
    let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output body");
    assert_eq!(
        value
            .get("instructions")
            .and_then(serde_json::Value::as_str),
        Some("You are Cherry")
    );
    let input = value
        .get("input")
        .and_then(serde_json::Value::as_array)
        .expect("input array");
    assert_eq!(input.len(), 1);
    assert_eq!(
        input[0].get("role").and_then(serde_json::Value::as_str),
        Some("user")
    );
}

#[test]
fn responses_codex_backend_hoists_single_object_system_input_to_instructions() {
    let _guard = crate::test_env_guard();
    let body = json!({
        "model": "gpt-5.4",
        "input": {
            "role": "system",
            "content": "You are Cherry"
        }
    });
    let out = apply_request_overrides(
        "/v1/responses",
        serde_json::to_vec(&body).expect("serialize request body"),
        None,
        None,
        Some("https://chatgpt.com/backend-api/codex"),
    );
    let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output body");
    assert_eq!(
        value
            .get("instructions")
            .and_then(serde_json::Value::as_str),
        Some("You are Cherry")
    );
    let input = value
        .get("input")
        .and_then(serde_json::Value::as_array)
        .expect("input array");
    assert!(input.is_empty());
}

#[test]
fn responses_codex_backend_preserves_existing_instructions() {
    let _guard = crate::test_env_guard();
    let body = json!({
        "model": "gpt-5.4",
        "instructions": "Keep me",
        "input": [
            {
                "role": "developer",
                "content": "Do not override"
            },
            {
                "role": "user",
                "content": [
                    { "type": "input_text", "text": "hello" }
                ]
            }
        ]
    });
    let out = apply_request_overrides(
        "/v1/responses",
        serde_json::to_vec(&body).expect("serialize request body"),
        None,
        None,
        Some("https://chatgpt.com/backend-api/codex"),
    );
    let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output body");
    assert_eq!(
        value
            .get("instructions")
            .and_then(serde_json::Value::as_str),
        Some("Keep me")
    );
    let input = value
        .get("input")
        .and_then(serde_json::Value::as_array)
        .expect("input array");
    assert_eq!(input.len(), 2);
    assert_eq!(
        input[0].get("role").and_then(serde_json::Value::as_str),
        Some("developer")
    );
}

#[test]
fn responses_codex_backend_adds_default_instructions_when_missing() {
    let _guard = crate::test_env_guard();
    let body = json!({
        "model": "gpt-5.4",
        "input": "hello"
    });
    let out = apply_codex_compat_request_overrides(
        "/v1/responses",
        serde_json::to_vec(&body).expect("serialize request body"),
        None,
        None,
        Some("https://chatgpt.com/backend-api/codex"),
    );
    let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output body");
    assert!(value
        .get("instructions")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|value| !value.trim().is_empty()));
}

#[test]
fn responses_codex_backend_replaces_empty_instructions() {
    let _guard = crate::test_env_guard();
    let body = json!({
        "model": "gpt-5.4",
        "instructions": "",
        "input": "hello"
    });
    let out = apply_codex_compat_request_overrides(
        "/v1/responses",
        serde_json::to_vec(&body).expect("serialize request body"),
        None,
        None,
        Some("https://chatgpt.com/backend-api/codex"),
    );
    let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output body");
    assert!(value
        .get("instructions")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|value| !value.trim().is_empty()));
}

/// 函数 `responses_infers_prompt_cache_key_from_conversation_id_for_codex_backend`
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
fn responses_infers_prompt_cache_key_from_conversation_id_for_codex_backend() {
    let _guard = crate::test_env_guard();
    let body = json!({
        "model": "gpt-5.3-codex",
        "input": "hello"
    });
    let out = apply_codex_compat_request_overrides_with_prompt_cache_key(
        "/v1/responses",
        serde_json::to_vec(&body).expect("serialize request body"),
        None,
        None,
        Some("https://chatgpt.com/backend-api/codex"),
        Some("thread_123"),
    );
    let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output body");
    assert_eq!(
        value
            .get("prompt_cache_key")
            .and_then(serde_json::Value::as_str),
        Some("thread_123")
    );
}

/// 函数 `responses_forced_prompt_cache_key_overrides_existing_value_for_codex_backend`
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
fn responses_forced_prompt_cache_key_overrides_existing_value_for_codex_backend() {
    let _guard = crate::test_env_guard();
    let body = json!({
        "model": "gpt-5.3-codex",
        "input": "hello",
        "prompt_cache_key": "thread_old"
    });
    let out = apply_request_overrides_with_forced_prompt_cache_key(
        "/v1/responses",
        serde_json::to_vec(&body).expect("serialize request body"),
        None,
        None,
        Some("https://chatgpt.com/backend-api/codex"),
        Some("thread_new"),
    );
    let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output body");
    assert_eq!(
        value
            .get("prompt_cache_key")
            .and_then(serde_json::Value::as_str),
        Some("thread_new")
    );
}

/// 函数 `responses_stream_passthrough_keeps_client_stream_flag_when_enabled`
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
fn responses_stream_passthrough_keeps_client_stream_flag_when_enabled() {
    let _guard = crate::test_env_guard();
    let body = json!({
        "model": "gpt-5.3-codex",
        "input": "hello",
        "stream": false,
        "stream_passthrough": true,
        "store": true
    });
    let out = apply_codex_compat_request_overrides(
        "/v1/responses",
        serde_json::to_vec(&body).expect("serialize request body"),
        None,
        None,
        Some("https://chatgpt.com/backend-api/codex"),
    );
    let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output body");
    assert_eq!(
        value.get("stream").and_then(serde_json::Value::as_bool),
        Some(false)
    );
    assert_eq!(
        value.get("store").and_then(serde_json::Value::as_bool),
        Some(false)
    );
    assert!(value.get("stream_passthrough").is_none());
}

#[test]
fn responses_official_allowlist_drops_stream_passthrough() {
    let _guard = crate::test_env_guard();
    let _strict_guard = RuntimeEnvGuard::set(STRICT_REQUEST_PARAM_ALLOWLIST_ENV, "true");
    let body = json!({
        "model": "gpt-4.1",
        "input": "hello",
        "stream_passthrough": true,
        "unknown_field": "drop-me"
    });
    let out = apply_request_overrides(
        "/v1/responses",
        serde_json::to_vec(&body).expect("serialize request body"),
        None,
        None,
        Some("https://api.openai.com/v1"),
    );
    let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output body");
    assert!(value.get("stream_passthrough").is_none());
    assert!(value.get("unknown_field").is_none());
}

/// 函数 `responses_dynamic_tools_are_mapped_to_tools_for_codex_backend`
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
fn responses_dynamic_tools_are_mapped_to_tools_for_codex_backend() {
    let _guard = crate::test_env_guard();
    let _inject_guard = RuntimeEnvGuard::set(CODEX_IMAGE_GENERATION_AUTO_INJECT_TOOL_ENV, "1");
    let body = json!({
        "model": "gpt-5.3-codex",
        "input": "hello",
        "tools": [{
            "type": "function",
            "name": "existing_tool",
            "parameters": { "type": "object", "properties": {} }
        }],
        "dynamicTools": [{
            "name": "dynamic_weather",
            "description": "lookup weather",
            "input_schema": {
                "type": "object",
                "properties": {
                    "city": { "type": "string" }
                },
                "required": ["city"]
            }
        }]
    });
    let out = apply_codex_compat_request_overrides(
        "/v1/responses",
        serde_json::to_vec(&body).expect("serialize request body"),
        None,
        None,
        Some("https://chatgpt.com/backend-api/codex"),
    );
    let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output body");
    let tools = value
        .get("tools")
        .and_then(serde_json::Value::as_array)
        .expect("tools array");
    assert_eq!(tools.len(), 3);
    assert!(value.get("dynamicTools").is_none());
    assert_eq!(
        tools[1].get("name").and_then(serde_json::Value::as_str),
        Some("dynamic_weather")
    );
    assert_eq!(
        tools[1]
            .get("description")
            .and_then(serde_json::Value::as_str),
        Some("lookup weather")
    );
    assert_eq!(
        tools[1]
            .get("parameters")
            .and_then(|parameters| parameters.get("properties"))
            .and_then(|properties| properties.get("city"))
            .and_then(|city| city.get("type"))
            .and_then(serde_json::Value::as_str),
        Some("string")
    );
    assert_eq!(
        tools[2].get("type").and_then(serde_json::Value::as_str),
        Some("image_generation")
    );
    assert_eq!(
        tools[2]
            .get("output_format")
            .and_then(serde_json::Value::as_str),
        Some("png")
    );
}

/// 函数 `responses_preserves_priority_service_tier_for_codex_backend`
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
fn responses_preserves_priority_service_tier_for_codex_backend() {
    let _guard = crate::test_env_guard();
    let body = json!({
        "model": "gpt-5.3-codex",
        "instructions": "stay",
        "input": [{ "type": "message", "role": "user", "content": [{ "type": "input_text", "text": "hello" }] }],
        "tools": [{ "type": "function", "name": "ping", "parameters": { "type": "object", "properties": {} } }],
        "tool_choice": "auto",
        "parallel_tool_calls": true,
        "reasoning": { "effort": "high" },
        "stream": true,
        "store": false,
        "include": ["reasoning.encrypted_content"],
        "prompt_cache_key": "pc_1",
        "text": { "format": { "type": "text" } },
        "service_tier": "priority",
        "temperature": 0.7,
        "user": "cherry-studio"
    });
    let out = apply_request_overrides(
        "/v1/responses",
        serde_json::to_vec(&body).expect("serialize request body"),
        None,
        None,
        Some("https://chatgpt.com/backend-api/codex"),
    );
    let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output body");
    assert!(value.get("model").is_some());
    assert!(value.get("instructions").is_some());
    assert!(value.get("input").is_some());
    assert!(value.get("max_output_tokens").is_none());
    assert!(value.get("previous_response_id").is_none());
    assert!(value.get("tools").is_some());
    assert!(value.get("tool_choice").is_some());
    assert!(value.get("parallel_tool_calls").is_some());
    assert!(value.get("reasoning").is_some());
    assert!(value.get("stream").is_some());
    assert!(value.get("store").is_some());
    assert!(value.get("include").is_some());
    assert!(value.get("prompt_cache_key").is_some());
    assert!(value.get("text").is_some());
    assert_eq!(
        value
            .get("service_tier")
            .and_then(serde_json::Value::as_str),
        Some("priority")
    );
    assert!(value.get("temperature").is_none());
    assert!(value.get("user").is_none());
}

#[test]
fn responses_codex_backend_keeps_conservative_field_snapshot() {
    let _guard = crate::test_env_guard();
    let body = json!({
        "model": "gpt-5.3-codex",
        "instructions": "stay",
        "input": [{ "type": "message", "role": "user", "content": [{ "type": "input_text", "text": "hello" }] }],
        "tools": [{ "type": "function", "name": "ping", "parameters": { "type": "object", "properties": {} } }],
        "tool_choice": "auto",
        "parallel_tool_calls": true,
        "reasoning": { "effort": "medium", "summary": "auto", "context": "current_turn" },
        "service_tier": "priority",
        "store": false,
        "stream": true,
        "include": ["reasoning.encrypted_content"],
        "prompt_cache_key": "pc_snapshot",
        "client_metadata": {
            "source": "snapshot"
        },
        "text": { "format": { "type": "text" } },
        "max_output_tokens": 1024,
        "metadata": { "client": "third-party" },
        "temperature": 0.2,
        "top_p": 0.9,
        "truncation": "auto",
        "user": "third-party-user",
        "previous_response_id": "resp_previous",
        "unknown_field": true
    });
    let out = apply_request_overrides(
        "/v1/responses",
        serde_json::to_vec(&body).expect("serialize request body"),
        None,
        None,
        Some("https://chatgpt.com/backend-api/codex"),
    );
    let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output body");
    let object = value.as_object().expect("rewritten body object");
    let keys = object
        .keys()
        .map(String::as_str)
        .collect::<std::collections::BTreeSet<_>>();
    let expected = [
        "client_metadata",
        "include",
        "input",
        "instructions",
        "model",
        "parallel_tool_calls",
        "prompt_cache_key",
        "reasoning",
        "service_tier",
        "store",
        "stream",
        "text",
        "tool_choice",
        "tools",
    ]
    .into_iter()
    .collect::<std::collections::BTreeSet<_>>();

    assert_eq!(keys, expected);
    assert_eq!(value["reasoning"]["context"], "current_turn");
    assert_eq!(value["reasoning"]["summary"], "auto");
    assert!(object.get("max_output_tokens").is_none());
    assert!(object.get("metadata").is_none());
    assert!(object.get("temperature").is_none());
    assert!(object.get("top_p").is_none());
    assert!(object.get("truncation").is_none());
    assert!(object.get("user").is_none());
    assert!(object.get("previous_response_id").is_none());
    assert!(object.get("unknown_field").is_none());
}

/// 函数 `responses_preserves_client_metadata_for_codex_backend`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-16
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn responses_preserves_client_metadata_for_codex_backend() {
    let _guard = crate::test_env_guard();
    let _db_guard = RuntimeEnvGuard::set(
        CODEXMANAGER_DB_PATH_ENV,
        isolated_db_path("responses-client-metadata").as_str(),
    );
    let body = json!({
        "model": "gpt-5.3-codex",
        "instructions": "stay",
        "input": "hello",
        "client_metadata": {
            "turn_id": "turn-123",
            "origin": "gaas"
        },
        "metadata": {
            "source": "open-code"
        }
    });
    let out = apply_request_overrides(
        "/v1/responses",
        serde_json::to_vec(&body).expect("serialize request body"),
        None,
        None,
        Some("https://chatgpt.com/backend-api/codex"),
    );
    let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output body");
    assert!(value.get("client_metadata").is_some());
    assert_eq!(
        value
            .get("client_metadata")
            .and_then(|v| v.get("turn_id"))
            .and_then(serde_json::Value::as_str),
        Some("turn-123")
    );
    assert_eq!(
        value
            .get("client_metadata")
            .and_then(|v| v.get("origin"))
            .and_then(serde_json::Value::as_str),
        Some("gaas")
    );
    let installation_id = value
        .get("client_metadata")
        .and_then(|v| v.get("x-codex-installation-id"))
        .and_then(serde_json::Value::as_str)
        .expect("installation id metadata");
    assert_eq!(installation_id.len(), 36);
    assert_eq!(installation_id.as_bytes().get(14).copied(), Some(b'4'));
    assert!(value.get("metadata").is_none());
}

/// 函数 `responses_defaults_tool_choice_and_reasoning_include_for_codex_backend`
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
fn responses_reuses_persisted_installation_id_for_codex_backend() {
    let _guard = crate::test_env_guard();
    let _db_guard = RuntimeEnvGuard::set(
        CODEXMANAGER_DB_PATH_ENV,
        isolated_db_path("responses-installation-id").as_str(),
    );
    let body = json!({
        "model": "gpt-5.3-codex",
        "input": "hello"
    });
    let first = apply_request_overrides(
        "/v1/responses",
        serde_json::to_vec(&body).expect("serialize first request body"),
        None,
        None,
        Some("https://chatgpt.com/backend-api/codex"),
    );
    let second = apply_request_overrides(
        "/v1/responses",
        serde_json::to_vec(&body).expect("serialize second request body"),
        None,
        None,
        Some("https://chatgpt.com/backend-api/codex"),
    );
    let first: serde_json::Value = serde_json::from_slice(&first).expect("parse first output body");
    let second: serde_json::Value =
        serde_json::from_slice(&second).expect("parse second output body");

    assert_eq!(
        first
            .pointer("/client_metadata/x-codex-installation-id")
            .and_then(serde_json::Value::as_str),
        second
            .pointer("/client_metadata/x-codex-installation-id")
            .and_then(serde_json::Value::as_str)
    );
}

#[test]
fn responses_defaults_tool_choice_and_reasoning_include_for_codex_backend() {
    let _guard = crate::test_env_guard();
    let body = json!({
        "model": "gpt-5.3-codex",
        "input": "hello",
        "reasoning": { "effort": "medium" }
    });
    let out = apply_codex_compat_request_overrides(
        "/v1/responses",
        serde_json::to_vec(&body).expect("serialize request body"),
        None,
        None,
        Some("https://chatgpt.com/backend-api/codex"),
    );
    let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output body");
    assert_eq!(
        value.get("tool_choice").and_then(serde_json::Value::as_str),
        Some("auto")
    );
    assert_eq!(
        value
            .get("parallel_tool_calls")
            .and_then(serde_json::Value::as_bool),
        Some(false)
    );
    assert!(value
        .get("tools")
        .and_then(serde_json::Value::as_array)
        .is_some());
    let include = value
        .get("include")
        .and_then(serde_json::Value::as_array)
        .expect("include array");
    assert!(include.iter().any(|item| {
        item.as_str()
            .map(|entry| entry == "reasoning.encrypted_content")
            .unwrap_or(false)
    }));
}

#[test]
fn responses_preserve_specific_function_tool_choice_object() {
    let _guard = crate::test_env_guard();
    let body = json!({
        "model": "gpt-5.3-codex",
        "input": [{ "type": "message", "role": "user", "content": [{ "type": "input_text", "text": "hello" }] }],
        "tools": [{ "type": "function", "name": "mcp__browser__take_screenshot", "parameters": { "type": "object", "properties": {} } }],
        "tool_choice": {
            "type": "function",
            "name": "mcp__browser__take_screenshot"
        }
    });
    let out = apply_request_overrides(
        "/v1/responses",
        serde_json::to_vec(&body).expect("serialize request body"),
        None,
        None,
        Some("https://chatgpt.com/backend-api/codex"),
    );
    let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output body");
    assert_eq!(value["tool_choice"]["type"], "function");
    assert_eq!(
        value["tool_choice"]["name"],
        "mcp__browser__take_screenshot"
    );
}

/// 函数 `responses_defaults_empty_include_without_reasoning_for_codex_backend`
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
fn responses_defaults_empty_include_without_reasoning_for_codex_backend() {
    let _guard = crate::test_env_guard();
    let body = json!({
        "model": "gpt-5.3-codex",
        "input": "hello"
    });
    let out = apply_codex_compat_request_overrides(
        "/v1/responses",
        serde_json::to_vec(&body).expect("serialize request body"),
        None,
        None,
        Some("https://chatgpt.com/backend-api/codex"),
    );
    let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output body");
    assert!(value
        .get("tools")
        .and_then(serde_json::Value::as_array)
        .is_some());
    assert_eq!(
        value
            .get("include")
            .and_then(serde_json::Value::as_array)
            .map(Vec::len),
        Some(0)
    );
}

/// 函数 `responses_normalizes_fast_service_tier_to_priority_for_codex_backend`
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
fn responses_normalizes_fast_service_tier_to_priority_for_codex_backend() {
    let _guard = crate::test_env_guard();
    let body = json!({
        "model": "gpt-5.3-codex",
        "input": "hello",
        "service_tier": "Fast"
    });
    let out = apply_request_overrides(
        "/v1/responses",
        serde_json::to_vec(&body).expect("serialize request body"),
        None,
        None,
        Some("https://chatgpt.com/backend-api/codex"),
    );
    let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output body");
    assert_eq!(
        value
            .get("service_tier")
            .and_then(serde_json::Value::as_str),
        Some("priority")
    );
}

/// 函数 `responses_applies_fast_service_tier_override_as_priority_for_codex_backend`
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
fn responses_applies_fast_service_tier_override_as_priority_for_codex_backend() {
    let _guard = crate::test_env_guard();
    let body = json!({
        "model": "gpt-5.3-codex",
        "input": "hello"
    });
    let out = apply_request_overrides_with_service_tier(
        "/v1/responses",
        serde_json::to_vec(&body).expect("serialize request body"),
        None,
        None,
        Some("fast"),
        Some("https://chatgpt.com/backend-api/codex"),
    );
    let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output body");
    assert_eq!(
        value
            .get("service_tier")
            .and_then(serde_json::Value::as_str),
        Some("priority")
    );
}

#[test]
fn responses_default_path_still_maps_fast_service_tier_to_priority_for_codex_backend() {
    let _guard = crate::test_env_guard();
    let body = json!({
        "model": "gpt-5.4",
        "input": "hello",
        "service_tier": "Fast"
    });
    let out = apply_request_overrides(
        "/v1/responses",
        serde_json::to_vec(&body).expect("serialize request body"),
        None,
        None,
        Some("https://chatgpt.com/backend-api/codex"),
    );
    let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output body");
    assert_eq!(
        value
            .get("service_tier")
            .and_then(serde_json::Value::as_str),
        Some("priority")
    );
}

#[test]
fn responses_default_path_maps_auto_service_tier_to_priority_for_codex_backend() {
    let _guard = crate::test_env_guard();
    let body = json!({
        "model": "gpt-5.4",
        "input": "hello",
        "service_tier": "auto"
    });
    let out = apply_request_overrides(
        "/v1/responses",
        serde_json::to_vec(&body).expect("serialize request body"),
        None,
        None,
        Some("https://chatgpt.com/backend-api/codex"),
    );
    let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output body");
    assert_eq!(
        value
            .get("service_tier")
            .and_then(serde_json::Value::as_str),
        Some("priority")
    );
}

/// 函数 `responses_ignores_unsupported_flex_service_tier_override_for_codex_backend`
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
fn responses_ignores_unsupported_flex_service_tier_override_for_codex_backend() {
    let _guard = crate::test_env_guard();
    let body = json!({
        "model": "gpt-5.3-codex",
        "input": "hello"
    });
    let out = apply_request_overrides_with_service_tier(
        "/v1/responses",
        serde_json::to_vec(&body).expect("serialize request body"),
        None,
        None,
        Some("flex"),
        Some("https://chatgpt.com/backend-api/codex"),
    );
    let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output body");
    assert!(value.get("service_tier").is_none());
}

/// 函数 `responses_compact_uses_codex_compat_rewrite`
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
fn responses_compact_uses_codex_compat_rewrite() {
    let _guard = crate::test_env_guard();
    let body = json!({
        "model": "gpt-5.3-codex",
        "tools": [{ "type": "function", "name": "ping", "parameters": { "type": "object", "properties": {} } }],
        "parallel_tool_calls": true,
        "reasoning": { "effort": "high" },
        "text": { "verbosity": "low" },
        "input": "compact me",
        "stream": false,
        "store": true,
        "service_tier": "priority",
        "user": "drop-me"
    });
    let out = apply_codex_compat_request_overrides(
        "/v1/responses/compact",
        serde_json::to_vec(&body).expect("serialize request body"),
        None,
        None,
        Some("https://chatgpt.com/backend-api/codex"),
    );
    let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output body");
    assert!(value.get("instructions").is_none());
    assert!(value.get("tools").is_some());
    assert_eq!(
        value
            .get("parallel_tool_calls")
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        value
            .get("reasoning")
            .and_then(|reasoning| reasoning.get("effort"))
            .and_then(serde_json::Value::as_str),
        Some("high")
    );
    assert!(value.get("text").is_some());
    assert!(value.get("stream").is_none());
    assert!(value.get("store").is_none());
    assert_eq!(
        value
            .get("service_tier")
            .and_then(serde_json::Value::as_str),
        Some("priority")
    );
    assert!(value.get("user").is_none());
    assert!(value
        .get("input")
        .and_then(serde_json::Value::as_array)
        .is_some());
}

#[test]
fn responses_compact_keeps_only_codex_compact_body_fields() {
    let _guard = crate::test_env_guard();
    let body = json!({
        "model": "gpt-5.3-codex",
        "instructions": "compact instructions",
        "input": "compact me",
        "tools": [{ "type": "function", "name": "ping", "parameters": { "type": "object", "properties": {} } }],
        "tool_choice": "auto",
        "parallel_tool_calls": true,
        "reasoning": { "effort": "high" },
        "text": { "verbosity": "low" },
        "stream": false,
        "store": true,
        "include": ["reasoning.encrypted_content"],
        "prompt_cache_key": "pc_compact",
        "client_metadata": {
            "source": "snapshot"
        },
        "service_tier": "priority",
        "metadata": { "client": "third-party" },
        "max_output_tokens": 1024,
        "temperature": 0.2,
        "top_p": 0.9,
        "truncation": "auto",
        "user": "third-party-user",
        "previous_response_id": "resp_previous",
        "unknown_field": true
    });
    let out = apply_codex_compat_request_overrides(
        "/v1/responses/compact",
        serde_json::to_vec(&body).expect("serialize request body"),
        None,
        None,
        Some("https://chatgpt.com/backend-api/codex"),
    );
    let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output body");
    let object = value.as_object().expect("rewritten compact body object");
    let keys = object
        .keys()
        .map(String::as_str)
        .collect::<std::collections::BTreeSet<_>>();
    let expected = [
        "input",
        "instructions",
        "model",
        "parallel_tool_calls",
        "prompt_cache_key",
        "reasoning",
        "service_tier",
        "text",
        "tools",
    ]
    .into_iter()
    .collect::<std::collections::BTreeSet<_>>();

    assert_eq!(keys, expected);
    assert!(object.get("tool_choice").is_none());
    assert!(object.get("stream").is_none());
    assert!(object.get("store").is_none());
    assert!(object.get("include").is_none());
    assert_eq!(
        object
            .get("prompt_cache_key")
            .and_then(serde_json::Value::as_str),
        Some("pc_compact")
    );
    assert!(object.get("client_metadata").is_none());
    assert_eq!(
        object
            .get("service_tier")
            .and_then(serde_json::Value::as_str),
        Some("priority")
    );
    assert!(object.get("metadata").is_none());
    assert!(object.get("max_output_tokens").is_none());
    assert!(object.get("temperature").is_none());
    assert!(object.get("top_p").is_none());
    assert!(object.get("truncation").is_none());
    assert!(object.get("user").is_none());
    assert!(object.get("previous_response_id").is_none());
    assert!(object.get("unknown_field").is_none());
}

#[test]
fn responses_compact_uses_configured_compact_model_before_attempt_override() {
    let _guard = crate::test_env_guard();
    let _compact_model = RuntimeEnvGuard::set(COMPACT_MODEL_ENV, "gpt-5.4-mini");
    let body = json!({
        "model": "gpt-5.5",
        "instructions": "compact instructions",
        "input": "compact me",
        "reasoning": { "effort": "low" },
        "tools": [],
        "parallel_tool_calls": false
    });

    let out = apply_codex_compat_request_overrides(
        "/v1/responses/compact",
        serde_json::to_vec(&body).expect("serialize request body"),
        Some("gpt-5.5"),
        None,
        Some("https://chatgpt.com/backend-api/codex"),
    );
    let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output body");

    assert_eq!(
        value.get("model").and_then(serde_json::Value::as_str),
        Some("gpt-5.4-mini")
    );
}

#[test]
fn responses_standard_path_ignores_configured_compact_model() {
    let _guard = crate::test_env_guard();
    let _compact_model = RuntimeEnvGuard::set(COMPACT_MODEL_ENV, "gpt-5.4-mini");
    let body = json!({
        "model": "gpt-5.5",
        "input": "hello"
    });

    let out = apply_codex_compat_request_overrides(
        "/v1/responses",
        serde_json::to_vec(&body).expect("serialize request body"),
        Some("gpt-5.5"),
        None,
        Some("https://chatgpt.com/backend-api/codex"),
    );
    let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output body");

    assert_eq!(
        value.get("model").and_then(serde_json::Value::as_str),
        Some("gpt-5.5")
    );
}

#[test]
fn responses_compact_preserves_prompt_cache_key_without_codex_base_rewrite() {
    let _guard = crate::test_env_guard();
    let body = json!({
        "model": "gpt-5.5",
        "instructions": "compact instructions",
        "input": "compact me",
        "prompt_cache_key": "pc_compact",
        "reasoning": { "effort": "low" },
        "text": { "verbosity": "low" },
        "tools": [],
        "parallel_tool_calls": false
    });
    let out = apply_request_overrides_with_service_tier_and_prompt_cache_key_scope(
        "/v1/responses/compact",
        serde_json::to_vec(&body).expect("serialize request body"),
        None,
        None,
        None,
        None,
        Some("pc_compact"),
        false,
    );
    let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output body");

    assert_eq!(
        value
            .get("prompt_cache_key")
            .and_then(serde_json::Value::as_str),
        Some("pc_compact")
    );
}

#[test]
fn responses_compact_injects_prompt_cache_key_from_thread_anchor_for_codex_backend() {
    let _guard = crate::test_env_guard();
    let body = json!({
        "model": "gpt-5.5",
        "instructions": "compact instructions",
        "input": "compact me",
        "reasoning": { "effort": "low" },
        "text": { "verbosity": "low" },
        "tools": [],
        "parallel_tool_calls": false
    });
    let out = apply_request_overrides_with_service_tier_and_forced_prompt_cache_key_scope(
        "/v1/responses/compact",
        serde_json::to_vec(&body).expect("serialize request body"),
        None,
        None,
        None,
        Some("https://chatgpt.com/backend-api/codex"),
        Some("pc_from_thread"),
        true,
    );
    let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output body");

    assert_eq!(
        value
            .get("prompt_cache_key")
            .and_then(serde_json::Value::as_str),
        Some("pc_from_thread")
    );
}

/// 函数 `responses_compact_defaults_parallel_tool_calls_to_false_for_codex_backend`
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
fn responses_compact_defaults_parallel_tool_calls_to_false_for_codex_backend() {
    let _guard = crate::test_env_guard();
    let body = json!({
        "model": "gpt-5.3-codex",
        "input": "compact me"
    });
    let out = apply_codex_compat_request_overrides(
        "/v1/responses/compact",
        serde_json::to_vec(&body).expect("serialize request body"),
        None,
        None,
        Some("https://chatgpt.com/backend-api/codex"),
    );
    let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output body");
    assert_eq!(
        value
            .get("parallel_tool_calls")
            .and_then(serde_json::Value::as_bool),
        Some(false)
    );
    assert!(value.get("include").is_none());
    assert!(value
        .get("tools")
        .and_then(serde_json::Value::as_array)
        .is_some());
}

#[test]
fn responses_compact_omits_missing_or_empty_instructions() {
    let _guard = crate::test_env_guard();

    for body in [
        json!({
            "model": "gpt-5.3-codex",
            "input": "compact me"
        }),
        json!({
            "model": "gpt-5.3-codex",
            "instructions": null,
            "input": "compact me"
        }),
        json!({
            "model": "gpt-5.3-codex",
            "instructions": "",
            "input": "compact me"
        }),
    ] {
        let out = apply_codex_compat_request_overrides(
            "/v1/responses/compact",
            serde_json::to_vec(&body).expect("serialize request body"),
            None,
            None,
            Some("https://chatgpt.com/backend-api/codex"),
        );
        let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output body");
        assert!(value.get("instructions").is_none());
    }
}

#[test]
fn responses_compact_default_path_keeps_normalized_service_tier_for_codex_backend() {
    let _guard = crate::test_env_guard();
    let body = json!({
        "model": "gpt-5.4",
        "input": "compact me",
        "service_tier": "fast"
    });
    let out = apply_request_overrides(
        "/v1/responses/compact",
        serde_json::to_vec(&body).expect("serialize request body"),
        None,
        None,
        Some("https://chatgpt.com/backend-api/codex"),
    );
    let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output body");
    assert_eq!(
        value
            .get("service_tier")
            .and_then(serde_json::Value::as_str),
        Some("priority")
    );
}

/// 函数 `responses_omits_include_when_reasoning_missing_for_codex_backend`
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
fn responses_omits_include_when_reasoning_missing_for_codex_backend() {
    let _guard = crate::test_env_guard();
    let body = json!({
        "model": "gpt-5.3-codex",
        "input": "hello"
    });
    let out = apply_request_overrides(
        "/v1/responses",
        serde_json::to_vec(&body).expect("serialize request body"),
        None,
        None,
        Some("https://chatgpt.com/backend-api/codex"),
    );
    let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output body");
    assert!(value.get("include").is_none());
}

/// 函数 `responses_codex_compat_defaults_parallel_tool_calls_false_when_tools_are_present`
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
fn responses_codex_compat_defaults_parallel_tool_calls_false_when_tools_are_present() {
    let _guard = crate::test_env_guard();
    let body = json!({
        "model": "gpt-5.3-codex",
        "input": "hello",
        "tools": [{ "type": "function", "name": "ping", "parameters": { "type": "object", "properties": {} } }]
    });
    let out = apply_codex_compat_request_overrides(
        "/v1/responses",
        serde_json::to_vec(&body).expect("serialize request body"),
        None,
        None,
        Some("https://chatgpt.com/backend-api/codex"),
    );
    let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output body");
    assert_eq!(
        value
            .get("parallel_tool_calls")
            .and_then(serde_json::Value::as_bool),
        Some(false)
    );
}

/// 函数 `responses_passthrough_for_non_codex_upstream`
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
fn responses_passthrough_for_non_codex_upstream() {
    let _guard = crate::test_env_guard();
    let body = json!({
        "model": "gpt-4.1",
        "input": "hello",
        "stream": false,
        "store": true,
        "service_tier": "default",
        "user": "cherry-studio",
        "encrypted_content": "gAAA_test_payload",
        "unknown_field": "drop-me"
    });
    let out = apply_request_overrides(
        "/v1/responses",
        serde_json::to_vec(&body).expect("serialize request body"),
        None,
        None,
        Some("https://api.openai.com/v1"),
    );
    let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output body");
    assert_eq!(
        value.get("stream").and_then(serde_json::Value::as_bool),
        Some(false)
    );
    assert_eq!(
        value.get("store").and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        value.get("input").and_then(serde_json::Value::as_str),
        Some("hello")
    );
    assert!(value.get("service_tier").is_some());
    assert!(value.get("user").is_some());
    assert!(value.get("encrypted_content").is_some());
    assert!(value.get("unknown_field").is_some());
}

#[test]
fn responses_apply_global_model_forward_rules_when_platform_key_not_bound() {
    let _guard = crate::test_env_guard();
    let original_rules = crate::gateway::current_model_forward_rules();
    crate::gateway::set_model_forward_rules("spark*=gpt-5.4-mini")
        .expect("set model forward rules");

    let body = json!({
        "model": "spark",
        "input": "hello"
    });
    let out = apply_codex_compat_request_overrides(
        "/v1/responses",
        serde_json::to_vec(&body).expect("serialize request body"),
        None,
        None,
        Some("https://chatgpt.com/backend-api/codex"),
    );
    let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output body");

    assert_eq!(
        value.get("model").and_then(serde_json::Value::as_str),
        Some("gpt-5.4-mini")
    );

    let _ = crate::gateway::set_model_forward_rules(original_rules.as_str());
}

#[test]
fn responses_platform_key_bound_model_overrides_global_model_forward_rules() {
    let _guard = crate::test_env_guard();
    let original_rules = crate::gateway::current_model_forward_rules();
    crate::gateway::set_model_forward_rules("spark*=gpt-5.4-mini")
        .expect("set model forward rules");

    let body = json!({
        "model": "spark",
        "input": "hello"
    });
    let out = apply_request_overrides(
        "/v1/responses",
        serde_json::to_vec(&body).expect("serialize request body"),
        Some("gpt-5.4"),
        None,
        Some("https://api.openai.com/v1"),
    );
    let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output body");

    assert_eq!(
        value.get("model").and_then(serde_json::Value::as_str),
        Some("gpt-5.4")
    );

    let _ = crate::gateway::set_model_forward_rules(original_rules.as_str());
}

#[test]
fn responses_platform_key_bound_spark_model_keeps_original_slug() {
    let _guard = crate::test_env_guard();
    let original_rules = crate::gateway::current_model_forward_rules();
    let _ = crate::gateway::set_model_forward_rules("");

    let body = json!({
        "model": "gpt-5.3-codex-spark",
        "input": "hello"
    });
    let out = apply_request_overrides(
        "/v1/responses",
        serde_json::to_vec(&body).expect("serialize request body"),
        Some("gpt-5.3-codex-spark"),
        None,
        Some("https://api.openai.com/v1"),
    );
    let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output body");

    assert_eq!(
        value.get("model").and_then(serde_json::Value::as_str),
        Some("gpt-5.3-codex-spark")
    );

    let _ = crate::gateway::set_model_forward_rules(original_rules.as_str());
}

#[test]
fn responses_compact_applies_compact_model_forward_rules() {
    let _guard = crate::test_env_guard();
    let original_rules = crate::gateway::current_compact_model_forward_rules();
    let _ = crate::gateway::set_compact_model_forward_rules("");
    crate::gateway::set_compact_model_forward_rules("gpt-5.4=gpt-5.4-openai-compact")
        .expect("set compact model forward rules");

    let body = json!({
        "model": "gpt-5.4",
        "input": "compact me"
    });
    let out = apply_request_overrides(
        "/v1/responses/compact",
        serde_json::to_vec(&body).expect("serialize request body"),
        None,
        None,
        Some("https://chatgpt.com/backend-api/codex"),
    );
    let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output body");

    assert_eq!(
        value.get("model").and_then(serde_json::Value::as_str),
        Some("gpt-5.4-openai-compact")
    );

    let _ = crate::gateway::set_compact_model_forward_rules(original_rules.as_str());
}

#[test]
fn compact_upstream_url_can_be_mapped_to_chat_completions() {
    let _guard = crate::test_env_guard();
    let _compact_api_path = RuntimeEnvGuard::set(COMPACT_API_PATH_ENV, "/v1/chat/completions");

    let (url, url_alt) = compute_upstream_url(
        "https://chatgpt.com/backend-api/codex",
        "/v1/responses/compact",
    );

    assert_eq!(
        url,
        "https://chatgpt.com/backend-api/codex/chat/completions"
    );
    assert_eq!(
        url_alt.as_deref(),
        Some("https://chatgpt.com/backend-api/codex/v1/chat/completions")
    );
}

#[test]
fn responses_compact_bridges_body_to_chat_completions_when_configured() {
    let _guard = crate::test_env_guard();
    let _compact_api_path = RuntimeEnvGuard::set(COMPACT_API_PATH_ENV, "/v1/chat/completions");

    let body = json!({
        "model": "gpt-5.4",
        "instructions": "compress this thread",
        "input": "hello compact",
        "tools": [],
        "parallel_tool_calls": false,
        "service_tier": "priority"
    });
    let out = apply_codex_compat_request_overrides(
        "/v1/responses/compact",
        serde_json::to_vec(&body).expect("serialize request body"),
        None,
        Some("low"),
        Some("https://chatgpt.com/backend-api/codex"),
    );
    let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output body");

    assert!(value
        .get("messages")
        .and_then(serde_json::Value::as_array)
        .is_some());
    assert!(value.get("input").is_none());
    assert!(value.get("instructions").is_none());
    assert_eq!(
        value
            .get("reasoning_effort")
            .and_then(serde_json::Value::as_str),
        Some("low")
    );
}

/// 函数 `non_matching_endpoint_keeps_non_json_body`
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
fn non_matching_endpoint_keeps_non_json_body() {
    let body = b"foo=1&bar=2".to_vec();
    let out = apply_request_overrides("/v1/non-standard", body.clone(), None, None, None);
    assert_eq!(out, body);
}
