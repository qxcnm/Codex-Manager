use super::*;
use crate::{
    CatalogModelRecord, CatalogPriceRecord, CatalogPriceTierRecord, ModelGroupModelRecord,
    ModelGroupRecord, ModelGroupsRepository as Groups, SeaOrmStorage, UserModelGroupRecord,
    UsersRepository as Users,
};
use codexmanager_core::storage::{
    AppUser, AppUserSession, ModelFastPolicyV2, ModelPriceTierV2, ModelPriceV2, StorageBackendKind,
};

async fn exercise(db: &DatabaseConnection) {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_micros() as i64;
    let id = format!("domain-{stamp}");
    let user = AppUser {
        id: id.clone(),
        username: id.clone(),
        display_name: None,
        password_hash: "fixture-hash".into(),
        role: "member".into(),
        status: "active".into(),
        created_at: 1,
        updated_at: 1,
        last_login_at: None,
    };
    Users::put(db, user.clone()).await.unwrap();
    let session = AppUserSession {
        id: format!("{id}-session"),
        user_id: id.clone(),
        token_hash: format!("{id}-token"),
        expires_at: 50,
        created_at: 1,
        last_seen_at: None,
        revoked_at: None,
    };
    Users::create_session(db, session.clone()).await.unwrap();
    assert!(Users::active_session(db, &session.token_hash, 49)
        .await
        .unwrap()
        .is_some());
    assert!(Users::active_session(db, &session.token_hash, 50)
        .await
        .unwrap()
        .is_none());
    let mut disabled = user.clone();
    disabled.status = "disabled".into();
    Users::put(db, disabled).await.unwrap();
    assert!(Users::active_session(db, &session.token_hash, 2)
        .await
        .unwrap()
        .is_none());
    Users::put(db, user).await.unwrap();
    Users::revoke_session(db, &session.token_hash, 3)
        .await
        .unwrap();
    assert!(Users::active_session(db, &session.token_hash, 4)
        .await
        .unwrap()
        .is_none());
    ModelCatalogRepository::put(
        db,
        CatalogModelRecord {
            id: id.clone(),
            slug: id.clone(),
            display_name: id.clone(),
            description: None,
            provider: None,
            family: None,
            category: None,
            origin: "custom".into(),
            enabled: true,
            supported_in_api: true,
            visibility: "list".into(),
            sort_order: 0,
            context_window: None,
            max_context_window: None,
            default_reasoning_effort: None,
            instructions_mode: "passthrough".into(),
            instructions_text: None,
            builtin_revision: None,
            user_edited: true,
            created_at: 1,
            updated_at: 1,
            tags: vec![],
            capabilities: serde_json::json!({}),
            fast_policy: ModelFastPolicyV2::Filter,
        },
    )
    .await
    .unwrap();
    ModelPricesRepository::put(
        db,
        CatalogPriceRecord {
            model_id: id.clone(),
            price: ModelPriceV2 {
                price_status: "custom".into(),
                price_source: None,
                input_microusd_per_1m: Some(2_000_000),
                cached_input_microusd_per_1m: Some(200_000),
                cache_write_microusd_per_1m: None,
                output_microusd_per_1m: Some(4_000_000),
            },
            created_at: 1,
            updated_at: 1,
        },
    )
    .await
    .unwrap();
    ModelPriceTiersRepository::replace_for_model(
        db,
        &id,
        &[CatalogPriceTierRecord {
            model_id: id.clone(),
            tier: ModelPriceTierV2 {
                min_input_tokens: 0,
                input_microusd_per_1m: 2_000_000,
                cached_input_microusd_per_1m: 200_000,
                cache_write_microusd_per_1m: None,
                output_microusd_per_1m: 4_000_000,
            },
            created_at: 1,
            updated_at: 1,
        }],
    )
    .await
    .unwrap();
    Groups::upsert(
        db,
        ModelGroupRecord {
            id: id.clone(),
            name: id.clone(),
            description: None,
            status: "active".into(),
            sort: 0,
            is_default: false,
            rate_multiplier_millis: 500,
            created_at: 1,
            updated_at: 1,
        },
    )
    .await
    .unwrap();
    let row = ModelGroupModelRecord {
        group_id: id.clone(),
        platform_model_slug: id.clone(),
        enabled: true,
        rate_multiplier_millis: Some(0),
        billing_model_slug: None,
        note: None,
        created_at: 1,
        updated_at: 1,
    };
    Groups::replace_models_v2(db, &id, &[row.clone()])
        .await
        .unwrap();
    Groups::replace_user_assignments(
        db,
        &id,
        &[UserModelGroupRecord {
            user_id: id.clone(),
            group_id: id.clone(),
            status: "active".into(),
            expires_at: Some(50),
            created_at: 1,
            updated_at: 1,
        }],
    )
    .await
    .unwrap();
    assert_eq!(
        Groups::resolve_access_v2(db, &id, &id.to_uppercase(), 49)
            .await
            .unwrap()
            .unwrap()
            .rate_multiplier_millis,
        0
    );
    assert!(Groups::resolve_access_v2(db, &id, &id, 50)
        .await
        .unwrap()
        .is_none());
    let mut bad = row.clone();
    bad.platform_model_slug = "nonexistent-model".into();
    assert!(Groups::replace_models_v2(db, &id, &[bad]).await.is_err());
    assert!(Groups::resolve_access_v2(db, &id, &id, 49)
        .await
        .unwrap()
        .is_some());
    let original_price = ModelPricesRepository::get(db, &id).await.unwrap().unwrap();
    let mut missing_price = original_price.clone();
    missing_price.price = ModelPriceV2 {
        price_status: "missing".into(),
        ..Default::default()
    };
    ModelPricesRepository::put(db, missing_price).await.unwrap();
    assert!(Groups::resolve_access_v2(db, &id, &id, 49)
        .await
        .unwrap()
        .is_none());
    let mut default_group = Groups::get(db, &id).await.unwrap().unwrap();
    default_group.is_default = true;
    Groups::upsert(db, default_group).await.unwrap();
    Groups::replace_models_v2(db, &id, &[]).await.unwrap();
    assert!(Groups::resolve_access_v2(db, &id, &id, 49)
        .await
        .unwrap()
        .is_some());
    assert!(
        !Groups::delete(db, &id).await.unwrap(),
        "default group cannot be deleted"
    );
    assert!(
        Groups::replace_models_v2(db, &id, &[row]).await.is_err(),
        "default group is dynamic"
    );
    ModelPricesRepository::put(db, original_price)
        .await
        .unwrap();
    BillingRepository::create_wallet(
        db,
        AppWallet {
            id: id.clone(),
            owner_kind: "user".into(),
            owner_id: id.clone(),
            balance_credit_micros: 1000,
            frozen_credit_micros: 0,
            status: "active".into(),
            created_at: 1,
            updated_at: 1,
        },
    )
    .await
    .unwrap();
    let input = ChargeSnapshotInputV2 {
        request_log_id: stamp,
        model_slug: id.clone(),
        usage_source: "actual".into(),
        input_tokens: 100,
        rate_multiplier_millis: 1000,
        wallet_id: Some(id.clone()),
        ..Default::default()
    };
    let mut pending = Vec::new();
    for _ in 0..8 {
        let db = db.clone();
        let input = input.clone();
        pending.push(tokio::spawn(async move {
            BillingRepository::record_charge(&db, &input).await
        }));
    }
    for task in pending {
        let snapshot = task.await.unwrap().expect("concurrent identical charge");
        assert_eq!(snapshot.charged_cost_microusd, 200);
    }
    assert_eq!(
        BillingRepository::wallet(db, &id)
            .await
            .unwrap()
            .unwrap()
            .balance_credit_micros,
        800
    );
    assert_eq!(
        BillingRepository::ledger(db, &id, 100).await.unwrap().len(),
        1
    );
    let mut conflict = input.clone();
    conflict.wallet_id = None;
    assert!(BillingRepository::record_charge(db, &conflict)
        .await
        .unwrap_err()
        .to_string()
        .contains("idempotency"));
    let mut pending = Vec::new();
    for n in 1..=8 {
        let db = db.clone();
        let mut input = input.clone();
        input.request_log_id += n;
        pending.push(tokio::spawn(async move {
            BillingRepository::record_charge(&db, &input).await
        }));
    }
    let mut success = 0;
    for task in pending {
        match task.await.unwrap() {
            Ok(_) => success += 1,
            Err(e) => assert!(e.to_string().contains("wallet_insufficient_balance"), "{e}"),
        }
    }
    assert_eq!(success, 4);
    assert_eq!(
        BillingRepository::wallet(db, &id)
            .await
            .unwrap()
            .unwrap()
            .balance_credit_micros,
        0
    );
    assert_eq!(
        BillingRepository::ledger(db, &id, 100).await.unwrap().len(),
        5
    );
    let mut free = input.clone();
    free.request_log_id += 20;
    free.rate_multiplier_millis = 0;
    assert_eq!(
        BillingRepository::record_charge(db, &free)
            .await
            .unwrap()
            .charged_cost_microusd,
        0
    );
    let entry = AppWalletLedgerEntry {
        id: format!("{id}-topup"),
        wallet_id: id.clone(),
        entry_kind: "manual_adjustment".into(),
        amount_credit_micros: 100,
        balance_after_credit_micros: 0,
        request_log_id: None,
        api_key_id: None,
        pricing_rule_id: None,
        raw_usage_json: None,
        note: None,
        created_by_user_id: None,
        created_at: 2,
    };
    BillingRepository::adjust_balance(db, entry.clone())
        .await
        .unwrap();
    BillingRepository::adjust_balance(db, entry).await.unwrap();
    assert_eq!(
        BillingRepository::wallet(db, &id)
            .await
            .unwrap()
            .unwrap()
            .balance_credit_micros,
        100
    );
    // Insufficient funds did not leave partial snapshots.
    let rows = BillingRepository::ledger(db, &id, 100).await.unwrap();
    for n in 1..=8 {
        assert_eq!(
            BillingRepository::snapshot(db, stamp + n)
                .await
                .unwrap()
                .is_some(),
            rows.iter().any(|r| r.request_log_id == Some(stamp + n))
        );
    }
}
#[tokio::test]
async fn sqlite_domains_preserve_permissions_and_concurrent_ledger() {
    let storage = SeaOrmStorage::connect(StorageBackendKind::Sqlite, "sqlite::memory:")
        .await
        .unwrap();
    storage.migrate().await.unwrap();
    exercise(storage.connection()).await;
}
#[tokio::test]
#[ignore = "requires isolated MySQL URL"]
async fn mysql_domains_preserve_permissions_and_concurrent_ledger() {
    let storage = SeaOrmStorage::connect(
        StorageBackendKind::Mysql,
        &std::env::var("CODEXMANAGER_TEST_MYSQL_URL").unwrap(),
    )
    .await
    .unwrap();
    storage.migrate().await.unwrap();
    exercise(storage.connection()).await;
}
#[tokio::test]
#[ignore = "requires isolated PostgreSQL URL"]
async fn postgres_domains_preserve_permissions_and_concurrent_ledger() {
    let storage = SeaOrmStorage::connect(
        StorageBackendKind::Postgres,
        &std::env::var("CODEXMANAGER_TEST_POSTGRES_URL").unwrap(),
    )
    .await
    .unwrap();
    storage.migrate().await.unwrap();
    exercise(storage.connection()).await;
}
