use codexmanager_core::storage::{
    now_ts, Account, AggregateApi, ApiKey, ApiKeyOwner, AppUser, AppWalletLedgerEntry, Event,
    ModelGroup, ModelSourceMapping, ModelSourceModel, RequestLog, RequestTokenStat, Storage, Token,
    UsageSnapshotRecord, UserModelGroup,
};

/// 函数 `storage_can_insert_account_and_token`
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
fn storage_can_insert_account_and_token() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init schema");

    let account = Account {
        id: "acc-1".to_string(),
        label: "main".to_string(),
        issuer: "https://auth.openai.com".to_string(),
        chatgpt_account_id: Some("acct_123".to_string()),
        workspace_id: Some("org_123".to_string()),
        group_name: None,
        sort: 0,
        status: "healthy".to_string(),
        created_at: now_ts(),
        updated_at: now_ts(),
    };
    storage.insert_account(&account).expect("insert account");

    let token = Token {
        account_id: "acc-1".to_string(),
        id_token: "id".to_string(),
        access_token: "access".to_string(),
        refresh_token: "refresh".to_string(),
        api_key_access_token: None,
        last_refresh: now_ts(),
    };
    storage.insert_token(&token).expect("insert token");

    assert_eq!(storage.account_count().expect("count accounts"), 1);
    assert_eq!(storage.token_count().expect("count tokens"), 1);
}

/// 函数 `storage_can_find_token_and_account_by_account_id`
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
fn storage_can_find_token_and_account_by_account_id() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init schema");

    let account = Account {
        id: "acc-find-1".to_string(),
        label: "main".to_string(),
        issuer: "https://auth.openai.com".to_string(),
        chatgpt_account_id: Some("acct_find".to_string()),
        workspace_id: Some("org_find".to_string()),
        group_name: None,
        sort: 0,
        status: "active".to_string(),
        created_at: now_ts(),
        updated_at: now_ts(),
    };
    storage.insert_account(&account).expect("insert account");

    let token = Token {
        account_id: "acc-find-1".to_string(),
        id_token: "id-find".to_string(),
        access_token: "access-find".to_string(),
        refresh_token: "refresh-find".to_string(),
        api_key_access_token: Some("api-key-find".to_string()),
        last_refresh: now_ts(),
    };
    storage.insert_token(&token).expect("insert token");

    let found_account = storage
        .find_account_by_id("acc-find-1")
        .expect("find account")
        .expect("account exists");
    assert_eq!(found_account.id, "acc-find-1");

    let found_token = storage
        .find_token_by_account_id("acc-find-1")
        .expect("find token")
        .expect("token exists");
    assert_eq!(found_token.account_id, "acc-find-1");
    assert_eq!(
        found_token.api_key_access_token.as_deref(),
        Some("api-key-find")
    );

    assert!(storage
        .find_account_by_id("missing-account")
        .expect("find missing account")
        .is_none());
    assert!(storage
        .find_token_by_account_id("missing-account")
        .expect("find missing token")
        .is_none());
}

#[test]
fn storage_can_upsert_and_resolve_model_source_mappings() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init schema");
    let now = now_ts();

    storage
        .upsert_model_source_model(&ModelSourceModel {
            source_kind: "openai_account".to_string(),
            source_id: "acc-routing-1".to_string(),
            upstream_model: "gpt-upstream".to_string(),
            display_name: Some("GPT Upstream".to_string()),
            status: "available".to_string(),
            discovery_kind: "manual".to_string(),
            last_synced_at: Some(now),
            extra_json: "{}".to_string(),
            created_at: now,
            updated_at: now,
        })
        .expect("upsert source model");
    storage
        .upsert_model_source_mapping(&ModelSourceMapping {
            id: "map-routing-1".to_string(),
            platform_model_slug: "gpt-platform".to_string(),
            source_kind: "openai_account".to_string(),
            source_id: "acc-routing-1".to_string(),
            upstream_model: "gpt-upstream".to_string(),
            enabled: true,
            priority: 2,
            weight: 3,
            billing_model_slug: Some("gpt-billing".to_string()),
            created_at: now,
            updated_at: now,
        })
        .expect("upsert mapping");

    let source_models = storage
        .list_model_source_models(Some("openai_account"), Some("acc-routing-1"))
        .expect("list source models");
    assert_eq!(source_models.len(), 1);
    assert_eq!(source_models[0].upstream_model, "gpt-upstream");
    assert_eq!(source_models[0].discovery_kind, "manual");

    let enabled = storage
        .list_enabled_model_source_mappings_for_platform("gpt-platform")
        .expect("list enabled mappings");
    assert_eq!(enabled.len(), 1);
    assert_eq!(enabled[0].priority, 2);
    assert_eq!(enabled[0].weight, 3);
    assert_eq!(
        enabled[0].billing_model_slug.as_deref(),
        Some("gpt-billing")
    );

    let account_mapping = storage
        .find_enabled_model_source_mapping("gpt-platform", "openai_account", "acc-routing-1")
        .expect("find enabled mapping")
        .expect("mapping exists");
    assert_eq!(account_mapping.upstream_model, "gpt-upstream");

    storage
        .delete_model_source_mapping("map-routing-1")
        .expect("delete mapping");
    assert!(storage
        .find_enabled_model_source_mapping("gpt-platform", "openai_account", "acc-routing-1")
        .expect("find deleted mapping")
        .is_none());
}

#[test]
fn upsert_discovered_source_models_prunes_stale_discovered_routes() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init schema");
    let now = now_ts();

    storage
        .upsert_model_source_model(&ModelSourceModel {
            source_kind: "openai_account".to_string(),
            source_id: "acc-sync-prune".to_string(),
            upstream_model: "deepseek-v4-pro".to_string(),
            display_name: Some("deepseek-v4-pro".to_string()),
            status: "available".to_string(),
            discovery_kind: "synced".to_string(),
            last_synced_at: Some(now),
            extra_json: "{}".to_string(),
            created_at: now,
            updated_at: now,
        })
        .expect("seed stale discovered model");
    storage
        .upsert_model_source_model(&ModelSourceModel {
            source_kind: "openai_account".to_string(),
            source_id: "acc-sync-prune".to_string(),
            upstream_model: "manual-keep".to_string(),
            display_name: Some("manual-keep".to_string()),
            status: "available".to_string(),
            discovery_kind: "manual".to_string(),
            last_synced_at: Some(now),
            extra_json: "{}".to_string(),
            created_at: now,
            updated_at: now,
        })
        .expect("seed manual model");

    storage
        .upsert_model_source_mapping(&ModelSourceMapping {
            id: "map-stale-discovered".to_string(),
            platform_model_slug: "deepseek-v4-pro".to_string(),
            source_kind: "openai_account".to_string(),
            source_id: "acc-sync-prune".to_string(),
            upstream_model: "deepseek-v4-pro".to_string(),
            enabled: true,
            priority: 0,
            weight: 1,
            billing_model_slug: None,
            created_at: now,
            updated_at: now,
        })
        .expect("seed stale discovered mapping");
    storage
        .upsert_model_source_mapping(&ModelSourceMapping {
            id: "map-manual-keep".to_string(),
            platform_model_slug: "manual-keep".to_string(),
            source_kind: "openai_account".to_string(),
            source_id: "acc-sync-prune".to_string(),
            upstream_model: "manual-keep".to_string(),
            enabled: true,
            priority: 0,
            weight: 1,
            billing_model_slug: None,
            created_at: now,
            updated_at: now,
        })
        .expect("seed manual mapping");

    storage
        .upsert_discovered_model_source_models(
            "openai_account",
            "acc-sync-prune",
            &["gpt-4.1".to_string()],
            "synced",
        )
        .expect("sync discovered models");

    let source_models = storage
        .list_model_source_models(Some("openai_account"), Some("acc-sync-prune"))
        .expect("list source models");
    assert!(source_models
        .iter()
        .any(|item| item.upstream_model == "gpt-4.1" && item.discovery_kind == "synced"));
    assert!(source_models
        .iter()
        .any(|item| item.upstream_model == "manual-keep" && item.discovery_kind == "manual"));
    assert!(!source_models
        .iter()
        .any(|item| item.upstream_model == "deepseek-v4-pro"));

    let stale_mappings = storage
        .list_model_source_mappings(Some("deepseek-v4-pro"))
        .expect("list stale mappings");
    assert!(stale_mappings.is_empty());
    let manual_mappings = storage
        .list_model_source_mappings(Some("manual-keep"))
        .expect("list manual mappings");
    assert_eq!(manual_mappings.len(), 1);
}

#[test]
fn delete_account_removes_openai_model_source_routes() {
    let mut storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init schema");
    let now = now_ts();
    storage
        .insert_account(&Account {
            id: "acc-routing-delete".to_string(),
            label: "delete route".to_string(),
            issuer: "https://auth.openai.com".to_string(),
            chatgpt_account_id: None,
            workspace_id: None,
            group_name: None,
            sort: 0,
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
        })
        .expect("insert account");
    storage
        .upsert_model_source_model(&ModelSourceModel {
            source_kind: "openai_account".to_string(),
            source_id: "acc-routing-delete".to_string(),
            upstream_model: "gpt-platform".to_string(),
            display_name: Some("GPT Platform".to_string()),
            status: "available".to_string(),
            discovery_kind: "synced".to_string(),
            last_synced_at: Some(now),
            extra_json: "{}".to_string(),
            created_at: now,
            updated_at: now,
        })
        .expect("upsert source model");
    storage
        .upsert_model_source_mapping(&ModelSourceMapping {
            id: "map-routing-delete".to_string(),
            platform_model_slug: "gpt-platform".to_string(),
            source_kind: "openai_account".to_string(),
            source_id: "acc-routing-delete".to_string(),
            upstream_model: "gpt-platform".to_string(),
            enabled: true,
            priority: 0,
            weight: 1,
            billing_model_slug: None,
            created_at: now,
            updated_at: now,
        })
        .expect("upsert mapping");

    storage
        .delete_account("acc-routing-delete")
        .expect("delete account");

    assert!(storage
        .list_model_source_models(Some("openai_account"), Some("acc-routing-delete"))
        .expect("list source models")
        .is_empty());
    assert!(storage
        .list_model_source_mappings(Some("gpt-platform"))
        .expect("list mappings")
        .is_empty());
}

/// 函数 `token_upsert_keeps_refresh_schedule_columns`
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
fn token_upsert_keeps_refresh_schedule_columns() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init schema");
    let account = Account {
        id: "acc-schedule-1".to_string(),
        label: "main".to_string(),
        issuer: "https://auth.openai.com".to_string(),
        chatgpt_account_id: None,
        workspace_id: None,
        group_name: None,
        sort: 0,
        status: "active".to_string(),
        created_at: now_ts(),
        updated_at: now_ts(),
    };
    storage.insert_account(&account).expect("insert account");

    let token = Token {
        account_id: "acc-schedule-1".to_string(),
        id_token: "id-1".to_string(),
        access_token: "access-1".to_string(),
        refresh_token: "refresh-1".to_string(),
        api_key_access_token: None,
        last_refresh: now_ts(),
    };
    storage.insert_token(&token).expect("insert token");
    storage
        .update_token_refresh_schedule("acc-schedule-1", Some(4_102_444_800), Some(4_102_444_200))
        .expect("set schedule");

    let token2 = Token {
        account_id: "acc-schedule-1".to_string(),
        id_token: "id-2".to_string(),
        access_token: "access-2".to_string(),
        refresh_token: "refresh-2".to_string(),
        api_key_access_token: Some("api-key".to_string()),
        last_refresh: now_ts(),
    };
    storage.insert_token(&token2).expect("upsert token");

    let due = storage
        .list_tokens_due_for_refresh(4_102_444_100, 4_102_444_700, 10)
        .expect("list due");
    assert!(due.is_empty());
    let due2 = storage
        .list_tokens_due_for_refresh(4_102_444_300, 4_102_444_900, 10)
        .expect("list due2");
    assert_eq!(due2.len(), 1);
    assert_eq!(due2[0].account_id, "acc-schedule-1");
}

/// 函数 `tokens_due_for_refresh_uses_access_exp_when_next_refresh_is_stale`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-26
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn tokens_due_for_refresh_uses_access_exp_when_next_refresh_is_stale() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init schema");
    let now = now_ts();
    let account = Account {
        id: "acc-stale-next-refresh".to_string(),
        label: "stale next refresh".to_string(),
        issuer: "https://auth.openai.com".to_string(),
        chatgpt_account_id: None,
        workspace_id: None,
        group_name: None,
        sort: 0,
        status: "active".to_string(),
        created_at: now,
        updated_at: now,
    };
    storage.insert_account(&account).expect("insert account");
    storage
        .insert_token(&Token {
            account_id: account.id.clone(),
            id_token: "id".to_string(),
            access_token: "access".to_string(),
            refresh_token: "refresh".to_string(),
            api_key_access_token: None,
            last_refresh: now,
        })
        .expect("insert token");
    storage
        .update_token_refresh_schedule(&account.id, Some(4_102_444_800), Some(4_102_999_999))
        .expect("set stale schedule");

    let due = storage
        .list_tokens_due_for_refresh(4_102_444_100, 4_102_444_700, 10)
        .expect("list due before access exp window");
    assert!(due.is_empty());

    let due = storage
        .list_tokens_due_for_refresh(4_102_444_100, 4_102_444_900, 10)
        .expect("list due after access exp window");
    assert_eq!(due.len(), 1);
    assert_eq!(due[0].account_id, account.id);
}

/// 函数 `tokens_due_for_refresh_include_other_unavailable_accounts_but_skip_deactivated`
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
fn tokens_due_for_refresh_include_other_unavailable_accounts_but_skip_deactivated() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init schema");
    let now = now_ts();

    for (id, status) in [
        ("acc-active-refresh", "active"),
        ("acc-region-blocked-refresh", "unavailable"),
        ("acc-unavailable-refresh", "unavailable"),
        ("acc-deactivated-refresh", "banned"),
    ] {
        storage
            .insert_account(&Account {
                id: id.to_string(),
                label: id.to_string(),
                issuer: "https://auth.openai.com".to_string(),
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
                id_token: format!("id-{id}"),
                access_token: format!("access-{id}"),
                refresh_token: format!("refresh-{id}"),
                api_key_access_token: None,
                last_refresh: now,
            })
            .expect("insert token");
        storage
            .update_token_refresh_schedule(id, Some(4_102_444_800), Some(4_102_444_200))
            .expect("set schedule");
    }
    storage
        .insert_event(&Event {
            account_id: Some("acc-deactivated-refresh".to_string()),
            event_type: "account_status_update".to_string(),
            message: "status=banned reason=account_deactivated".to_string(),
            created_at: now + 1,
        })
        .expect("insert deactivated event");
    storage
        .insert_event(&Event {
            account_id: Some("acc-region-blocked-refresh".to_string()),
            event_type: "account_status_update".to_string(),
            message: "status=unavailable reason=refresh_token_region_blocked".to_string(),
            created_at: now + 1,
        })
        .expect("insert region blocked event");

    let due = storage
        .list_tokens_due_for_refresh(4_102_444_300, 4_102_444_900, 10)
        .expect("list due");
    let account_ids = due
        .into_iter()
        .map(|token| token.account_id)
        .collect::<Vec<_>>();
    assert_eq!(
        account_ids,
        vec![
            "acc-active-refresh".to_string(),
            "acc-unavailable-refresh".to_string()
        ]
    );

    storage
        .insert_event(&Event {
            account_id: Some("acc-region-blocked-refresh".to_string()),
            event_type: "account_status_update".to_string(),
            message: "status=active reason=manual_enable".to_string(),
            created_at: now + 2,
        })
        .expect("insert manual enable event");
    let recovered_due = storage
        .list_tokens_due_for_refresh(4_102_444_300, 4_102_444_900, 10)
        .expect("list due after manual enable")
        .into_iter()
        .map(|token| token.account_id)
        .collect::<Vec<_>>();
    assert!(recovered_due.contains(&"acc-region-blocked-refresh".to_string()));
}

/// 函数 `storage_login_session_roundtrip`
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
fn storage_login_session_roundtrip() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init schema");

    let session = codexmanager_core::storage::LoginSession {
        login_id: "login-1".to_string(),
        code_verifier: "verifier".to_string(),
        state: "state".to_string(),
        status: "pending".to_string(),
        error: None,
        workspace_id: Some("org_123".to_string()),
        note: None,
        tags: None,
        group_name: None,
        created_at: now_ts(),
        updated_at: now_ts(),
    };
    storage
        .insert_login_session(&session)
        .expect("insert session");
    let loaded = storage
        .get_login_session("login-1")
        .expect("load session")
        .expect("session exists");
    assert_eq!(loaded.status, "pending");
    assert_eq!(loaded.workspace_id.as_deref(), Some("org_123"));
}

/// 函数 `storage_account_metadata_roundtrip_and_delete_cleanup`
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
fn storage_account_metadata_roundtrip_and_delete_cleanup() {
    let mut storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init schema");

    let account = Account {
        id: "acc-meta-1".to_string(),
        label: "metadata account".to_string(),
        issuer: "https://auth.openai.com".to_string(),
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
        .upsert_account_metadata("acc-meta-1", Some("主账号"), Some("高频,团队A"))
        .expect("upsert metadata");

    let metadata = storage
        .find_account_metadata("acc-meta-1")
        .expect("find metadata")
        .expect("metadata exists");
    assert_eq!(metadata.note.as_deref(), Some("主账号"));
    assert_eq!(metadata.tags.as_deref(), Some("高频,团队A"));

    storage
        .delete_account("acc-meta-1")
        .expect("delete account");
    assert!(storage
        .find_account_metadata("acc-meta-1")
        .expect("find metadata after delete")
        .is_none());
}

/// 函数 `storage_account_subscription_roundtrip_and_delete_cleanup`
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
fn storage_account_subscription_roundtrip_and_delete_cleanup() {
    let mut storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init schema");

    let account = Account {
        id: "acc-sub-1".to_string(),
        label: "subscription account".to_string(),
        issuer: "https://auth.openai.com".to_string(),
        chatgpt_account_id: Some("org-sub-1".to_string()),
        workspace_id: Some("org-sub-1".to_string()),
        group_name: None,
        sort: 0,
        status: "active".to_string(),
        created_at: now_ts(),
        updated_at: now_ts(),
    };
    storage.insert_account(&account).expect("insert account");
    storage
        .upsert_account_subscription(
            "acc-sub-1",
            true,
            Some("pro"),
            Some("plus"),
            Some(1_746_501_889),
            Some(1_746_501_889),
        )
        .expect("upsert subscription");

    let subscription = storage
        .find_account_subscription("acc-sub-1")
        .expect("find subscription")
        .expect("subscription exists");
    assert!(subscription.has_subscription);
    assert_eq!(subscription.account_plan_type.as_deref(), Some("pro"));
    assert_eq!(subscription.plan_type.as_deref(), Some("plus"));
    assert_eq!(subscription.expires_at, Some(1_746_501_889));
    assert_eq!(subscription.renews_at, Some(1_746_501_889));

    storage.delete_account("acc-sub-1").expect("delete account");
    assert!(storage
        .find_account_subscription("acc-sub-1")
        .expect("find subscription after delete")
        .is_none());
}

/// 函数 `storage_can_update_account_status`
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
fn storage_can_update_account_status() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init schema");

    let account = Account {
        id: "acc-1".to_string(),
        label: "main".to_string(),
        issuer: "https://auth.openai.com".to_string(),
        chatgpt_account_id: Some("acct_123".to_string()),
        workspace_id: Some("org_123".to_string()),
        group_name: None,
        sort: 0,
        status: "active".to_string(),
        created_at: now_ts(),
        updated_at: now_ts(),
    };
    storage.insert_account(&account).expect("insert account");

    storage
        .update_account_status("acc-1", "inactive")
        .expect("update status");

    let loaded = storage
        .list_accounts()
        .expect("list accounts")
        .into_iter()
        .find(|acc| acc.id == "acc-1")
        .expect("account exists");

    assert_eq!(loaded.status, "inactive");
}

/// 函数 `storage_updates_account_status_only_when_changed`
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
fn storage_updates_account_status_only_when_changed() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init schema");

    let account = Account {
        id: "acc-conditional-1".to_string(),
        label: "main".to_string(),
        issuer: "https://auth.openai.com".to_string(),
        chatgpt_account_id: Some("acct_123".to_string()),
        workspace_id: Some("org_123".to_string()),
        group_name: None,
        sort: 0,
        status: "active".to_string(),
        created_at: now_ts(),
        updated_at: now_ts(),
    };
    storage.insert_account(&account).expect("insert account");

    let unchanged = storage
        .update_account_status_if_changed("acc-conditional-1", "active")
        .expect("conditional update unchanged");
    assert!(!unchanged);

    let changed = storage
        .update_account_status_if_changed("acc-conditional-1", "inactive")
        .expect("conditional update changed");
    assert!(changed);

    let loaded = storage
        .find_account_by_id("acc-conditional-1")
        .expect("find account")
        .expect("account exists");
    assert_eq!(loaded.status, "inactive");
}

/// 函数 `storage_gateway_candidates_exclude_unavailable_or_missing_token_accounts`
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
fn storage_gateway_candidates_exclude_unavailable_or_missing_token_accounts() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init schema");
    let now = now_ts();

    let accounts = [
        ("acc-ready", "active", 0_i64),
        ("acc-no-snapshot", "active", 1_i64),
        ("acc-exhausted", "active", 2_i64),
        ("acc-partial", "active", 3_i64),
        ("acc-inactive", "inactive", 4_i64),
        ("acc-no-token", "active", 5_i64),
        ("acc-limited", "limited", 6_i64),
    ];
    for (id, status, sort) in accounts {
        storage
            .insert_account(&Account {
                id: id.to_string(),
                label: id.to_string(),
                issuer: "https://auth.openai.com".to_string(),
                chatgpt_account_id: None,
                workspace_id: None,
                group_name: None,
                sort,
                status: status.to_string(),
                created_at: now + sort,
                updated_at: now + sort,
            })
            .expect("insert account");
    }

    for id in [
        "acc-ready",
        "acc-no-snapshot",
        "acc-exhausted",
        "acc-partial",
        "acc-inactive",
        "acc-limited",
    ] {
        storage
            .insert_token(&Token {
                account_id: id.to_string(),
                id_token: format!("id-{id}"),
                access_token: format!("access-{id}"),
                refresh_token: format!("refresh-{id}"),
                api_key_access_token: None,
                last_refresh: now,
            })
            .expect("insert token");
    }

    storage
        .insert_usage_snapshot(&UsageSnapshotRecord {
            account_id: "acc-ready".to_string(),
            used_percent: Some(12.0),
            window_minutes: Some(300),
            resets_at: None,
            secondary_used_percent: None,
            secondary_window_minutes: None,
            secondary_resets_at: None,
            credits_json: None,
            captured_at: now,
        })
        .expect("insert ready usage");
    storage
        .insert_usage_snapshot(&UsageSnapshotRecord {
            account_id: "acc-exhausted".to_string(),
            used_percent: Some(100.0),
            window_minutes: Some(300),
            resets_at: None,
            secondary_used_percent: None,
            secondary_window_minutes: None,
            secondary_resets_at: None,
            credits_json: None,
            captured_at: now,
        })
        .expect("insert exhausted usage");
    storage
        .insert_usage_snapshot(&UsageSnapshotRecord {
            account_id: "acc-partial".to_string(),
            used_percent: Some(20.0),
            window_minutes: Some(300),
            resets_at: None,
            secondary_used_percent: Some(10.0),
            secondary_window_minutes: None,
            secondary_resets_at: None,
            credits_json: None,
            captured_at: now,
        })
        .expect("insert partial usage");
    storage
        .insert_usage_snapshot(&UsageSnapshotRecord {
            account_id: "acc-inactive".to_string(),
            used_percent: Some(10.0),
            window_minutes: Some(300),
            resets_at: None,
            secondary_used_percent: None,
            secondary_window_minutes: None,
            secondary_resets_at: None,
            credits_json: None,
            captured_at: now,
        })
        .expect("insert inactive usage");

    let candidates = storage
        .list_gateway_candidates()
        .expect("list gateway candidates");
    let candidate_ids = candidates
        .iter()
        .map(|(account, _)| account.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(candidate_ids, vec!["acc-ready", "acc-no-snapshot"]);
}

/// 函数 `latest_usage_snapshots_break_ties_by_latest_id`
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
fn latest_usage_snapshots_break_ties_by_latest_id() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init schema");

    let tie_ts = now_ts();

    storage
        .insert_usage_snapshot(&UsageSnapshotRecord {
            account_id: "acc-1".to_string(),
            used_percent: Some(10.0),
            window_minutes: Some(300),
            resets_at: None,
            secondary_used_percent: None,
            secondary_window_minutes: None,
            secondary_resets_at: None,
            credits_json: None,
            captured_at: tie_ts,
        })
        .expect("insert first snapshot");

    storage
        .insert_usage_snapshot(&UsageSnapshotRecord {
            account_id: "acc-1".to_string(),
            used_percent: Some(30.0),
            window_minutes: Some(300),
            resets_at: None,
            secondary_used_percent: None,
            secondary_window_minutes: None,
            secondary_resets_at: None,
            credits_json: None,
            captured_at: tie_ts,
        })
        .expect("insert second snapshot with same timestamp");

    storage
        .insert_usage_snapshot(&UsageSnapshotRecord {
            account_id: "acc-2".to_string(),
            used_percent: Some(50.0),
            window_minutes: Some(300),
            resets_at: None,
            secondary_used_percent: None,
            secondary_window_minutes: None,
            secondary_resets_at: None,
            credits_json: None,
            captured_at: tie_ts - 10,
        })
        .expect("insert snapshot for acc-2");

    let latest = storage
        .latest_usage_snapshots_by_account()
        .expect("read latest snapshots");

    assert_eq!(latest.len(), 2);
    assert_eq!(latest[0].account_id, "acc-1");

    let acc1 = latest
        .iter()
        .find(|item| item.account_id == "acc-1")
        .expect("acc-1 exists");
    assert_eq!(acc1.used_percent, Some(30.0));
}

/// 函数 `request_logs_support_prefixed_query_filters`
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
fn request_logs_support_prefixed_query_filters() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init schema");

    for (id, label) in [
        ("acc-1", "owner-alpha@example.com"),
        ("acc-2", "owner-beta@example.com"),
    ] {
        storage
            .insert_account(&Account {
                id: id.to_string(),
                label: label.to_string(),
                issuer: "https://auth.openai.com".to_string(),
                chatgpt_account_id: None,
                workspace_id: None,
                group_name: None,
                sort: 0,
                status: "active".to_string(),
                created_at: now_ts(),
                updated_at: now_ts(),
            })
            .expect("insert account");
    }

    storage
        .insert_request_log(&RequestLog {
            trace_id: Some("trc-alpha-extra".to_string()),
            key_id: Some("key-alpha-extra".to_string()),
            account_id: Some("acc-1".to_string()),
            initial_account_id: Some("acc-1".to_string()),
            attempted_account_ids_json: Some(r#"["acc-1"]"#.to_string()),
            request_path: "/v1/responses".to_string(),
            original_path: Some("/v1/chat/completions".to_string()),
            adapted_path: Some("/v1/responses".to_string()),
            method: "POST".to_string(),
            model: Some("gpt-5.1".to_string()),
            reasoning_effort: Some("low".to_string()),
            effective_service_tier: Some("priority".to_string()),
            response_adapter: Some("OpenAIChatCompletionsJson".to_string()),
            upstream_url: Some("https://chatgpt.com/backend-api/codex/v1/responses".to_string()),
            aggregate_api_supplier_name: None,
            aggregate_api_url: None,
            status_code: Some(201),
            duration_ms: Some(320),
            first_response_ms: None,
            input_tokens: Some(11),
            cached_input_tokens: Some(3),
            output_tokens: Some(7),
            total_tokens: Some(18),
            reasoning_output_tokens: Some(2),
            estimated_cost_usd: Some(0.0),
            error: None,
            created_at: now_ts() - 2,
            ..Default::default()
        })
        .expect("insert request log 0");

    storage
        .insert_request_log(&RequestLog {
            trace_id: Some("trc-alpha".to_string()),
            key_id: Some("key-alpha".to_string()),
            account_id: Some("acc-1".to_string()),
            initial_account_id: Some("acc-1".to_string()),
            attempted_account_ids_json: Some(r#"["acc-1"]"#.to_string()),
            request_path: "/v1/responses".to_string(),
            original_path: Some("/v1/responses".to_string()),
            adapted_path: Some("/v1/responses".to_string()),
            method: "POST".to_string(),
            model: Some("gpt-5.1".to_string()),
            reasoning_effort: Some("low".to_string()),
            response_adapter: Some("Passthrough".to_string()),
            upstream_url: Some("https://chatgpt.com/backend-api/codex/v1/responses".to_string()),
            aggregate_api_supplier_name: None,
            aggregate_api_url: None,
            status_code: Some(200),
            duration_ms: Some(210),
            first_response_ms: None,
            input_tokens: Some(9),
            cached_input_tokens: Some(1),
            output_tokens: Some(5),
            total_tokens: Some(14),
            reasoning_output_tokens: Some(1),
            estimated_cost_usd: Some(0.0),
            error: None,
            created_at: now_ts() - 1,
            ..Default::default()
        })
        .expect("insert request log 1");

    storage
        .insert_request_log(&RequestLog {
            trace_id: Some("trc-beta".to_string()),
            key_id: Some("key-beta".to_string()),
            account_id: Some("acc-2".to_string()),
            initial_account_id: Some("acc-2".to_string()),
            attempted_account_ids_json: Some(r#"["acc-2"]"#.to_string()),
            request_path: "/v1/models".to_string(),
            original_path: Some("/v1/models".to_string()),
            adapted_path: Some("/v1/models".to_string()),
            method: "GET".to_string(),
            model: Some("gpt-4.1".to_string()),
            reasoning_effort: Some("xhigh".to_string()),
            response_adapter: None,
            upstream_url: Some("https://api.openai.com/v1/models".to_string()),
            aggregate_api_supplier_name: None,
            aggregate_api_url: None,
            status_code: Some(503),
            duration_ms: Some(1800),
            first_response_ms: None,
            input_tokens: None,
            cached_input_tokens: None,
            output_tokens: None,
            total_tokens: None,
            reasoning_output_tokens: None,
            estimated_cost_usd: Some(0.0),
            error: Some("upstream timeout".to_string()),
            created_at: now_ts(),
            ..Default::default()
        })
        .expect("insert request log 2");

    let method_filtered = storage
        .list_request_logs(Some("method:GET"), 100)
        .expect("filter by method");
    assert_eq!(method_filtered.len(), 1);
    assert_eq!(method_filtered[0].method, "GET");

    let status_filtered = storage
        .list_request_logs(Some("status:5xx"), 100)
        .expect("filter by status range");
    assert_eq!(status_filtered.len(), 1);
    assert_eq!(status_filtered[0].status_code, Some(503));

    let key_filtered = storage
        .list_request_logs(Some("key:key-alpha"), 100)
        .expect("filter by key id");
    assert_eq!(key_filtered.len(), 2);

    let key_exact_filtered = storage
        .list_request_logs(Some("key:=key-alpha"), 100)
        .expect("filter by exact key id");
    assert_eq!(key_exact_filtered.len(), 1);
    assert_eq!(key_exact_filtered[0].key_id.as_deref(), Some("key-alpha"));

    let trace_filtered = storage
        .list_request_logs(Some("trace:=trc-alpha"), 100)
        .expect("filter by trace id");
    assert_eq!(trace_filtered.len(), 1);
    assert_eq!(trace_filtered[0].trace_id.as_deref(), Some("trc-alpha"));

    let original_path_filtered = storage
        .list_request_logs(Some("original:=/v1/chat/completions"), 100)
        .expect("filter by original path");
    assert_eq!(original_path_filtered.len(), 1);
    assert_eq!(
        original_path_filtered[0].original_path.as_deref(),
        Some("/v1/chat/completions")
    );

    let adapter_filtered = storage
        .list_request_logs(Some("adapter:=OpenAIChatCompletionsJson"), 100)
        .expect("filter by response adapter");
    assert_eq!(adapter_filtered.len(), 1);
    assert_eq!(
        adapter_filtered[0].response_adapter.as_deref(),
        Some("OpenAIChatCompletionsJson")
    );

    let effective_tier_filtered = storage
        .list_request_logs(Some("effective_tier:=priority"), 100)
        .expect("filter by effective service tier");
    assert_eq!(effective_tier_filtered.len(), 1);
    assert_eq!(
        effective_tier_filtered[0].effective_service_tier.as_deref(),
        Some("priority")
    );

    let fallback_filtered = storage
        .list_request_logs(Some("timeout"), 100)
        .expect("fallback fuzzy query");
    assert_eq!(fallback_filtered.len(), 1);
    assert_eq!(
        fallback_filtered[0].error.as_deref(),
        Some("upstream timeout")
    );

    let account_label_filtered = storage
        .list_request_logs(Some("owner-alpha@example.com"), 100)
        .expect("filter by account label");
    assert_eq!(account_label_filtered.len(), 2);
    assert!(account_label_filtered
        .iter()
        .all(|log| log.account_id.as_deref() == Some("acc-1")));

    let account_prefixed_filtered = storage
        .list_request_logs(Some("account:=owner-alpha@example.com"), 100)
        .expect("filter by account label with account prefix");
    assert_eq!(account_prefixed_filtered.len(), 2);
    assert!(account_prefixed_filtered
        .iter()
        .all(|log| log.account_id.as_deref() == Some("acc-1")));
}

/// 函数 `request_log_today_summary_reads_from_token_stats_table`
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
fn request_log_today_summary_reads_from_token_stats_table() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init schema");
    let created_at = now_ts();
    let request_log_id = storage
        .insert_request_log(&RequestLog {
            trace_id: Some("trc-summary".to_string()),
            key_id: Some("key-summary".to_string()),
            account_id: Some("acc-summary".to_string()),
            initial_account_id: Some("acc-summary".to_string()),
            attempted_account_ids_json: Some(r#"["acc-summary"]"#.to_string()),
            request_path: "/v1/responses".to_string(),
            original_path: Some("/v1/responses".to_string()),
            adapted_path: Some("/v1/responses".to_string()),
            method: "POST".to_string(),
            model: Some("gpt-5.3-codex".to_string()),
            reasoning_effort: Some("high".to_string()),
            response_adapter: Some("Passthrough".to_string()),
            upstream_url: Some("https://chatgpt.com/backend-api/codex/responses".to_string()),
            aggregate_api_supplier_name: None,
            aggregate_api_url: None,
            status_code: Some(200),
            duration_ms: Some(1450),
            first_response_ms: None,
            input_tokens: None,
            cached_input_tokens: None,
            output_tokens: None,
            total_tokens: None,
            reasoning_output_tokens: None,
            estimated_cost_usd: None,
            error: None,
            created_at,
            ..Default::default()
        })
        .expect("insert request log");

    storage
        .insert_request_token_stat(&RequestTokenStat {
            request_log_id,
            key_id: Some("key-summary".to_string()),
            account_id: Some("acc-summary".to_string()),
            model: Some("gpt-5.3-codex".to_string()),
            input_tokens: Some(120),
            cached_input_tokens: Some(80),
            output_tokens: Some(22),
            total_tokens: Some(142),
            reasoning_output_tokens: Some(9),
            estimated_cost_usd: Some(0.33),
            created_at,
        })
        .expect("insert token stat");

    let summary = storage
        .summarize_request_logs_between(created_at - 1, created_at + 1)
        .expect("summarize");
    assert_eq!(summary.input_tokens, 120);
    assert_eq!(summary.cached_input_tokens, 80);
    assert_eq!(summary.output_tokens, 22);
    assert_eq!(summary.reasoning_output_tokens, 9);
    assert!(summary.estimated_cost_usd > 0.32);
}

/// 函数 `insert_request_log_with_token_stat_writes_both_tables_in_one_call`
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
fn insert_request_log_with_token_stat_writes_both_tables_in_one_call() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init schema");
    let created_at = now_ts();

    let (request_log_id, token_stat_error) = storage
        .insert_request_log_with_token_stat(
            &RequestLog {
                trace_id: Some("trc-atomic".to_string()),
                key_id: Some("key-atomic".to_string()),
                account_id: Some("acc-atomic".to_string()),
                initial_account_id: Some("acc-atomic".to_string()),
                attempted_account_ids_json: Some(r#"["acc-atomic"]"#.to_string()),
                request_path: "/v1/responses".to_string(),
                original_path: Some("/v1/responses".to_string()),
                adapted_path: Some("/v1/responses".to_string()),
                method: "POST".to_string(),
                model: Some("gpt-5.3-codex".to_string()),
                reasoning_effort: Some("high".to_string()),
                response_adapter: Some("Passthrough".to_string()),
                upstream_url: Some("https://chatgpt.com/backend-api/codex/responses".to_string()),
                aggregate_api_supplier_name: None,
                aggregate_api_url: None,
                status_code: Some(200),
                duration_ms: Some(980),
                first_response_ms: None,
                input_tokens: None,
                cached_input_tokens: None,
                output_tokens: None,
                total_tokens: None,
                reasoning_output_tokens: None,
                estimated_cost_usd: None,
                error: None,
                created_at,
                ..Default::default()
            },
            &RequestTokenStat {
                request_log_id: 0,
                key_id: Some("key-atomic".to_string()),
                account_id: Some("acc-atomic".to_string()),
                model: Some("gpt-5.3-codex".to_string()),
                input_tokens: Some(10),
                cached_input_tokens: Some(2),
                output_tokens: Some(5),
                total_tokens: Some(15),
                reasoning_output_tokens: Some(1),
                estimated_cost_usd: Some(0.01),
                created_at,
            },
        )
        .expect("insert request log with token stat");

    assert!(request_log_id > 0);
    assert!(
        token_stat_error.is_none(),
        "token stat insert should succeed: {:?}",
        token_stat_error
    );

    let logs = storage
        .list_request_logs(Some("key:=key-atomic"), 10)
        .expect("list logs");
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].key_id.as_deref(), Some("key-atomic"));
    assert_eq!(logs[0].input_tokens, Some(10));
    assert_eq!(logs[0].cached_input_tokens, Some(2));
    assert_eq!(logs[0].output_tokens, Some(5));
    assert_eq!(logs[0].total_tokens, Some(15));
    assert_eq!(logs[0].reasoning_output_tokens, Some(1));
}

#[test]
fn request_token_stats_rollups_use_owner_and_actual_source_precedence() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init schema");
    let base = 1_700_000_000;

    for (id, username) in [("ledger-user", "ledger"), ("current-user", "current")] {
        storage
            .insert_app_user(&AppUser {
                id: id.to_string(),
                username: username.to_string(),
                display_name: None,
                password_hash: "hash".to_string(),
                role: "member".to_string(),
                status: "active".to_string(),
                created_at: base,
                updated_at: base,
                last_login_at: None,
            })
            .expect("insert app user");
    }

    let ledger_wallet = storage
        .ensure_wallet_for_owner("wallet-ledger-user", "user", "ledger-user")
        .expect("ensure ledger wallet");

    for key_id in ["key-shared", "key-unowned"] {
        storage
            .insert_api_key(&ApiKey {
                id: key_id.to_string(),
                name: Some(key_id.to_string()),
                model_slug: Some("gpt-5-mini".to_string()),
                reasoning_effort: None,
                service_tier: None,
                rotation_strategy: "account_rotation".to_string(),
                aggregate_api_id: None,
                account_plan_filter: None,
                aggregate_api_url: None,
                client_type: "codex".to_string(),
                protocol_type: "openai_compat".to_string(),
                auth_scheme: "authorization_bearer".to_string(),
                upstream_base_url: None,
                static_headers_json: None,
                key_hash: format!("hash-{key_id}"),
                status: "active".to_string(),
                created_at: base,
                last_used_at: None,
            })
            .expect("insert api key");
    }

    storage
        .upsert_api_key_owner(&ApiKeyOwner {
            key_id: "key-shared".to_string(),
            owner_kind: "user".to_string(),
            owner_user_id: Some("current-user".to_string()),
            project_id: None,
            updated_at: base,
        })
        .expect("upsert key owner");

    for account_id in ["acc-actual", "acc-legacy"] {
        storage
            .insert_account(&Account {
                id: account_id.to_string(),
                label: account_id.to_string(),
                issuer: "https://auth.openai.com".to_string(),
                chatgpt_account_id: None,
                workspace_id: None,
                group_name: None,
                sort: 0,
                status: "active".to_string(),
                created_at: base,
                updated_at: base,
            })
            .expect("insert account");
    }

    for aggregate_id in ["agg-actual", "agg-legacy"] {
        storage
            .insert_aggregate_api(&AggregateApi {
                id: aggregate_id.to_string(),
                provider_type: "openai-compatible".to_string(),
                supplier_name: Some(aggregate_id.to_string()),
                sort: 0,
                url: format!("https://{aggregate_id}.example/v1"),
                auth_type: "bearer".to_string(),
                auth_params_json: None,
                action: None,
                model_override: None,
                status: "active".to_string(),
                created_at: base,
                updated_at: base,
                last_test_at: None,
                last_test_status: None,
                last_test_error: None,
                balance_query_enabled: false,
                balance_query_template: None,
                balance_query_base_url: None,
                balance_query_user_id: None,
                balance_query_config_json: None,
                last_balance_at: None,
                last_balance_status: None,
                last_balance_error: None,
                last_balance_json: None,
            })
            .expect("insert aggregate api");
    }

    let insert_usage_log = |trace_id: &str,
                            key_id: &str,
                            created_at: i64,
                            status_code: i64,
                            account_id: Option<&str>,
                            initial_aggregate_api_id: Option<&str>,
                            actual_source_kind: Option<&str>,
                            actual_source_id: Option<&str>,
                            input_tokens: i64,
                            cached_input_tokens: i64,
                            output_tokens: i64,
                            total_tokens: Option<i64>,
                            estimated_cost_usd: f64| {
        let (request_log_id, token_stat_error) = storage
            .insert_request_log_with_token_stat(
                &RequestLog {
                    trace_id: Some(trace_id.to_string()),
                    key_id: Some(key_id.to_string()),
                    account_id: account_id.map(str::to_string),
                    initial_aggregate_api_id: initial_aggregate_api_id.map(str::to_string),
                    request_path: "/v1/chat/completions".to_string(),
                    method: "POST".to_string(),
                    model: Some("gpt-5-mini".to_string()),
                    actual_source_kind: actual_source_kind.map(str::to_string),
                    actual_source_id: actual_source_id.map(str::to_string),
                    status_code: Some(status_code),
                    created_at,
                    ..RequestLog::default()
                },
                &RequestTokenStat {
                    key_id: Some(key_id.to_string()),
                    account_id: account_id.map(str::to_string),
                    model: Some("gpt-5-mini".to_string()),
                    input_tokens: Some(input_tokens),
                    cached_input_tokens: Some(cached_input_tokens),
                    output_tokens: Some(output_tokens),
                    total_tokens,
                    estimated_cost_usd: Some(estimated_cost_usd),
                    created_at,
                    ..RequestTokenStat::default()
                },
            )
            .expect("insert usage log");
        assert!(
            token_stat_error.is_none(),
            "token stat insert failed: {:?}",
            token_stat_error
        );
        request_log_id
    };

    let ledger_log_id = insert_usage_log(
        "trace-ledger-owner",
        "key-shared",
        base + 60,
        200,
        Some("acc-legacy"),
        None,
        Some("openai_account"),
        Some("acc-actual"),
        100,
        20,
        50,
        Some(100),
        0.10,
    );
    storage
        .adjust_wallet_balance(&AppWalletLedgerEntry {
            id: "ledger-charge-one".to_string(),
            wallet_id: ledger_wallet.id.clone(),
            entry_kind: "request_charge".to_string(),
            amount_credit_micros: -10_000,
            balance_after_credit_micros: 0,
            request_log_id: Some(ledger_log_id),
            api_key_id: None,
            pricing_rule_id: None,
            raw_usage_json: None,
            note: None,
            created_by_user_id: None,
            created_at: base + 61,
        })
        .expect("insert request charge ledger");

    insert_usage_log(
        "trace-current-owner",
        "key-shared",
        base + 120,
        500,
        Some("acc-legacy"),
        None,
        None,
        None,
        80,
        30,
        20,
        None,
        0.20,
    );
    insert_usage_log(
        "trace-aggregate-actual",
        "key-unowned",
        base + 86_400 + 30,
        200,
        Some("acc-legacy"),
        Some("agg-legacy"),
        Some("aggregate_api"),
        Some("agg-actual"),
        40,
        0,
        10,
        Some(30),
        0.30,
    );
    insert_usage_log(
        "trace-aggregate-legacy",
        "key-unowned",
        base + 86_400 + 60,
        200,
        None,
        Some("agg-legacy"),
        None,
        None,
        25,
        5,
        20,
        None,
        0.40,
    );

    let daily = storage
        .summarize_request_token_stats_daily(base, base + 2 * 86_400, 86_400)
        .expect("daily rollup");
    assert_eq!(daily.len(), 2);
    assert_eq!(daily[0].usage.total_tokens, 170);
    assert_eq!(daily[0].usage.input_tokens, 180);
    assert_eq!(daily[0].usage.cached_input_tokens, 50);
    assert_eq!(daily[0].usage.output_tokens, 70);
    assert_eq!(daily[0].usage.request_count, 2);
    assert_eq!(daily[0].usage.success_count, 1);
    assert_eq!(daily[0].usage.error_count, 1);
    assert_eq!(daily[1].usage.total_tokens, 70);

    let by_user = storage
        .summarize_request_token_stats_by_user_between(base, base + 86_400)
        .expect("user rollup");
    let ledger_user = by_user
        .iter()
        .find(|item| item.user_id == "ledger-user")
        .expect("ledger owner rollup");
    let current_user = by_user
        .iter()
        .find(|item| item.user_id == "current-user")
        .expect("current owner fallback rollup");
    assert_eq!(ledger_user.usage.total_tokens, 100);
    assert_eq!(current_user.usage.total_tokens, 70);

    let ledger_direct = storage
        .summarize_request_token_stats_for_user_between("ledger-user", base, base + 86_400)
        .expect("direct user rollup");
    assert_eq!(ledger_direct.total_tokens, 100);

    let openai_sources = storage
        .summarize_request_token_stats_by_source_between("openai_account", base, base + 2 * 86_400)
        .expect("openai source rollup");
    assert_eq!(
        openai_sources
            .iter()
            .find(|item| item.source_id == "acc-actual")
            .expect("actual account")
            .usage
            .total_tokens,
        100
    );
    assert_eq!(
        openai_sources
            .iter()
            .find(|item| item.source_id == "acc-legacy")
            .expect("legacy account")
            .usage
            .total_tokens,
        70
    );

    let aggregate_sources = storage
        .summarize_request_token_stats_by_source_between("aggregate_api", base, base + 2 * 86_400)
        .expect("aggregate source rollup");
    assert_eq!(
        aggregate_sources
            .iter()
            .find(|item| item.source_id == "agg-actual")
            .expect("actual aggregate")
            .usage
            .total_tokens,
        30
    );
    assert_eq!(
        aggregate_sources
            .iter()
            .find(|item| item.source_id == "agg-legacy")
            .expect("legacy aggregate")
            .usage
            .total_tokens,
        40
    );
}

#[test]
fn delete_app_user_removes_model_group_assignments() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init schema");
    let now = now_ts();

    storage
        .insert_app_user(&AppUser {
            id: "delete-user".to_string(),
            username: "delete-user".to_string(),
            display_name: None,
            password_hash: "hash".to_string(),
            role: "member".to_string(),
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
            last_login_at: None,
        })
        .expect("insert user");
    storage
        .upsert_model_group(&ModelGroup {
            id: "delete-group".to_string(),
            name: "Delete Group".to_string(),
            description: None,
            status: "active".to_string(),
            sort: 0,
            is_default: false,
            rate_multiplier_millis: 1000,
            created_at: now,
            updated_at: now,
        })
        .expect("insert model group");
    storage
        .replace_user_model_groups_for_group(
            "delete-group",
            &[UserModelGroup {
                user_id: "delete-user".to_string(),
                group_id: "delete-group".to_string(),
                status: "active".to_string(),
                expires_at: None,
                created_at: now,
                updated_at: now,
            }],
        )
        .expect("assign model group");

    let deleted = storage
        .delete_app_user("delete-user")
        .expect("delete app user");

    assert_eq!(deleted, 1);
    assert!(storage
        .find_app_user_by_id("delete-user")
        .expect("find deleted user")
        .is_none());
    assert!(storage
        .list_user_model_groups_for_user("delete-user")
        .expect("list deleted user model groups")
        .is_empty());
}

/// 函数 `clear_request_logs_keeps_token_stats_for_usage_summary`
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
fn clear_request_logs_rolls_token_stats_into_long_term_totals() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init schema");
    let created_at = now_ts();
    let request_log_id = storage
        .insert_request_log(&RequestLog {
            trace_id: Some("trc-clear".to_string()),
            key_id: Some("key-clear".to_string()),
            account_id: Some("acc-clear".to_string()),
            initial_account_id: Some("acc-clear".to_string()),
            attempted_account_ids_json: Some(r#"["acc-clear"]"#.to_string()),
            request_path: "/v1/responses".to_string(),
            original_path: Some("/v1/responses".to_string()),
            adapted_path: Some("/v1/responses".to_string()),
            method: "POST".to_string(),
            model: Some("gpt-5.3-codex".to_string()),
            reasoning_effort: Some("high".to_string()),
            response_adapter: Some("Passthrough".to_string()),
            upstream_url: Some("https://chatgpt.com/backend-api/codex/responses".to_string()),
            aggregate_api_supplier_name: None,
            aggregate_api_url: None,
            status_code: Some(200),
            duration_ms: Some(760),
            first_response_ms: None,
            input_tokens: None,
            cached_input_tokens: None,
            output_tokens: None,
            total_tokens: None,
            reasoning_output_tokens: None,
            estimated_cost_usd: None,
            error: None,
            created_at,
            ..Default::default()
        })
        .expect("insert request log");
    storage
        .insert_request_token_stat(&RequestTokenStat {
            request_log_id,
            key_id: Some("key-clear".to_string()),
            account_id: Some("acc-clear".to_string()),
            model: Some("gpt-5.3-codex".to_string()),
            input_tokens: Some(100),
            cached_input_tokens: Some(30),
            output_tokens: Some(20),
            total_tokens: Some(120),
            reasoning_output_tokens: Some(5),
            estimated_cost_usd: Some(0.12),
            created_at,
        })
        .expect("insert token stat");

    storage.clear_request_logs().expect("clear request logs");

    let logs = storage.list_request_logs(None, 100).expect("list logs");
    assert!(logs.is_empty(), "request logs should be cleared");

    let summary = storage
        .summarize_request_logs_between(created_at - 1, created_at + 1)
        .expect("summarize");
    assert_eq!(summary.input_tokens, 0);
    assert_eq!(summary.cached_input_tokens, 0);
    assert_eq!(summary.output_tokens, 0);
    assert_eq!(summary.reasoning_output_tokens, 0);
    assert_eq!(summary.estimated_cost_usd, 0.0);

    let usage_by_key = storage
        .summarize_request_token_stats_by_key()
        .expect("summarize by key");
    assert_eq!(usage_by_key.len(), 1);
    assert_eq!(usage_by_key[0].key_id, "key-clear");
    assert_eq!(usage_by_key[0].total_tokens, 120);
    assert!(usage_by_key[0].estimated_cost_usd > 0.11);
}

/// 函数 `request_token_stats_can_summarize_total_tokens_by_key`
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
fn request_token_stats_can_summarize_total_tokens_by_key() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init schema");
    let created_at = now_ts();

    for (
        request_log_id,
        key_id,
        total_tokens,
        input_tokens,
        cached_input_tokens,
        output_tokens,
        estimated_cost_usd,
    ) in [
        (
            101_i64,
            "gk_alpha",
            Some(120_i64),
            None,
            None,
            None,
            Some(0.12),
        ),
        (
            102_i64,
            "gk_alpha",
            None,
            Some(90_i64),
            Some(30_i64),
            Some(25_i64),
            Some(0.34),
        ),
        (
            103_i64,
            "gk_beta",
            Some(75_i64),
            None,
            None,
            None,
            Some(0.78),
        ),
        (104_i64, "", Some(999_i64), None, None, None, Some(9.99)),
    ] {
        storage
            .insert_request_token_stat(&RequestTokenStat {
                request_log_id,
                key_id: if key_id.is_empty() {
                    None
                } else {
                    Some(key_id.to_string())
                },
                account_id: Some("acc-summary".to_string()),
                model: Some("gpt-5.3-codex".to_string()),
                input_tokens,
                cached_input_tokens,
                output_tokens,
                total_tokens,
                reasoning_output_tokens: Some(0),
                estimated_cost_usd,
                created_at,
            })
            .expect("insert token stat");
    }

    let summary = storage
        .summarize_request_token_stats_by_key()
        .expect("summarize by key");

    assert_eq!(summary.len(), 2);
    assert_eq!(summary[0].key_id, "gk_alpha");
    assert_eq!(summary[0].total_tokens, 205);
    assert!((summary[0].estimated_cost_usd - 0.46).abs() < f64::EPSILON);
    assert_eq!(summary[1].key_id, "gk_beta");
    assert_eq!(summary[1].total_tokens, 75);
    assert!((summary[1].estimated_cost_usd - 0.78).abs() < f64::EPSILON);
}

/// 函数 `usage_snapshots_can_prune_history_per_account`
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
fn usage_snapshots_can_prune_history_per_account() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init schema");
    let now = now_ts();

    for offset in 0..5 {
        storage
            .insert_usage_snapshot(&UsageSnapshotRecord {
                account_id: "acc-prune-1".to_string(),
                used_percent: Some(10.0 + offset as f64),
                window_minutes: Some(300),
                resets_at: None,
                secondary_used_percent: None,
                secondary_window_minutes: None,
                secondary_resets_at: None,
                credits_json: None,
                captured_at: now + offset,
            })
            .expect("insert acc-prune-1 snapshot");
    }

    storage
        .insert_usage_snapshot(&UsageSnapshotRecord {
            account_id: "acc-prune-2".to_string(),
            used_percent: Some(30.0),
            window_minutes: Some(300),
            resets_at: None,
            secondary_used_percent: None,
            secondary_window_minutes: None,
            secondary_resets_at: None,
            credits_json: None,
            captured_at: now,
        })
        .expect("insert acc-prune-2 snapshot");

    let deleted = storage
        .prune_usage_snapshots_for_account("acc-prune-1", 2)
        .expect("prune snapshots");
    assert_eq!(deleted, 3);

    let kept = storage
        .usage_snapshot_count_for_account("acc-prune-1")
        .expect("count kept");
    assert_eq!(kept, 2);

    let untouched = storage
        .usage_snapshot_count_for_account("acc-prune-2")
        .expect("count untouched");
    assert_eq!(untouched, 1);
}

/// 函数 `storage_api_keys_include_profile_fields`
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
fn storage_api_keys_include_profile_fields() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init schema");

    storage
        .insert_api_key(&ApiKey {
            id: "key-1".to_string(),
            name: Some("main".to_string()),
            model_slug: Some("claude-sonnet-4".to_string()),
            reasoning_effort: Some("medium".to_string()),
            service_tier: Some("fast".to_string()),
            rotation_strategy: "account_rotation".to_string(),
            aggregate_api_id: None,
            account_plan_filter: None,
            aggregate_api_url: None,
            client_type: "claude_code".to_string(),
            protocol_type: "anthropic_native".to_string(),
            auth_scheme: "x_api_key".to_string(),
            upstream_base_url: Some("https://api.anthropic.com".to_string()),
            static_headers_json: Some("{\"anthropic-version\":\"2023-06-01\"}".to_string()),
            key_hash: "hash-1".to_string(),
            status: "active".to_string(),
            created_at: now_ts(),
            last_used_at: None,
        })
        .expect("insert key");

    let key = storage
        .list_api_keys()
        .expect("list keys")
        .into_iter()
        .find(|item| item.id == "key-1")
        .expect("key exists");
    assert_eq!(key.client_type, "claude_code");
    assert_eq!(key.protocol_type, "anthropic_native");
    assert_eq!(key.auth_scheme, "x_api_key");
    assert_eq!(key.model_slug.as_deref(), Some("claude-sonnet-4"));
    assert_eq!(key.service_tier.as_deref(), Some("fast"));
}

/// 函数 `storage_can_roundtrip_api_key_secret`
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
fn storage_can_roundtrip_api_key_secret() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init schema");

    storage
        .insert_api_key(&ApiKey {
            id: "key-secret-1".to_string(),
            name: Some("secret".to_string()),
            model_slug: None,
            reasoning_effort: None,
            service_tier: None,
            rotation_strategy: "account_rotation".to_string(),
            aggregate_api_id: None,
            account_plan_filter: None,
            aggregate_api_url: None,
            client_type: "codex".to_string(),
            protocol_type: "openai_compat".to_string(),
            auth_scheme: "authorization_bearer".to_string(),
            upstream_base_url: None,
            static_headers_json: None,
            key_hash: "hash-secret-1".to_string(),
            status: "active".to_string(),
            created_at: now_ts(),
            last_used_at: None,
        })
        .expect("insert key");

    storage
        .upsert_api_key_secret("key-secret-1", "sk-secret-value")
        .expect("upsert secret");

    let loaded = storage
        .find_api_key_secret_by_id("key-secret-1")
        .expect("load secret");
    assert_eq!(loaded.as_deref(), Some("sk-secret-value"));

    storage.delete_api_key("key-secret-1").expect("delete key");
    let removed = storage
        .find_api_key_secret_by_id("key-secret-1")
        .expect("load removed secret");
    assert!(removed.is_none());
}

#[test]
fn storage_can_roundtrip_api_key_quota_limit_and_usage() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init schema");

    storage
        .insert_api_key(&ApiKey {
            id: "key-quota-1".to_string(),
            name: Some("quota".to_string()),
            model_slug: None,
            reasoning_effort: None,
            service_tier: None,
            rotation_strategy: "account_rotation".to_string(),
            aggregate_api_id: None,
            account_plan_filter: None,
            aggregate_api_url: None,
            client_type: "codex".to_string(),
            protocol_type: "openai_compat".to_string(),
            auth_scheme: "authorization_bearer".to_string(),
            upstream_base_url: None,
            static_headers_json: None,
            key_hash: "hash-quota-1".to_string(),
            status: "active".to_string(),
            created_at: now_ts(),
            last_used_at: None,
        })
        .expect("insert key");

    storage
        .upsert_api_key_quota_limit("key-quota-1", Some(1000))
        .expect("upsert quota");
    assert_eq!(
        storage
            .find_api_key_quota_limit("key-quota-1")
            .expect("read quota"),
        Some(1000)
    );

    storage
        .insert_request_token_stat(&RequestTokenStat {
            request_log_id: 1,
            key_id: Some("key-quota-1".to_string()),
            input_tokens: Some(700),
            cached_input_tokens: Some(100),
            output_tokens: Some(300),
            total_tokens: None,
            created_at: now_ts(),
            ..RequestTokenStat::default()
        })
        .expect("insert stat");
    storage
        .insert_request_token_stat(&RequestTokenStat {
            request_log_id: 2,
            key_id: Some("key-quota-1".to_string()),
            total_tokens: Some(250),
            created_at: now_ts(),
            ..RequestTokenStat::default()
        })
        .expect("insert total stat");

    assert_eq!(
        storage
            .api_key_total_token_usage("key-quota-1")
            .expect("read usage"),
        1150
    );

    storage
        .rollup_all_request_token_stats()
        .expect("roll up request stats");
    assert_eq!(
        storage
            .api_key_total_token_usage("key-quota-1")
            .expect("read rolled usage"),
        1150
    );

    storage
        .upsert_api_key_quota_limit("key-quota-1", None)
        .expect("clear quota");
    assert_eq!(
        storage
            .find_api_key_quota_limit("key-quota-1")
            .expect("read cleared quota"),
        None
    );
}
