use super::delete_accounts;
use codexmanager_core::storage::{Account, Storage};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::test_env_guard;

static DELETE_MANY_TEST_DIR_SEQ: AtomicUsize = AtomicUsize::new(0);

fn new_test_dir(prefix: &str) -> PathBuf {
    let seq = DELETE_MANY_TEST_DIR_SEQ.fetch_add(1, Ordering::Relaxed);
    let mut dir = std::env::temp_dir();
    dir.push(format!("{prefix}-{}-{seq}", std::process::id()));
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

#[test]
fn delete_accounts_dedupes_ids_and_reports_missing_accounts() {
    let _lock = test_env_guard();
    let dir = new_test_dir("delete-many-accounts");
    let db_path = dir.join("codexmanager.db");
    let storage = Storage::open(&db_path).expect("open db");
    storage.init().expect("init db");
    storage
        .insert_account(&account("acc-delete", 1))
        .expect("insert delete target");
    storage
        .insert_account(&account("acc-keep", 2))
        .expect("insert keep target");

    // Publish only the fully initialized fixture: existing background workers
    // may open the process-wide database path as soon as it changes.
    let _guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());

    let result = delete_accounts(vec![
        " acc-delete ".to_string(),
        "".to_string(),
        "acc-delete".to_string(),
        " missing ".to_string(),
    ])
    .expect("delete result");

    assert_eq!(result.requested, 2);
    assert_eq!(result.deleted, 1);
    assert_eq!(result.failed, 1);
    assert_eq!(result.deleted_account_ids, vec!["acc-delete".to_string()]);
    assert_eq!(result.errors.len(), 1);
    assert_eq!(result.errors[0].account_id, "missing");
    assert_eq!(result.errors[0].message, "account not found");

    let storage = Storage::open(&db_path).expect("reopen db");
    assert!(!storage
        .account_exists("acc-delete")
        .expect("deleted account existence"));
    assert!(storage
        .account_exists("acc-keep")
        .expect("kept account existence"));
}

#[test]
fn delete_accounts_rejects_empty_input_after_normalization() {
    let err = delete_accounts(vec![" ".to_string(), "".to_string()])
        .expect_err("empty ids should be rejected");

    assert_eq!(err, "missing accountIds");
}
