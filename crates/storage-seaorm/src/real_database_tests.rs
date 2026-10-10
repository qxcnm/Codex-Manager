use super::*;
use sea_orm::{EntityTrait, TransactionTrait};

pub(super) async fn run(backend: StorageBackendKind, url_env: &str) {
    let url = std::env::var(url_env).expect("isolated database URL must be provided");
    let storage = SeaOrmStorage::connect(backend, &url)
        .await
        .expect("connect isolated database");
    storage.migrate().await.expect("initial migration");
    storage.migrate().await.expect("repeat migration");
    assert_eq!(
        storage.health_check().await.expect("health").backend,
        backend
    );
    let db = storage.connection();
    let run = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_micros() as i64;
    let id = format!("seaorm-repository-{run}");
    let hash = format!("fixture-hash-{run}");
    crate::accounts_proxy::tests::exercise(db, &format!("proxy-{run}")).await;
    crate::plugins::tests::exercise(db, &format!("plugin-{run}")).await;
    crate::api_key_details::tests::exercise(db, &format!("details-{run}"), run + 100).await;
    let key = ApiKeyRecord {
        id: id.clone(),
        name: Some("跨数据库 API Key".into()),
        model_slug: Some("gpt-5".into()),
        reasoning_effort: None,
        service_tier: Some("priority".into()),
        rotation_strategy: "account_rotation".into(),
        aggregate_api_id: None,
        account_plan_filter: None,
        account_group_filter: Some("研发".into()),
        client_type: "codex".into(),
        protocol_type: "openai_compat".into(),
        auth_scheme: "authorization_bearer".into(),
        upstream_base_url: None,
        static_headers_json: Some(format!("{{\"fixture\":\"{}\"}}", "x".repeat(512))),
        key_hash: hash.clone(),
        status: "active".into(),
        created_at: run,
        last_used_at: None,
    };
    // `key` is reserved in MySQL; large setting/header payloads must remain TEXT.
    SettingsRepository::set(
        db,
        AppSetting {
            key: id.clone(),
            value: "x".repeat(512),
            updated_at: run,
        },
    )
    .await
    .expect("insert setting");
    assert_eq!(
        SettingsRepository::get(db, &id)
            .await
            .expect("read setting")
            .expect("setting")
            .value
            .len(),
        512
    );
    ApiKeysRepository::upsert(db, key.clone())
        .await
        .expect("insert api key");
    assert_eq!(
        ApiKeysRepository::get(db, &id).await.expect("read api key"),
        Some(key.clone())
    );
    assert_eq!(
        ApiKeysRepository::find_by_hash(db, &hash)
            .await
            .expect("hash lookup")
            .expect("key")
            .id,
        id
    );
    ApiKeysRepository::update_status(db, &id, "disabled")
        .await
        .expect("update status");
    ApiKeysRepository::touch_last_used(db, &id, run + 1)
        .await
        .expect("touch");
    let updated = ApiKeysRepository::get(db, &id)
        .await
        .expect("read updated key")
        .expect("key");
    assert_eq!(updated.status, "disabled");
    assert_eq!(updated.last_used_at, Some(run + 1));

    let mut log = crate::request_logs::fixture();
    log.key_id = Some(id.clone());
    RequestLogsRepository::insert(db, run, log.clone())
        .await
        .expect("insert immutable request log");
    let stored_log = RequestLogsRepository::get(db, run)
        .await
        .expect("read request log")
        .expect("request log");
    assert_eq!(stored_log.id, run);
    assert_eq!(format!("{:?}", stored_log.log), format!("{:?}", log));
    let page = RequestLogsRepository::list_before(db, Some(run + 1), 1)
        .await
        .expect("bounded log page");
    assert_eq!(page.len(), 1);
    assert_eq!(page[0].id, run);

    let stat = RequestTokenStatRecord {
        request_log_id: run,
        key_id: Some(id.clone()),
        account_id: None,
        model: Some("gpt-5".into()),
        actual_source_kind: Some("account".into()),
        actual_source_id: Some("fixture-source".into()),
        input_tokens: Some(i64::from(i32::MAX) + 10),
        cached_input_tokens: None,
        output_tokens: Some(2),
        total_tokens: Some(i64::from(i32::MAX) + 12),
        reasoning_output_tokens: Some(1),
        estimated_cost_usd: Some(0.125),
        usage_included: true,
        created_at: run,
    };
    RequestTokenStatsRepository::upsert(db, stat.clone())
        .await
        .expect("insert stat");
    assert_eq!(
        RequestTokenStatsRepository::get(db, run)
            .await
            .expect("read stat"),
        Some(stat.clone())
    );
    let mut replacement = stat.clone();
    replacement.usage_included = false;
    replacement.total_tokens = Some(7);
    RequestTokenStatsRepository::upsert(db, replacement.clone())
        .await
        .expect("replace stat");
    assert_eq!(
        RequestTokenStatsRepository::get(db, run)
            .await
            .expect("read replacement"),
        Some(replacement)
    );

    let token = AccountTokenRecord {
        account_id: format!("account-{run}"),
        id_token: "id-token-跨库".into(),
        access_token: "access-token".into(),
        refresh_token: "refresh-token".into(),
        api_key_access_token: Some("api-key-token".into()),
        last_refresh: run,
        access_token_exp: Some(run + 3_600),
        next_refresh_at: Some(run + 1_800),
        last_refresh_attempt_at: None,
    };
    AccountsRepository::upsert(
        db,
        AccountRecord {
            id: token.account_id.clone(),
            label: "token fixture".into(),
            issuer: "openai".into(),
            chatgpt_account_id: None,
            workspace_id: None,
            subject_account_id: None,
            note: None,
            tags: None,
            group_name: None,
            sort: 0,
            status: "active".into(),
            created_at: run,
            updated_at: run,
        },
    )
    .await
    .expect("insert token account");
    AccountTokensRepository::upsert(db, token.clone())
        .await
        .expect("insert token metadata");
    assert_eq!(
        AccountTokensRepository::get(db, &token.account_id)
            .await
            .expect("read token metadata"),
        Some(token.clone())
    );
    assert_eq!(
        AccountTokensRepository::list_due(db, run + 1_800, run + 3_600, 1000)
            .await
            .expect("list due tokens")
            .into_iter()
            .filter(|t| t.account_id == token.account_id)
            .count(),
        1
    );
    assert!(
        AccountTokensRepository::touch_refresh_attempt(db, &token.account_id, run + 2)
            .await
            .expect("touch token attempt")
    );
    assert!(AccountTokensRepository::update_refresh_schedule(
        db,
        &token.account_id,
        Some(run + 9_000),
        Some(run + 8_000)
    )
    .await
    .expect("update token schedule"));

    // Both repositories accept the same transaction connection; rollback must
    // discard both the new key and token record while preserving existing rows.
    let rollback_id = format!("{id}-rollback");
    let mut rollback_key = key;
    rollback_key.id = rollback_id.clone();
    rollback_key.key_hash = format!("{hash}-rollback");
    let mut rollback_stat = stat;
    rollback_stat.request_log_id = run + 1;
    rollback_stat.key_id = Some(rollback_id.clone());
    let tx = db.begin().await.expect("begin transaction");
    ApiKeysRepository::upsert(&tx, rollback_key)
        .await
        .expect("transaction key write");
    RequestTokenStatsRepository::upsert(&tx, rollback_stat)
        .await
        .expect("transaction stat write");
    RequestLogsRepository::insert(&tx, run + 1, log)
        .await
        .expect("transaction request log write");
    assert!(RequestLogsRepository::get(&tx, run + 1)
        .await
        .expect("read own transaction log")
        .is_some());
    assert!(ApiKeysRepository::get(&tx, &rollback_id)
        .await
        .expect("read own transaction")
        .is_some());
    tx.rollback().await.expect("rollback");
    assert!(ApiKeysRepository::get(db, &rollback_id)
        .await
        .expect("read rolled back key")
        .is_none());
    assert!(RequestTokenStatsRepository::get(db, run + 1)
        .await
        .expect("read rolled back stat")
        .is_none());
    assert!(RequestLogsRepository::get(db, run + 1)
        .await
        .expect("read rolled back log")
        .is_none());
    assert!(ApiKeysRepository::get(db, &id)
        .await
        .expect("existing key survives")
        .is_some());
    assert!(RequestTokenStatsRepository::delete(db, run)
        .await
        .expect("delete stat"));
    // The public repository is append-only. Only this isolated test removes
    // its own fixture directly after proving commit/readback and rollback.
    crate::request_logs::Entity::delete_by_id(run)
        .exec(db)
        .await
        .expect("remove isolated log fixture");
    assert!(ApiKeysRepository::delete(db, &id)
        .await
        .expect("delete key"));
    assert!(SettingsRepository::delete(db, &id)
        .await
        .expect("delete setting"));
    assert!(AccountTokensRepository::delete(db, &token.account_id)
        .await
        .expect("delete token metadata"));
    assert!(AccountsRepository::delete(db, &token.account_id)
        .await
        .expect("delete token account"));
    assert!(RequestTokenStatsRepository::get(db, run)
        .await
        .expect("deleted stat")
        .is_none());
    assert!(ApiKeysRepository::get(db, &id)
        .await
        .expect("deleted key")
        .is_none());
    crate::usage_snapshots::tests::exercise(db).await;
    crate::model_catalog::tests::exercise(db).await;
    let group_id = format!("group-{run}");
    ModelGroupsRepository::upsert(
        db,
        ModelGroupRecord {
            id: group_id.clone(),
            name: "Fixture group".into(),
            description: Some("跨库".into()),
            status: "active".into(),
            sort: 1,
            is_default: false,
            rate_multiplier_millis: 1000,
            created_at: run,
            updated_at: run,
        },
    )
    .await
    .expect("insert model group");
    assert_eq!(
        ModelGroupsRepository::get(db, &group_id)
            .await
            .unwrap()
            .unwrap()
            .name,
        "Fixture group"
    );
    ModelGroupsRepository::replace_models(
        db,
        &group_id,
        &[ModelGroupModelRecord {
            group_id: group_id.clone(),
            platform_model_slug: "gpt-5".into(),
            enabled: true,
            rate_multiplier_millis: Some(900),
            billing_model_slug: Some("gpt-5".into()),
            note: Some("fixture".into()),
            created_at: run,
            updated_at: run,
        }],
    )
    .await
    .expect("replace group models");
    assert_eq!(
        ModelGroupsRepository::list_models(db, &group_id, 10)
            .await
            .unwrap()
            .len(),
        1
    );
    ModelGroupsRepository::replace_user_assignments(
        db,
        &group_id,
        &[UserModelGroupRecord {
            user_id: "fixture-user".into(),
            group_id: group_id.clone(),
            status: "active".into(),
            expires_at: None,
            created_at: run,
            updated_at: run,
        }],
    )
    .await
    .expect("assign user model group");
    assert_eq!(
        ModelGroupsRepository::list_user_assignments(db, "fixture-user", 10)
            .await
            .unwrap()
            .len(),
        1
    );
    assert!(ModelGroupsRepository::delete(db, &group_id).await.unwrap());
    assert!(ModelGroupsRepository::list_models(db, &group_id, 10)
        .await
        .unwrap()
        .is_empty());
    assert!(
        ModelGroupsRepository::list_user_assignments(db, "fixture-user", 10)
            .await
            .unwrap()
            .is_empty()
    );
}
