use super::*;
use crate::{AccountRecord, AccountTokenRecord, SeaOrmStorage};
use codexmanager_core::storage::StorageBackendKind;

async fn exercise(db: &DatabaseConnection) {
    let id = format!(
        "warmup-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    AccountsRepository::upsert(
        db,
        AccountRecord {
            id: id.clone(),
            label: "test".into(),
            issuer: "test".into(),
            chatgpt_account_id: None,
            workspace_id: None,
            subject_account_id: None,
            note: None,
            tags: None,
            group_name: None,
            sort: 0,
            status: "active".into(),
            created_at: 1,
            updated_at: 1,
        },
    )
    .await
    .unwrap();
    AccountTokensRepository::upsert(
        db,
        AccountTokenRecord {
            account_id: id.clone(),
            id_token: String::new(),
            access_token: "fixture".into(),
            refresh_token: String::new(),
            api_key_access_token: None,
            last_refresh: 1,
            access_token_exp: None,
            next_refresh_at: None,
            last_refresh_attempt_at: None,
        },
    )
    .await
    .unwrap();
    let mut snap = UsageSnapshotRecord {
        account_id: id.clone(),
        used_percent: Some(100.0),
        window_minutes: Some(300),
        resets_at: Some(100),
        secondary_used_percent: Some(100.0),
        secondary_window_minutes: Some(10080),
        secondary_resets_at: None,
        credits_json: None,
        captured_at: 90,
    };
    async fn save(db: &DatabaseConnection, snap: &UsageSnapshotRecord) {
        let tx = db.begin().await.unwrap();
        UsersRepository::lock(&tx, "accounts").await.unwrap();
        UsageSnapshotsRepository::insert(&tx, snap).await.unwrap();
        observe_usage_snapshot(&tx, snap).await.unwrap();
        tx.commit().await.unwrap();
    }
    save(db, &snap).await;
    assert!(
        AccountsRepository::list_account_reset_warmup_targets(db, 200, 100)
            .await
            .unwrap()
            .iter()
            .all(|t| t.account_id != id),
        "unknown exhausted weekly reset blocks send"
    );
    snap.secondary_resets_at = Some(110);
    snap.captured_at = 91;
    save(db, &snap).await;
    assert!(
        !AccountsRepository::claim_account_reset_warmup(db, &id, 100, 114)
            .await
            .unwrap()
    );
    assert!(AccountsRepository::set_account_reset_warmup_enabled(
        db,
        &[id.clone(), "missing-account-fixture".into()],
        false
    )
    .await
    .is_err());
    assert_eq!(
        AccountsRepository::list_account_reset_warmup_settings_for_accounts(db, &[id.clone()])
            .await
            .unwrap(),
        vec![(id.clone(), true)]
    );
    let mut joins = Vec::new();
    for _ in 0..8 {
        let db = db.clone();
        let id = id.clone();
        joins.push(tokio::spawn(async move {
            AccountsRepository::claim_account_reset_warmup(&db, &id, 100, 115)
                .await
                .unwrap()
        }));
    }
    let mut claimed = 0;
    for j in joins {
        claimed += usize::from(j.await.unwrap());
    }
    assert_eq!(claimed, 1, "only one process can consume a cycle");
    save(db, &snap).await;
    assert!(
        !AccountsRepository::claim_account_reset_warmup(db, &id, 100, 200)
            .await
            .unwrap(),
        "stale exhausted snapshot cannot rearm consumed cycle"
    );
    snap.resets_at = Some(300);
    snap.captured_at = 250;
    snap.secondary_used_percent = Some(20.0);
    save(db, &snap).await;
    snap.used_percent = Some(1.0);
    snap.resets_at = Some(600);
    snap.captured_at = 301;
    save(db, &snap).await;
    assert!(
        !AccountsRepository::claim_account_reset_warmup(db, &id, 300, 500)
            .await
            .unwrap(),
        "already started next cycle must consume pending reminder"
    );
    snap.used_percent = Some(100.0);
    snap.resets_at = Some(300);
    snap.captured_at = 250;
    save(db, &snap).await;
    assert!(
        !AccountsRepository::claim_account_reset_warmup(db, &id, 300, 500)
            .await
            .unwrap()
    );
}
#[tokio::test]
async fn sqlite_reset_cycle_eligibility_and_concurrent_claim() {
    let s = SeaOrmStorage::connect(StorageBackendKind::Sqlite, "sqlite::memory:")
        .await
        .unwrap();
    s.migrate().await.unwrap();
    exercise(s.connection()).await;
}
#[cfg(feature = "mysql")]
#[tokio::test]
#[ignore = "requires isolated MySQL test database"]
async fn mysql_reset_cycle_eligibility_and_concurrent_claim() {
    let s = SeaOrmStorage::connect(
        StorageBackendKind::Mysql,
        &std::env::var("CODEXMANAGER_TEST_MYSQL_URL").unwrap(),
    )
    .await
    .unwrap();
    s.migrate().await.unwrap();
    exercise(s.connection()).await;
}
#[cfg(feature = "postgres")]
#[tokio::test]
#[ignore = "requires isolated PostgreSQL test database"]
async fn postgres_reset_cycle_eligibility_and_concurrent_claim() {
    let s = SeaOrmStorage::connect(
        StorageBackendKind::Postgres,
        &std::env::var("CODEXMANAGER_TEST_POSTGRES_URL").unwrap(),
    )
    .await
    .unwrap();
    s.migrate().await.unwrap();
    exercise(s.connection()).await;
}
