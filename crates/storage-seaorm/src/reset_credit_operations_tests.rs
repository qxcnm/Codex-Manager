use super::*;
use crate::SeaOrmStorage;
use codexmanager_core::storage::StorageBackendKind;
use sea_orm::DatabaseConnection;

fn unique(prefix: &str) -> String {
    format!(
        "{prefix}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    )
}

async fn exercise(db: &DatabaseConnection) {
    let operation_id = unique("reset-credit-operation");
    let created =
        ResetCreditOperationsRepository::claim(db, &operation_id, "account-1", "redeem-1", 10)
            .await
            .unwrap();
    assert!(matches!(created, ResetCreditOperationClaim::Created(_)));

    let existing = ResetCreditOperationsRepository::claim(
        db,
        &operation_id,
        "account-1",
        "redeem-ignored",
        11,
    )
    .await
    .unwrap();
    assert!(matches!(existing, ResetCreditOperationClaim::Existing(_)));
    assert_eq!(existing.operation().redeem_request_id, "redeem-1");

    let conflict =
        ResetCreditOperationsRepository::claim(db, &operation_id, "account-2", "redeem-2", 12)
            .await
            .unwrap();
    assert!(matches!(
        conflict,
        ResetCreditOperationClaim::AccountConflict(_)
    ));

    let completed = ResetCreditOperationsRepository::complete(
        db,
        &operation_id,
        "account-1",
        r#"{"consumed":true}"#,
        20,
    )
    .await
    .unwrap();
    assert!(matches!(completed, ResetCreditOperationUpdate::Updated(_)));
    let repeated =
        ResetCreditOperationsRepository::complete(db, &operation_id, "account-1", "ignored", 30)
            .await
            .unwrap();
    assert!(matches!(repeated, ResetCreditOperationUpdate::Existing(_)));
    assert_eq!(
        repeated.operation().unwrap().result_json.as_deref(),
        Some(r#"{"consumed":true}"#)
    );
    let enriched = ResetCreditOperationsRepository::update_completed_result(
        db,
        &operation_id,
        "account-1",
        r#"{"consumed":true,"remaining":2}"#,
        35,
    )
    .await
    .unwrap();
    assert!(matches!(enriched, ResetCreditOperationUpdate::Updated(_)));
    assert_eq!(
        enriched.operation().unwrap().result_json.as_deref(),
        Some(r#"{"consumed":true,"remaining":2}"#)
    );

    let failed_id = unique("reset-credit-failed");
    ResetCreditOperationsRepository::claim(db, &failed_id, "account-1", "redeem-3", 10)
        .await
        .unwrap();
    let failed =
        ResetCreditOperationsRepository::fail(db, &failed_id, "account-1", "provider rejected", 20)
            .await
            .unwrap();
    assert!(matches!(failed, ResetCreditOperationUpdate::Updated(_)));
    assert_eq!(
        failed.operation().unwrap().status,
        ResetCreditOperationStatus::Failed
    );
    assert!(matches!(
        ResetCreditOperationsRepository::update_completed_result(
            db,
            &failed_id,
            "account-1",
            r#"{"consumed":true}"#,
            30,
        )
        .await
        .unwrap(),
        ResetCreditOperationUpdate::Existing(_)
    ));
}

async fn exercise_concurrent_claim(db: &DatabaseConnection) {
    let operation_id = unique("reset-credit-concurrent");
    let mut tasks = Vec::new();
    for index in 0..8 {
        let db = db.clone();
        let operation_id = operation_id.clone();
        tasks.push(tokio::spawn(async move {
            ResetCreditOperationsRepository::claim(
                &db,
                &operation_id,
                "account-1",
                &format!("redeem-{index}"),
                10 + index,
            )
            .await
            .unwrap()
        }));
    }
    let mut claims = Vec::new();
    for task in tasks {
        claims.push(task.await.unwrap());
    }
    assert_eq!(
        claims
            .iter()
            .filter(|claim| matches!(claim, ResetCreditOperationClaim::Created(_)))
            .count(),
        1
    );
    let redeem_request_id = claims[0].operation().redeem_request_id.as_str();
    assert!(claims
        .iter()
        .all(|claim| claim.operation().redeem_request_id == redeem_request_id));
}

async fn exercise_concurrent_distinct_claims(db: &DatabaseConnection) {
    let account_id = unique("reset-credit-distinct-account");
    let mut tasks = Vec::new();
    for index in 0..8 {
        let db = db.clone();
        let account_id = account_id.clone();
        tasks.push(tokio::spawn(async move {
            let operation_id = unique(&format!("reset-credit-distinct-{index}"));
            let claim = ResetCreditOperationsRepository::claim(
                &db,
                &operation_id,
                &account_id,
                &format!("redeem-distinct-{index}"),
                10 + index,
            )
            .await
            .unwrap();
            (operation_id, claim)
        }));
    }
    let mut claims = Vec::new();
    for task in tasks {
        claims.push(task.await.unwrap());
    }
    assert_eq!(
        claims
            .iter()
            .filter(|(_, claim)| matches!(claim, ResetCreditOperationClaim::Created(_)))
            .count(),
        1
    );
    let created = claims
        .iter()
        .find_map(|(_, claim)| match claim {
            ResetCreditOperationClaim::Created(operation) => Some(operation.clone()),
            _ => None,
        })
        .expect("one distinct claim should create the pending operation");
    assert!(claims.iter().all(|(_, claim)| {
        claim.operation().account_id == account_id
            && (claim.operation().operation_id == created.operation_id
                || matches!(claim, ResetCreditOperationClaim::PendingAccount(_)))
    }));

    ResetCreditOperationsRepository::complete(
        db,
        &created.operation_id,
        &account_id,
        r#"{"consumed":true}"#,
        30,
    )
    .await
    .unwrap();
    let replacement = ResetCreditOperationsRepository::claim(
        db,
        &unique("reset-credit-distinct-replacement"),
        &account_id,
        "redeem-replacement",
        40,
    )
    .await
    .unwrap();
    assert!(matches!(replacement, ResetCreditOperationClaim::Created(_)));
}

async fn exercise_pending_account_guard(db: &DatabaseConnection) {
    let first_id = unique("reset-credit-pending");
    ResetCreditOperationsRepository::claim(db, &first_id, "account-pending", "redeem-1", 10)
        .await
        .unwrap();
    let second_id = unique("reset-credit-pending");
    let blocked =
        ResetCreditOperationsRepository::claim(db, &second_id, "account-pending", "redeem-2", 20)
            .await
            .unwrap();
    assert!(matches!(
        blocked,
        ResetCreditOperationClaim::PendingAccount(_)
    ));
    assert_eq!(blocked.operation().operation_id, first_id);

    ResetCreditOperationsRepository::complete(
        db,
        &first_id,
        "account-pending",
        r#"{"consumed":true}"#,
        30,
    )
    .await
    .unwrap();
    let next =
        ResetCreditOperationsRepository::claim(db, &second_id, "account-pending", "redeem-2", 40)
            .await
            .unwrap();
    assert!(matches!(next, ResetCreditOperationClaim::Created(_)));
}

#[tokio::test]
async fn sqlite_reset_credit_operation_idempotency() {
    let storage = SeaOrmStorage::connect(StorageBackendKind::Sqlite, "sqlite::memory:")
        .await
        .unwrap();
    storage.migrate().await.unwrap();
    exercise(storage.connection()).await;
    exercise_concurrent_claim(storage.connection()).await;
    exercise_concurrent_distinct_claims(storage.connection()).await;
    exercise_pending_account_guard(storage.connection()).await;
}

#[cfg(feature = "mysql")]
#[tokio::test]
#[ignore = "requires isolated MySQL test database"]
async fn mysql_reset_credit_operation_idempotency() {
    let storage = SeaOrmStorage::connect(
        StorageBackendKind::Mysql,
        &std::env::var("CODEXMANAGER_TEST_MYSQL_URL").unwrap(),
    )
    .await
    .unwrap();
    storage.migrate().await.unwrap();
    exercise(storage.connection()).await;
    exercise_concurrent_claim(storage.connection()).await;
    exercise_concurrent_distinct_claims(storage.connection()).await;
    exercise_pending_account_guard(storage.connection()).await;
}

#[cfg(feature = "postgres")]
#[tokio::test]
#[ignore = "requires isolated PostgreSQL test database"]
async fn postgres_reset_credit_operation_idempotency() {
    let storage = SeaOrmStorage::connect(
        StorageBackendKind::Postgres,
        &std::env::var("CODEXMANAGER_TEST_POSTGRES_URL").unwrap(),
    )
    .await
    .unwrap();
    storage.migrate().await.unwrap();
    exercise(storage.connection()).await;
    exercise_concurrent_claim(storage.connection()).await;
    exercise_concurrent_distinct_claims(storage.connection()).await;
    exercise_pending_account_guard(storage.connection()).await;
}
