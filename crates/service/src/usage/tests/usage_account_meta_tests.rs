use super::{
    build_workspace_map_from_accounts, clean_header_value, derive_account_meta, patch_account_meta,
    patch_account_meta_cached, resolve_workspace_id_for_account,
};
use codexmanager_core::storage::{now_ts, Account, Storage, Token};
use std::collections::HashMap;

/// 函数 `build_account`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - id: 参数 id
/// - workspace_id: 参数 workspace_id
/// - chatgpt_account_id: 参数 chatgpt_account_id
///
/// # 返回
/// 返回函数执行结果
fn build_account(
    id: &str,
    workspace_id: Option<&str>,
    chatgpt_account_id: Option<&str>,
) -> Account {
    Account {
        id: id.to_string(),
        label: format!("label-{id}"),
        issuer: "issuer".to_string(),
        chatgpt_account_id: chatgpt_account_id.map(|value| value.to_string()),
        workspace_id: workspace_id.map(|value| value.to_string()),
        group_name: None,
        sort: 0,
        status: "active".to_string(),
        created_at: now_ts(),
        updated_at: now_ts(),
    }
}

/// 函数 `clean_header_value_trims_and_drops_empty`
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
fn clean_header_value_trims_and_drops_empty() {
    assert_eq!(
        clean_header_value(Some(" abc ".to_string())),
        Some("abc".to_string())
    );
    assert_eq!(clean_header_value(Some("   ".to_string())), None);
    assert_eq!(clean_header_value(None), None);
}

/// 函数 `resolve_workspace_prefers_workspace_then_chatgpt`
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
fn resolve_workspace_prefers_workspace_then_chatgpt() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let account = build_account("acc-1", Some(" ws-primary "), Some("chatgpt-fallback"));
    storage.insert_account(&account).expect("insert");

    let resolved = resolve_workspace_id_for_account(&storage, "acc-1");
    assert_eq!(resolved, Some("ws-primary".to_string()));
}

/// 函数 `build_workspace_map_from_accounts_uses_preloaded_snapshot`
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
fn build_workspace_map_from_accounts_uses_preloaded_snapshot() {
    let accounts = vec![
        build_account("acc-3", Some(" ws-3 "), None),
        build_account("acc-4", None, Some(" chatgpt-4 ")),
    ];
    let workspace_map = build_workspace_map_from_accounts(&accounts);
    assert_eq!(workspace_map.get("acc-3"), Some(&Some("ws-3".to_string())));
    assert_eq!(
        workspace_map.get("acc-4"),
        Some(&Some("chatgpt-4".to_string()))
    );
}

/// 函数 `patch_account_meta_cached_updates_preloaded_account_without_lookup`
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
fn patch_account_meta_cached_updates_preloaded_account_without_lookup() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let account = build_account("acc-5", None, None);
    storage.insert_account(&account).expect("insert");
    let mut account_map = HashMap::new();
    account_map.insert(account.id.clone(), account);

    patch_account_meta_cached(
        &storage,
        &mut account_map,
        "acc-5",
        Some("chatgpt-5".to_string()),
        Some("workspace-5".to_string()),
    );

    let updated = storage
        .find_account_by_id("acc-5")
        .expect("find")
        .expect("account");
    assert_eq!(updated.chatgpt_account_id.as_deref(), Some("chatgpt-5"));
    assert_eq!(updated.workspace_id.as_deref(), Some("workspace-5"));
}

#[test]
fn patch_account_meta_cached_updates_identity_without_rewriting_account() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let mut account = build_account("acc-cached-identity", None, None);
    account.label = "keep cached label".to_string();
    account.issuer = "keep cached issuer".to_string();
    account.group_name = Some("keep cached group".to_string());
    account.sort = 29;
    account.status = "limited".to_string();
    let created_at = account.created_at;
    storage.insert_account(&account).expect("insert");
    let mut account_map = HashMap::new();
    account_map.insert(account.id.clone(), account);

    patch_account_meta_cached(
        &storage,
        &mut account_map,
        "acc-cached-identity",
        Some("chatgpt-cached".to_string()),
        Some("workspace-cached".to_string()),
    );

    let updated = storage
        .find_account_by_id("acc-cached-identity")
        .expect("find")
        .expect("account");
    assert_eq!(updated.label, "keep cached label");
    assert_eq!(updated.issuer, "keep cached issuer");
    assert_eq!(updated.group_name.as_deref(), Some("keep cached group"));
    assert_eq!(updated.sort, 29);
    assert_eq!(updated.status, "limited");
    assert_eq!(updated.created_at, created_at);
    assert_eq!(
        updated.chatgpt_account_id.as_deref(),
        Some("chatgpt-cached")
    );
    assert_eq!(updated.workspace_id.as_deref(), Some("workspace-cached"));
    let cached = account_map
        .get("acc-cached-identity")
        .expect("cached account updated");
    assert_eq!(cached.chatgpt_account_id.as_deref(), Some("chatgpt-cached"));
    assert_eq!(cached.workspace_id.as_deref(), Some("workspace-cached"));
}

/// 函数 `patch_account_meta_updates_identity_without_rewriting_account`
///
/// 作者: gaohongshun
///
/// 时间: 2026-06-20
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn patch_account_meta_updates_identity_without_rewriting_account() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let mut account = build_account("acc-identity-only", Some("old-workspace"), None);
    account.label = "keep label".to_string();
    account.issuer = "keep issuer".to_string();
    account.group_name = Some("keep group".to_string());
    account.sort = 17;
    account.status = "limited".to_string();
    let created_at = account.created_at;
    storage.insert_account(&account).expect("insert");

    patch_account_meta(
        &storage,
        "acc-identity-only",
        Some("chatgpt-identity".to_string()),
        Some("workspace-identity".to_string()),
    );

    let updated = storage
        .find_account_by_id("acc-identity-only")
        .expect("find")
        .expect("account");
    assert_eq!(updated.label, "keep label");
    assert_eq!(updated.issuer, "keep issuer");
    assert_eq!(updated.group_name.as_deref(), Some("keep group"));
    assert_eq!(updated.sort, 17);
    assert_eq!(updated.status, "limited");
    assert_eq!(updated.created_at, created_at);
    assert_eq!(
        updated.chatgpt_account_id.as_deref(),
        Some("chatgpt-identity")
    );
    assert_eq!(updated.workspace_id.as_deref(), Some("workspace-identity"));
}

/// 函数 `patch_account_meta_cached_replaces_subject_style_scope_values`
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
fn patch_account_meta_cached_replaces_subject_style_scope_values() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let account = build_account("acc-6", Some("auth0|legacy"), Some("auth0|legacy"));
    storage.insert_account(&account).expect("insert");
    let mut account_map = HashMap::new();
    account_map.insert(account.id.clone(), account);

    patch_account_meta_cached(
        &storage,
        &mut account_map,
        "acc-6",
        Some("org-correct".to_string()),
        Some("ws-correct".to_string()),
    );

    let updated = storage
        .find_account_by_id("acc-6")
        .expect("find")
        .expect("account");
    assert_eq!(updated.chatgpt_account_id.as_deref(), Some("org-correct"));
    assert_eq!(updated.workspace_id.as_deref(), Some("ws-correct"));
}

/// 函数 `patch_account_meta_cached_overrides_stale_team_scope_with_latest_token_scope`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-03
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn patch_account_meta_cached_overrides_stale_team_scope_with_latest_token_scope() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let account = build_account("acc-7", Some("org-team"), Some("org-team"));
    storage.insert_account(&account).expect("insert");
    let mut account_map = HashMap::new();
    account_map.insert(account.id.clone(), account);

    patch_account_meta_cached(
        &storage,
        &mut account_map,
        "acc-7",
        Some("org-free".to_string()),
        Some("org-free".to_string()),
    );

    let updated = storage
        .find_account_by_id("acc-7")
        .expect("find")
        .expect("account");
    assert_eq!(updated.chatgpt_account_id.as_deref(), Some("org-free"));
    assert_eq!(updated.workspace_id.as_deref(), Some("org-free"));
}

/// 函数 `resolve_workspace_id_for_account_filters_storage_style_scope_suffix`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-17
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn resolve_workspace_id_for_account_filters_storage_style_scope_suffix() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let composite =
        "google-oauth2|105671307665841419748::cgpt=ed08d56a-c038-4322-b325-53f504c0c88c|ws=org-AP6ypcMi84Thfueli6EU3B4m";
    storage
        .insert_account(&build_account("acc-8", Some(composite), Some(composite)))
        .expect("insert");

    let resolved = resolve_workspace_id_for_account(&storage, "acc-8");

    assert_eq!(resolved, Some("org-AP6ypcMi84Thfueli6EU3B4m".to_string()));
}

/// 函数 `derive_account_meta_filters_storage_style_scope_suffix_from_token_claims`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-17
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn derive_account_meta_filters_storage_style_scope_suffix_from_token_claims() {
    let token = Token {
        account_id: "acc-9".to_string(),
        id_token: "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJ1c2VyLTEiLCJ3b3Jrc3BhY2VfaWQiOiJnb29nbGUtb2F1dGgyfDEwNTY3MTMwNzY2NTg0MTQxOTc0ODo6Y2dwdD1lZDA4ZDU2YS1jMDM4LTQzMjItYjMyNS01M2Y1MDRjMGM4OGN8d3M9b3JnLUFQNnlwY01pODRUaGZ1ZWxpNkVVM0I0bSIsImh0dHBzOi8vYXBpLm9wZW5haS5jb20vYXV0aCI6eyJjaGF0Z3B0X2FjY291bnRfaWQiOiJnb29nbGUtb2F1dGgyfDEwNTY3MTMwNzY2NTg0MTQxOTc0ODo6Y2dwdD1lZDA4ZDU2YS1jMDM4LTQzMjItYjMyNS01M2Y1MDRjMGM4OGN8d3M9b3JnLUFQNnlwY01pODRUaGZ1ZWxpNkVVM0I0bSJ9fQ.sig".to_string(),
        access_token: String::new(),
        refresh_token: String::new(),
        api_key_access_token: None,
        last_refresh: now_ts(),
    };

    let (chatgpt_account_id, workspace_id) = derive_account_meta(&token, None);

    assert_eq!(
        chatgpt_account_id.as_deref(),
        Some("ed08d56a-c038-4322-b325-53f504c0c88c")
    );
    assert_eq!(
        workspace_id.as_deref(),
        Some("org-AP6ypcMi84Thfueli6EU3B4m")
    );
}

#[test]
fn derive_account_meta_requires_matching_user_and_selected_workspace() {
    use base64::Engine as _;
    let jwt = |user: Option<&str>, workspace: &str| {
        let claims = serde_json::json!({
            "sub": "fixture-subject",
            "workspace_id": workspace,
            "https://api.openai.com/auth": {
                "chatgpt_user_id": user,
                "chatgpt_account_id": workspace
            }
        });
        format!(
            "e30.{}.fixture",
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(claims.to_string())
        )
    };
    for (access_user, selected, expected) in [
        (Some("fixture-user"), Some("workspace-new"), "workspace-new"),
        (
            Some("different-user"),
            Some("workspace-new"),
            "workspace-old",
        ),
        (None, Some("workspace-new"), "workspace-old"),
        (Some("fixture-user"), Some("unmatched"), "workspace-old"),
        (Some("fixture-user"), None, "workspace-old"),
    ] {
        let token = Token {
            account_id: "local-account".to_string(),
            id_token: jwt(Some("fixture-user"), "workspace-old"),
            access_token: jwt(access_user, "workspace-new"),
            refresh_token: String::new(),
            api_key_access_token: None,
            last_refresh: now_ts(),
        };
        assert_eq!(
            derive_account_meta(&token, selected),
            (Some(expected.to_string()), Some(expected.to_string())),
        );
    }
}
