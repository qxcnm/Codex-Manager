use super::*;

/// 函数 `resolve_openai_bearer_token_uses_cached_storage_value`
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
fn resolve_openai_bearer_token_uses_cached_storage_value() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let account = Account {
        id: "acc-1".to_string(),
        label: "main".to_string(),
        issuer: "".to_string(),
        chatgpt_account_id: None,
        workspace_id: None,
        group_name: None,
        sort: 0,
        status: "active".to_string(),
        created_at: now_ts(),
        updated_at: now_ts(),
    };
    storage.insert_account(&account).expect("insert account");
    storage
        .insert_token(&Token {
            account_id: "acc-1".to_string(),
            id_token: "id-token".to_string(),
            access_token: "access-token".to_string(),
            refresh_token: "refresh-token".to_string(),
            api_key_access_token: Some("cached-api-key-token".to_string()),
            last_refresh: now_ts(),
        })
        .expect("insert token");
    let mut runtime_token = Token {
        account_id: "acc-1".to_string(),
        id_token: "runtime-id-token".to_string(),
        access_token: "runtime-access-token".to_string(),
        refresh_token: "runtime-refresh-token".to_string(),
        api_key_access_token: None,
        last_refresh: now_ts(),
    };

    let bearer = crate::gateway::run_upstream_io(resolve_openai_bearer_token(
        &storage,
        &account,
        &mut runtime_token,
    ))
    .expect("gateway async test runtime")
    .expect("resolve");
    assert_eq!(bearer, "cached-api-key-token");
    assert_eq!(
        runtime_token.api_key_access_token.as_deref(),
        Some("cached-api-key-token")
    );
}

/// 函数 `drop_incoming_header_keeps_session_affinity_for_primary_attempt`
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
fn drop_incoming_header_keeps_session_affinity_for_primary_attempt() {
    assert!(should_drop_incoming_header("ChatGPT-Account-Id"));
    assert!(should_drop_incoming_header("authorization"));
    assert!(should_drop_incoming_header("x-api-key"));
    assert!(should_drop_incoming_header("x-goog-api-key"));
    assert!(should_drop_incoming_header("anthropic-version"));
    assert!(should_drop_incoming_header("x-stainless-lang"));
    assert!(!should_drop_incoming_header("session_id"));
    assert!(!should_drop_incoming_header("x-codex-other-limit-name"));
    assert!(!should_drop_incoming_header("x-codex-turn-state"));
    assert!(!should_drop_incoming_header("Content-Type"));
}

/// 函数 `drop_incoming_header_for_failover_strips_session_affinity`
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
fn drop_incoming_header_for_failover_strips_session_affinity() {
    assert!(should_drop_incoming_header_for_failover(
        "ChatGPT-Account-Id"
    ));
    assert!(should_drop_incoming_header_for_failover("session_id"));
    assert!(should_drop_incoming_header_for_failover(
        "x-codex-turn-state"
    ));
    assert!(!should_drop_incoming_header_for_failover(
        "x-codex-other-limit-name"
    ));
    assert!(!should_drop_incoming_header_for_failover("Content-Type"));
}
