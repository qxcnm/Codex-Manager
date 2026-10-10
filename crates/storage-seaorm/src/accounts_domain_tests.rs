use crate::{
    AccountTokensRepository, AccountsRepository as Accounts, AggregateApisRepository as Providers,
    SeaOrmStorage, UsageSnapshotsRepository as Usage,
};
use codexmanager_core::storage::*;
use sea_orm::DatabaseConnection;

async fn exercise(db: &DatabaseConnection) {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_micros();
    let prefix = format!("account-domain-{stamp}");
    let account = Account {
        id: format!("{prefix}-a"),
        label: "fixture".into(),
        issuer: "https://provider.invalid".into(),
        chatgpt_account_id: None,
        workspace_id: None,
        group_name: None,
        sort: 1,
        status: "active".into(),
        created_at: 1,
        updated_at: 2,
    };
    let second = Account {
        id: format!("{prefix}-b"),
        ..account.clone()
    };
    let token = Token {
        account_id: account.id.clone(),
        id_token: "fixture-id".into(),
        access_token: "fixture-access".into(),
        refresh_token: "fixture-refresh".into(),
        api_key_access_token: None,
        last_refresh: 3,
    };
    let identity = AccountAgentIdentity {
        account_id: account.id.clone(),
        agent_runtime_id: "fixture-runtime".into(),
        agent_private_key: "fixture-key".into(),
        task_id: None,
        chatgpt_user_id: "fixture-user".into(),
        chatgpt_account_is_fedramp: false,
        auth_mode: "agentidentity".into(),
        workspace_id: None,
        created_at: 1,
        updated_at: 2,
    };
    Accounts::upsert_imported_account_bundle(
        db,
        &account,
        Some(" note "),
        Some(" tags "),
        &token,
        Some(&identity),
    )
    .await
    .unwrap();
    Accounts::insert_account(db, &second).await.unwrap();
    Accounts::insert_token(
        db,
        &Token {
            account_id: second.id.clone(),
            ..token.clone()
        },
    )
    .await
    .unwrap();
    Accounts::update_token_refresh_schedule(db, &account.id, Some(500), Some(400))
        .await
        .unwrap();
    Accounts::touch_token_refresh_attempt(db, &account.id, 7)
        .await
        .unwrap();
    Accounts::insert_token(
        db,
        &Token {
            access_token: "rotated-fixture".into(),
            ..token.clone()
        },
    )
    .await
    .unwrap();
    let token_state = AccountTokensRepository::get(db, &account.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(token_state.next_refresh_at, Some(400));
    assert_eq!(token_state.last_refresh_attempt_at, Some(7));
    assert!(
        Accounts::list_tokens_due_for_refresh(db, 100, 100, usize::MAX)
            .await
            .unwrap()
            .iter()
            .all(|t| t.account_id != account.id)
    );
    Accounts::insert_event(
        db,
        &Event {
            account_id: Some(account.id.clone()),
            event_type: "account_status_update".into(),
            message: "status=unavailable reason=account_deactivated".into(),
            created_at: 4,
        },
    )
    .await
    .unwrap();
    assert!(
        Accounts::list_tokens_due_for_refresh(db, 1000, 1000, usize::MAX)
            .await
            .unwrap()
            .iter()
            .all(|t| t.account_id != account.id)
    );
    assert_eq!(
        Accounts::latest_account_status_reasons(db, &[account.id.clone()])
            .await
            .unwrap()[&account.id],
        "account_deactivated"
    );
    assert!(!Accounts::update_account_agent_identity_task_id(
        db,
        &account.id,
        "stale",
        "fixture-key",
        Some("task")
    )
    .await
    .unwrap());
    assert!(Accounts::update_account_agent_identity_task_id(
        db,
        &account.id,
        "fixture-runtime",
        "fixture-key",
        Some("task")
    )
    .await
    .unwrap());
    Accounts::set_preferred_account(db, Some(&account.id))
        .await
        .unwrap();
    Accounts::insert_account(db, &account).await.unwrap();
    assert_eq!(
        Accounts::preferred_account_id(db).await.unwrap().as_deref(),
        Some(account.id.as_str())
    );
    let ids = vec![second.id.clone(), account.id.clone()];
    let candidates = Accounts::list_gateway_candidates_for_accounts(db, &ids)
        .await
        .unwrap();
    assert_eq!(
        candidates
            .iter()
            .map(|(a, _)| a.id.clone())
            .collect::<Vec<_>>(),
        vec![account.id.clone(), second.id.clone()]
    );

    let snapshot = UsageSnapshotRecord {
        account_id: account.id.clone(),
        used_percent: Some(100.),
        window_minutes: Some(300),
        resets_at: Some(200),
        secondary_used_percent: None,
        secondary_window_minutes: None,
        secondary_resets_at: None,
        credits_json: None,
        captured_at: 100,
    };
    Usage::insert_and_prune(db, &snapshot, 0).await.unwrap();
    let (_, pruned) = Usage::insert_and_prune(
        db,
        &UsageSnapshotRecord {
            captured_at: 90,
            ..snapshot.clone()
        },
        0,
    )
    .await
    .unwrap();
    assert_eq!(pruned, 0);
    assert_eq!(Usage::count_for_account(db, &account.id).await.unwrap(), 2);
    assert_eq!(
        Usage::latest_for_account(db, &account.id)
            .await
            .unwrap()
            .unwrap()
            .snapshot
            .captured_at,
        100
    );
    assert_eq!(
        Accounts::list_gateway_candidates_for_accounts(db, &ids)
            .await
            .unwrap()
            .len(),
        1
    );
    Accounts::update_account_status(db, &account.id, "force_enabled")
        .await
        .unwrap();
    assert_eq!(
        Accounts::list_gateway_candidates_for_accounts(db, &ids)
            .await
            .unwrap()
            .len(),
        2
    );
    Accounts::update_account_status(db, &account.id, "limited")
        .await
        .unwrap();
    assert_eq!(
        Accounts::list_gateway_candidates_unfiltered_for_accounts(db, &ids)
            .await
            .unwrap()
            .len(),
        2
    );
    Accounts::update_account_status(db, &account.id, "disabled")
        .await
        .unwrap();
    assert_eq!(
        Accounts::list_gateway_candidates_unfiltered_for_accounts(db, &ids)
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        Usage::insert_and_prune(db, &snapshot, 1).await.unwrap().1,
        2
    );
    assert_eq!(Usage::count_for_account(db, &account.id).await.unwrap(), 1);
    let (tx, previous) = Usage::begin_snapshot_write(db, &account.id).await.unwrap();
    assert!(previous.is_some());
    tx.rollback().await.unwrap();
    assert_eq!(Usage::count_for_account(db, &account.id).await.unwrap(), 1);

    let session = LoginSession {
        login_id: prefix.clone(),
        code_verifier: "fixture-verifier".into(),
        state: prefix.clone(),
        status: "pending".into(),
        error: None,
        workspace_id: None,
        note: None,
        tags: None,
        group_name: None,
        created_at: 1,
        updated_at: 1,
    };
    Accounts::insert_login_session(db, &session).await.unwrap();
    let mut attempts = vec![];
    for _ in 0..4 {
        let db = db.clone();
        let id = prefix.clone();
        attempts.push(tokio::spawn(async move {
            Accounts::claim_login_session_for_completion(&db, &id)
                .await
                .unwrap()
        }));
    }
    let mut claimed = 0;
    for attempt in attempts {
        claimed += usize::from(attempt.await.unwrap());
    }
    assert_eq!(claimed, 1);
    assert!(!Accounts::cancel_login_session(db, &prefix).await.unwrap());
    let mut stale_owner = session.clone();
    stale_owner.code_verifier = "stale-verifier".into();
    assert!(
        !Accounts::finish_claimed_login_session(db, &stale_owner, "failed", Some("cancelled"))
            .await
            .unwrap()
    );
    let mut case_changed_owner = session.clone();
    case_changed_owner.code_verifier = session.code_verifier.to_uppercase();
    assert_ne!(case_changed_owner.code_verifier, session.code_verifier);
    assert!(!Accounts::finish_claimed_login_session(
        db,
        &case_changed_owner,
        "failed",
        Some("case changed")
    )
    .await
    .unwrap());
    case_changed_owner = session.clone();
    case_changed_owner.state = session.state.to_uppercase();
    assert_ne!(case_changed_owner.state, session.state);
    assert!(!Accounts::finish_claimed_login_session(
        db,
        &case_changed_owner,
        "failed",
        Some("case changed")
    )
    .await
    .unwrap());
    assert!(
        Accounts::finish_claimed_login_session(db, &session, "success", None)
            .await
            .unwrap()
    );
    assert!(!Accounts::finish_claimed_login_session(
        db,
        &session,
        "failed",
        Some("late cancellation")
    )
    .await
    .unwrap());
    assert_eq!(
        Accounts::get_login_session(db, &prefix)
            .await
            .unwrap()
            .unwrap()
            .code_verifier,
        ""
    );
    assert!(
        !Accounts::finish_login_session(db, &prefix, "failed", Some("stale"))
            .await
            .unwrap()
    );

    let provider = AggregateApi {
        id: prefix.clone(),
        provider_type: "openai-compatible".into(),
        supplier_name: Some("Fixture supplier".into()),
        sort: 0,
        url: "https://provider.invalid/v1".into(),
        auth_type: "apikey".into(),
        auth_params_json: None,
        action: None,
        model_override: None,
        user_agent: None,
        status: "active".into(),
        created_at: 1,
        updated_at: 2,
        last_test_at: None,
        last_test_status: None,
        last_test_error: None,
        balance_query_enabled: true,
        balance_query_template: None,
        balance_query_base_url: None,
        balance_query_user_id: None,
        balance_query_config_json: None,
        last_balance_at: None,
        last_balance_status: None,
        last_balance_error: None,
        last_balance_json: None,
    };
    Providers::insert_aggregate_api(db, &provider)
        .await
        .unwrap();
    Providers::upsert_aggregate_api_secret(db, &prefix, "fixture-secret")
        .await
        .unwrap();
    Providers::upsert_aggregate_api_balance_secret(db, &prefix, "fixture-balance-secret")
        .await
        .unwrap();
    Providers::update_aggregate_api_balance_result(
        db,
        &prefix,
        true,
        Some("{\"remaining\":7}"),
        None,
    )
    .await
    .unwrap();
    Providers::update_aggregate_api_status(db, &prefix, "disabled")
        .await
        .unwrap();
    let bundle = Providers::find_aggregate_api_with_secrets_by_id(db, &prefix)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(bundle.secret_value.as_deref(), Some("fixture-secret"));
    assert_eq!(
        bundle.balance_access_token.as_deref(),
        Some("fixture-balance-secret")
    );
    assert_eq!(bundle.api.last_balance_status.as_deref(), Some("success"));
    assert_eq!(bundle.api.status, "disabled");
    let model = AggregateApiSupplierModel {
        supplier_key: prefix.clone(),
        provider_type: "openai".into(),
        upstream_model: "fixture-model".into(),
        display_name: Some("first".into()),
        status: "active".into(),
        created_at: 1,
        updated_at: 2,
    };
    Providers::upsert_aggregate_api_supplier_model(db, &model)
        .await
        .unwrap();
    Providers::upsert_aggregate_api_supplier_model(
        db,
        &AggregateApiSupplierModel {
            display_name: Some("second".into()),
            created_at: 100,
            updated_at: 3,
            ..model.clone()
        },
    )
    .await
    .unwrap();
    let models = Providers::list_aggregate_api_supplier_models(db, Some(&prefix), Some("openai"))
        .await
        .unwrap();
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].created_at, 1);
    assert_eq!(models[0].display_name.as_deref(), Some("second"));
    Providers::delete_aggregate_api_supplier_model(db, &prefix, "openai", "fixture-model")
        .await
        .unwrap();
    Providers::delete_aggregate_api(db, &prefix).await.unwrap();
    assert!(Providers::find_aggregate_api_secret_by_id(db, &prefix)
        .await
        .unwrap()
        .is_none());
    assert!(
        Providers::find_aggregate_api_balance_secret_by_id(db, &prefix)
            .await
            .unwrap()
            .is_none()
    );

    Accounts::upsert_imported_account_bundle(db, &account, None, None, &token, None)
        .await
        .unwrap();
    assert!(Accounts::find_account_agent_identity(db, &account.id)
        .await
        .unwrap()
        .is_none());
    assert_eq!(
        Accounts::find_account_metadata(db, &account.id)
            .await
            .unwrap()
            .unwrap()
            .note
            .as_deref(),
        Some("note")
    );
    assert_eq!(Accounts::delete_accounts(db, &ids).await.unwrap(), 2);
    assert_eq!(Usage::count_for_account(db, &account.id).await.unwrap(), 0);
    assert!(Accounts::find_account_metadata(db, &account.id)
        .await
        .unwrap()
        .is_none());
    assert!(Accounts::find_token_by_account_id(db, &account.id)
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn sqlite_account_provider_domains() {
    let storage = SeaOrmStorage::connect(StorageBackendKind::Sqlite, "sqlite::memory:")
        .await
        .unwrap();
    storage.migrate().await.unwrap();
    exercise(storage.connection()).await;
}

#[tokio::test]
#[ignore = "requires isolated MySQL URL"]
async fn mysql_account_provider_domains() {
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
async fn postgres_account_provider_domains() {
    let storage = SeaOrmStorage::connect(
        StorageBackendKind::Postgres,
        &std::env::var("CODEXMANAGER_TEST_POSTGRES_URL").unwrap(),
    )
    .await
    .unwrap();
    storage.migrate().await.unwrap();
    exercise(storage.connection()).await;
}
