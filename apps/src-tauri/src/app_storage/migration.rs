use rusqlite::{backup::Backup, Connection};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use codexmanager_core::storage::default_token_encryption_key_file_path;

const PRIMARY_APP_IDENTIFIER: &str = "com.codexmanager.desktop";
const QA_APP_IDENTIFIER: &str = "com.codexmanager.desktop.qa";

/// 函数 `maybe_migrate_legacy_db`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - super: 参数 super
///
/// # 返回
/// 无
pub(super) fn maybe_migrate_legacy_db(current_db: &Path) {
    let current_has_data = db_has_user_data(current_db);
    if current_has_data {
        return;
    }

    let needs_bootstrap = !current_db.is_file() || !current_has_data;
    if !needs_bootstrap {
        return;
    }

    for legacy_db in bootstrap_db_candidates(current_db) {
        if !legacy_db.is_file() {
            continue;
        }
        if !db_has_user_data(&legacy_db) {
            continue;
        }

        if let Some(parent) = current_db.parent() {
            let _ = fs::create_dir_all(parent);
        }

        if current_db.is_file() {
            let backup = current_db.with_extension("db.empty.bak");
            if let Err(err) = fs::copy(current_db, &backup) {
                log::warn!(
                    "Failed to backup empty current db {} -> {}: {}",
                    current_db.display(),
                    backup.display(),
                    err
                );
            }
        }

        match copy_db_snapshot(&legacy_db, current_db) {
            Ok(_) => {
                log::info!(
                    "Migrated legacy db {} -> {}",
                    legacy_db.display(),
                    current_db.display()
                );
                return;
            }
            Err(err) => {
                log::warn!(
                    "Failed to migrate legacy db {} -> {}: {}",
                    legacy_db.display(),
                    current_db.display(),
                    err
                );
            }
        }
    }
}

/// 函数 `copy_db_snapshot`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - source: 参数 source
/// - target: 参数 target
///
/// # 返回
/// 返回函数执行结果
fn copy_db_snapshot(source: &Path, target: &Path) -> Result<(), String> {
    copy_fallback_token_key(source, target)?;
    remove_db_sidecars(target);
    if target.is_file() {
        fs::remove_file(target).map_err(|err| {
            format!(
                "remove existing target db {} failed: {err}",
                target.display()
            )
        })?;
    }

    let source_conn = Connection::open(source)
        .map_err(|err| format!("open source db {} failed: {err}", source.display()))?;
    source_conn
        .busy_timeout(Duration::from_millis(3000))
        .map_err(|err| format!("configure source db {} failed: {err}", source.display()))?;

    let mut target_conn = Connection::open(target)
        .map_err(|err| format!("open target db {} failed: {err}", target.display()))?;
    target_conn
        .busy_timeout(Duration::from_millis(3000))
        .map_err(|err| format!("configure target db {} failed: {err}", target.display()))?;

    let backup = Backup::new(&source_conn, &mut target_conn).map_err(|err| {
        format!(
            "create sqlite backup {} -> {} failed: {err}",
            source.display(),
            target.display()
        )
    })?;
    backup
        .run_to_completion(64, Duration::from_millis(25), None)
        .map_err(|err| {
            format!(
                "run sqlite backup {} -> {} failed: {err}",
                source.display(),
                target.display()
            )
        })?;
    Ok(())
}

fn copy_fallback_token_key(source_db: &Path, target_db: &Path) -> Result<(), String> {
    let source_key = default_token_encryption_key_file_path(source_db);
    let target_key = default_token_encryption_key_file_path(target_db);
    if source_key == target_key || !source_key.is_file() {
        return Ok(());
    }

    if let Some(parent) = target_key.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            format!(
                "create token key target directory {} failed: {err}",
                parent.display()
            )
        })?;
    }
    fs::copy(&source_key, &target_key).map_err(|err| {
        format!(
            "copy account token key {} -> {} failed: {err}",
            source_key.display(),
            target_key.display()
        )
    })?;
    Ok(())
}

pub(crate) fn create_pre_migration_backup(
    current_db: &Path,
    app_version: &str,
) -> Result<Option<PathBuf>, String> {
    if !current_db.is_file() {
        return Ok(None);
    }

    let version = app_version
        .chars()
        .map(|value| {
            if value.is_ascii_alphanumeric() || matches!(value, '.' | '-' | '_') {
                value
            } else {
                '_'
            }
        })
        .collect::<String>();
    let file_name = current_db
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| format!("database file name is not valid UTF-8: {}", current_db.display()))?;
    let backup_path = current_db.with_file_name(format!("{file_name}.pre-{version}.bak"));
    if backup_path.is_file() {
        return Ok(Some(backup_path));
    }

    if let Err(err) = copy_db_snapshot(current_db, &backup_path) {
        let _ = fs::remove_file(&backup_path);
        return Err(err);
    }
    Ok(Some(backup_path))
}

/// 函数 `remove_db_sidecars`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - path: 参数 path
///
/// # 返回
/// 无
fn remove_db_sidecars(path: &Path) {
    for sidecar in db_sidecar_paths(path) {
        if sidecar.is_file() {
            let _ = fs::remove_file(sidecar);
        }
    }
}

/// 函数 `db_sidecar_paths`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - path: 参数 path
///
/// # 返回
/// 返回函数执行结果
fn db_sidecar_paths(path: &Path) -> [PathBuf; 2] {
    let base = path.as_os_str().to_string_lossy();
    [
        PathBuf::from(format!("{base}-wal")),
        PathBuf::from(format!("{base}-shm")),
    ]
}

/// 函数 `db_has_user_data`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - path: 参数 path
///
/// # 返回
/// 返回函数执行结果
fn db_has_user_data(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    let conn = match Connection::open(path) {
        Ok(conn) => conn,
        Err(_) => return false,
    };
    ["accounts", "tokens", "api_keys"].into_iter().any(|table| {
        let exists = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                [table],
                |row| row.get::<_, i64>(0),
            )
            .map(|count| count > 0)
            .unwrap_or(false);
        exists
            && conn
                .query_row(&format!("SELECT EXISTS(SELECT 1 FROM {table} LIMIT 1)"), [], |row| {
                    row.get::<_, i64>(0)
                })
                .map(|count| count > 0)
                .unwrap_or(false)
    })
}

/// 函数 `legacy_db_candidates`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - current_db: 参数 current_db
///
/// # 返回
/// 返回函数执行结果
fn legacy_db_candidates(current_db: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();

    if let Some(parent) = current_db.parent() {
        out.push(parent.join("gpttools.db"));
        if parent
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("com.codexmanager.desktop"))
        {
            if let Some(root) = parent.parent() {
                out.push(root.join("com.gpttools.desktop").join("gpttools.db"));
            }
        }
    }

    dedup_candidates(current_db, out)
}

/// 函数 `bootstrap_db_candidates`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - current_db: 参数 current_db
///
/// # 返回
/// 返回函数执行结果
fn bootstrap_db_candidates(current_db: &Path) -> Vec<PathBuf> {
    let mut out = profile_db_candidates(current_db);
    out.extend(legacy_db_candidates(current_db));
    dedup_candidates(current_db, out)
}

/// 函数 `profile_db_candidates`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - current_db: 参数 current_db
///
/// # 返回
/// 返回函数执行结果
fn profile_db_candidates(current_db: &Path) -> Vec<PathBuf> {
    let Some(parent) = current_db.parent() else {
        return Vec::new();
    };
    let Some(parent_name) = parent.file_name().and_then(|name| name.to_str()) else {
        return Vec::new();
    };
    if !parent_name.eq_ignore_ascii_case(QA_APP_IDENTIFIER) {
        return Vec::new();
    }

    let Some(root) = parent.parent() else {
        return Vec::new();
    };
    vec![root.join(PRIMARY_APP_IDENTIFIER).join("codexmanager.db")]
}

/// 函数 `dedup_candidates`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - current_db: 参数 current_db
/// - candidates: 参数 candidates
///
/// # 返回
/// 返回函数执行结果
fn dedup_candidates(current_db: &Path, candidates: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut dedup = Vec::new();
    for candidate in candidates {
        if candidate == current_db {
            continue;
        }
        if !dedup.iter().any(|item| item == &candidate) {
            dedup.push(candidate);
        }
    }
    dedup
}

#[cfg(test)]
mod tests {
    use super::{
        create_pre_migration_backup, maybe_migrate_legacy_db, profile_db_candidates,
        PRIMARY_APP_IDENTIFIER, QA_APP_IDENTIFIER,
    };
    use codexmanager_core::storage::{now_ts, Account, Storage, Token};
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    /// 函数 `unique_temp_dir`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// 无
    ///
    /// # 返回
    /// 返回函数执行结果
    fn unique_temp_dir() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!("codexmanager-qa-migration-test-{unique}"))
    }

    /// 函数 `create_populated_db`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - path: 参数 path
    ///
    /// # 返回
    /// 返回函数执行结果
    fn create_populated_db(path: &Path) -> Storage {
        let storage = Storage::open(path).expect("open storage");
        storage.init().expect("init storage");
        storage
            .insert_account(&Account {
                id: "acc-1".to_string(),
                label: "main".to_string(),
                issuer: "https://auth.openai.com".to_string(),
                chatgpt_account_id: Some("acct_123".to_string()),
                workspace_id: Some("org_123".to_string()),
                group_name: None,
                sort: 0,
                status: "healthy".to_string(),
                created_at: now_ts(),
                updated_at: now_ts(),
            })
            .expect("insert account");
        storage
            .insert_token(&Token {
                account_id: "acc-1".to_string(),
                id_token: "id-token".to_string(),
                access_token: "access-token".to_string(),
                refresh_token: "refresh-token".to_string(),
                api_key_access_token: Some("api-key-access-token".to_string()),
                last_refresh: now_ts(),
            })
            .expect("insert token");
        storage
    }

    /// 函数 `profile_db_candidates_only_seed_qa_profile_from_primary_profile`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// 无
    ///
    /// # 返回
    /// 无
    #[test]
    fn profile_db_candidates_only_seed_qa_profile_from_primary_profile() {
        let qa_db = PathBuf::from(format!(
            "C:/Users/test/AppData/Roaming/{QA_APP_IDENTIFIER}/codexmanager.db"
        ));
        assert_eq!(
            profile_db_candidates(&qa_db),
            vec![PathBuf::from(format!(
                "C:/Users/test/AppData/Roaming/{PRIMARY_APP_IDENTIFIER}/codexmanager.db"
            ))]
        );

        let primary_db = PathBuf::from(format!(
            "C:/Users/test/AppData/Roaming/{PRIMARY_APP_IDENTIFIER}/codexmanager.db"
        ));
        assert!(profile_db_candidates(&primary_db).is_empty());
    }

    /// 函数 `maybe_migrate_legacy_db_seeds_empty_qa_profile_from_primary_profile`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// 无
    ///
    /// # 返回
    /// 无
    #[test]
    fn maybe_migrate_legacy_db_seeds_empty_qa_profile_from_primary_profile() {
        let root = unique_temp_dir();
        let qa_dir = root.join(QA_APP_IDENTIFIER);
        let primary_dir = root.join(PRIMARY_APP_IDENTIFIER);
        std::fs::create_dir_all(&qa_dir).expect("create qa dir");
        std::fs::create_dir_all(&primary_dir).expect("create primary dir");

        let qa_db = qa_dir.join("codexmanager.db");
        let primary_db = primary_dir.join("codexmanager.db");

        let qa_storage = Storage::open(&qa_db).expect("open qa storage");
        qa_storage.init().expect("init qa storage");
        drop(qa_storage);

        let _source_storage = create_populated_db(&primary_db);

        maybe_migrate_legacy_db(&qa_db);

        let migrated = Storage::open(&qa_db).expect("open migrated qa storage");
        migrated.init().expect("init migrated qa storage");
        assert_eq!(migrated.account_count().expect("count accounts"), 1);
        assert_eq!(
            migrated
                .find_token_by_account_id("acc-1")
                .expect("find migrated token")
                .expect("migrated token")
                .access_token,
            "access-token"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn pre_migration_backup_is_versioned_and_never_overwritten() {
        let root = unique_temp_dir();
        std::fs::create_dir_all(&root).expect("create test dir");
        let db_path = root.join("codexmanager.db");
        let storage = create_populated_db(&db_path);
        drop(storage);

        let backup_path = create_pre_migration_backup(&db_path, "0.5.1")
            .expect("create backup")
            .expect("existing database should be backed up");
        assert_eq!(
            backup_path.file_name().and_then(|value| value.to_str()),
            Some("codexmanager.db.pre-0.5.1.bak")
        );

        let backup_modified = std::fs::metadata(&backup_path)
            .expect("backup metadata")
            .modified()
            .expect("backup modified time");
        std::fs::write(&db_path, b"replaced after backup").expect("replace source db");
        let second_path = create_pre_migration_backup(&db_path, "0.5.1")
            .expect("reuse backup")
            .expect("backup path");
        assert_eq!(second_path, backup_path);
        assert_eq!(
            std::fs::metadata(&backup_path)
                .expect("backup metadata")
                .modified()
                .expect("backup modified time"),
            backup_modified
        );

        let backup_storage = Storage::open(&backup_path).expect("open backup");
        assert_eq!(backup_storage.account_count().expect("count accounts"), 1);
        assert_eq!(
            backup_storage
                .find_token_by_account_id("acc-1")
                .expect("find backup token")
                .expect("backup token")
                .refresh_token,
            "refresh-token"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
