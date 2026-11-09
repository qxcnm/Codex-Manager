use super::*;
use crate::{AccountRecord, SeaOrmStorage};
use codexmanager_core::storage::StorageBackendKind;

#[tokio::test]
async fn bps_adapter_batch_identity_and_deletion_match_sqlite() {
    let storage = SeaOrmStorage::connect(StorageBackendKind::Sqlite, "sqlite::memory:")
        .await
        .unwrap();
    storage.migrate().await.unwrap();
    let db = storage.connection();
    for id in ["a", "b"] {
        AccountsRepository::upsert(
            db,
            AccountRecord {
                id: id.into(),
                label: id.into(),
                issuer: "https://auth.openai.com".into(),
                chatgpt_account_id: Some(format!("up-{id}")),
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
    }
    let credentials = |id: &str| AccountBpsSettings {
        account_id: id.into(),
        enabled: false,
        access_token: Some("bps-access".into()),
        refresh_token: Some("bps-refresh".into()),
        upstream_account_id: Some(format!("up-{id}")),
        expires_at: Some(now_ts() + 3600),
        updated_at: now_ts(),
    };
    AccountsRepository::save_account_bps_credentials(db, &credentials("a"))
        .await
        .unwrap();
    assert!(
        AccountsRepository::set_account_bps_enabled(db, &["a".into(), "b".into()], true)
            .await
            .is_err()
    );
    assert!(!AccountsRepository::any_account_bps_enabled(db)
        .await
        .unwrap());
    let mut wrong = credentials("b");
    wrong.upstream_account_id = Some("other".into());
    assert!(AccountsRepository::save_account_bps_credentials(db, &wrong)
        .await
        .is_err());
    AccountsRepository::save_account_bps_credentials(db, &credentials("b"))
        .await
        .unwrap();
    AccountsRepository::set_account_bps_enabled(db, &["a".into(), "b".into()], true)
        .await
        .unwrap();
    AccountsRepository::save_account_bps_credentials(db, &credentials("a"))
        .await
        .unwrap();
    assert!(
        AccountsRepository::find_account_bps_settings(db, "a")
            .await
            .unwrap()
            .unwrap()
            .enabled
    );
    assert_eq!(
        AccountsRepository::list_account_bps_settings(db, &["a".into(), "b".into()])
            .await
            .unwrap()
            .len(),
        2
    );
    AccountsRepository::delete_accounts(db, &["a".into(), "b".into()])
        .await
        .unwrap();
    assert!(!AccountsRepository::any_account_bps_enabled(db)
        .await
        .unwrap());
    assert!(AccountsRepository::find_account_bps_settings(db, "a")
        .await
        .unwrap()
        .is_none());
}
