use super::*;
use crate::{AccountsRepository, SeaOrmStorage};
use codexmanager_core::storage::{Account, StorageBackendKind, Token};

async fn exercise(storage: SeaOrmStorage) {
    storage.migrate().await.unwrap();
    let db = storage.connection();
    let id = format!(
        "token-cas-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    AccountsRepository::insert_account(
        db,
        &Account {
            id: id.clone(),
            label: "token CAS fixture".into(),
            issuer: "fixture".into(),
            chatgpt_account_id: None,
            workspace_id: None,
            group_name: None,
            sort: 0,
            status: "active".into(),
            created_at: 1,
            updated_at: 1,
        },
    )
    .await
    .unwrap();
    let expected = Token {
        account_id: id.clone(),
        id_token: "fixture-id".into(),
        access_token: "access-old".into(),
        refresh_token: "refresh-old".into(),
        api_key_access_token: None,
        last_refresh: 1,
    };
    AccountsRepository::insert_token(db, &expected)
        .await
        .unwrap();
    AccountTokensRepository::update_refresh_schedule(db, &id, Some(120), Some(60))
        .await
        .unwrap();
    let first = Token {
        access_token: "access-first".into(),
        refresh_token: "refresh-first".into(),
        last_refresh: 2,
        ..expected.clone()
    };
    let second = Token {
        access_token: "access-second".into(),
        refresh_token: "refresh-second".into(),
        last_refresh: 3,
        ..expected.clone()
    };
    let (left, right) = tokio::join!(
        AccountTokensRepository::compare_and_swap_token(db, &expected, &first),
        AccountTokensRepository::compare_and_swap_token(db, &expected, &second),
    );
    let left = left.unwrap();
    assert_ne!(left, right.unwrap());
    let winner = if left { first } else { second };
    let stored = AccountTokensRepository::get(db, &id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.access_token, winner.access_token);
    assert_eq!(stored.refresh_token, winner.refresh_token);
    assert_eq!(stored.last_refresh, winner.last_refresh);
    assert_eq!(stored.access_token_exp, Some(120));
    assert_eq!(stored.next_refresh_at, Some(60));
    let stale_access = Token {
        access_token: expected.access_token.clone(),
        ..winner.clone()
    };
    let stale_refresh = Token {
        refresh_token: expected.refresh_token.clone(),
        ..winner.clone()
    };
    assert!(
        !AccountTokensRepository::compare_and_swap_token(db, &stale_access, &expected)
            .await
            .unwrap()
    );
    assert!(
        !AccountTokensRepository::compare_and_swap_token(db, &stale_refresh, &expected)
            .await
            .unwrap()
    );
    let wrong_case = Token {
        access_token: winner.access_token.to_uppercase(),
        ..winner.clone()
    };
    assert!(
        !AccountTokensRepository::compare_and_swap_token(db, &wrong_case, &expected)
            .await
            .unwrap()
    );
    let exchanged = Token {
        api_key_access_token: Some("api-key-new".into()),
        ..winner.clone()
    };
    assert!(
        AccountTokensRepository::compare_and_swap_token(db, &winner, &exchanged)
            .await
            .unwrap()
    );
    let stale_clear = Token {
        api_key_access_token: None,
        ..winner.clone()
    };
    assert!(
        !AccountTokensRepository::compare_and_swap_token(db, &winner, &stale_clear)
            .await
            .unwrap()
    );
    assert_eq!(
        AccountTokensRepository::get(db, &id)
            .await
            .unwrap()
            .unwrap()
            .api_key_access_token
            .as_deref(),
        Some("api-key-new")
    );
    AccountTokensRepository::delete(db, &id).await.unwrap();
    assert!(
        !AccountTokensRepository::compare_and_swap_token(db, &winner, &expected)
            .await
            .unwrap()
    );
    assert!(AccountTokensRepository::get(db, &id)
        .await
        .unwrap()
        .is_none());
    AccountsRepository::delete_accounts(db, &[id])
        .await
        .unwrap();
}

#[tokio::test]
async fn sqlite_token_compare_and_swap() {
    exercise(
        SeaOrmStorage::connect(StorageBackendKind::Sqlite, "sqlite::memory:")
            .await
            .unwrap(),
    )
    .await;
}

#[tokio::test]
#[ignore = "requires isolated MySQL URL"]
async fn mysql_token_compare_and_swap() {
    exercise(
        SeaOrmStorage::connect(
            StorageBackendKind::Mysql,
            &std::env::var("CODEXMANAGER_TEST_MYSQL_URL").unwrap(),
        )
        .await
        .unwrap(),
    )
    .await;
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL URL"]
async fn postgres_token_compare_and_swap() {
    exercise(
        SeaOrmStorage::connect(
            StorageBackendKind::Postgres,
            &std::env::var("CODEXMANAGER_TEST_POSTGRES_URL").unwrap(),
        )
        .await
        .unwrap(),
    )
    .await;
}
