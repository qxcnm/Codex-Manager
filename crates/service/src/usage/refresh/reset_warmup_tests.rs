use super::*;
use codexmanager_core::storage::{Account, Token, UsageSnapshotRecord};

const RESET_AT: i64 = 2_000_000_000;
const DUE_AT: i64 = RESET_AT + 5;

fn storage() -> Storage {
    let storage = Storage::open_in_memory().expect("open storage");
    storage.init().expect("initialize storage");
    storage
}

fn seed_account(storage: &Storage, account_id: &str, reset_at: i64) {
    storage
        .insert_account(&Account {
            id: account_id.to_string(),
            label: account_id.to_string(),
            issuer: "https://auth.openai.com".to_string(),
            chatgpt_account_id: None,
            workspace_id: None,
            group_name: None,
            sort: 0,
            status: "active".to_string(),
            created_at: reset_at - 100,
            updated_at: reset_at - 100,
        })
        .expect("insert account");
    storage
        .insert_token(&Token {
            account_id: account_id.to_string(),
            id_token: String::new(),
            access_token: "test-access-token".to_string(),
            refresh_token: "test-refresh-token".to_string(),
            api_key_access_token: None,
            last_refresh: reset_at - 100,
        })
        .expect("insert token");
    storage
        .insert_usage_snapshot(&exhausted_snapshot(account_id, reset_at))
        .expect("record exhausted cycle");
}

fn exhausted_snapshot(account_id: &str, reset_at: i64) -> UsageSnapshotRecord {
    UsageSnapshotRecord {
        account_id: account_id.to_string(),
        used_percent: Some(100.0),
        window_minutes: Some(300),
        resets_at: Some(reset_at),
        secondary_used_percent: Some(25.0),
        secondary_window_minutes: Some(10080),
        secondary_resets_at: Some(reset_at + 86400),
        credits_json: None,
        captured_at: reset_at - 100,
    }
}

fn sent(account_id: &str) -> AccountWarmupItemResult {
    AccountWarmupItemResult {
        account_id: account_id.to_string(),
        account_name: account_id.to_string(),
        ok: true,
        message: "sent".to_string(),
    }
}

#[test]
fn reset_warmup_sends_only_due_account_once() {
    let _guard = crate::test_env_guard();
    let storage = storage();
    seed_account(&storage, "due", RESET_AT);
    seed_account(&storage, "future", RESET_AT + 300);
    assert!(storage
        .list_account_reset_warmup_targets(RESET_AT - 1, 128)
        .unwrap()
        .is_empty());
    let targets = storage
        .list_account_reset_warmup_targets(DUE_AT, 128)
        .unwrap();
    assert_eq!(targets.len(), 1);
    let target = &targets[0];
    assert_eq!(target.account_id, "due");
    let mut sent_ids = Vec::new();
    for _ in 0..2 {
        run_reset_warmup_task(&storage, target, DUE_AT, |_, id| {
            sent_ids.push(id.to_string());
            Ok(sent(id))
        })
        .unwrap();
    }
    assert_eq!(sent_ids, ["due"]);
}

#[test]
fn reset_warmup_rechecks_disabled_switch_after_queueing() {
    let _guard = crate::test_env_guard();
    let storage = storage();
    seed_account(&storage, "disabled-after-queue", RESET_AT);
    let target = storage
        .list_account_reset_warmup_targets(DUE_AT, 128)
        .unwrap()
        .remove(0);
    storage
        .set_account_reset_warmup_enabled(&[target.account_id.clone()], false)
        .unwrap();
    let result = run_reset_warmup_task(&storage, &target, DUE_AT, |_, _| {
        panic!("disabled account must not send")
    })
    .unwrap();
    assert!(result.is_none());
}

#[test]
fn reset_warmup_rechecks_newly_exhausted_weekly_quota() {
    let _guard = crate::test_env_guard();
    let storage = storage();
    seed_account(&storage, "weekly", RESET_AT);
    let target = storage
        .list_account_reset_warmup_targets(DUE_AT, 128)
        .unwrap()
        .remove(0);
    let mut snapshot = exhausted_snapshot("weekly", RESET_AT);
    snapshot.secondary_used_percent = Some(100.0);
    snapshot.captured_at = DUE_AT;
    storage.insert_usage_snapshot(&snapshot).unwrap();
    assert!(run_reset_warmup_task(&storage, &target, DUE_AT, |_, _| {
        panic!("exhausted weekly quota must block send")
    })
    .unwrap()
    .is_none());
}

#[test]
fn reset_warmup_skips_cycle_already_started_by_real_traffic() {
    let _guard = crate::test_env_guard();
    let storage = storage();
    seed_account(&storage, "already-started", RESET_AT);
    let target = storage
        .list_account_reset_warmup_targets(DUE_AT, 128)
        .unwrap()
        .remove(0);
    let mut snapshot = exhausted_snapshot("already-started", RESET_AT);
    snapshot.used_percent = Some(1.0);
    snapshot.resets_at = Some(RESET_AT + 18000);
    snapshot.captured_at = DUE_AT;
    storage.insert_usage_snapshot(&snapshot).unwrap();
    assert!(run_reset_warmup_task(&storage, &target, DUE_AT, |_, _| {
        panic!("new window already running must not send")
    })
    .unwrap()
    .is_none());
}

#[test]
fn reset_warmup_failed_send_is_not_repeated_for_same_cycle() {
    let _guard = crate::test_env_guard();
    let storage = storage();
    seed_account(&storage, "failed", RESET_AT);
    let target = storage
        .list_account_reset_warmup_targets(DUE_AT, 128)
        .unwrap()
        .remove(0);
    let result = run_reset_warmup_task(&storage, &target, DUE_AT, |_, _| {
        Err("upstream timeout".to_string())
    });
    assert_eq!(result.unwrap_err(), "upstream timeout");
    assert!(
        run_reset_warmup_task(&storage, &target, DUE_AT + 5, |_, _| {
            panic!("uncertain send must not be repeated")
        })
        .unwrap()
        .is_none()
    );
}

#[test]
fn reset_warmup_queue_is_bounded_and_deduplicated_before_claiming() {
    let (sender, mut receiver) = bounded(1);
    let executor = ResetWarmupExecutor {
        sender,
        pending: Arc::new(Mutex::new(HashSet::new())),
    };
    let target = AccountResetWarmupTarget {
        account_id: "first".to_string(),
        reset_at: RESET_AT,
        due_at: DUE_AT,
    };
    assert!(executor.enqueue(target.clone()));
    assert!(!executor.enqueue(target));
    let second = AccountResetWarmupTarget {
        account_id: "second".to_string(),
        reset_at: RESET_AT,
        due_at: DUE_AT,
    };
    assert!(!executor.enqueue(second.clone()));
    receiver.try_recv().unwrap();
    assert!(executor.enqueue(second));
}
