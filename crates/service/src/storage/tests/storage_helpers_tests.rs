use super::{
    clear_storage_cache_for_tests, clear_storage_open_count_for_tests, initialize_storage,
    model_catalog_v2_migration_needed, open_storage_at_path, preflight_model_catalog_v2,
    storage_open_count_for_tests,
};
use rusqlite::Connection;
use std::ffi::OsString;
use std::path::Path;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use std::time::{SystemTime, UNIX_EPOCH};

struct EnvGuard {
    key: &'static str,
    original: Option<OsString>,
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

#[test]
fn incomplete_remote_configuration_cannot_fall_back_to_sqlite() {
    let _guard = crate::test_env_guard();
    let _backend = EnvGuard::set("CODEXMANAGER_STORAGE_BACKEND", "mysql");
    let _url = EnvGuard::set("CODEXMANAGER_DATABASE_URL", "");
    assert!(super::seaorm_enabled());
    assert!(initialize_storage()
        .unwrap_err()
        .contains("database URL is required"));
    let _sqlite = EnvGuard::set("CODEXMANAGER_STORAGE_BACKEND", "sqlite");
    assert!(super::seaorm_enabled());
    assert!(initialize_storage()
        .unwrap_err()
        .contains("database URL is required"));
    let _wrong_scheme = EnvGuard::set("CODEXMANAGER_DATABASE_URL", "postgres://invalid");
    assert!(initialize_storage()
        .unwrap_err()
        .contains("database URL scheme"));
    let _unknown = EnvGuard::set("CODEXMANAGER_STORAGE_BACKEND", "unknown");
    assert!(initialize_storage()
        .unwrap_err()
        .contains("unsupported database backend"));
}

/// 函数 `unique_db_path`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - prefix: 参数 prefix
///
/// # 返回
/// 返回函数执行结果
fn unique_db_path(prefix: &str) -> String {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    std::env::temp_dir()
        .join(format!("{prefix}-{nonce}.db"))
        .to_string_lossy()
        .to_string()
}

#[test]
fn pending_model_catalog_data_migration_requires_backup() {
    let db_path = unique_db_path("codexmanager-model-billing-backup");
    let conn = Connection::open(&db_path).expect("open database");
    conn.execute_batch(
        "CREATE TABLE schema_migrations (
           version TEXT PRIMARY KEY,
           applied_at INTEGER NOT NULL
         );
         INSERT INTO schema_migrations(version,applied_at)
         VALUES('112_model_catalog_v2',1);",
    )
    .expect("create migration fixture");

    assert!(
        model_catalog_v2_migration_needed(std::path::Path::new(&db_path))
            .expect("inspect pending hardening")
    );

    conn.execute(
        "INSERT INTO schema_migrations(version,applied_at) VALUES(?1,2)",
        ["113_model_billing_v2_hardening"],
    )
    .expect("mark hardening complete");

    assert!(
        model_catalog_v2_migration_needed(std::path::Path::new(&db_path))
            .expect("inspect pending GPT-5.6 pricing migration")
    );
    conn.execute(
        "INSERT INTO schema_migrations(version,applied_at) VALUES(?1,3)",
        ["114_model_catalog_gpt56_prices"],
    )
    .expect("mark GPT-5.6 pricing migration complete");

    assert!(
        model_catalog_v2_migration_needed(std::path::Path::new(&db_path))
            .expect("inspect pending revision 9 catalog migration")
    );
    conn.execute(
        "INSERT INTO schema_migrations(version,applied_at) VALUES(?1,4)",
        ["137_model_catalog_revision9"],
    )
    .expect("mark revision 9 catalog migration complete");
    assert!(model_catalog_v2_migration_needed(Path::new(&db_path))
        .expect("inspect pending GPT-6.1 Sol migration"));
    conn.execute(
        "INSERT INTO schema_migrations(version,applied_at) VALUES(?1,5)",
        ["138_model_catalog_gpt61_sol"],
    )
    .expect("mark GPT-6.1 Sol migration complete");
    drop(conn);

    assert!(
        !model_catalog_v2_migration_needed(std::path::Path::new(&db_path))
            .expect("inspect completed catalog migrations")
    );
    let _ = std::fs::remove_file(&db_path);
}

#[test]
fn model_catalog_preflight_allows_case_conflicting_legacy_slugs() {
    let db_path = unique_db_path("codexmanager-model-catalog-duplicate-slugs");
    let conn = Connection::open(&db_path).expect("open database");
    conn.execute_batch(
        "CREATE TABLE model_catalog_models (
           scope TEXT NOT NULL,
           slug TEXT NOT NULL
         );
         INSERT INTO model_catalog_models(scope,slug)
         VALUES
           ('default','Legacy-Model'),
           ('default','legacy-model'),
           ('account_proxy','LEGACY-MODEL');",
    )
    .expect("create duplicate legacy catalog fixture");
    drop(conn);

    preflight_model_catalog_v2(Path::new(&db_path))
        .expect("case-conflicting legacy slugs are migratable");

    let _ = std::fs::remove_file(&db_path);
}

#[test]
fn initialize_storage_migrates_case_conflicting_legacy_slugs() {
    let _env_lock = crate::test_env_guard();
    let db_path = unique_db_path("codexmanager-model-catalog-duplicate-migration");
    let conn = Connection::open(&db_path).expect("open database");
    conn.execute_batch(
        "CREATE TABLE model_catalog_models (
           scope TEXT NOT NULL,
           slug TEXT NOT NULL,
           display_name TEXT NOT NULL,
           source_kind TEXT NOT NULL DEFAULT 'remote',
           user_edited INTEGER NOT NULL DEFAULT 0,
           description TEXT,
           default_reasoning_level TEXT,
           visibility TEXT,
           supported_in_api INTEGER,
           context_window INTEGER,
           extra_json TEXT NOT NULL DEFAULT '{}',
           sort_index INTEGER NOT NULL DEFAULT 0,
           updated_at INTEGER NOT NULL,
           PRIMARY KEY (scope, slug)
         );
         INSERT INTO model_catalog_models(
           scope,slug,display_name,source_kind,user_edited,description,
           visibility,supported_in_api,extra_json,sort_index,updated_at
         ) VALUES
           ('default','Legacy-Model','First legacy row','custom',1,'first','list',1,'{}',0,100),
           ('default','legacy-model','Second legacy row','custom',1,'second','list',1,'{}',1,200);",
    )
    .expect("create duplicate legacy catalog fixture");
    drop(conn);

    clear_storage_cache_for_tests();
    let _db_env = EnvGuard::set("CODEXMANAGER_DB_PATH", &db_path);
    initialize_storage().expect("migrate duplicate legacy slugs");

    let storage = open_storage_at_path(&db_path).expect("open migrated storage");
    let migrated = storage
        .list_managed_models_v2(true)
        .expect("list migrated models");
    let legacy_rows = migrated
        .iter()
        .filter(|model| model.slug.eq_ignore_ascii_case("legacy-model"))
        .collect::<Vec<_>>();
    assert_eq!(legacy_rows.len(), 1);
    assert_eq!(legacy_rows[0].slug, "Legacy-Model");
    assert_eq!(legacy_rows[0].display_name, "First legacy row");
    drop(storage);

    clear_storage_cache_for_tests();
    let _ = std::fs::remove_file(&db_path);
    if let Some(parent) = Path::new(&db_path).parent() {
        let backup_prefix = format!("{db_path}.model-catalog-v2.");
        if let Ok(entries) = std::fs::read_dir(parent) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.to_string_lossy().starts_with(&backup_prefix) {
                    let _ = std::fs::remove_file(path);
                }
            }
        }
    }
}

/// 函数 `open_storage_reuses_cached_connection_in_same_thread`
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
fn open_storage_reuses_cached_connection_in_same_thread() {
    let _env_lock = crate::test_env_guard();
    let db_path = unique_db_path("codexmanager-open-storage-reuse");
    clear_storage_cache_for_tests();
    clear_storage_open_count_for_tests(&db_path);

    let storage = open_storage_at_path(&db_path).expect("open storage 1");
    storage.init().expect("init");
    drop(storage);

    let storage = open_storage_at_path(&db_path).expect("open storage 2");
    drop(storage);

    assert_eq!(storage_open_count_for_tests(&db_path), 1);

    clear_storage_cache_for_tests();
    clear_storage_open_count_for_tests(&db_path);
    let _ = std::fs::remove_file(&db_path);
}

/// 函数 `open_storage_reopens_when_db_path_changes`
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
fn open_storage_reopens_when_db_path_changes() {
    let _env_lock = crate::test_env_guard();
    let db_path_1 = unique_db_path("codexmanager-open-storage-path-1");
    let db_path_2 = unique_db_path("codexmanager-open-storage-path-2");
    clear_storage_cache_for_tests();
    clear_storage_open_count_for_tests(&db_path_1);
    clear_storage_open_count_for_tests(&db_path_2);

    let storage = open_storage_at_path(&db_path_1).expect("open storage path 1");
    storage.init().expect("init 1");
    drop(storage);

    let storage = open_storage_at_path(&db_path_2).expect("open storage path 2");
    storage.init().expect("init 2");
    drop(storage);

    assert_eq!(storage_open_count_for_tests(&db_path_1), 1);
    assert_eq!(storage_open_count_for_tests(&db_path_2), 1);

    clear_storage_cache_for_tests();
    clear_storage_open_count_for_tests(&db_path_1);
    clear_storage_open_count_for_tests(&db_path_2);
    let _ = std::fs::remove_file(&db_path_1);
    let _ = std::fs::remove_file(&db_path_2);
}

/// 函数 `open_storage_waits_for_bounded_pool_slot`
///
/// 作者: gaohongshun
///
/// 时间: 2026-05-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn open_storage_waits_for_bounded_pool_slot() {
    let _env_lock = crate::test_env_guard();
    let _max_guard = EnvGuard::set("CODEXMANAGER_STORAGE_MAX_CONNECTIONS", "2");
    let _idle_guard = EnvGuard::set("CODEXMANAGER_STORAGE_MAX_IDLE_CONNECTIONS", "2");
    let _timeout_guard = EnvGuard::set("CODEXMANAGER_STORAGE_ACQUIRE_TIMEOUT_MS", "3000");
    let db_path = unique_db_path("codexmanager-open-storage-bounded");
    clear_storage_cache_for_tests();
    clear_storage_open_count_for_tests(&db_path);

    let release_pair = Arc::new((Mutex::new(false), Condvar::new()));
    let mut holders = Vec::new();
    for _ in 0..2 {
        let db_path = db_path.clone();
        let release_pair = Arc::clone(&release_pair);
        holders.push(thread::spawn(move || {
            let storage = open_storage_at_path(&db_path).expect("open held storage");
            let (lock, condvar) = &*release_pair;
            let mut released = lock.lock().expect("release lock");
            while !*released {
                released = condvar.wait(released).expect("release wait");
            }
            drop(storage);
        }));
    }

    let wait_started = Instant::now();
    while storage_open_count_for_tests(&db_path) < 2
        && wait_started.elapsed() < Duration::from_secs(3)
    {
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(storage_open_count_for_tests(&db_path), 2);

    let mut waiters = Vec::new();
    for _ in 0..4 {
        let db_path = db_path.clone();
        waiters.push(thread::spawn(move || {
            let storage = open_storage_at_path(&db_path).expect("open waited storage");
            drop(storage);
        }));
    }

    thread::sleep(Duration::from_millis(50));
    assert_eq!(storage_open_count_for_tests(&db_path), 2);

    {
        let (lock, condvar) = &*release_pair;
        let mut released = lock.lock().expect("release lock");
        *released = true;
        condvar.notify_all();
    }

    for holder in holders {
        holder.join().expect("holder join");
    }
    for waiter in waiters {
        waiter.join().expect("waiter join");
    }

    assert_eq!(storage_open_count_for_tests(&db_path), 2);

    clear_storage_cache_for_tests();
    clear_storage_open_count_for_tests(&db_path);
    let _ = std::fs::remove_file(&db_path);
}

/// 函数 `open_storage_times_out_when_pool_is_exhausted`
///
/// 作者: gaohongshun
///
/// 时间: 2026-05-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn open_storage_times_out_when_pool_is_exhausted() {
    let _env_lock = crate::test_env_guard();
    let _max_guard = EnvGuard::set("CODEXMANAGER_STORAGE_MAX_CONNECTIONS", "1");
    let _idle_guard = EnvGuard::set("CODEXMANAGER_STORAGE_MAX_IDLE_CONNECTIONS", "1");
    let _timeout_guard = EnvGuard::set("CODEXMANAGER_STORAGE_ACQUIRE_TIMEOUT_MS", "50");
    let db_path = unique_db_path("codexmanager-open-storage-timeout");
    clear_storage_cache_for_tests();
    clear_storage_open_count_for_tests(&db_path);

    let storage = open_storage_at_path(&db_path).expect("open held storage");
    let waited = open_storage_at_path(&db_path);
    assert!(waited.is_none());
    assert_eq!(storage_open_count_for_tests(&db_path), 1);
    drop(storage);

    clear_storage_cache_for_tests();
    clear_storage_open_count_for_tests(&db_path);
    let _ = std::fs::remove_file(&db_path);
}

#[test]
fn exhausted_pool_yields_single_tokio_worker_to_the_task_returning_its_lease() {
    let _env_lock = crate::test_env_guard();
    let _max_guard = EnvGuard::set("CODEXMANAGER_STORAGE_MAX_CONNECTIONS", "1");
    let _idle_guard = EnvGuard::set("CODEXMANAGER_STORAGE_MAX_IDLE_CONNECTIONS", "1");
    let _timeout_guard = EnvGuard::set("CODEXMANAGER_STORAGE_ACQUIRE_TIMEOUT_MS", "1500");
    let db_path = unique_db_path("codexmanager-storage-async-pool-release");
    clear_storage_cache_for_tests();
    clear_storage_open_count_for_tests(&db_path);
    let held = open_storage_at_path(&db_path).expect("reserve the only storage lease");
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let (acquiring, started) = tokio::sync::oneshot::channel();
        let releaser = tokio::spawn(async move {
            started.await.unwrap();
            tokio::time::sleep(Duration::from_millis(30)).await;
            drop(held);
        });
        let waiter_path = db_path.clone();
        let waiter = tokio::spawn(async move {
            acquiring.send(()).unwrap();
            // This runs on the sole async worker. A Condvar wait without
            // block_in_place would prevent the releaser above from running.
            open_storage_at_path(&waiter_path)
        });
        let acquired = tokio::time::timeout(Duration::from_secs(3), waiter)
            .await
            .expect("the lease holder must keep making progress")
            .unwrap()
            .expect("the returned lease must satisfy the pending acquisition");
        releaser.await.unwrap();
        drop(acquired);
    });
    drop(runtime);
    assert_eq!(storage_open_count_for_tests(&db_path), 1);
    clear_storage_cache_for_tests();
    clear_storage_open_count_for_tests(&db_path);
    let _ = std::fs::remove_file(&db_path);
}

#[test]
fn exhausted_pool_fails_fast_on_current_thread_and_allows_async_lease_release() {
    let _env_lock = crate::test_env_guard();
    let _max_guard = EnvGuard::set("CODEXMANAGER_STORAGE_MAX_CONNECTIONS", "1");
    let _idle_guard = EnvGuard::set("CODEXMANAGER_STORAGE_MAX_IDLE_CONNECTIONS", "1");
    let _timeout_guard = EnvGuard::set("CODEXMANAGER_STORAGE_ACQUIRE_TIMEOUT_MS", "2000");
    let db_path = unique_db_path("codexmanager-storage-current-thread-pool");
    clear_storage_cache_for_tests();
    clear_storage_open_count_for_tests(&db_path);
    let held = open_storage_at_path(&db_path).expect("reserve the only storage lease");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let (release, released) = tokio::sync::oneshot::channel();
        let releaser = tokio::spawn(async move {
            released.await.unwrap();
            drop(held);
        });
        release.send(()).unwrap();
        let started = Instant::now();
        assert!(open_storage_at_path(&db_path).is_none());
        assert!(
            started.elapsed() < Duration::from_millis(250),
            "a current-thread executor must not enter the configured two-second pool wait"
        );
        tokio::time::timeout(Duration::from_millis(250), releaser)
            .await
            .expect("the lease holder must run as soon as acquisition returns")
            .unwrap();
        let acquired = open_storage_at_path(&db_path)
            .expect("a retry must reuse the lease returned by the async task");
        drop(acquired);
    });
    drop(runtime);
    assert_eq!(storage_open_count_for_tests(&db_path), 1);
    clear_storage_cache_for_tests();
    clear_storage_open_count_for_tests(&db_path);
    let _ = std::fs::remove_file(&db_path);
}
