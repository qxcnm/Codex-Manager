use super::*;
use std::sync::atomic::AtomicU64;

static DB_COUNTER: AtomicU64 = AtomicU64::new(0);

struct TempDb(PathBuf);

impl TempDb {
    fn new(tag: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        Self(std::env::temp_dir().join(format!(
            "cm-space-{tag}-{}-{nanos}-{}.db",
            std::process::id(),
            DB_COUNTER.fetch_add(1, Ordering::Relaxed)
        )))
    }
}

impl Drop for TempDb {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
        let _ = std::fs::remove_file(wal_path(&self.0));
        let mut shm = self.0.as_os_str().to_owned();
        shm.push("-shm");
        let _ = std::fs::remove_file(PathBuf::from(shm));
    }
}

fn fill_and_free(storage: &Storage) {
    storage
        .conn
        .execute_batch("CREATE TABLE IF NOT EXISTS filler (body BLOB)")
        .unwrap();
    for _ in 0..64 {
        storage
            .conn
            .execute("INSERT INTO filler (body) VALUES (zeroblob(32768))", [])
            .unwrap();
    }
    storage.conn.execute("DELETE FROM filler", []).unwrap();
}

#[test]
fn new_database_uses_incremental_auto_vacuum() {
    let db = TempDb::new("new");
    let storage = Storage::open(&db.0).unwrap();
    storage.init().unwrap();
    assert_eq!(
        storage.database_space_usage().unwrap().auto_vacuum,
        AUTO_VACUUM_INCREMENTAL
    );
    // Re-running init on an existing database keeps the mode.
    storage.init().unwrap();
    assert_eq!(
        storage.database_space_usage().unwrap().auto_vacuum,
        AUTO_VACUUM_INCREMENTAL
    );
}

#[test]
fn existing_database_is_never_converted_automatically() {
    let db = TempDb::new("legacy");
    let storage = Storage::open(&db.0).unwrap();
    storage
        .conn
        .execute_batch("CREATE TABLE legacy_marker (id INTEGER)")
        .unwrap();
    storage.init().unwrap();
    assert_eq!(
        storage.database_space_usage().unwrap().auto_vacuum,
        AUTO_VACUUM_NONE
    );
}

#[test]
fn incremental_vacuum_step_returns_free_pages() {
    let db = TempDb::new("incremental");
    let storage = Storage::open(&db.0).unwrap();
    storage.init().unwrap();
    fill_and_free(&storage);
    let before = storage.database_space_usage().unwrap();
    assert!(before.freelist_count > 0, "{before:?}");
    assert_eq!(
        before.reclaimable_bytes,
        before.freelist_count * before.page_size
    );
    let freed = storage.incremental_vacuum_step(16).unwrap();
    assert!(freed > 0 && freed <= 16, "freed={freed}");
    let after = storage.database_space_usage().unwrap();
    assert_eq!(after.freelist_count, before.freelist_count - freed);
    assert_eq!(
        after.used_bytes,
        (after.page_count - after.freelist_count) * after.page_size
    );
    assert!(after.file_bytes.is_some());
}

#[test]
fn incremental_vacuum_step_is_a_no_op_for_legacy_databases() {
    let db = TempDb::new("legacy-step");
    let storage = Storage::open(&db.0).unwrap();
    storage
        .conn
        .execute_batch("CREATE TABLE legacy_marker (id INTEGER)")
        .unwrap();
    storage.init().unwrap();
    fill_and_free(&storage);
    assert_eq!(storage.incremental_vacuum_step(64).unwrap(), 0);
}

#[test]
fn explicit_conversion_enables_incremental_reclaim() {
    let db = TempDb::new("convert");
    let storage = Storage::open(&db.0).unwrap();
    storage
        .conn
        .execute_batch("CREATE TABLE legacy_marker (id INTEGER)")
        .unwrap();
    storage.init().unwrap();
    fill_and_free(&storage);
    assert!(storage.convert_to_incremental_auto_vacuum().unwrap());
    let usage = storage.database_space_usage().unwrap();
    assert_eq!(usage.auto_vacuum, AUTO_VACUUM_INCREMENTAL);
    // The rebuild already dropped the free pages.
    assert_eq!(usage.freelist_count, 0);
    // temp_store is restored to MEMORY for regular queries.
    assert_eq!(storage.pragma_i64("temp_store").unwrap(), 2);
    assert!(!storage.convert_to_incremental_auto_vacuum().unwrap());
    fill_and_free(&storage);
    assert!(storage.incremental_vacuum_step(8).unwrap() > 0);
}

#[test]
fn secure_delete_is_restored_even_when_the_operation_fails() {
    let storage = Storage::open_in_memory().unwrap();
    storage.init().unwrap();
    let before = storage.secure_delete_mode().unwrap();
    let result: Result<()> = storage.with_secure_delete(|| {
        assert_eq!(storage.secure_delete_mode().unwrap(), 1);
        Err(rusqlite::Error::SqliteFailure((), Some("boom".to_string())))
    });
    assert!(result.is_err());
    assert_eq!(storage.secure_delete_mode().unwrap(), before);
}

#[test]
fn every_connection_limits_the_wal_size() {
    let storage = Storage::open_in_memory().unwrap();
    assert_eq!(
        storage.pragma_i64("journal_size_limit").unwrap(),
        67_108_864
    );
}
