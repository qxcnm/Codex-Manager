use super::{update_account, update_account_sorts, AccountSortUpdate};
use codexmanager_core::storage::{Account, Storage};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

static ACCOUNT_UPDATE_TEST_DIR_SEQ: AtomicUsize = AtomicUsize::new(0);

fn new_test_dir(prefix: &str) -> PathBuf {
    let seq = ACCOUNT_UPDATE_TEST_DIR_SEQ.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let mut dir = std::env::temp_dir();
    dir.push(format!("{prefix}-{}-{seq}-{nanos}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    dir
}

struct EnvGuard {
    key: &'static str,
    original: Option<std::ffi::OsString>,
}

impl EnvGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let original = std::env::var_os(key);
        std::env::set_var(key, value);
        Self { key, original }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        if let Some(value) = &self.original {
            std::env::set_var(self.key, value);
        } else {
            std::env::remove_var(self.key);
        }
    }
}

fn account(id: &str, sort: i64) -> Account {
    Account {
        id: id.to_string(),
        label: id.to_string(),
        issuer: "chatgpt".to_string(),
        chatgpt_account_id: None,
        workspace_id: None,
        group_name: None,
        sort,
        status: "active".to_string(),
        created_at: 1,
        updated_at: 1,
    }
}

fn set_test_db(prefix: &str) -> (PathBuf, EnvGuard) {
    let dir = new_test_dir(prefix);
    let db_path = dir.join("codexmanager.db");
    let guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());
    (db_path, guard)
}

#[test]
fn update_account_preferred_uses_existence_check_without_loading_account() {
    let _lock = crate::test_env_guard();
    let (db_path, _guard) = set_test_db("account-update-preferred");

    let storage = Storage::open(&db_path).expect("open db");
    storage.init().expect("init db");
    storage
        .insert_account(&account("acc-preferred", 1))
        .expect("insert preferred");

    update_account(
        "acc-preferred",
        None,
        Some(true),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    )
    .expect("set preferred");

    assert_eq!(
        Storage::open(&db_path)
            .expect("reopen db")
            .preferred_account_id()
            .expect("read preferred")
            .as_deref(),
        Some("acc-preferred")
    );
    let err = update_account(
        "acc-missing",
        None,
        Some(true),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    )
    .expect_err("missing account should fail");
    assert_eq!(err, "account not found");
}

#[test]
fn update_account_sorts_updates_all_rows_and_records_events() {
    let _lock = crate::test_env_guard();
    let (db_path, _guard) = set_test_db("account-update-sorts");

    let storage = Storage::open(&db_path).expect("open db");
    storage.init().expect("init db");
    storage
        .insert_account(&account("acc-sort-a", 1))
        .expect("insert first account");
    storage
        .insert_account(&account("acc-sort-b", 2))
        .expect("insert second account");

    let result = update_account_sorts(vec![
        AccountSortUpdate {
            account_id: "acc-sort-a".to_string(),
            sort: 20,
        },
        AccountSortUpdate {
            account_id: "acc-sort-b".to_string(),
            sort: 10,
        },
    ])
    .expect("update account sorts");

    assert_eq!(result.updated, 2);
    let storage = Storage::open(&db_path).expect("reopen db");
    assert_eq!(
        storage
            .find_account_by_id("acc-sort-a")
            .expect("find first")
            .expect("first exists")
            .sort,
        20
    );
    assert_eq!(
        storage
            .find_account_by_id("acc-sort-b")
            .expect("find second")
            .expect("second exists")
            .sort,
        10
    );
    assert_eq!(storage.event_count().expect("event count"), 2);
}

#[test]
fn update_account_sorts_rejects_empty_and_duplicate_ids() {
    let _lock = crate::test_env_guard();
    let (_db_path, _guard) = set_test_db("account-update-sorts-invalid");

    let empty_err = update_account_sorts(Vec::new()).expect_err("empty updates fail");
    assert_eq!(empty_err, "missing account sort updates");

    let duplicate_err = update_account_sorts(vec![
        AccountSortUpdate {
            account_id: "acc-dup".to_string(),
            sort: 1,
        },
        AccountSortUpdate {
            account_id: " acc-dup ".to_string(),
            sort: 2,
        },
    ])
    .expect_err("duplicate updates fail");
    assert_eq!(duplicate_err, "duplicate accountId: acc-dup");
}

#[test]
fn update_account_sorts_rolls_back_when_any_account_is_missing() {
    let _lock = crate::test_env_guard();
    let (db_path, _guard) = set_test_db("account-update-sorts-rollback");

    let storage = Storage::open(&db_path).expect("open db");
    storage.init().expect("init db");
    storage
        .insert_account(&account("acc-sort-rollback", 1))
        .expect("insert account");

    let err = update_account_sorts(vec![
        AccountSortUpdate {
            account_id: "acc-sort-rollback".to_string(),
            sort: 99,
        },
        AccountSortUpdate {
            account_id: "acc-sort-missing".to_string(),
            sort: 100,
        },
    ])
    .expect_err("missing account fails batch");
    assert!(err.contains("account not found: acc-sort-missing"));

    let storage = Storage::open(&db_path).expect("reopen db");
    assert_eq!(
        storage
            .find_account_by_id("acc-sort-rollback")
            .expect("find account")
            .expect("account exists")
            .sort,
        1
    );
    assert_eq!(storage.event_count().expect("event count"), 0);
}
