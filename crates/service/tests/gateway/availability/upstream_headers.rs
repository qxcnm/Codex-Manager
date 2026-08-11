use super::*;
use crate::gateway::{build_codex_compact_upstream_headers, CodexCompactUpstreamHeaderInput};
use std::sync::MutexGuard;

const OPENAI_ORGANIZATION_ENV: &str = "OPENAI_ORGANIZATION";
const OPENAI_PROJECT_ENV: &str = "OPENAI_PROJECT";

/// 函数 `header_runtime_scope`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 返回函数执行结果
fn header_runtime_scope() -> (MutexGuard<'static, ()>, GatewayHeaderRuntimeRestore) {
    let guard = crate::test_env_guard();
    let restore = GatewayHeaderRuntimeRestore::capture();
    std::env::remove_var(OPENAI_ORGANIZATION_ENV);
    std::env::remove_var(OPENAI_PROJECT_ENV);
    let _ = crate::set_gateway_originator("codex_cli_rs");
    let _ = crate::set_gateway_residency_requirement(None);
    (guard, restore)
}

struct GatewayHeaderRuntimeRestore {
    originator: String,
    residency_requirement: Option<String>,
    openai_organization: Option<String>,
    openai_project: Option<String>,
}

impl GatewayHeaderRuntimeRestore {
    /// 函数 `capture`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// 无
    ///
    /// # 返回
    /// 返回函数执行结果
    fn capture() -> Self {
        Self {
            originator: crate::current_gateway_originator(),
            residency_requirement: crate::current_gateway_residency_requirement(),
            openai_organization: std::env::var(OPENAI_ORGANIZATION_ENV).ok(),
            openai_project: std::env::var(OPENAI_PROJECT_ENV).ok(),
        }
    }
}

fn restore_env_var(name: &str, value: Option<&str>) {
    match value {
        Some(value) => std::env::set_var(name, value),
        None => std::env::remove_var(name),
    }
}

impl Drop for GatewayHeaderRuntimeRestore {
    /// 函数 `drop`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    ///
    /// # 返回
    /// 无
    fn drop(&mut self) {
        let _ = crate::set_gateway_originator(&self.originator);
        let _ = crate::set_gateway_residency_requirement(self.residency_requirement.as_deref());
        restore_env_var(OPENAI_ORGANIZATION_ENV, self.openai_organization.as_deref());
        restore_env_var(OPENAI_PROJECT_ENV, self.openai_project.as_deref());
    }
}

/// 函数 `find_header`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - headers: 参数 headers
/// - name: 参数 name
///
/// # 返回
/// 返回函数执行结果
fn find_header(headers: &[(String, String)], name: &str) -> Option<String> {
    headers
        .iter()
        .find(|(header_name, _)| header_name.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.clone())
}

/// 函数 `codex_header_profile_sets_required_headers_for_stream`
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
fn codex_header_profile_sets_required_headers_for_stream() {
    let (_guard, _restore) = header_runtime_scope();
    let expected_version = crate::current_gateway_user_agent_version();
    let passthrough = vec![(
        "x-codex-other-limit-name".to_string(),
        "promo_header_main".to_string(),
    )];
    let headers = build_codex_upstream_headers(CodexUpstreamHeaderInput {
        auth_token: "token-123",
        chatgpt_account_id: Some("workspace-1"),
        incoming_user_agent: None,
        incoming_originator: None,
        preserve_client_identity: false,
        incoming_session_id: None,
        incoming_window_id: None,
        incoming_client_request_id: Some("client-req-1"),
        incoming_subagent: Some("review"),
        incoming_beta_features: Some("reasoning_summaries"),
        incoming_turn_metadata: Some("{\"workspace\":\"repo\"}"),
        incoming_parent_thread_id: Some("thread-parent-1"),
        incoming_responsesapi_include_timing_metrics: Some("true"),
        incoming_inference_call_id: Some("inference-call-1"),
        incoming_oai_attestation: Some("attestation-1"),
        passthrough_codex_headers: passthrough.as_slice(),
        fallback_session_id: None,
        incoming_turn_state: Some("turn-state"),
        include_turn_state: true,
        strip_session_affinity: false,
        has_body: true,
    });

    assert_eq!(
        find_header(&headers, "Authorization").as_deref(),
        Some("Bearer token-123")
    );
    assert_eq!(
        find_header(&headers, "ChatGPT-Account-ID").as_deref(),
        Some("workspace-1")
    );
    assert_eq!(
        find_header(&headers, "Content-Type").as_deref(),
        Some("application/json")
    );
    assert_eq!(
        find_header(&headers, "Accept").as_deref(),
        Some("text/event-stream")
    );
    assert!(find_header(&headers, "Connection").is_none());
    assert_eq!(
        find_header(&headers, "Version").as_deref(),
        Some(expected_version.as_str())
    );
    assert!(find_header(&headers, "User-Agent")
        .as_deref()
        .is_some_and(|value| value.contains(expected_version.as_str())));
    assert_eq!(
        find_header(&headers, "OpenAI-Beta").as_deref(),
        Some("responses=experimental")
    );
    assert_eq!(
        find_header(&headers, "x-responsesapi-include-timing-metrics").as_deref(),
        Some("true")
    );
    assert_eq!(
        find_header(&headers, "x-codex-inference-call-id").as_deref(),
        Some("inference-call-1")
    );
    assert_eq!(
        find_header(&headers, "x-oai-attestation").as_deref(),
        Some("attestation-1")
    );
    assert_eq!(
        find_header(&headers, "Originator").as_deref(),
        Some("codex_cli_rs")
    );
    assert_eq!(
        find_header(&headers, "x-client-request-id").as_deref(),
        Some("client-req-1")
    );
    assert_eq!(
        find_header(&headers, "x-openai-subagent").as_deref(),
        Some("review")
    );
    assert_eq!(
        find_header(&headers, "x-codex-parent-thread-id").as_deref(),
        Some("thread-parent-1")
    );
    assert!(find_header(&headers, "x-codex-other-limit-name").is_none());
    assert_eq!(
        find_header(&headers, "x-codex-beta-features").as_deref(),
        Some("reasoning_summaries")
    );
    assert_eq!(
        find_header(&headers, "x-codex-turn-metadata").as_deref(),
        Some("{\"workspace\":\"repo\"}")
    );
    assert_eq!(
        find_header(&headers, "x-codex-turn-state").as_deref(),
        Some("turn-state")
    );
    assert!(find_header(&headers, "Conversation_id").is_none());
    assert!(find_header(&headers, "session_id").is_none());
    assert!(find_header(&headers, "x-codex-window-id").is_none());
}

/// 函数 `codex_header_profile_uses_json_accept_for_non_stream`
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
fn codex_header_profile_uses_json_accept_for_non_stream() {
    let (_guard, _restore) = header_runtime_scope();
    let expected_version = crate::current_gateway_user_agent_version();
    let headers = build_codex_upstream_headers(CodexUpstreamHeaderInput {
        auth_token: "token-456",
        chatgpt_account_id: None,
        incoming_user_agent: None,
        incoming_originator: None,
        preserve_client_identity: false,
        incoming_session_id: None,
        incoming_window_id: None,
        incoming_client_request_id: None,
        incoming_subagent: None,
        incoming_beta_features: None,
        incoming_turn_metadata: None,
        incoming_parent_thread_id: None,
        incoming_responsesapi_include_timing_metrics: None,
        incoming_inference_call_id: None,
        incoming_oai_attestation: None,
        passthrough_codex_headers: &[],
        fallback_session_id: None,
        incoming_turn_state: None,
        include_turn_state: true,
        strip_session_affinity: false,
        has_body: false,
    });

    assert_eq!(
        find_header(&headers, "Accept").as_deref(),
        Some("text/event-stream")
    );
    assert!(find_header(&headers, "Content-Type").is_none());
    assert_eq!(
        find_header(&headers, "OpenAI-Beta").as_deref(),
        Some("responses=experimental")
    );
    assert!(find_header(&headers, "x-responsesapi-include-timing-metrics").is_none());
    assert_eq!(
        find_header(&headers, "Version").as_deref(),
        Some(expected_version.as_str())
    );
    assert!(find_header(&headers, "User-Agent")
        .as_deref()
        .is_some_and(|value| value.contains(expected_version.as_str())));
}

/// 函数 `codex_compact_header_profile_matches_remote_compact_shape`
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
fn codex_compact_header_profile_matches_remote_compact_shape() {
    let (_guard, _restore) = header_runtime_scope();
    let expected_version = crate::current_gateway_user_agent_version();
    let passthrough = vec![(
        "x-codex-other-limit-name".to_string(),
        "promo_header_compact".to_string(),
    )];
    let headers = build_codex_compact_upstream_headers(CodexCompactUpstreamHeaderInput {
        auth_token: "token-compact",
        chatgpt_account_id: Some("workspace-compact"),
        installation_id: Some("install-compact"),
        incoming_user_agent: None,
        incoming_originator: None,
        preserve_client_identity: false,
        incoming_session_id: Some("session-compact"),
        thread_id: Some("thread-compact"),
        incoming_window_id: Some("session-compact:7"),
        incoming_subagent: Some("compact"),
        incoming_parent_thread_id: Some("thread-parent-compact"),
        incoming_oai_attestation: Some("attestation-compact"),
        passthrough_codex_headers: passthrough.as_slice(),
        fallback_session_id: Some("fallback-session"),
        strip_session_affinity: false,
        has_body: true,
    });

    assert_eq!(
        find_header(&headers, "Authorization").as_deref(),
        Some("Bearer token-compact")
    );
    assert_eq!(
        find_header(&headers, "ChatGPT-Account-ID").as_deref(),
        Some("workspace-compact")
    );
    assert_eq!(
        find_header(&headers, "x-codex-installation-id").as_deref(),
        Some("install-compact")
    );
    assert_eq!(
        find_header(&headers, "x-oai-attestation").as_deref(),
        Some("attestation-compact")
    );
    assert_eq!(
        find_header(&headers, "Content-Type").as_deref(),
        Some("application/json")
    );
    assert_eq!(
        find_header(&headers, "Accept").as_deref(),
        Some("application/json")
    );
    assert_eq!(
        find_header(&headers, "Version").as_deref(),
        Some(expected_version.as_str())
    );
    assert_eq!(
        find_header(&headers, "session_id").as_deref(),
        Some("session-compact")
    );
    assert_eq!(
        find_header(&headers, "thread_id").as_deref(),
        Some("thread-compact")
    );
    assert!(find_header(&headers, "Cookie").is_none());
    assert_eq!(
        find_header(&headers, "OpenAI-Beta").as_deref(),
        Some("responses=experimental")
    );
    assert_eq!(
        find_header(&headers, "Originator").as_deref(),
        Some("codex_cli_rs")
    );
    assert!(find_header(&headers, "User-Agent")
        .as_deref()
        .is_some_and(|value| value.contains(expected_version.as_str())));
    assert_eq!(
        find_header(&headers, "x-openai-subagent").as_deref(),
        Some("compact")
    );
    assert_eq!(
        find_header(&headers, "x-codex-parent-thread-id").as_deref(),
        Some("thread-parent-compact")
    );
    assert!(find_header(&headers, "x-codex-other-limit-name").is_none());
    assert!(find_header(&headers, "Conversation_id").is_none());
    assert!(find_header(&headers, "x-codex-turn-state").is_none());
    assert_eq!(
        find_header(&headers, "x-codex-window-id").as_deref(),
        Some("session-compact:7")
    );
}

/// 函数 `codex_compact_header_profile_omits_subagent_without_explicit_source`
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
fn codex_compact_header_profile_omits_subagent_without_explicit_source() {
    let (_guard, _restore) = header_runtime_scope();
    let headers = build_codex_compact_upstream_headers(CodexCompactUpstreamHeaderInput {
        auth_token: "token-compact-default",
        chatgpt_account_id: None,
        installation_id: None,
        incoming_user_agent: None,
        incoming_originator: None,
        preserve_client_identity: false,
        incoming_session_id: Some("session-compact-default"),
        thread_id: None,
        incoming_window_id: None,
        incoming_subagent: None,
        incoming_parent_thread_id: None,
        incoming_oai_attestation: None,
        passthrough_codex_headers: &[],
        fallback_session_id: Some("fallback-session"),
        strip_session_affinity: false,
        has_body: true,
    });

    assert!(find_header(&headers, "x-openai-subagent").is_none());
    assert!(find_header(&headers, "x-codex-installation-id").is_none());
}

/// 函数 `codex_compact_header_profile_omits_session_without_thread_anchor`
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
fn codex_compact_header_profile_omits_session_without_thread_anchor() {
    let (_guard, _restore) = header_runtime_scope();
    let headers = build_codex_compact_upstream_headers(CodexCompactUpstreamHeaderInput {
        auth_token: "token-compact-no-session",
        chatgpt_account_id: None,
        installation_id: None,
        incoming_user_agent: None,
        incoming_originator: None,
        preserve_client_identity: false,
        incoming_session_id: None,
        thread_id: None,
        incoming_window_id: None,
        incoming_subagent: None,
        incoming_parent_thread_id: None,
        incoming_oai_attestation: None,
        passthrough_codex_headers: &[],
        fallback_session_id: None,
        strip_session_affinity: false,
        has_body: true,
    });

    assert!(find_header(&headers, "session_id").is_none());
}

/// 函数 `codex_header_profile_uses_dynamic_originator_and_residency_requirement`
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
fn codex_header_profile_uses_dynamic_originator_and_residency_requirement() {
    let (_guard, _restore) = header_runtime_scope();
    std::env::set_var(OPENAI_ORGANIZATION_ENV, "org_dynamic");
    std::env::set_var(OPENAI_PROJECT_ENV, "proj_dynamic");
    crate::set_gateway_originator("codex_cli_rs_e2e").expect("set gateway originator");
    crate::set_gateway_residency_requirement(Some("us"))
        .expect("set gateway residency requirement");

    let expected_version = crate::current_gateway_user_agent_version();
    let headers = build_codex_upstream_headers(CodexUpstreamHeaderInput {
        auth_token: "token-dynamic",
        chatgpt_account_id: Some("workspace-dynamic"),
        incoming_user_agent: None,
        incoming_originator: None,
        preserve_client_identity: false,
        incoming_session_id: None,
        incoming_window_id: None,
        incoming_client_request_id: None,
        incoming_subagent: None,
        incoming_beta_features: None,
        incoming_turn_metadata: None,
        incoming_parent_thread_id: None,
        incoming_responsesapi_include_timing_metrics: None,
        incoming_inference_call_id: None,
        incoming_oai_attestation: None,
        passthrough_codex_headers: &[],
        fallback_session_id: None,
        incoming_turn_state: None,
        include_turn_state: true,
        strip_session_affinity: false,
        has_body: true,
    });

    assert_eq!(
        find_header(&headers, "Originator").as_deref(),
        Some("codex_cli_rs_e2e")
    );
    assert_eq!(
        find_header(&headers, "x-openai-internal-codex-residency").as_deref(),
        Some("us")
    );
    assert_eq!(
        find_header(&headers, "Version").as_deref(),
        Some(expected_version.as_str())
    );
    assert!(find_header(&headers, "OpenAI-Organization").is_none());
    assert!(find_header(&headers, "OpenAI-Project").is_none());
    assert_eq!(
        find_header(&headers, "ChatGPT-Account-ID").as_deref(),
        Some("workspace-dynamic")
    );
    assert!(find_header(&headers, "User-Agent")
        .as_deref()
        .is_some_and(|value| value.contains(expected_version.as_str())));
}

/// 函数 `codex_header_profile_regenerates_session_on_failover`
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
fn codex_header_profile_regenerates_session_on_failover() {
    let (_guard, _restore) = header_runtime_scope();
    let passthrough = vec![(
        "x-codex-other-limit-name".to_string(),
        "promo_header_failover".to_string(),
    )];
    let headers = build_codex_upstream_headers(CodexUpstreamHeaderInput {
        auth_token: "token-789",
        chatgpt_account_id: None,
        incoming_user_agent: None,
        incoming_originator: None,
        preserve_client_identity: false,
        incoming_session_id: Some("sticky-session"),
        incoming_window_id: Some("sticky-session:7"),
        incoming_client_request_id: None,
        incoming_subagent: None,
        incoming_beta_features: None,
        incoming_turn_metadata: None,
        incoming_parent_thread_id: None,
        incoming_responsesapi_include_timing_metrics: None,
        incoming_inference_call_id: None,
        incoming_oai_attestation: None,
        passthrough_codex_headers: passthrough.as_slice(),
        fallback_session_id: Some("fallback-session"),
        incoming_turn_state: Some("sticky-turn"),
        include_turn_state: true,
        strip_session_affinity: true,
        has_body: true,
    });

    assert_ne!(
        find_header(&headers, "session_id").as_deref(),
        Some("sticky-session")
    );
    assert_eq!(
        find_header(&headers, "session_id").as_deref(),
        Some("fallback-session")
    );
    assert_eq!(
        find_header(&headers, "x-codex-window-id").as_deref(),
        Some("fallback-session:0")
    );
    assert!(find_header(&headers, "x-codex-turn-state").is_none());
    assert!(find_header(&headers, "x-codex-other-limit-name").is_none());
    assert!(find_header(&headers, "Conversation_id").is_none());
}

/// 函数 `codex_header_profile_uses_fallback_session_when_incoming_missing`
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
fn codex_header_profile_uses_fallback_session_when_incoming_missing() {
    let (_guard, _restore) = header_runtime_scope();
    let headers = build_codex_upstream_headers(CodexUpstreamHeaderInput {
        auth_token: "token-fallback",
        chatgpt_account_id: None,
        incoming_user_agent: None,
        incoming_originator: None,
        preserve_client_identity: false,
        incoming_session_id: None,
        incoming_window_id: None,
        incoming_client_request_id: None,
        incoming_subagent: None,
        incoming_beta_features: None,
        incoming_turn_metadata: None,
        incoming_parent_thread_id: None,
        incoming_responsesapi_include_timing_metrics: None,
        incoming_inference_call_id: None,
        incoming_oai_attestation: None,
        passthrough_codex_headers: &[],
        fallback_session_id: Some("fallback-session"),
        incoming_turn_state: None,
        include_turn_state: true,
        strip_session_affinity: false,
        has_body: true,
    });

    assert_eq!(
        find_header(&headers, "session_id").as_deref(),
        Some("fallback-session")
    );
    assert_eq!(
        find_header(&headers, "x-codex-window-id").as_deref(),
        Some("fallback-session:0")
    );
    assert!(find_header(&headers, "x-client-request-id").is_none());
}

/// 函数 `codex_header_profile_does_not_forward_conversation_header_even_with_fallback`
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
fn codex_header_profile_does_not_forward_conversation_header_even_with_fallback() {
    let (_guard, _restore) = header_runtime_scope();
    let headers = build_codex_upstream_headers(CodexUpstreamHeaderInput {
        auth_token: "token-fallback-conv",
        chatgpt_account_id: None,
        incoming_user_agent: None,
        incoming_originator: None,
        preserve_client_identity: false,
        incoming_session_id: None,
        incoming_window_id: None,
        incoming_client_request_id: None,
        incoming_subagent: None,
        incoming_beta_features: None,
        incoming_turn_metadata: None,
        incoming_parent_thread_id: None,
        incoming_responsesapi_include_timing_metrics: None,
        incoming_inference_call_id: None,
        incoming_oai_attestation: None,
        passthrough_codex_headers: &[],
        fallback_session_id: Some("fallback-session"),
        incoming_turn_state: None,
        include_turn_state: true,
        strip_session_affinity: false,
        has_body: true,
    });

    assert!(find_header(&headers, "Conversation_id").is_none());
}

/// 函数 `codex_header_profile_skips_account_header_when_disabled`
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
fn codex_header_profile_skips_account_header_when_disabled() {
    let (_guard, _restore) = header_runtime_scope();
    let headers = build_codex_upstream_headers(CodexUpstreamHeaderInput {
        auth_token: "token-no-acc",
        chatgpt_account_id: None,
        incoming_user_agent: None,
        incoming_originator: None,
        preserve_client_identity: false,
        incoming_session_id: None,
        incoming_window_id: None,
        incoming_client_request_id: None,
        incoming_subagent: None,
        incoming_beta_features: None,
        incoming_turn_metadata: None,
        incoming_parent_thread_id: None,
        incoming_responsesapi_include_timing_metrics: None,
        incoming_inference_call_id: None,
        incoming_oai_attestation: None,
        passthrough_codex_headers: &[],
        fallback_session_id: None,
        incoming_turn_state: None,
        include_turn_state: true,
        strip_session_affinity: false,
        has_body: true,
    });

    assert!(find_header(&headers, "ChatGPT-Account-ID").is_none());
}

/// 函数 `codex_header_profile_can_disable_affinity_headers`
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
fn codex_header_profile_can_disable_affinity_headers() {
    let (_guard, _restore) = header_runtime_scope();
    let headers = build_codex_upstream_headers(CodexUpstreamHeaderInput {
        auth_token: "token-no-affinity",
        chatgpt_account_id: None,
        incoming_user_agent: None,
        incoming_originator: None,
        preserve_client_identity: false,
        incoming_session_id: Some("sticky-session"),
        incoming_window_id: None,
        incoming_client_request_id: None,
        incoming_subagent: None,
        incoming_beta_features: None,
        incoming_turn_metadata: None,
        incoming_parent_thread_id: None,
        incoming_responsesapi_include_timing_metrics: None,
        incoming_inference_call_id: None,
        incoming_oai_attestation: None,
        passthrough_codex_headers: &[],
        fallback_session_id: None,
        incoming_turn_state: Some("sticky-turn"),
        include_turn_state: false,
        strip_session_affinity: false,
        has_body: true,
    });

    assert_eq!(
        find_header(&headers, "OpenAI-Beta").as_deref(),
        Some("responses=experimental")
    );
    assert!(find_header(&headers, "x-codex-turn-state").is_none());
    assert!(find_header(&headers, "Conversation_id").is_none());
    assert_eq!(
        find_header(&headers, "session_id").as_deref(),
        Some("sticky-session")
    );
    assert_eq!(
        find_header(&headers, "x-codex-window-id").as_deref(),
        Some("sticky-session:0")
    );
    assert!(find_header(&headers, "x-client-request-id").is_none());
}

/// 函数 `codex_header_profile_does_not_invent_client_request_id_on_failover`
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
fn codex_header_profile_does_not_invent_client_request_id_on_failover() {
    let (_guard, _restore) = header_runtime_scope();
    let headers = build_codex_upstream_headers(CodexUpstreamHeaderInput {
        auth_token: "token-failover-stable",
        chatgpt_account_id: None,
        incoming_user_agent: None,
        incoming_originator: None,
        preserve_client_identity: false,
        incoming_session_id: Some("sticky-session"),
        incoming_window_id: None,
        incoming_client_request_id: None,
        incoming_subagent: None,
        incoming_beta_features: None,
        incoming_turn_metadata: None,
        incoming_parent_thread_id: None,
        incoming_responsesapi_include_timing_metrics: None,
        incoming_inference_call_id: None,
        incoming_oai_attestation: None,
        passthrough_codex_headers: &[],
        fallback_session_id: Some("fallback-session"),
        incoming_turn_state: Some("sticky-turn"),
        include_turn_state: true,
        strip_session_affinity: true,
        has_body: true,
    });

    assert_ne!(
        find_header(&headers, "session_id").as_deref(),
        Some("sticky-session")
    );
    assert_eq!(
        find_header(&headers, "x-codex-window-id").as_deref(),
        Some("fallback-session:0")
    );
    assert!(find_header(&headers, "x-client-request-id").is_none());
}
