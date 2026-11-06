use super::{
    extract_token_payload, import_account_auth_json_with_storage, import_single_item,
    resolve_logical_account_id, ExistingAccountIndex, ImportTokenPayload,
};
use crate::account_identity::build_account_storage_id;
use base64::engine::general_purpose::{STANDARD as BASE64_STANDARD, URL_SAFE_NO_PAD};
use base64::Engine as _;
use codexmanager_core::storage::{now_ts, Account, Storage, Token};
use ed25519_dalek::pkcs8::EncodePrivateKey;
use ed25519_dalek::SigningKey;
use serde_json::json;

const TEST_ID_TOKEN_WS_A: &str = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJzdWItMSIsImVtYWlsIjoidGVzdEBleGFtcGxlLmNvbSIsIndvcmtzcGFjZV9pZCI6IndzLWEiLCJodHRwczovL2FwaS5vcGVuYWkuY29tL2F1dGgiOnsiY2hhdGdwdF9hY2NvdW50X2lkIjoiY2dwdC0xIn19.sig";
const TEST_ID_TOKEN_META: &str = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJzdWItMSIsImVtYWlsIjoibWV0YUBleGFtcGxlLmNvbSIsIndvcmtzcGFjZV9pZCI6IndzLW1ldGEiLCJodHRwczovL2FwaS5vcGVuYWkuY29tL2F1dGgiOnsiY2hhdGdwdF9hY2NvdW50X2lkIjoiY2dwdC1tZXRhIn19.sig";
const TEST_ACCESS_TOKEN_TEAM_USER_A: &str = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJzdWJqZWN0LWEiLCJodHRwczovL2FwaS5vcGVuYWkuY29tL2F1dGgiOnsiY2hhdGdwdF9hY2NvdW50X2lkIjoidGVhbS0xIiwiY2hhdGdwdF91c2VyX2lkIjoidXNlci1hIn19.sig";
const TEST_ACCESS_TOKEN_TEAM_USER_B: &str = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJzdWJqZWN0LWIiLCJodHRwczovL2FwaS5vcGVuYWkuY29tL2F1dGgiOnsiY2hhdGdwdF9hY2NvdW50X2lkIjoidGVhbS0xIiwiY2hhdGdwdF91c2VyX2lkIjoidXNlci1iIn19.sig";
const TEST_ID_TOKEN_SAME_SUB_TEAM: &str = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJzYW1lLXVzZXIiLCJlbWFpbCI6InNhbWVAZXhhbXBsZS5jb20iLCJ3b3Jrc3BhY2VfaWQiOiJ3cy1zaGFyZWQiLCJodHRwczovL2FwaS5vcGVuYWkuY29tL2F1dGgiOnsiY2hhdGdwdF9hY2NvdW50X2lkIjoiY2dwdC10ZWFtIiwiY2hhdGdwdF9wbGFuX3R5cGUiOiJ0ZWFtIiwiY2hhdGdwdF91c2VyX2lkIjoic2FtZS11c2VyIn19.sig";
const TEST_ID_TOKEN_SAME_SUB_PLUS: &str = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJzYW1lLXVzZXIiLCJlbWFpbCI6InNhbWVAZXhhbXBsZS5jb20iLCJ3b3Jrc3BhY2VfaWQiOiJ3cy1zaGFyZWQiLCJodHRwczovL2FwaS5vcGVuYWkuY29tL2F1dGgiOnsiY2hhdGdwdF9hY2NvdW50X2lkIjoiY2dwdC1wbHVzIiwiY2hhdGdwdF9wbGFuX3R5cGUiOiJwbHVzIiwiY2hhdGdwdF91c2VyX2lkIjoic2FtZS11c2VyIn19.sig";
const TEST_ID_TOKEN_SAME_SUB_TEAM_A: &str = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJzYW1lLXVzZXIiLCJlbWFpbCI6InNhbWVAZXhhbXBsZS5jb20iLCJ3b3Jrc3BhY2VfaWQiOiJ3cy10ZWFtLWEiLCJodHRwczovL2FwaS5vcGVuYWkuY29tL2F1dGgiOnsiY2hhdGdwdF9hY2NvdW50X2lkIjoiY2dwdC10ZWFtLWEiLCJjaGF0Z3B0X3BsYW5fdHlwZSI6InRlYW0iLCJjaGF0Z3B0X3VzZXJfaWQiOiJzYW1lLXVzZXIifX0.sig";
const TEST_ID_TOKEN_SAME_SUB_TEAM_B: &str = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJzYW1lLXVzZXIiLCJlbWFpbCI6InNhbWVAZXhhbXBsZS5jb20iLCJ3b3Jrc3BhY2VfaWQiOiJ3cy10ZWFtLWIiLCJodHRwczovL2FwaS5vcGVuYWkuY29tL2F1dGgiOnsiY2hhdGdwdF9hY2NvdW50X2lkIjoiY2dwdC10ZWFtLWIiLCJjaGF0Z3B0X3BsYW5fdHlwZSI6InRlYW0iLCJjaGF0Z3B0X3VzZXJfaWQiOiJzYW1lLXVzZXIifX0.sig";

fn test_agent_private_key(seed: u8) -> String {
    let signing_key = SigningKey::from_bytes(&[seed; 32]);
    BASE64_STANDARD.encode(
        signing_key
            .to_pkcs8_der()
            .expect("encode test agent key")
            .as_bytes(),
    )
}

fn test_jwt(payload: serde_json::Value) -> String {
    let header = URL_SAFE_NO_PAD.encode(r#"{"alg":"none","typ":"JWT"}"#);
    let payload = URL_SAFE_NO_PAD.encode(payload.to_string());
    format!("{header}.{payload}.signature")
}

#[test]
fn import_codex_selected_workspace_survives_metadata_refresh() {
    let storage = Storage::open_in_memory().expect("open storage");
    storage.init().expect("init storage");
    let mut index = ExistingAccountIndex::build(&storage).expect("account index");
    let id_token = test_jwt(json!({
        "sub": "fixture-user", "workspace_id": "workspace-old",
        "https://api.openai.com/auth": {
            "chatgpt_user_id": "fixture-user", "chatgpt_account_id": "workspace-old"
        }
    }));
    let access_token = test_jwt(json!({
        "sub": "fixture-user",
        "https://api.openai.com/auth": {
            "chatgpt_user_id": "fixture-user", "chatgpt_account_id": "workspace-selected"
        }
    }));
    let item = json!({"tokens": {
        "id_token": id_token, "access_token": access_token,
        "refresh_token": "fixture-refresh", "account_id": "workspace-selected"
    }});
    import_single_item(&storage, &mut index, &item, 1).expect("import selected workspace");
    let accounts = storage.list_accounts().expect("accounts");
    assert_eq!(accounts.len(), 1);
    let account = &accounts[0];
    assert_eq!(
        account.chatgpt_account_id.as_deref(),
        Some("workspace-selected")
    );
    assert_eq!(account.workspace_id.as_deref(), Some("workspace-selected"));
    let token = storage
        .find_token_by_account_id(&account.id)
        .expect("token")
        .expect("stored token");
    let derived = crate::usage_account_meta::derive_account_meta(
        &token,
        account.chatgpt_account_id.as_deref(),
    );
    assert_eq!(
        derived,
        (
            Some("workspace-selected".into()),
            Some("workspace-selected".into())
        )
    );
    crate::usage_account_meta::patch_account_meta(&storage, &account.id, derived.0, derived.1);
    let refreshed = storage
        .find_account_by_id(&account.id)
        .expect("account")
        .expect("stored account");
    assert_eq!(
        refreshed.chatgpt_account_id.as_deref(),
        Some("workspace-selected")
    );
    assert_eq!(
        refreshed.workspace_id.as_deref(),
        Some("workspace-selected")
    );
}

#[test]
fn import_workspace_hint_does_not_override_unrelated_or_unselected_claims() {
    for (access_user, selected) in [
        ("different-user", "workspace-new"),
        ("fixture-user", "unmatched"),
    ] {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        let mut index = ExistingAccountIndex::build(&storage).expect("account index");
        let item = json!({"tokens": {
            "id_token": test_jwt(json!({
                "sub": "fixture-user", "workspace_id": "workspace-old",
                "https://api.openai.com/auth": {
                    "chatgpt_user_id": "fixture-user", "chatgpt_account_id": "workspace-old"
                }
            })),
            "access_token": test_jwt(json!({
                "sub": access_user,
                "https://api.openai.com/auth": {
                    "chatgpt_user_id": access_user, "chatgpt_account_id": "workspace-new"
                }
            })),
            "refresh_token": "fixture-refresh", "account_id": selected
        }});
        import_single_item(&storage, &mut index, &item, 1).expect("import preserves priority");
        let accounts = storage.list_accounts().expect("accounts");
        assert_eq!(
            accounts[0].chatgpt_account_id.as_deref(),
            Some("workspace-old")
        );
        assert_eq!(accounts[0].workspace_id.as_deref(), Some("workspace-old"));
    }
}

/// 函数 `payload`
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
fn payload() -> ImportTokenPayload {
    ImportTokenPayload {
        access_token: "access".to_string(),
        id_token: "id".to_string(),
        refresh_token: "refresh".to_string(),
        account_id_hint: None,
        chatgpt_account_id_hint: None,
        agent_identity: None,
    }
}

/// 函数 `resolve_logical_account_id_distinguishes_workspace_under_same_chatgpt`
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
fn resolve_logical_account_id_distinguishes_workspace_under_same_chatgpt() {
    let input = payload();
    let a = resolve_logical_account_id(
        &input,
        Some("sub-1"),
        Some("cgpt-1"),
        Some("ws-a"),
        Some("same-fp"),
    )
    .expect("resolve ws-a");
    let b = resolve_logical_account_id(
        &input,
        Some("sub-1"),
        Some("cgpt-1"),
        Some("ws-b"),
        Some("same-fp"),
    )
    .expect("resolve ws-b");

    assert_ne!(a, b);
}

/// 函数 `resolve_logical_account_id_is_stable_when_scope_is_stable`
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
fn resolve_logical_account_id_is_stable_when_scope_is_stable() {
    let input = payload();
    let first = resolve_logical_account_id(
        &input,
        Some("sub-1"),
        Some("cgpt-1"),
        Some("ws-a"),
        Some("fp-1"),
    )
    .expect("resolve first");
    let second = resolve_logical_account_id(
        &input,
        Some("sub-1"),
        Some("cgpt-1"),
        Some("ws-a"),
        Some("fp-2"),
    )
    .expect("resolve second");

    assert_eq!(first, second);
    assert_eq!(
        first,
        build_account_storage_id("sub-1", Some("cgpt-1"), Some("ws-a"), None)
    );
}

/// 函数 `existing_account_index_next_sort_uses_step_five`
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
fn existing_account_index_next_sort_uses_step_five() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init");
    let now = now_ts();
    storage
        .insert_account(&Account {
            id: "acc-1".to_string(),
            label: "acc-1".to_string(),
            issuer: "https://auth.openai.com".to_string(),
            chatgpt_account_id: Some("cgpt-1".to_string()),
            workspace_id: Some("ws-1".to_string()),
            group_name: None,
            sort: 0,
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
        })
        .expect("insert acc-1");
    storage
        .insert_account(&Account {
            id: "acc-2".to_string(),
            label: "acc-2".to_string(),
            issuer: "https://auth.openai.com".to_string(),
            chatgpt_account_id: Some("cgpt-2".to_string()),
            workspace_id: Some("ws-2".to_string()),
            group_name: None,
            sort: 9,
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
        })
        .expect("insert acc-2");

    let idx = ExistingAccountIndex::build(&storage).expect("build index");
    assert_eq!(idx.next_sort, 14);
}

/// 函数 `extract_token_payload_supports_flat_codex_format`
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
fn extract_token_payload_supports_flat_codex_format() {
    let value = json!({
        "type": "codex",
        "email": "u@example.com",
        "id_token": "id.flat",
        "account_id": "acc-flat",
        "access_token": "access.flat",
        "refresh_token": "refresh.flat"
    });

    let payload = extract_token_payload(&value).expect("parse flat payload");
    assert_eq!(payload.access_token, "access.flat");
    assert_eq!(payload.id_token, "id.flat");
    assert_eq!(payload.refresh_token, "refresh.flat");
    assert_eq!(payload.account_id_hint.as_deref(), Some("acc-flat"));
    assert_eq!(payload.chatgpt_account_id_hint, None);
}

/// 函数 `extract_token_payload_supports_camel_case_fields`
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
fn extract_token_payload_supports_camel_case_fields() {
    let value = json!({
        "tokens": {
            "idToken": "id.camel",
            "accessToken": "access.camel",
            "refreshToken": "refresh.camel",
            "accountId": "acc-camel",
            "chatgptAccountId": "cgpt-camel"
        }
    });

    let payload = extract_token_payload(&value).expect("parse camel payload");
    assert_eq!(payload.access_token, "access.camel");
    assert_eq!(payload.id_token, "id.camel");
    assert_eq!(payload.refresh_token, "refresh.camel");
    assert_eq!(payload.account_id_hint.as_deref(), Some("acc-camel"));
    assert_eq!(
        payload.chatgpt_account_id_hint.as_deref(),
        Some("cgpt-camel")
    );
}

/// 函数 `extract_token_payload_allows_missing_id_and_refresh_tokens`
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
fn extract_token_payload_allows_missing_id_and_refresh_tokens() {
    let value = json!({
        "tokens": {
            "access_token": "access.only",
            "account_id": "acc-only"
        }
    });

    let payload = extract_token_payload(&value).expect("parse optional token payload");
    assert_eq!(payload.access_token, "access.only");
    assert_eq!(payload.id_token, "");
    assert_eq!(payload.refresh_token, "");
    assert_eq!(payload.account_id_hint.as_deref(), Some("acc-only"));
}

#[test]
fn extract_token_payload_supports_sub2api_agent_identity_credentials() {
    let value = json!({
        "name": "agent@example.com",
        "platform": "openai",
        "type": "oauth",
        "credentials": {
            "account_id": "chatgpt-agent",
            "agent_private_key": "private-key",
            "agent_runtime_id": "agent-runtime-1",
            "auth_mode": "agentIdentity",
            "chatgpt_account_id": "chatgpt-agent",
            "chatgpt_account_is_fedramp": false,
            "chatgpt_user_id": "user-agent",
            "id_token": TEST_ID_TOKEN_META,
            "task_id": "task-agent",
            "workspace_id": "workspace-agent"
        }
    });

    let payload = extract_token_payload(&value).expect("parse agent identity payload");
    assert_eq!(payload.access_token, "");
    assert_eq!(payload.id_token, TEST_ID_TOKEN_META);
    assert_eq!(
        payload.chatgpt_account_id_hint.as_deref(),
        Some("chatgpt-agent")
    );
    let identity = payload.agent_identity.expect("agent identity");
    assert_eq!(identity.agent_runtime_id, "agent-runtime-1");
    assert_eq!(identity.agent_private_key, "private-key");
    assert_eq!(identity.task_id.as_deref(), Some("task-agent"));
    assert_eq!(identity.chatgpt_user_id, "user-agent");
    assert_eq!(identity.workspace_id.as_deref(), Some("workspace-agent"));
    assert!(!identity.chatgpt_account_is_fedramp);
}

#[test]
fn extract_token_payload_supports_official_agent_identity_record() {
    let value = json!({
        "auth_mode": "chatgpt",
        "agent_identity": {
            "agent_runtime_id": "official-runtime",
            "agent_private_key": "official-private-key",
            "account_id": "official-account",
            "chatgpt_user_id": "official-user",
            "email": "official@example.com",
            "plan_type": "plus",
            "chatgpt_account_is_fedramp": true,
            "task_id": "official-task"
        }
    });

    let payload = extract_token_payload(&value).expect("parse official agent identity record");
    assert!(payload.access_token.is_empty());
    assert!(payload.id_token.is_empty());
    assert!(payload.refresh_token.is_empty());
    assert_eq!(payload.account_id_hint.as_deref(), Some("official-account"));
    assert_eq!(
        payload.chatgpt_account_id_hint.as_deref(),
        Some("official-account")
    );
    let identity = payload.agent_identity.expect("agent identity");
    assert_eq!(identity.agent_runtime_id, "official-runtime");
    assert_eq!(identity.agent_private_key, "official-private-key");
    assert_eq!(identity.task_id.as_deref(), Some("official-task"));
    assert_eq!(identity.chatgpt_user_id, "official-user");
    assert!(identity.chatgpt_account_is_fedramp);
    assert_eq!(identity.auth_mode, "agentIdentity");
    assert_eq!(identity.workspace_id.as_deref(), Some("official-account"));
}

#[test]
fn extract_token_payload_supports_camel_case_agent_identity_record() {
    let value = json!({
        "authMode": "agentIdentity",
        "agentIdentity": {
            "agentRuntimeId": "camel-runtime",
            "agentPrivateKey": "camel-private-key",
            "accountId": "camel-account",
            "chatgptUserId": "camel-user",
            "chatgptAccountIsFedramp": false,
            "taskId": "camel-task",
            "workspaceId": "camel-workspace"
        }
    });

    let payload = extract_token_payload(&value).expect("parse camel agent identity record");
    assert_eq!(payload.account_id_hint.as_deref(), Some("camel-account"));
    assert_eq!(
        payload.chatgpt_account_id_hint.as_deref(),
        Some("camel-account")
    );
    let identity = payload.agent_identity.expect("agent identity");
    assert_eq!(identity.agent_runtime_id, "camel-runtime");
    assert_eq!(identity.agent_private_key, "camel-private-key");
    assert_eq!(identity.task_id.as_deref(), Some("camel-task"));
    assert_eq!(identity.chatgpt_user_id, "camel-user");
    assert_eq!(identity.workspace_id.as_deref(), Some("camel-workspace"));
}

#[test]
fn extract_token_payload_rejects_agent_identity_jwt_storage() {
    let value = json!({
        "auth_mode": "agentIdentity",
        "agent_identity": "header.payload.signature"
    });

    let error =
        extract_token_payload(&value).expect_err("JWT storage must not be trusted as a record");
    assert!(error.contains("agent_runtime_id"));
}

#[test]
fn extract_token_payload_ignores_stale_nested_identity_for_non_agent_mode() {
    let value = json!({
        "auth_mode": "personalAccessToken",
        "tokens": {
            "access_token": "at-personal-token",
            "account_id": "real-account",
            "chatgpt_account_id": "real-chatgpt-account"
        },
        "agent_identity": {
            "agent_runtime_id": "stale-runtime",
            "agent_private_key": "stale-private-key",
            "account_id": "stale-account",
            "chatgpt_user_id": "stale-user"
        }
    });

    let payload = extract_token_payload(&value).expect("parse personal access token payload");
    assert!(payload.agent_identity.is_none());
    assert_eq!(payload.account_id_hint.as_deref(), Some("real-account"));
    assert_eq!(
        payload.chatgpt_account_id_hint.as_deref(),
        Some("real-chatgpt-account")
    );
}

#[test]
fn import_official_agent_identity_record_persists_identity_and_scope() {
    let storage = Storage::open_in_memory().expect("open storage");
    storage.init().expect("init storage");
    let private_key = test_agent_private_key(21);
    let content = json!({
        "auth_mode": "chatgpt",
        "agent_identity": {
            "agent_runtime_id": "official-runtime",
            "agent_private_key": private_key,
            "account_id": "official-account",
            "chatgpt_user_id": "official-user",
            "email": "official@example.com",
            "plan_type": "plus",
            "chatgpt_account_is_fedramp": true,
            "task_id": "official-task"
        }
    })
    .to_string();

    let result = import_account_auth_json_with_storage(&storage, vec![content], false)
        .expect("import official agent identity record");
    assert_eq!(result.total, 1);
    assert_eq!(result.created, 1);
    assert_eq!(result.updated, 0);
    assert_eq!(result.failed, 0);
    assert_eq!(
        result.usage_refresh_account_ids,
        result.imported_account_ids
    );

    let account = storage
        .list_accounts()
        .expect("list accounts")
        .into_iter()
        .next()
        .expect("stored account");
    assert_eq!(account.label, "official@example.com");
    assert_eq!(account.status, "active");
    assert_eq!(
        account.chatgpt_account_id.as_deref(),
        Some("official-account")
    );
    assert_eq!(account.workspace_id.as_deref(), Some("official-account"));

    let token = storage
        .find_token_by_account_id(&account.id)
        .expect("find token")
        .expect("stored token");
    assert!(token.access_token.is_empty());
    assert!(token.id_token.is_empty());
    assert!(token.refresh_token.is_empty());

    let identity = storage
        .find_account_agent_identity(&account.id)
        .expect("find identity")
        .expect("stored identity");
    assert_eq!(identity.agent_runtime_id, "official-runtime");
    assert_eq!(identity.agent_private_key, private_key);
    assert_eq!(identity.task_id.as_deref(), Some("official-task"));
    assert_eq!(identity.chatgpt_user_id, "official-user");
    assert!(identity.chatgpt_account_is_fedramp);
    assert_eq!(identity.auth_mode, "agentIdentity");
    assert_eq!(identity.workspace_id.as_deref(), Some("official-account"));
}

#[test]
fn import_auth_session_access_token_uses_profile_email_as_label() {
    let storage = Storage::open_in_memory().expect("open storage");
    storage.init().expect("init storage");
    let access_token = test_jwt(json!({
        "sub": "session-subject",
        "https://api.openai.com/auth": {
            "chatgpt_account_id": "session-account",
            "chatgpt_user_id": "session-user",
            "chatgpt_plan_type": "plus"
        },
        "https://api.openai.com/profile": {
            "email": "session@example.com",
            "name": "Session User"
        }
    }));
    let content = json!({
        "user": { "name": "ChatGPT Session User" },
        "accessToken": access_token
    })
    .to_string();

    let result = import_account_auth_json_with_storage(&storage, vec![content], false)
        .expect("import auth session");
    assert_eq!(result.created, 1);
    assert_eq!(result.failed, 0);

    let account = storage
        .list_accounts()
        .expect("list accounts")
        .into_iter()
        .next()
        .expect("stored account");
    assert_eq!(account.label, "session@example.com");
    assert_eq!(
        account.chatgpt_account_id.as_deref(),
        Some("session-account")
    );
    assert_eq!(account.workspace_id.as_deref(), Some("session-account"));
}

#[test]
fn reimport_replaces_generated_label_but_preserves_account_identity() {
    let storage = Storage::open_in_memory().expect("open storage");
    storage.init().expect("init storage");
    let token_without_profile = test_jwt(json!({
        "sub": "session-subject",
        "https://api.openai.com/auth": {
            "chatgpt_account_id": "session-account",
            "chatgpt_user_id": "session-user"
        }
    }));
    let first = import_account_auth_json_with_storage(
        &storage,
        vec![json!({ "accessToken": token_without_profile }).to_string()],
        false,
    )
    .expect("initial import");
    assert_eq!(first.created, 1);
    let account_id = first.imported_account_ids[0].clone();
    assert_eq!(
        storage.list_accounts().expect("list accounts")[0].label,
        "导入账号0001"
    );

    let token_with_profile = test_jwt(json!({
        "sub": "session-subject",
        "https://api.openai.com/auth": {
            "chatgpt_account_id": "session-account",
            "chatgpt_user_id": "session-user"
        },
        "https://api.openai.com/profile": {
            "email": "upgraded@example.com"
        }
    }));
    let second = import_account_auth_json_with_storage(
        &storage,
        vec![json!({ "accessToken": token_with_profile }).to_string()],
        false,
    )
    .expect("reimport auth session");
    assert_eq!(second.created, 0);
    assert_eq!(second.updated, 1);
    assert_eq!(second.imported_account_ids, vec![account_id.clone()]);

    let account = storage
        .find_account_by_id(&account_id)
        .expect("find account")
        .expect("stored account");
    assert_eq!(account.label, "upgraded@example.com");
}

#[test]
fn extract_token_payload_respects_explicit_non_agent_auth_mode() {
    let value = json!({
        "platform": "openai",
        "type": "oauth",
        "credentials": {
            "auth_mode": "personalAccessToken",
            "access_token": "at-personal-token",
            "agent_private_key": "stale-private-key",
            "agent_runtime_id": "stale-runtime-id"
        }
    });

    let payload = extract_token_payload(&value).expect("parse personal access token payload");
    assert_eq!(payload.access_token, "at-personal-token");
    assert!(payload.agent_identity.is_none());
}

#[test]
fn extract_token_payload_infers_agent_identity_without_requiring_task_id() {
    let value = json!({
        "platform": "openai",
        "type": "oauth",
        "credentials": {
            "agent_private_key": "private-key",
            "agent_runtime_id": "runtime-id",
            "chatgpt_user_id": "user-id"
        }
    });

    let payload = extract_token_payload(&value).expect("infer legacy agent identity payload");
    let identity = payload.agent_identity.expect("agent identity");
    assert_eq!(identity.auth_mode, "agentIdentity");
    assert_eq!(identity.chatgpt_user_id, "user-id");
    assert_eq!(identity.task_id, None);
}

#[test]
fn import_account_auth_json_skips_non_chatgpt_sub2api_accounts() {
    let storage = Storage::open_in_memory().expect("open storage");
    storage.init().expect("init storage");

    let content = json!({
        "type": "sub2api-data",
        "version": 1,
        "accounts": [
            {
                "name": "chatgpt@example.com",
                "platform": "openai",
                "type": "oauth",
                "credentials": {
                    "access_token": "access.openai",
                    "refresh_token": "refresh.openai",
                    "account_id": "chatgpt-account"
                }
            },
            {
                "name": "claude@example.com",
                "platform": "anthropic",
                "type": "oauth",
                "credentials": {
                    "access_token": "access.anthropic",
                    "refresh_token": "refresh.anthropic",
                    "account_id": "anthropic-account"
                }
            },
            {
                "name": "gemini@example.com",
                "platform": "gemini",
                "type": "oauth",
                "credentials": {
                    "access_token": "access.gemini",
                    "refresh_token": "refresh.gemini",
                    "account_id": "gemini-account"
                }
            },
            {
                "name": "openai-api-key@example.com",
                "platform": "openai",
                "type": "api_key",
                "credentials": {
                    "access_token": "sk-not-chatgpt",
                    "account_id": "openai-api-key-account"
                }
            }
        ]
    })
    .to_string();

    let result = import_account_auth_json_with_storage(&storage, vec![content], false)
        .expect("import mixed sub2api data");
    assert_eq!(result.total, 1);
    assert_eq!(result.created, 1);
    assert_eq!(result.updated, 0);
    assert_eq!(result.failed, 0);
    assert!(result.errors.is_empty());
    assert_eq!(result.imported_account_ids.len(), 1);
    assert_eq!(result.usage_refresh_account_ids.len(), 1);

    let accounts = storage.list_accounts().expect("list accounts");
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0].label, "chatgpt@example.com");
}

#[test]
fn import_account_auth_json_skips_standalone_non_openai_sub2api_account() {
    let storage = Storage::open_in_memory().expect("open storage");
    storage.init().expect("init storage");

    let content = json!({
        "name": "claude@example.com",
        "platform": "anthropic",
        "type": "oauth",
        "credentials": {
            "access_token": "access.anthropic",
            "refresh_token": "refresh.anthropic",
            "account_id": "anthropic-account"
        }
    })
    .to_string();

    let result = import_account_auth_json_with_storage(&storage, vec![content], false)
        .expect("ignore standalone anthropic account");
    assert_eq!(result.total, 0);
    assert_eq!(result.created, 0);
    assert_eq!(result.updated, 0);
    assert_eq!(result.failed, 0);
    assert!(result.errors.is_empty());
    assert!(result.imported_account_ids.is_empty());
    assert!(result.usage_refresh_account_ids.is_empty());
    assert!(storage.list_accounts().expect("list accounts").is_empty());
}

#[test]
fn import_sub2api_personal_access_token_ignores_stale_agent_fields() {
    let storage = Storage::open_in_memory().expect("open storage");
    storage.init().expect("init storage");

    let content = json!({
        "type": "sub2api-data",
        "version": 1,
        "accounts": [{
            "name": "pat@example.com",
            "platform": "openai",
            "type": "oauth",
            "credentials": {
                "account_id": "pat-account",
                "access_token": "at-personal-token",
                "auth_mode": "personalAccessToken",
                "agent_private_key": "stale-private-key",
                "agent_runtime_id": "stale-runtime-id"
            }
        }]
    })
    .to_string();

    let result = import_account_auth_json_with_storage(&storage, vec![content], false)
        .expect("import personal access token");
    assert_eq!(result.total, 1);
    assert_eq!(result.created, 1);
    assert_eq!(result.failed, 0);
    assert!(result.usage_refresh_account_ids.is_empty());

    let account = storage
        .list_accounts()
        .expect("list accounts")
        .into_iter()
        .next()
        .expect("stored account");
    assert!(storage
        .find_account_agent_identity(&account.id)
        .expect("find identity")
        .is_none());
    let token = storage
        .find_token_by_account_id(&account.id)
        .expect("find token")
        .expect("stored token");
    assert_eq!(token.access_token, "at-personal-token");
}

#[test]
fn import_agent_identities_distinguishes_users_in_the_same_workspace() {
    let storage = Storage::open_in_memory().expect("open storage");
    storage.init().expect("init storage");

    let private_key_a = test_agent_private_key(11);
    let private_key_b = test_agent_private_key(12);
    let content = json!({
        "type": "sub2api-data",
        "version": 1,
        "accounts": [
            {
                "name": "member-a@example.com",
                "platform": "openai",
                "type": "oauth",
                "credentials": {
                    "account_id": "chatgpt-team",
                    "agent_private_key": private_key_a,
                    "agent_runtime_id": "runtime-a",
                    "auth_mode": "agentIdentity",
                    "chatgpt_account_id": "chatgpt-team",
                    "chatgpt_user_id": "member-a",
                    "task_id": "task-a",
                    "workspace_id": "workspace-shared"
                }
            },
            {
                "name": "member-b@example.com",
                "platform": "openai",
                "type": "oauth",
                "credentials": {
                    "account_id": "chatgpt-team",
                    "agent_private_key": private_key_b,
                    "agent_runtime_id": "runtime-b",
                    "auth_mode": "agentIdentity",
                    "chatgpt_account_id": "chatgpt-team",
                    "chatgpt_user_id": "member-b",
                    "workspace_id": "workspace-shared"
                }
            }
        ]
    })
    .to_string();

    let result = import_account_auth_json_with_storage(&storage, vec![content], false)
        .expect("import team members");
    assert_eq!(result.created, 2);
    assert_eq!(result.updated, 0);
    assert_eq!(result.failed, 0);

    let reimport = json!({
        "type": "sub2api-data",
        "version": 1,
        "accounts": [{
            "name": "member-a@example.com",
            "platform": "openai",
            "type": "oauth",
            "credentials": {
                "account_id": "chatgpt-team",
                "agent_private_key": test_agent_private_key(13),
                "agent_runtime_id": "runtime-a-updated",
                "auth_mode": "agentIdentity",
                "chatgpt_account_id": "chatgpt-team",
                "chatgpt_user_id": "member-a",
                "task_id": "task-a-updated",
                "workspace_id": "workspace-shared"
            }
        }]
    })
    .to_string();
    let reimport_result = import_account_auth_json_with_storage(&storage, vec![reimport], false)
        .expect("reimport member a");
    assert_eq!(reimport_result.created, 0);
    assert_eq!(reimport_result.updated, 1);
    assert_eq!(reimport_result.failed, 0);

    let accounts = storage.list_accounts().expect("list accounts");
    assert_eq!(accounts.len(), 2);
    let identities = accounts
        .iter()
        .map(|account| {
            storage
                .find_account_agent_identity(&account.id)
                .expect("find identity")
                .expect("stored identity")
        })
        .collect::<Vec<_>>();
    assert!(identities
        .iter()
        .any(|identity| identity.chatgpt_user_id == "member-a"));
    assert!(identities
        .iter()
        .any(|identity| identity.chatgpt_user_id == "member-b"));
    assert!(identities.iter().any(|identity| {
        identity.chatgpt_user_id == "member-a" && identity.agent_runtime_id == "runtime-a-updated"
    }));
    assert!(identities.iter().any(|identity| {
        identity.chatgpt_user_id == "member-b" && identity.agent_runtime_id == "runtime-b"
    }));
    assert!(identities
        .iter()
        .any(|identity| { identity.chatgpt_user_id == "member-b" && identity.task_id.is_none() }));
    assert_ne!(accounts[0].id, accounts[1].id);
}

#[test]
fn import_account_auth_json_expands_sub2api_accounts_and_persists_agent_identities() {
    let storage = Storage::open_in_memory().expect("open storage");
    storage.init().expect("init storage");

    let private_key_a = test_agent_private_key(1);
    let private_key_b = test_agent_private_key(2);
    let content = json!({
        "type": "sub2api-data",
        "version": 1,
        "exported_at": "2026-07-21T00:00:00Z",
        "proxies": [],
        "accounts": [
            {
                "name": "agent-a@example.com",
                "platform": "openai",
                "type": "oauth",
                "credentials": {
                    "account_id": "chatgpt-agent-a",
                    "agent_private_key": private_key_a,
                    "agent_runtime_id": "agent-runtime-a",
                    "auth_mode": "agentIdentity",
                    "chatgpt_account_id": "chatgpt-agent-a",
                    "chatgpt_account_is_fedramp": false,
                    "chatgpt_user_id": "user-agent-a",
                    "id_token": TEST_ID_TOKEN_META,
                    "task_id": "task-agent-a",
                    "workspace_id": "workspace-agent-a"
                },
                "extra": { "name": "Agent A" }
            },
            {
                "name": "agent-b@example.com",
                "platform": "openai",
                "type": "oauth",
                "credentials": {
                    "account_id": "chatgpt-agent-b",
                    "agent_private_key": private_key_b,
                    "agent_runtime_id": "agent-runtime-b",
                    "auth_mode": "agentIdentity",
                    "chatgpt_account_id": "chatgpt-agent-b",
                    "chatgpt_account_is_fedramp": true,
                    "chatgpt_user_id": "user-agent-b",
                    "id_token": TEST_ID_TOKEN_SAME_SUB_PLUS,
                    "task_id": "task-agent-b",
                    "workspace_id": "workspace-agent-b"
                }
            }
        ]
    })
    .to_string();

    let result = import_account_auth_json_with_storage(&storage, vec![content], false)
        .expect("import sub2api data");
    assert_eq!(result.total, 2);
    assert_eq!(result.created, 2);
    assert_eq!(result.updated, 0);
    assert_eq!(result.failed, 0);
    assert_eq!(result.imported_account_ids.len(), 2);
    assert_eq!(result.usage_refresh_account_ids.len(), 2);
    assert_eq!(
        result.usage_refresh_account_ids,
        result.imported_account_ids
    );

    let accounts = storage.list_accounts().expect("list accounts");
    assert_eq!(accounts.len(), 2);
    assert!(accounts
        .iter()
        .any(|account| account.label == "agent-a@example.com"));
    for account in accounts {
        let token = storage
            .find_token_by_account_id(&account.id)
            .expect("find token")
            .expect("stored token");
        assert!(token.access_token.is_empty());
        assert!(token.refresh_token.is_empty());
        assert!(!token.id_token.is_empty());
        let identity = storage
            .find_account_agent_identity(&account.id)
            .expect("find identity")
            .expect("stored identity");
        assert!(identity.agent_runtime_id.starts_with("agent-runtime-"));
        assert!(identity
            .task_id
            .as_deref()
            .is_some_and(|task_id| task_id.starts_with("task-agent-")));
    }
}

/// 函数 `import_single_item_reuses_existing_login_account_by_scope_identity`
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
fn import_single_item_reuses_existing_login_account_by_scope_identity() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init");
    let now = now_ts();
    let existing_id = build_account_storage_id("sub-1", Some("cgpt-1"), Some("ws-a"), None);
    storage
        .insert_account(&Account {
            id: existing_id.clone(),
            label: "existing".to_string(),
            issuer: "https://auth.openai.com".to_string(),
            chatgpt_account_id: Some("cgpt-1".to_string()),
            workspace_id: Some("ws-a".to_string()),
            group_name: Some("LOGIN".to_string()),
            sort: 0,
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
        })
        .expect("insert existing account");

    let mut idx = ExistingAccountIndex::build(&storage).expect("build index");
    let item = json!({
        "tokens": {
            "access_token": "access.import",
            "id_token": TEST_ID_TOKEN_WS_A,
            "refresh_token": "refresh.import",
            "account_id": "legacy-import-id"
        }
    });

    let created = import_single_item(&storage, &mut idx, &item, 1).expect("import item");
    assert!(!created);

    let accounts = storage.list_accounts().expect("list accounts");
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0].id, existing_id);
    assert_eq!(accounts[0].group_name, None);
    assert!(storage
        .find_account_metadata(&accounts[0].id)
        .expect("find metadata")
        .is_none());

    let token = storage
        .find_token_by_account_id(&accounts[0].id)
        .expect("find token")
        .expect("token");
    assert_eq!(token.account_id, accounts[0].id);
}

/// 函数 `import_single_item_distinguishes_team_members_sharing_account_hint`
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
fn import_single_item_distinguishes_team_members_sharing_account_hint() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init");
    let mut idx = ExistingAccountIndex::build(&storage).expect("build index");

    let user_a = json!({
        "tokens": {
            "access_token": TEST_ACCESS_TOKEN_TEAM_USER_A,
            "account_id": "team-1",
            "refresh_token": "refresh.user-a"
        }
    });
    let user_b = json!({
        "tokens": {
            "access_token": TEST_ACCESS_TOKEN_TEAM_USER_B,
            "account_id": "team-1",
            "refresh_token": "refresh.user-b"
        }
    });

    assert!(import_single_item(&storage, &mut idx, &user_a, 1).expect("import user a"));
    assert!(import_single_item(&storage, &mut idx, &user_b, 2).expect("import user b"));

    let accounts = storage.list_accounts().expect("list accounts");
    assert_eq!(accounts.len(), 2);
    assert!(accounts
        .iter()
        .any(|account| account.id.starts_with("user-a::")));
    assert!(accounts
        .iter()
        .any(|account| account.id.starts_with("user-b::")));
    assert!(accounts
        .iter()
        .all(|account| account.workspace_id.as_deref() == Some("team-1")));

    assert!(!import_single_item(&storage, &mut idx, &user_a, 3).expect("reimport user a"));
    assert_eq!(storage.list_accounts().expect("list accounts").len(), 2);
}

/// 函数 `import_single_item_distinguishes_same_subject_with_different_chatgpt_accounts`
///
/// 作者: gaohongshun
///
/// 时间: 2026-05-08
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn import_single_item_distinguishes_same_subject_with_different_chatgpt_accounts() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init");
    let mut idx = ExistingAccountIndex::build(&storage).expect("build index");

    let team = json!({
        "tokens": {
            "access_token": "team.access",
            "id_token": TEST_ID_TOKEN_SAME_SUB_TEAM,
            "refresh_token": "team.refresh"
        }
    });
    let plus = json!({
        "tokens": {
            "access_token": "plus.access",
            "id_token": TEST_ID_TOKEN_SAME_SUB_PLUS,
            "refresh_token": "plus.refresh"
        }
    });

    assert!(import_single_item(&storage, &mut idx, &team, 1).expect("import team"));
    assert!(import_single_item(&storage, &mut idx, &plus, 2).expect("import plus"));

    let accounts = storage.list_accounts().expect("list accounts");
    assert_eq!(accounts.len(), 2);
    assert!(accounts
        .iter()
        .any(|account| account.chatgpt_account_id.as_deref() == Some("cgpt-team")));
    assert!(accounts
        .iter()
        .any(|account| account.chatgpt_account_id.as_deref() == Some("cgpt-plus")));

    assert!(!import_single_item(&storage, &mut idx, &team, 3).expect("reimport team"));
    assert_eq!(storage.list_accounts().expect("list accounts").len(), 2);
}

/// 函数 `import_single_item_distinguishes_same_subject_across_team_workspaces`
///
/// 作者: gaohongshun
///
/// 时间: 2026-05-08
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn import_single_item_distinguishes_same_subject_across_team_workspaces() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init");
    let mut idx = ExistingAccountIndex::build(&storage).expect("build index");

    let team_a = json!({
        "tokens": {
            "access_token": "team-a.access",
            "id_token": TEST_ID_TOKEN_SAME_SUB_TEAM_A,
            "refresh_token": "team-a.refresh"
        }
    });
    let team_b = json!({
        "tokens": {
            "access_token": "team-b.access",
            "id_token": TEST_ID_TOKEN_SAME_SUB_TEAM_B,
            "refresh_token": "team-b.refresh"
        }
    });

    assert!(import_single_item(&storage, &mut idx, &team_a, 1).expect("import team a"));
    assert!(import_single_item(&storage, &mut idx, &team_b, 2).expect("import team b"));

    let accounts = storage.list_accounts().expect("list accounts");
    assert_eq!(accounts.len(), 2);
    assert!(accounts
        .iter()
        .any(|account| account.workspace_id.as_deref() == Some("ws-team-a")));
    assert!(accounts
        .iter()
        .any(|account| account.workspace_id.as_deref() == Some("ws-team-b")));
}

/// 函数 `import_single_item_restores_account_when_old_import_overwrote_scoped_identity`
///
/// 作者: gaohongshun
///
/// 时间: 2026-05-08
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn import_single_item_restores_account_when_old_import_overwrote_scoped_identity() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init");
    let now = now_ts();
    let old_team_scoped_id =
        build_account_storage_id("same-user", Some("cgpt-team"), Some("ws-shared"), None);
    storage
        .insert_account(&Account {
            id: old_team_scoped_id.clone(),
            label: "same@example.com".to_string(),
            issuer: "https://auth.openai.com".to_string(),
            chatgpt_account_id: Some("cgpt-plus".to_string()),
            workspace_id: Some("ws-shared".to_string()),
            group_name: Some("IMPORT".to_string()),
            sort: 0,
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
        })
        .expect("insert old overwritten account");
    storage
        .insert_token(&Token {
            account_id: old_team_scoped_id.clone(),
            id_token: TEST_ID_TOKEN_SAME_SUB_PLUS.to_string(),
            access_token: "plus.access.old".to_string(),
            refresh_token: "plus.refresh.old".to_string(),
            api_key_access_token: None,
            last_refresh: now,
        })
        .expect("insert plus token");

    let mut idx = ExistingAccountIndex::build(&storage).expect("build index");
    let team = json!({
        "tokens": {
            "access_token": "team.access",
            "id_token": TEST_ID_TOKEN_SAME_SUB_TEAM,
            "refresh_token": "team.refresh"
        }
    });
    let plus = json!({
        "tokens": {
            "access_token": "plus.access",
            "id_token": TEST_ID_TOKEN_SAME_SUB_PLUS,
            "refresh_token": "plus.refresh"
        }
    });

    assert!(import_single_item(&storage, &mut idx, &team, 1).expect("restore team"));
    assert!(!import_single_item(&storage, &mut idx, &plus, 2).expect("refresh plus"));

    let accounts = storage.list_accounts().expect("list accounts");
    assert_eq!(accounts.len(), 2);
    assert!(accounts
        .iter()
        .any(|account| account.chatgpt_account_id.as_deref() == Some("cgpt-team")));
    assert!(accounts
        .iter()
        .any(|account| account.chatgpt_account_id.as_deref() == Some("cgpt-plus")));
}

/// 函数 `import_single_item_reuses_legacy_team_account_when_token_subject_matches`
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
fn import_single_item_reuses_legacy_team_account_when_token_subject_matches() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init");
    let now = now_ts();
    storage
        .insert_account(&Account {
            id: "team-1".to_string(),
            label: "legacy team account".to_string(),
            issuer: "https://auth.openai.com".to_string(),
            chatgpt_account_id: Some("team-1".to_string()),
            workspace_id: Some("team-1".to_string()),
            group_name: None,
            sort: 0,
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
        })
        .expect("insert legacy account");
    storage
        .insert_token(&Token {
            account_id: "team-1".to_string(),
            id_token: "".to_string(),
            access_token: TEST_ACCESS_TOKEN_TEAM_USER_A.to_string(),
            refresh_token: "refresh.user-a.old".to_string(),
            api_key_access_token: None,
            last_refresh: now,
        })
        .expect("insert legacy token");

    let mut idx = ExistingAccountIndex::build(&storage).expect("build index");
    let item = json!({
        "tokens": {
            "access_token": TEST_ACCESS_TOKEN_TEAM_USER_A,
            "account_id": "team-1",
            "refresh_token": "refresh.user-a.new"
        }
    });

    let created = import_single_item(&storage, &mut idx, &item, 1).expect("import item");
    assert!(!created);

    let accounts = storage.list_accounts().expect("list accounts");
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0].id, "team-1");
    let token = storage
        .find_token_by_account_id("team-1")
        .expect("find token")
        .expect("token");
    assert_eq!(token.refresh_token, "refresh.user-a.new");
}

/// 函数 `import_single_item_prefers_meta_fields_for_new_account`
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
fn import_single_item_prefers_meta_fields_for_new_account() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init");
    let mut idx = ExistingAccountIndex::build(&storage).expect("build index");
    let item = json!({
        "tokens": {
            "access_token": "access.meta",
            "id_token": TEST_ID_TOKEN_META,
            "refresh_token": "refresh.meta",
            "account_id": "exported-account-id"
        },
        "meta": {
            "label": "Meta Label",
            "issuer": "https://issuer.example",
            "note": "Meta Note",
            "tags": ["高频", "团队A"],
            "workspace_id": "ws-manual",
            "chatgpt_account_id": "cgpt-manual"
        }
    });

    let created = import_single_item(&storage, &mut idx, &item, 1).expect("import item");
    assert!(created);

    let accounts = storage.list_accounts().expect("list accounts");
    assert_eq!(accounts.len(), 1);
    assert_eq!(
        accounts[0].id,
        build_account_storage_id("sub-1", Some("cgpt-manual"), Some("ws-manual"), None)
    );
    assert_eq!(accounts[0].label, "Meta Label");
    assert_eq!(accounts[0].issuer, "https://issuer.example");
    assert_eq!(accounts[0].group_name, None);
    assert_eq!(
        accounts[0].chatgpt_account_id.as_deref(),
        Some("cgpt-manual")
    );
    assert_eq!(accounts[0].workspace_id.as_deref(), Some("ws-manual"));
    let metadata = storage
        .find_account_metadata(&accounts[0].id)
        .expect("find metadata")
        .expect("metadata");
    assert_eq!(metadata.note.as_deref(), Some("Meta Note"));
    assert_eq!(metadata.tags.as_deref(), Some("高频,团队A"));
}

/// 函数 `import_single_item_allows_missing_id_and_refresh_tokens`
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
fn import_single_item_allows_missing_id_and_refresh_tokens() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init");
    let mut idx = ExistingAccountIndex::build(&storage).expect("build index");
    let item = json!({
        "tokens": {
            "access_token": "access.only",
            "account_id": "legacy-import-id"
        },
        "meta": {
            "label": "Only Access Token",
            "workspace_id": "ws-manual",
            "chatgpt_account_id": "cgpt-manual"
        }
    });

    let created = import_single_item(&storage, &mut idx, &item, 1).expect("import item");
    assert!(created);

    let accounts = storage.list_accounts().expect("list accounts");
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0].label, "Only Access Token");
    assert_eq!(
        accounts[0].chatgpt_account_id.as_deref(),
        Some("cgpt-manual")
    );
    assert_eq!(accounts[0].workspace_id.as_deref(), Some("ws-manual"));

    let token = storage
        .find_token_by_account_id(&accounts[0].id)
        .expect("find token")
        .expect("token");
    assert_eq!(token.access_token, "access.only");
    assert_eq!(token.id_token, "");
    assert_eq!(token.refresh_token, "");
}

/// 函数 `import_account_auth_json_keeps_valid_items_when_one_content_is_invalid`
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
fn import_account_auth_json_keeps_valid_items_when_one_content_is_invalid() {
    let storage = Storage::open_in_memory().expect("open storage");
    storage.init().expect("init storage");

    let result = import_account_auth_json_with_storage(
        &storage,
        vec![
            json!({
                "type": "codex",
                "email": "valid@example.com",
                "id_token": TEST_ID_TOKEN_META,
                "account_id": "valid-account",
                "access_token": "access.valid",
                "refresh_token": "refresh.valid"
            })
            .to_string(),
            "not-json".to_string(),
        ],
        false,
    )
    .expect("import account auth json");

    assert_eq!(result.total, 2);
    assert_eq!(result.created, 1);
    assert_eq!(result.updated, 0);
    assert_eq!(result.failed, 1);
    assert_eq!(result.imported_account_ids.len(), 1);
    assert!(result.imported_account_ids.iter().all(|id| !id.is_empty()));
    assert!(result
        .errors
        .iter()
        .any(|item| { item.message.contains("invalid JSON object stream") }));

    let accounts = storage.list_accounts().expect("list accounts");
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0].label, "meta@example.com");
}

/// 函数 `import_account_auth_json_handles_large_multi_batch_payload`
///
/// 作者: gaohongshun
///
/// 时间: 2026-06-11
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn import_account_auth_json_handles_large_multi_batch_payload() {
    let storage = Storage::open_in_memory().expect("open storage");
    storage.init().expect("init storage");

    let contents = (0..1000)
        .map(|index| {
            json!({
                "type": "codex",
                "email": format!("bulk-{index}@example.com"),
                "account_id": format!("bulk-account-{index}"),
                "chatgpt_account_id": format!("bulk-chatgpt-{index}"),
                "workspace_id": format!("bulk-workspace-{index}"),
                "access_token": format!("access.bulk.{index}"),
                "refresh_token": format!("refresh.bulk.{index}")
            })
            .to_string()
        })
        .collect::<Vec<_>>();

    let result = import_account_auth_json_with_storage(&storage, contents, false)
        .expect("import account auth json");

    assert_eq!(result.total, 1000);
    assert_eq!(result.created, 1000);
    assert_eq!(result.updated, 0);
    assert_eq!(result.failed, 0);
    assert_eq!(result.imported_account_ids.len(), 1000);
    assert!(result.errors.is_empty());

    assert_eq!(storage.list_accounts().expect("list accounts").len(), 1000);
}
