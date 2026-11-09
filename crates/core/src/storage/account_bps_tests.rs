use super::*;
use crate::storage::{Account, Token};

fn account(id: &str) -> Account {
    Account {
        id: id.into(),
        label: id.into(),
        issuer: "https://auth.openai.com".into(),
        chatgpt_account_id: Some(format!("up-{id}")),
        workspace_id: None,
        group_name: None,
        sort: 0,
        status: "active".into(),
        created_at: 1,
        updated_at: 1,
    }
}
fn credentials(id: &str) -> AccountBpsSettings {
    AccountBpsSettings {
        account_id: id.into(),
        enabled: false,
        access_token: Some("bps-access".into()),
        refresh_token: Some("bps-refresh".into()),
        upstream_account_id: Some(format!("up-{id}")),
        expires_at: Some(now_ts() + 3600),
        updated_at: now_ts(),
    }
}
#[test]
fn bps_batch_enable_requires_every_credential_and_preserves_codex_tokens() {
    let db = Storage::open_in_memory().unwrap();
    db.init().unwrap();
    for id in ["a", "b"] {
        db.insert_account(&account(id)).unwrap();
    }
    db.insert_token(&Token {
        account_id: "a".into(),
        id_token: "codex-id".into(),
        access_token: "codex-access".into(),
        refresh_token: "codex-refresh".into(),
        api_key_access_token: None,
        last_refresh: 1,
    })
    .unwrap();
    assert!(!db.any_account_bps_enabled().unwrap());
    db.save_account_bps_credentials(&credentials("a")).unwrap();
    assert!(db
        .set_account_bps_enabled(&["a".into(), "b".into()], true)
        .is_err());
    assert!(!db.find_account_bps_settings("a").unwrap().unwrap().enabled);
    assert!(db.find_account_bps_settings("b").unwrap().is_none());
    db.save_account_bps_credentials(&credentials("b")).unwrap();
    assert_eq!(
        db.set_account_bps_enabled(&["a".into(), "b".into()], true)
            .unwrap(),
        2
    );
    assert!(db.any_account_bps_enabled().unwrap());
    let mut replacement = credentials("a");
    replacement.access_token = Some("replacement".into());
    db.save_account_bps_credentials(&replacement).unwrap();
    assert!(db.find_account_bps_settings("a").unwrap().unwrap().enabled);
    db.insert_account(&account("a")).unwrap();
    let reimported = db.find_account_bps_settings("a").unwrap().unwrap();
    assert!(
        reimported.enabled,
        "reimport must preserve the BPS route choice"
    );
    assert_eq!(reimported.access_token.as_deref(), Some("replacement"));
    let original = db.find_token_by_account_id("a").unwrap().unwrap();
    assert_eq!(original.access_token, "codex-access");
    assert_eq!(original.refresh_token, "codex-refresh");
    db.set_account_bps_enabled(&["a".into(), "b".into()], false)
        .unwrap();
    assert!(!db.any_account_bps_enabled().unwrap());
    assert!(db
        .find_account_bps_settings("a")
        .unwrap()
        .unwrap()
        .authorized_at(now_ts()));
}
#[test]
fn bps_rejects_wrong_identity_expiry_and_missing_account_without_partial_update() {
    let db = Storage::open_in_memory().unwrap();
    db.init().unwrap();
    db.insert_account(&account("a")).unwrap();
    let mut invalid = credentials("a");
    invalid.upstream_account_id = Some("different".into());
    assert!(db.save_account_bps_credentials(&invalid).is_err());
    let mut expired = credentials("a");
    expired.expires_at = Some(now_ts() - 1);
    assert!(db.save_account_bps_credentials(&expired).is_err());
    db.save_account_bps_credentials(&credentials("a")).unwrap();
    assert!(db
        .set_account_bps_enabled(&["a".into(), "missing".into()], true)
        .is_err());
    assert!(!db.find_account_bps_settings("a").unwrap().unwrap().enabled);
    db.conn
        .execute("UPDATE account_bps_settings SET expires_at = 0", [])
        .unwrap();
    assert!(db.set_account_bps_enabled(&["a".into()], true).is_err());
    db.delete_account("a").unwrap();
    assert!(db.find_account_bps_settings("a").unwrap().is_none());
}
