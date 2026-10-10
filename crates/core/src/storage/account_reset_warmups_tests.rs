use super::*;
use crate::storage::{Account, Token};

const RESET_AT: i64 = 2_000_000;

fn account(id: &str) -> Account {
    Account {
        id: id.to_string(),
        label: id.to_string(),
        issuer: "https://auth.openai.com".to_string(),
        chatgpt_account_id: None,
        workspace_id: None,
        group_name: None,
        sort: 0,
        status: "active".to_string(),
        created_at: RESET_AT - 100,
        updated_at: RESET_AT - 100,
    }
}

fn add_account(storage: &Storage, id: &str) {
    storage.insert_account(&account(id)).unwrap();
    storage
        .insert_token(&Token {
            account_id: id.to_string(),
            id_token: "test-id-token".to_string(),
            access_token: "test-access-token".to_string(),
            refresh_token: "test-refresh-token".to_string(),
            api_key_access_token: None,
            last_refresh: RESET_AT - 100,
        })
        .unwrap();
}

fn snapshot(id: &str) -> UsageSnapshotRecord {
    UsageSnapshotRecord {
        account_id: id.to_string(),
        used_percent: Some(100.0),
        window_minutes: Some(300),
        resets_at: Some(RESET_AT),
        secondary_used_percent: Some(10.0),
        secondary_window_minutes: Some(10080),
        secondary_resets_at: Some(RESET_AT + 86_400),
        credits_json: None,
        captured_at: RESET_AT - 100,
    }
}

fn storage_with_account(id: &str) -> Storage {
    let storage = Storage::open_in_memory().unwrap();
    storage.init().unwrap();
    add_account(&storage, id);
    storage
}

#[test]
fn settings_default_on_bulk_updates_are_atomic_and_import_preserves_them() {
    let storage = storage_with_account("a");
    add_account(&storage, "b");
    let ids = vec!["a".to_string(), "b".to_string()];
    assert_eq!(
        storage
            .list_account_reset_warmup_settings_for_accounts(&ids)
            .unwrap(),
        vec![("a".to_string(), true), ("b".to_string(), true)]
    );
    assert!(storage
        .set_account_reset_warmup_enabled(&[], false)
        .is_err());
    assert!(storage
        .set_account_reset_warmup_enabled(&["a".into(), " ".into()], false)
        .is_err());
    assert!(storage
        .set_account_reset_warmup_enabled(&["a".into(), "missing".into()], false)
        .is_err());
    assert_eq!(
        storage
            .list_account_reset_warmup_settings_for_accounts(&ids)
            .unwrap()[0]
            .1,
        true
    );
    assert_eq!(
        storage
            .set_account_reset_warmup_enabled(&["a".into(), "a".into(), "b".into()], false)
            .unwrap(),
        2
    );
    add_account(&storage, "a");
    assert_eq!(
        storage
            .list_account_reset_warmup_settings_for_accounts(&ids)
            .unwrap(),
        vec![("a".to_string(), false), ("b".to_string(), false)]
    );
    assert_eq!(
        storage
            .set_account_reset_warmup_enabled(&ids, true)
            .unwrap(),
        2
    );
}

#[test]
fn exhausted_main_five_hour_window_is_due_only_after_reset_and_grace() {
    let storage = storage_with_account("primary");
    for id in [
        "secondary",
        "not-exhausted",
        "weekly-only",
        "unknown-window",
        "unknown-reset",
        "optional-only",
    ] {
        add_account(&storage, id);
    }
    storage.insert_usage_snapshot(&snapshot("primary")).unwrap();
    let mut secondary = snapshot("secondary");
    std::mem::swap(
        &mut secondary.used_percent,
        &mut secondary.secondary_used_percent,
    );
    std::mem::swap(
        &mut secondary.window_minutes,
        &mut secondary.secondary_window_minutes,
    );
    std::mem::swap(&mut secondary.resets_at, &mut secondary.secondary_resets_at);
    storage.insert_usage_snapshot(&secondary).unwrap();
    for (id, used, minutes, reset) in [
        ("not-exhausted", Some(99.9), Some(300), Some(RESET_AT)),
        ("weekly-only", Some(100.0), Some(10080), Some(RESET_AT)),
        ("unknown-window", Some(100.0), None, Some(RESET_AT)),
        ("unknown-reset", Some(100.0), Some(300), None),
        ("optional-only", Some(10.0), Some(300), Some(RESET_AT)),
    ] {
        storage.insert_usage_snapshot(&UsageSnapshotRecord {
            used_percent: used, window_minutes: minutes, resets_at: reset,
            credits_json: Some(r#"{"_codexmanager_extra_rate_limits":[{"primary":{"used_percent":100,"window_minutes":300,"resets_at":2000000}}]}"#.into()),
            ..snapshot(id)
        }).unwrap();
    }
    assert!(storage
        .list_account_reset_warmup_targets(RESET_AT + 4, 100)
        .unwrap()
        .is_empty());
    assert_eq!(
        storage
            .list_account_reset_warmup_targets(RESET_AT + 5, 100)
            .unwrap(),
        vec![
            AccountResetWarmupTarget {
                account_id: "primary".into(),
                reset_at: RESET_AT,
                due_at: RESET_AT + 5
            },
            AccountResetWarmupTarget {
                account_id: "secondary".into(),
                reset_at: RESET_AT,
                due_at: RESET_AT + 5
            },
        ]
    );
    assert_eq!(
        storage
            .list_account_reset_warmup_targets(RESET_AT + 5, 1)
            .unwrap()
            .len(),
        1
    );
    assert!(storage
        .list_account_reset_warmup_targets(RESET_AT + 5, 0)
        .unwrap()
        .is_empty());
    assert!(!storage
        .claim_account_reset_warmup("primary", RESET_AT, RESET_AT + 4)
        .unwrap());
}

#[test]
fn pending_survives_recovered_snapshot_pruning_and_only_one_claim_per_cycle() {
    let storage = storage_with_account("a");
    storage
        .insert_usage_snapshot_and_prune(&snapshot("a"), 1)
        .unwrap();
    let recovered = UsageSnapshotRecord {
        used_percent: Some(0.0),
        captured_at: RESET_AT + 1,
        ..snapshot("a")
    };
    storage
        .insert_usage_snapshot_and_prune(&recovered, 1)
        .unwrap();
    assert_eq!(storage.usage_snapshot_count_for_account("a").unwrap(), 1);
    assert_eq!(
        storage
            .list_account_reset_warmup_targets(RESET_AT + 5, 10)
            .unwrap()
            .len(),
        1
    );
    assert!(!storage
        .claim_account_reset_warmup("a", RESET_AT - 1, RESET_AT + 5)
        .unwrap());
    assert!(storage
        .claim_account_reset_warmup("a", RESET_AT, RESET_AT + 5)
        .unwrap());
    assert!(!storage
        .claim_account_reset_warmup("a", RESET_AT, RESET_AT + 6)
        .unwrap());
    storage
        .insert_usage_snapshot(&UsageSnapshotRecord {
            captured_at: RESET_AT + 10,
            ..snapshot("a")
        })
        .unwrap();
    assert!(storage
        .list_account_reset_warmup_targets(RESET_AT + 20, 10)
        .unwrap()
        .is_empty());
    let next_reset = RESET_AT + 18_000;
    storage
        .insert_usage_snapshot(&UsageSnapshotRecord {
            resets_at: Some(next_reset),
            captured_at: RESET_AT + 11,
            ..snapshot("a")
        })
        .unwrap();
    assert!(storage
        .claim_account_reset_warmup("a", next_reset, next_reset + 5)
        .unwrap());
}

#[test]
fn a_real_request_starting_the_next_window_consumes_the_old_reminder() {
    let storage = storage_with_account("a");
    storage.insert_usage_snapshot(&snapshot("a")).unwrap();
    storage
        .insert_usage_snapshot_and_prune(
            &UsageSnapshotRecord {
                used_percent: Some(1.0),
                resets_at: Some(RESET_AT + 18_000),
                captured_at: RESET_AT + 1,
                ..snapshot("a")
            },
            1,
        )
        .unwrap();
    assert!(!storage
        .claim_account_reset_warmup("a", RESET_AT, RESET_AT + 5)
        .unwrap());
    storage
        .insert_usage_snapshot(&UsageSnapshotRecord {
            captured_at: RESET_AT + 10,
            ..snapshot("a")
        })
        .unwrap();
    assert!(storage
        .list_account_reset_warmup_targets(RESET_AT + 20, 10)
        .unwrap()
        .is_empty());
}

#[test]
fn a_fresh_zero_usage_reset_does_not_prove_a_real_request_started() {
    let storage = storage_with_account("a");
    storage.insert_usage_snapshot(&snapshot("a")).unwrap();
    storage
        .insert_usage_snapshot_and_prune(
            &UsageSnapshotRecord {
                used_percent: Some(0.0),
                resets_at: Some(RESET_AT + 18_000),
                captured_at: RESET_AT + 1,
                ..snapshot("a")
            },
            1,
        )
        .unwrap();
    assert!(storage
        .claim_account_reset_warmup("a", RESET_AT, RESET_AT + 5)
        .unwrap());
}

#[test]
fn weekly_quota_delays_both_listing_and_claim_and_unknown_reset_blocks() {
    let storage = storage_with_account("a");
    storage.insert_usage_snapshot(&snapshot("a")).unwrap();
    assert_eq!(
        storage
            .list_account_reset_warmup_targets(RESET_AT + 5, 10)
            .unwrap()
            .len(),
        1
    );
    let weekly_reset = RESET_AT + 1000;
    let exhausted_weekly = UsageSnapshotRecord {
        secondary_used_percent: Some(100.0),
        secondary_resets_at: Some(weekly_reset),
        captured_at: RESET_AT + 1,
        ..snapshot("a")
    };
    storage.insert_usage_snapshot(&exhausted_weekly).unwrap();
    assert!(storage
        .list_account_reset_warmup_targets(weekly_reset + 4, 10)
        .unwrap()
        .is_empty());
    assert!(!storage
        .claim_account_reset_warmup("a", RESET_AT, RESET_AT + 5)
        .unwrap());
    assert_eq!(
        storage
            .list_account_reset_warmup_targets(weekly_reset + 5, 10)
            .unwrap()[0]
            .due_at,
        weekly_reset + 5
    );
    storage
        .insert_usage_snapshot(&UsageSnapshotRecord {
            secondary_resets_at: None,
            captured_at: RESET_AT + 2,
            ..exhausted_weekly.clone()
        })
        .unwrap();
    assert!(!storage
        .claim_account_reset_warmup("a", RESET_AT, weekly_reset + 5)
        .unwrap());
    storage
        .insert_usage_snapshot(&UsageSnapshotRecord {
            secondary_resets_at: Some(0),
            captured_at: RESET_AT + 2,
            ..exhausted_weekly.clone()
        })
        .unwrap();
    assert!(!storage
        .claim_account_reset_warmup("a", RESET_AT, weekly_reset + 5)
        .unwrap());
    let mut swapped = UsageSnapshotRecord {
        captured_at: RESET_AT + 3,
        ..exhausted_weekly
    };
    std::mem::swap(
        &mut swapped.used_percent,
        &mut swapped.secondary_used_percent,
    );
    std::mem::swap(
        &mut swapped.window_minutes,
        &mut swapped.secondary_window_minutes,
    );
    std::mem::swap(&mut swapped.resets_at, &mut swapped.secondary_resets_at);
    storage.insert_usage_snapshot(&swapped).unwrap();
    assert!(!storage
        .claim_account_reset_warmup("a", RESET_AT, weekly_reset + 4)
        .unwrap());
    assert!(storage
        .claim_account_reset_warmup("a", RESET_AT, weekly_reset + 5)
        .unwrap());
}

#[test]
fn claims_recheck_toggle_account_status_and_token() {
    let storage = storage_with_account("a");
    storage.insert_usage_snapshot(&snapshot("a")).unwrap();
    assert_eq!(
        storage
            .list_account_reset_warmup_targets(RESET_AT + 5, 10)
            .unwrap()
            .len(),
        1
    );
    storage
        .set_account_reset_warmup_enabled(&["a".into()], false)
        .unwrap();
    assert!(!storage
        .claim_account_reset_warmup("a", RESET_AT, RESET_AT + 5)
        .unwrap());
    storage
        .set_account_reset_warmup_enabled(&["a".into()], true)
        .unwrap();
    for status in ["inactive", " DISABLED ", "unavailable", "banned"] {
        storage
            .insert_account(&Account {
                status: status.into(),
                ..account("a")
            })
            .unwrap();
        assert!(!storage
            .claim_account_reset_warmup("a", RESET_AT, RESET_AT + 5)
            .unwrap());
        assert!(storage
            .list_account_reset_warmup_targets(RESET_AT + 5, 10)
            .unwrap()
            .is_empty());
    }
    storage.insert_account(&account("a")).unwrap();
    storage
        .conn
        .execute("DELETE FROM tokens WHERE account_id = 'a'", [])
        .unwrap();
    assert!(!storage
        .claim_account_reset_warmup("a", RESET_AT, RESET_AT + 5)
        .unwrap());
    add_account(&storage, "a");
    storage
        .conn
        .execute(
            "UPDATE tokens SET access_token = '  ' WHERE account_id = 'a'",
            [],
        )
        .unwrap();
    assert!(!storage
        .claim_account_reset_warmup("a", RESET_AT, RESET_AT + 5)
        .unwrap());
    add_account(&storage, "a");
    assert!(storage
        .claim_account_reset_warmup("a", RESET_AT, RESET_AT + 5)
        .unwrap());
}

#[test]
fn migration_bootstraps_only_latest_exhausted_snapshots_and_is_idempotent() {
    let storage = storage_with_account("exhausted");
    add_account(&storage, "recovered");
    add_account(&storage, "secondary");
    storage
        .insert_usage_snapshot(&snapshot("exhausted"))
        .unwrap();
    storage
        .insert_usage_snapshot(&snapshot("recovered"))
        .unwrap();
    storage
        .insert_usage_snapshot(&UsageSnapshotRecord {
            used_percent: Some(0.0),
            captured_at: RESET_AT + 1,
            ..snapshot("recovered")
        })
        .unwrap();
    storage
        .insert_usage_snapshot(&UsageSnapshotRecord {
            used_percent: Some(10.0),
            window_minutes: Some(10080),
            resets_at: Some(RESET_AT + 1000),
            secondary_used_percent: Some(100.0),
            secondary_window_minutes: Some(300),
            secondary_resets_at: Some(RESET_AT),
            ..snapshot("secondary")
        })
        .unwrap();
    storage
        .conn
        .execute_batch("DROP TABLE account_reset_warmups;")
        .unwrap();
    let migration = include_str!("../../migrations/134_account_reset_warmups.sql");
    storage.conn.execute_batch(migration).unwrap();
    let ids = storage
        .list_account_reset_warmup_targets(RESET_AT + 5, 10)
        .unwrap()
        .into_iter()
        .map(|target| target.account_id)
        .collect::<Vec<_>>();
    assert_eq!(ids, vec!["exhausted", "secondary"]);
    storage
        .set_account_reset_warmup_enabled(&["exhausted".into()], false)
        .unwrap();
    storage.conn.execute_batch(migration).unwrap();
    assert_eq!(
        storage
            .list_account_reset_warmup_targets(RESET_AT + 5, 10)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn deleting_an_account_cascades_settings_and_pending_cycles() {
    let mut storage = storage_with_account("a");
    storage.insert_usage_snapshot(&snapshot("a")).unwrap();
    storage.delete_account("a").unwrap();
    let remaining: i64 = storage
        .conn
        .query_row("SELECT COUNT(*) FROM account_reset_warmups", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(remaining, 0);
    add_account(&storage, "a");
    assert_eq!(
        storage
            .list_account_reset_warmup_settings_for_accounts(&["a".into()])
            .unwrap(),
        vec![("a".into(), true)]
    );
}

#[test]
fn snapshot_and_pending_cycle_are_written_atomically() {
    let storage = storage_with_account("a");
    storage
        .conn
        .execute(
            "CREATE TRIGGER fail_reset_warmup BEFORE INSERT ON account_reset_warmups
         BEGIN SELECT RAISE(ABORT, 'test write failure'); END;",
            [],
        )
        .unwrap();
    assert!(storage.insert_usage_snapshot(&snapshot("a")).is_err());
    assert_eq!(storage.usage_snapshot_count_for_account("a").unwrap(), 0);
    assert!(storage
        .insert_usage_snapshot_and_prune(&snapshot("a"), 1)
        .is_err());
    assert_eq!(storage.usage_snapshot_count_for_account("a").unwrap(), 0);
}

#[test]
fn concurrent_claims_and_restarts_cannot_repeat_a_cycle() {
    let path = std::env::temp_dir().join(format!(
        "codexmanager-reset-warmups-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let storage = Storage::open(&path).unwrap();
    storage.init().unwrap();
    add_account(&storage, "a");
    storage.insert_usage_snapshot(&snapshot("a")).unwrap();
    drop(storage);
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let handles = (0..2)
        .map(|_| {
            let path = path.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let storage = Storage::open(path).unwrap();
                barrier.wait();
                storage
                    .claim_account_reset_warmup("a", RESET_AT, RESET_AT + 5)
                    .unwrap()
            })
        })
        .collect::<Vec<_>>();
    let claims = handles
        .into_iter()
        .map(|handle| usize::from(handle.join().unwrap()))
        .sum::<usize>();
    assert_eq!(claims, 1);
    let storage = Storage::open(&path).unwrap();
    storage.init().unwrap();
    assert!(!storage
        .claim_account_reset_warmup("a", RESET_AT, RESET_AT + 5)
        .unwrap());
    drop(storage);
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(path.with_extension("db-wal"));
    let _ = std::fs::remove_file(path.with_extension("db-shm"));
}
