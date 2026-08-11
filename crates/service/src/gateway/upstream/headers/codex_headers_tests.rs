use super::{
    authorization_header_value, build_codex_compact_upstream_headers, build_codex_upstream_headers,
    resolve_codex_installation_id,
};
use crate::gateway::{
    set_codex_user_agent_version, set_originator, CodexCompactUpstreamHeaderInput,
    CodexUpstreamHeaderInput,
};
use std::time::{SystemTime, UNIX_EPOCH};

const CODEXMANAGER_DB_PATH_ENV: &str = "CODEXMANAGER_DB_PATH";

#[test]
fn agent_identity_authorization_is_not_wrapped_as_bearer() {
    assert_eq!(
        authorization_header_value("AgentAssertion signed-envelope"),
        "AgentAssertion signed-envelope"
    );
    assert_eq!(
        authorization_header_value("plain-token"),
        "Bearer plain-token"
    );
}

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
        .join(format!(
            "codexmanager-codex-headers-{}-{}-{}.db",
            label,
            std::process::id(),
            nanos
        ))
        .to_string_lossy()
        .into_owned()
}

/// 函数 `header_value`
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
fn header_value<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(header_name, _)| header_name.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

#[test]
fn resolve_codex_installation_id_prefers_incoming_header() {
    let _guard = crate::test_env_guard();

    assert_eq!(
        resolve_codex_installation_id(Some(" install-from-client ")).as_deref(),
        Some("install-from-client")
    );
}

#[test]
fn resolve_codex_installation_id_uses_persisted_fallback_when_incoming_missing() {
    let _guard = crate::test_env_guard();
    let _db_guard = RuntimeEnvGuard::set(
        CODEXMANAGER_DB_PATH_ENV,
        isolated_db_path("compact-installation-id").as_str(),
    );

    let first = resolve_codex_installation_id(None).expect("first installation id");
    let second = resolve_codex_installation_id(None).expect("second installation id");

    assert_eq!(first, second);
    assert_eq!(first.len(), 36);
    assert_eq!(first.as_bytes().get(14).copied(), Some(b'4'));
}

/// 函数 `build_codex_upstream_headers_keeps_final_affinity_shape`
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
fn build_codex_upstream_headers_keeps_final_affinity_shape() {
    let _guard = crate::test_env_guard();
    let _ = set_originator("codex_cli_rs_tests").expect("set originator");
    let _ = set_codex_user_agent_version("0.999.0").expect("set ua version");
    let passthrough = vec![(
        "x-codex-other-limit-name".to_string(),
        "promo_header_a".to_string(),
    )];

    let headers = build_codex_upstream_headers(CodexUpstreamHeaderInput {
        auth_token: "token-123",
        chatgpt_account_id: Some("account-123"),
        incoming_user_agent: None,
        incoming_originator: None,
        preserve_client_identity: false,
        incoming_session_id: Some("conversation-anchor"),
        incoming_window_id: Some("conversation-anchor:7"),
        incoming_client_request_id: Some("conversation-anchor"),
        incoming_subagent: Some("subagent-a"),
        incoming_beta_features: Some("beta-a"),
        incoming_turn_metadata: Some("meta-a"),
        incoming_parent_thread_id: Some("thread-parent-a"),
        incoming_responsesapi_include_timing_metrics: Some("true"),
        incoming_inference_call_id: Some("inference-call-a"),
        incoming_oai_attestation: Some("attestation-a"),
        passthrough_codex_headers: passthrough.as_slice(),
        fallback_session_id: Some("conversation-anchor"),
        incoming_turn_state: Some("turn-state-a"),
        include_turn_state: true,
        strip_session_affinity: false,
        has_body: true,
    });

    assert_eq!(
        header_value(&headers, "Authorization"),
        Some("Bearer token-123")
    );
    assert_eq!(
        header_value(&headers, "ChatGPT-Account-ID"),
        Some("account-123")
    );
    assert_eq!(
        header_value(&headers, "Content-Type"),
        Some("application/json")
    );
    assert_eq!(header_value(&headers, "Accept"), Some("text/event-stream"));
    assert_eq!(
        header_value(&headers, "OpenAI-Beta"),
        Some("responses=experimental")
    );
    assert_eq!(
        header_value(&headers, "x-responsesapi-include-timing-metrics"),
        Some("true")
    );
    assert_eq!(
        header_value(&headers, "x-codex-inference-call-id"),
        Some("inference-call-a")
    );
    assert_eq!(
        header_value(&headers, "x-oai-attestation"),
        Some("attestation-a")
    );
    let expected_user_agent_prefix =
        format!("{}/0.999.0", crate::gateway::current_wire_originator());
    assert_eq!(
        header_value(&headers, "User-Agent")
            .map(|value| value.starts_with(expected_user_agent_prefix.as_str())),
        Some(true)
    );
    assert_eq!(
        header_value(&headers, "originator"),
        Some("codex_cli_rs_tests")
    );
    assert_eq!(header_value(&headers, "version"), Some("0.999.0"));
    assert_eq!(header_value(&headers, "OpenAI-Organization"), None);
    assert_eq!(header_value(&headers, "OpenAI-Project"), None);
    assert_eq!(
        header_value(&headers, "x-client-request-id"),
        Some("conversation-anchor")
    );
    assert_eq!(
        header_value(&headers, "session_id"),
        Some("conversation-anchor")
    );
    assert_eq!(
        header_value(&headers, "x-codex-window-id"),
        Some("conversation-anchor:7")
    );
    assert_eq!(
        header_value(&headers, "x-codex-turn-state"),
        Some("turn-state-a")
    );
    assert_eq!(
        header_value(&headers, "x-codex-parent-thread-id"),
        Some("thread-parent-a")
    );
    assert_eq!(header_value(&headers, "x-codex-other-limit-name"), None);
}

/// 函数 `build_codex_upstream_headers_clears_turn_state_when_affinity_diverges`
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
fn build_codex_upstream_headers_clears_turn_state_when_affinity_diverges() {
    let _guard = crate::test_env_guard();
    let _ = set_originator("codex_cli_rs_tests").expect("set originator");
    let _ = set_codex_user_agent_version("0.999.1").expect("set ua version");
    let passthrough = vec![(
        "x-codex-other-limit-name".to_string(),
        "promo_header_b".to_string(),
    )];

    let headers = build_codex_upstream_headers(CodexUpstreamHeaderInput {
        auth_token: "token-456",
        chatgpt_account_id: None,
        incoming_user_agent: None,
        incoming_originator: None,
        preserve_client_identity: false,
        incoming_session_id: Some("conversation-anchor"),
        incoming_window_id: Some("conversation-anchor:9"),
        incoming_client_request_id: Some("conversation-anchor"),
        incoming_subagent: None,
        incoming_beta_features: None,
        incoming_turn_metadata: None,
        incoming_parent_thread_id: Some("thread-parent-b"),
        incoming_responsesapi_include_timing_metrics: None,
        incoming_inference_call_id: None,
        incoming_oai_attestation: None,
        passthrough_codex_headers: passthrough.as_slice(),
        fallback_session_id: Some("prompt-cache-anchor"),
        incoming_turn_state: None,
        include_turn_state: true,
        strip_session_affinity: false,
        has_body: false,
    });

    assert_eq!(header_value(&headers, "Accept"), Some("text/event-stream"));
    assert_eq!(
        header_value(&headers, "x-client-request-id"),
        Some("conversation-anchor")
    );
    assert_eq!(
        header_value(&headers, "session_id"),
        Some("conversation-anchor")
    );
    assert_eq!(
        header_value(&headers, "x-codex-window-id"),
        Some("conversation-anchor:9")
    );
    assert_eq!(header_value(&headers, "x-codex-turn-state"), None);
    assert_eq!(
        header_value(&headers, "x-codex-parent-thread-id"),
        Some("thread-parent-b")
    );
    assert_eq!(header_value(&headers, "x-codex-other-limit-name"), None);
}

/// 函数 `build_codex_compact_upstream_headers_use_session_fallback_only`
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
fn build_codex_compact_upstream_headers_use_session_fallback_only() {
    let _guard = crate::test_env_guard();
    let _ = set_originator("codex_cli_rs_tests").expect("set originator");
    let _ = set_codex_user_agent_version("0.999.2").expect("set ua version");
    let passthrough = vec![(
        "x-codex-other-limit-name".to_string(),
        "promo_header_c".to_string(),
    )];

    let headers = build_codex_compact_upstream_headers(CodexCompactUpstreamHeaderInput {
        auth_token: "token-789",
        chatgpt_account_id: Some("account-compact"),
        installation_id: Some("install-compact-internal"),
        incoming_user_agent: None,
        incoming_originator: None,
        preserve_client_identity: false,
        incoming_session_id: None,
        thread_id: Some("thread-anchor-c"),
        incoming_window_id: Some("conversation-anchor:11"),
        incoming_subagent: Some("subagent-b"),
        incoming_parent_thread_id: Some("thread-parent-c"),
        incoming_oai_attestation: Some("attestation-compact"),
        passthrough_codex_headers: passthrough.as_slice(),
        fallback_session_id: Some("conversation-anchor"),
        strip_session_affinity: true,
        has_body: true,
    });

    assert_eq!(header_value(&headers, "Accept"), Some("application/json"));
    assert_eq!(
        header_value(&headers, "ChatGPT-Account-ID"),
        Some("account-compact")
    );
    assert_eq!(
        header_value(&headers, "x-codex-installation-id"),
        Some("install-compact-internal")
    );
    assert_eq!(
        header_value(&headers, "x-oai-attestation"),
        Some("attestation-compact")
    );
    assert_eq!(header_value(&headers, "x-client-request-id"), None);
    assert_eq!(
        header_value(&headers, "session_id"),
        Some("conversation-anchor")
    );
    assert_eq!(header_value(&headers, "thread_id"), Some("thread-anchor-c"));
    assert_eq!(
        header_value(&headers, "x-codex-window-id"),
        Some("conversation-anchor:0")
    );
    assert_eq!(header_value(&headers, "x-codex-turn-state"), None);
    assert_eq!(
        header_value(&headers, "OpenAI-Beta"),
        Some("responses=experimental")
    );
    assert_eq!(
        header_value(&headers, "x-responsesapi-include-timing-metrics"),
        None
    );
    assert_eq!(header_value(&headers, "version"), Some("0.999.2"));
    assert_eq!(
        header_value(&headers, "x-openai-subagent"),
        Some("subagent-b")
    );
    assert_eq!(
        header_value(&headers, "x-codex-parent-thread-id"),
        Some("thread-parent-c")
    );
    assert_eq!(header_value(&headers, "x-codex-other-limit-name"), None);
}

#[test]
fn build_codex_upstream_headers_rebuilds_mismatched_window_id_from_session() {
    let _guard = crate::test_env_guard();
    let _ = set_originator("codex_cli_rs_tests").expect("set originator");
    let _ = set_codex_user_agent_version("0.999.3").expect("set ua version");

    let headers = build_codex_upstream_headers(CodexUpstreamHeaderInput {
        auth_token: "token-window-fix",
        chatgpt_account_id: None,
        incoming_user_agent: None,
        incoming_originator: None,
        preserve_client_identity: false,
        incoming_session_id: Some("session-anchor"),
        incoming_window_id: Some("stale-window-anchor:9"),
        incoming_client_request_id: Some("request-anchor"),
        incoming_subagent: None,
        incoming_beta_features: None,
        incoming_turn_metadata: None,
        incoming_parent_thread_id: None,
        incoming_responsesapi_include_timing_metrics: None,
        incoming_inference_call_id: None,
        incoming_oai_attestation: None,
        passthrough_codex_headers: &[],
        fallback_session_id: Some("fallback-anchor"),
        incoming_turn_state: Some("turn-state-window-fix"),
        include_turn_state: true,
        strip_session_affinity: false,
        has_body: true,
    });

    assert_eq!(header_value(&headers, "session_id"), Some("session-anchor"));
    assert_eq!(
        header_value(&headers, "x-codex-window-id"),
        Some("session-anchor:0")
    );
}

#[test]
fn build_codex_upstream_headers_prefers_incoming_codex_identity() {
    let _guard = crate::test_env_guard();
    let _ = set_originator("codex_cli_rs_tests").expect("set originator");
    let _ = set_codex_user_agent_version("0.999.4").expect("set ua version");

    let headers = build_codex_upstream_headers(CodexUpstreamHeaderInput {
        auth_token: "token-ident",
        chatgpt_account_id: None,
        incoming_user_agent: Some("codex_sdk_ts/1.2.3 (Windows 11; x86_64) node"),
        incoming_originator: Some("codex_sdk_ts"),
        preserve_client_identity: false,
        incoming_session_id: Some("thread-ident"),
        incoming_window_id: Some("thread-ident:0"),
        incoming_client_request_id: Some("thread-ident"),
        incoming_subagent: None,
        incoming_beta_features: None,
        incoming_turn_metadata: None,
        incoming_parent_thread_id: None,
        incoming_responsesapi_include_timing_metrics: None,
        incoming_inference_call_id: None,
        incoming_oai_attestation: None,
        passthrough_codex_headers: &[],
        fallback_session_id: Some("thread-ident"),
        incoming_turn_state: None,
        include_turn_state: true,
        strip_session_affinity: false,
        has_body: true,
    });

    assert_eq!(header_value(&headers, "originator"), Some("codex_sdk_ts"));
    assert_eq!(
        header_value(&headers, "User-Agent"),
        Some("codex_sdk_ts/1.2.3 (Windows 11; x86_64) node")
    );
    assert_eq!(header_value(&headers, "version"), Some("1.2.3"));
}

#[test]
fn build_codex_upstream_headers_upgrades_obsolete_identity_version_header() {
    let _guard = crate::test_env_guard();
    let _ = set_originator("codex_cli_rs").expect("set originator");
    let _ = set_codex_user_agent_version("0.146.0").expect("set ua version");

    let headers = build_codex_upstream_headers(CodexUpstreamHeaderInput {
        auth_token: "token-old-client",
        chatgpt_account_id: None,
        incoming_user_agent: Some("codex_cli_rs/0.118.0 (Ubuntu 22.4.0; x86_64) xterm-256color"),
        incoming_originator: Some("codex_cli_rs"),
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
        fallback_session_id: Some("fallback-old-client"),
        incoming_turn_state: None,
        include_turn_state: true,
        strip_session_affinity: false,
        has_body: true,
    });

    assert_eq!(header_value(&headers, "originator"), Some("codex_cli_rs"));
    assert_eq!(
        header_value(&headers, "User-Agent"),
        Some("codex_cli_rs/0.118.0 (Ubuntu 22.4.0; x86_64) xterm-256color")
    );
    assert_eq!(header_value(&headers, "version"), Some("0.146.0"));
}

#[test]
fn build_codex_upstream_headers_rebuilds_unpaired_codex_identity() {
    let _guard = crate::test_env_guard();
    let _ = set_originator("codex_cli_rs").expect("set originator");
    let _ = set_codex_user_agent_version("0.999.6").expect("set ua version");

    let headers = build_codex_upstream_headers(CodexUpstreamHeaderInput {
        auth_token: "token-ident-mismatch",
        chatgpt_account_id: None,
        incoming_user_agent: Some("codex-tui/0.146.0 (Mac OS; arm64)"),
        incoming_originator: Some("codex_sdk_ts"),
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
        fallback_session_id: Some("fallback-ident"),
        incoming_turn_state: None,
        include_turn_state: true,
        strip_session_affinity: false,
        has_body: true,
    });

    assert_eq!(header_value(&headers, "originator"), Some("codex_cli_rs"));
    assert!(header_value(&headers, "User-Agent")
        .is_some_and(|value| value.starts_with("codex_cli_rs/0.999.6")));
    assert_eq!(header_value(&headers, "version"), Some("0.999.6"));
}

#[test]
fn build_codex_upstream_headers_preserves_non_codex_identity_for_compat_routes() {
    let _guard = crate::test_env_guard();
    let _ = set_originator("codex_cli_rs_tests").expect("set originator");
    let _ = set_codex_user_agent_version("0.999.5").expect("set ua version");

    let headers = build_codex_upstream_headers(CodexUpstreamHeaderInput {
        auth_token: "token-compat",
        chatgpt_account_id: None,
        incoming_user_agent: Some("gemini-cli/0.1.14 (Windows 11; x86_64)"),
        incoming_originator: Some("gemini_cli"),
        preserve_client_identity: true,
        incoming_session_id: Some("thread-compat"),
        incoming_window_id: Some("thread-compat:0"),
        incoming_client_request_id: Some("thread-compat"),
        incoming_subagent: None,
        incoming_beta_features: None,
        incoming_turn_metadata: None,
        incoming_parent_thread_id: None,
        incoming_responsesapi_include_timing_metrics: None,
        incoming_inference_call_id: None,
        incoming_oai_attestation: None,
        passthrough_codex_headers: &[],
        fallback_session_id: Some("thread-compat"),
        incoming_turn_state: None,
        include_turn_state: true,
        strip_session_affinity: false,
        has_body: true,
    });

    assert_eq!(
        header_value(&headers, "User-Agent"),
        Some("gemini-cli/0.1.14 (Windows 11; x86_64)")
    );
    assert_eq!(header_value(&headers, "originator"), Some("gemini_cli"));
}

#[test]
fn build_codex_compact_upstream_headers_omits_thread_id_when_missing() {
    let _guard = crate::test_env_guard();
    let headers = build_codex_compact_upstream_headers(CodexCompactUpstreamHeaderInput {
        auth_token: "token-thread-missing",
        chatgpt_account_id: None,
        installation_id: None,
        incoming_user_agent: None,
        incoming_originator: None,
        preserve_client_identity: false,
        incoming_session_id: Some("session-anchor"),
        thread_id: None,
        incoming_window_id: None,
        incoming_subagent: None,
        incoming_parent_thread_id: None,
        incoming_oai_attestation: None,
        passthrough_codex_headers: &[],
        fallback_session_id: Some("session-anchor"),
        strip_session_affinity: false,
        has_body: false,
    });

    assert_eq!(header_value(&headers, "thread_id"), None);
}
