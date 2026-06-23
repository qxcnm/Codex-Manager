use super::{
    build_warmup_headers, consume_warmup_stream, resolve_target_accounts,
    resolve_warmup_model_slug, should_retry_warmup_with_refresh, DEFAULT_WARMUP_MODEL,
};
use crate::apikey_models::save_managed_model_catalog_with_storage;
use codexmanager_core::rpc::types::{
    ManagedModelCatalogEntry, ManagedModelCatalogResult, ModelInfo,
};
use codexmanager_core::storage::{now_ts, Account, Storage, Token};
use std::io::Cursor;

fn make_model(slug: &str, sort_index: i64, supported_in_api: bool) -> ManagedModelCatalogEntry {
    ManagedModelCatalogEntry {
        model: ModelInfo {
            slug: slug.to_string(),
            display_name: slug.to_string(),
            supported_in_api,
            visibility: Some("list".to_string()),
            ..ModelInfo::default()
        },
        sort_index,
        ..ManagedModelCatalogEntry::default()
    }
}

#[test]
fn resolve_warmup_model_slug_uses_first_supported_model_from_catalog_order() {
    let storage = Storage::open_in_memory().expect("open in-memory storage");
    storage.init().expect("init in-memory storage");
    let mut hidden = make_model("gpt-hidden", 0, true);
    hidden.model.visibility = Some("hidden".to_string());
    save_managed_model_catalog_with_storage(
        &storage,
        &ManagedModelCatalogResult {
            items: vec![
                hidden,
                make_model("gpt-unsupported", 1, false),
                make_model("gpt-latest", 1, true),
                make_model("gpt-older", 2, true),
            ],
            ..ManagedModelCatalogResult::default()
        },
    )
    .expect("save model catalog");

    assert_eq!(resolve_warmup_model_slug(&storage), "gpt-latest");
}

#[test]
fn resolve_warmup_model_slug_falls_back_when_catalog_missing() {
    let storage = Storage::open_in_memory().expect("open in-memory storage");
    storage.init().expect("init in-memory storage");
    assert_eq!(resolve_warmup_model_slug(&storage), DEFAULT_WARMUP_MODEL);
}

#[test]
fn should_retry_warmup_with_refresh_only_for_auth_errors_with_refresh_token() {
    let mut token = Token {
        account_id: "account-1".to_string(),
        id_token: String::new(),
        access_token: String::new(),
        refresh_token: "refresh-token".to_string(),
        api_key_access_token: None,
        last_refresh: 0,
    };

    assert!(should_retry_warmup_with_refresh(
        &token,
        "status=401 body=Unauthorized"
    ));
    assert!(!should_retry_warmup_with_refresh(
        &token,
        "status=500 body=server error"
    ));

    token.refresh_token.clear();
    assert!(!should_retry_warmup_with_refresh(
        &token,
        "status=401 body=Unauthorized"
    ));
}

#[test]
fn resolve_target_accounts_only_returns_gateway_available_accounts() {
    let storage = Storage::open_in_memory().expect("open in-memory storage");
    storage.init().expect("init in-memory storage");
    let now = now_ts();

    for (id, status) in [
        ("acc-active", "active"),
        ("acc-unavailable", "unavailable"),
        ("acc-disabled", "disabled"),
        ("acc-banned", "banned"),
        ("acc-inactive", "inactive"),
    ] {
        storage
            .insert_account(&Account {
                id: id.to_string(),
                label: id.to_string(),
                issuer: "issuer".to_string(),
                chatgpt_account_id: None,
                workspace_id: None,
                group_name: None,
                sort: 0,
                status: status.to_string(),
                created_at: now,
                updated_at: now,
            })
            .expect("insert account");
        storage
            .insert_token(&Token {
                account_id: id.to_string(),
                id_token: "id-token".to_string(),
                access_token: "access-token".to_string(),
                refresh_token: "refresh-token".to_string(),
                api_key_access_token: None,
                last_refresh: now,
            })
            .expect("insert token");
    }

    let all_targets = resolve_target_accounts(&storage, &[]).expect("resolve all targets");
    assert_eq!(all_targets.len(), 1);
    assert_eq!(all_targets[0].account.id, "acc-active");
    assert_eq!(all_targets[0].token.account_id, "acc-active");

    let selected_targets = resolve_target_accounts(
        &storage,
        &[
            "acc-unavailable".to_string(),
            "acc-active".to_string(),
            "acc-disabled".to_string(),
        ],
    )
    .expect("resolve selected targets");
    assert_eq!(selected_targets.len(), 1);
    assert_eq!(selected_targets[0].account.id, "acc-active");
    assert_eq!(selected_targets[0].token.account_id, "acc-active");
}

#[test]
fn build_warmup_headers_omits_non_codex_headers() {
    let account = Account {
        id: "acc-1".to_string(),
        label: "acc-1".to_string(),
        issuer: "issuer".to_string(),
        chatgpt_account_id: None,
        workspace_id: None,
        group_name: None,
        sort: 0,
        status: "active".to_string(),
        created_at: 0,
        updated_at: 0,
    };

    let headers = build_warmup_headers(&account, "bearer-token").expect("build warmup headers");

    assert!(headers.get("version").is_none());
    assert!(headers.get("openai-organization").is_none());
    assert!(headers.get("openai-project").is_none());
    assert!(headers.get("client_version").is_none());
}

#[test]
fn consume_warmup_stream_waits_for_response_completed() {
    let stream = Cursor::new(
        "event: response.created\n\
         data: {\"type\":\"response.created\"}\n\n\
         event: response.completed\n\
         data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\"}}\n\n",
    );

    assert!(consume_warmup_stream(stream).is_ok());
}

#[test]
fn consume_warmup_stream_rejects_incomplete_stream() {
    let stream = Cursor::new(
        "event: response.created\n\
         data: {\"type\":\"response.created\"}\n\n",
    );

    let err = consume_warmup_stream(stream).expect_err("stream should be incomplete");
    assert!(err.contains("before response.completed"));
}

#[test]
fn consume_warmup_stream_reports_error_event() {
    let stream = Cursor::new(
        "event: response.failed\n\
         data: {\"type\":\"response.failed\",\"error\":{\"message\":\"quota exceeded\"}}\n\n",
    );

    let err = consume_warmup_stream(stream).expect_err("stream should fail");
    assert!(err.contains("quota exceeded"));
}
