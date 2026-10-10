use super::remote_storage::AccountStorage;
use codexmanager_core::storage::{Account, Event, Storage, Token, UsageSnapshotRecord};

struct RestoreEnv(Vec<(&'static str, Option<std::ffi::OsString>)>);
impl Drop for RestoreEnv {
    fn drop(&mut self) {
        crate::storage::seaorm_runtime::clear_for_tests();
        for (key, value) in self.0.drain(..) {
            if let Some(value) = value {
                std::env::set_var(key, value);
            } else {
                std::env::remove_var(key);
            }
        }
    }
}

fn exercise(backend: &str, url_name: &str) {
    let _lock = crate::test_env_guard();
    let _restore = RestoreEnv(
        ["CODEXMANAGER_STORAGE_BACKEND", "CODEXMANAGER_DATABASE_URL"]
            .into_iter()
            .map(|key| (key, std::env::var_os(key)))
            .collect(),
    );
    crate::storage::seaorm_runtime::clear_for_tests();
    std::env::set_var("CODEXMANAGER_STORAGE_BACKEND", backend);
    std::env::set_var(
        "CODEXMANAGER_DATABASE_URL",
        std::env::var(url_name).unwrap(),
    );
    let sqlite = Storage::open_in_memory().unwrap();
    sqlite.init().unwrap();
    let remote = AccountStorage::new(&sqlite);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_micros();
    let id = format!("remote-facade-{stamp}");
    let account = Account {
        id: id.clone(),
        label: "fixture".into(),
        issuer: "https://provider.invalid".into(),
        chatgpt_account_id: None,
        workspace_id: None,
        group_name: None,
        sort: 0,
        status: "active".into(),
        created_at: 1,
        updated_at: 1,
    };
    remote.insert_account(&account).unwrap();
    remote
        .insert_token(&Token {
            account_id: id.clone(),
            id_token: "fixture".into(),
            access_token: "fixture".into(),
            refresh_token: "fixture".into(),
            api_key_access_token: None,
            last_refresh: 1,
        })
        .unwrap();
    assert!(sqlite.find_account_by_id(&id).unwrap().is_none());
    assert!(remote.find_account_by_id(&id).unwrap().is_some());
    let snapshot = UsageSnapshotRecord {
        account_id: id.clone(),
        used_percent: Some(10.),
        window_minutes: Some(300),
        resets_at: None,
        secondary_used_percent: None,
        secondary_window_minutes: None,
        secondary_resets_at: None,
        credits_json: None,
        captured_at: 10,
    };
    let rejected = remote.insert_usage_snapshot_and_prune_with_previous(&snapshot, 0, |_, _| {
        assert!(remote.find_account_with_token_by_id(&id)?.is_some());
        remote.update_account_status_if_changed_with_existence(&id, "limited")?;
        remote.insert_event(&Event {
            account_id: Some(id.clone()),
            event_type: "account_status_update".into(),
            message: "status=limited reason=rollback".into(),
            created_at: 2,
        })?;
        Err(rusqlite::Error::SqliteFailure(
            (),
            Some("fixture rollback".into()),
        ))
    });
    assert!(rejected.is_err());
    assert_eq!(
        remote.find_account_by_id(&id).unwrap().unwrap().status,
        "active"
    );
    assert_eq!(remote.usage_snapshot_count_for_account(&id).unwrap(), 0);
    assert!(remote
        .latest_account_status_reasons(&[id.clone()])
        .unwrap()
        .is_empty());

    // More callers than the bounded database operation semaphore must still
    // complete: resolver queries and COMMIT use the held transaction directly.
    let (completed, received) = std::sync::mpsc::channel();
    let mut workers = Vec::new();
    for n in 0..12 {
        let id = id.clone();
        let snapshot = UsageSnapshotRecord {
            captured_at: 10 + n,
            ..snapshot.clone()
        };
        let completed = completed.clone();
        workers.push(std::thread::spawn(move || {
            let empty_compatibility_handle = Storage::open_in_memory().unwrap();
            let remote = AccountStorage::new(&empty_compatibility_handle);
            let result =
                remote.insert_usage_snapshot_and_prune_with_previous(&snapshot, 0, |_, credits| {
                    let account = remote.find_account_by_id(&id)?.unwrap();
                    assert_eq!(account.status, "active");
                    let _ = remote.latest_account_status_reasons(&[id.clone()])?;
                    *credits = Some(format!("{{\"sequence\":{n}}}"));
                    Ok(())
                });
            completed
                .send(result.map(|_| ()).map_err(|e| e.to_string()))
                .unwrap();
        }));
    }
    for _ in 0..12 {
        received
            .recv_timeout(std::time::Duration::from_secs(30))
            .expect("transaction pool deadlock")
            .unwrap();
    }
    for worker in workers {
        worker.join().unwrap();
    }
    assert_eq!(remote.usage_snapshot_count_for_account(&id).unwrap(), 12);
    assert_eq!(sqlite.usage_snapshot_count_for_account(&id).unwrap(), 0);
    remote.delete_account(&id).unwrap();
}

#[test]
#[ignore = "requires isolated MySQL URL"]
fn mysql_remote_account_facade_is_authoritative_and_transactional() {
    exercise("mysql", "CODEXMANAGER_TEST_MYSQL_URL");
}

#[test]
#[ignore = "requires isolated PostgreSQL URL"]
fn postgres_remote_account_facade_is_authoritative_and_transactional() {
    exercise("postgres", "CODEXMANAGER_TEST_POSTGRES_URL");
}
