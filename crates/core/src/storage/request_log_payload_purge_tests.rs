use super::super::{
    now_ts, RequestLog, RequestLogPayload, RequestLogPayloadManifestInput,
    RequestLogPayloadParentHint, RequestLogPayloadPart, RequestTokenStat, Storage,
    PAYLOAD_STAGE_CLIENT,
};
use super::DrainSlot;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static DB_COUNTER: AtomicU64 = AtomicU64::new(0);

/// File backed database removed on drop. `legacy` creates a table before
/// `init`, so the database keeps `auto_vacuum = NONE` like an existing
/// installation and freed pages stay inside the file.
struct TempDb {
    path: PathBuf,
}

impl TempDb {
    fn new(tag: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!(
            "cm-purge-{tag}-{}-{nanos}-{}.db",
            std::process::id(),
            DB_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        Self { path }
    }

    fn open(&self, legacy: bool) -> Storage {
        let storage = Storage::open(&self.path).expect("open file storage");
        if legacy {
            storage
                .conn
                .execute_batch("CREATE TABLE IF NOT EXISTS legacy_marker (id INTEGER)")
                .expect("create legacy table");
        }
        storage.init().expect("init storage");
        storage
    }

    fn bytes(&self, suffix: &str) -> Vec<u8> {
        let mut raw = self.path.as_os_str().to_owned();
        raw.push(suffix);
        std::fs::read(PathBuf::from(raw)).unwrap_or_default()
    }
}

impl Drop for TempDb {
    fn drop(&mut self) {
        for suffix in ["", "-wal", "-shm"] {
            let mut raw = self.path.as_os_str().to_owned();
            raw.push(suffix);
            let _ = std::fs::remove_file(PathBuf::from(raw));
        }
    }
}

fn memory_storage() -> Storage {
    let storage = Storage::open_in_memory().expect("open in-memory storage");
    storage.init().expect("init storage");
    storage
}

fn part(content: &str) -> RequestLogPayloadPart {
    RequestLogPayloadPart {
        hash: format!("h:{content}"),
        content: content.to_string(),
    }
}

fn manifest(trace_id: &str, created_at: i64, items: &[&str]) -> RequestLogPayloadManifestInput {
    RequestLogPayloadManifestInput {
        trace_id: trace_id.to_string(),
        stage: "upstream".to_string(),
        body_hash: String::new(),
        body_kind: "json_list".to_string(),
        list_field: Some("input".to_string()),
        conversation_key: Some("gk:purge".to_string()),
        previous_response_id: None,
        payload_bytes: 100,
        redacted: false,
        created_at,
        fields: vec![("model".to_string(), part("\"gpt-6-astra\""))],
        items: items.iter().map(|item| part(item)).collect(),
    }
}

fn count(storage: &Storage, table: &str) -> i64 {
    storage
        .conn
        .query_row(&format!("SELECT COUNT(1) FROM {table}"), [], |row| {
            row.get(0)
        })
        .expect("count rows")
}

fn seed_log(storage: &Storage, trace_id: &str) {
    storage
        .insert_request_log_with_token_stat(
            &RequestLog {
                trace_id: Some(trace_id.to_string()),
                key_id: Some("gk_purge".to_string()),
                request_path: "/v1/responses".to_string(),
                method: "POST".to_string(),
                created_at: now_ts(),
                ..Default::default()
            },
            &RequestTokenStat::default(),
        )
        .expect("insert request log");
}

fn begin_clear_without_drain(storage: &Storage, cleared_at: i64) {
    let tx = storage.conn.unchecked_transaction().expect("begin");
    storage
        .begin_request_log_payload_clear(cleared_at)
        .expect("mark clear");
    tx.commit().expect("commit clear");
}

#[test]
fn clear_keeps_free_pages_instead_of_vacuuming_and_restores_secure_delete() {
    let db = TempDb::new("no-vacuum");
    let storage = db.open(true);
    assert_eq!(
        storage.database_space_usage().unwrap().auto_vacuum,
        crate::storage::AUTO_VACUUM_NONE
    );
    let big = "x".repeat(64 * 1024);
    for index in 0..40 {
        let trace = format!("trc_big_{index}");
        seed_log(&storage, &trace);
        storage
            .insert_request_log_payload_manifest(
                &manifest(&trace, now_ts(), &[&format!("{big}{index}")]),
                None,
            )
            .expect("insert manifest");
    }
    let before = storage.secure_delete_mode().unwrap();
    storage.clear_request_logs().expect("clear");
    assert_eq!(storage.secure_delete_mode().unwrap(), before);
    assert_eq!(count(&storage, "request_log_payload_blobs"), 0);
    let usage = storage.database_space_usage().unwrap();
    // Without VACUUM the freed pages stay in the file as free pages.
    assert!(usage.freelist_count > 0, "{usage:?}");
    assert_eq!(
        usage.used_bytes,
        (usage.page_count - usage.freelist_count) * usage.page_size
    );
    assert!(!storage.request_log_payload_purge_pending().unwrap());
}

#[test]
fn cleared_unredacted_payloads_do_not_remain_in_database_or_wal_bytes() {
    let db = TempDb::new("sentinel");
    let storage = db.open(true);
    let sentinel = "sk-sentinel-7f3c2a9e41d84b60";
    seed_log(&storage, "trc_secret");
    storage
        .insert_request_log_payload(&RequestLogPayload {
            trace_id: "trc_secret".to_string(),
            stage: PAYLOAD_STAGE_CLIENT.to_string(),
            payload: format!("{{\"api_key\":\"{sentinel}\"}}"),
            payload_bytes: 64,
            payload_truncated: false,
            redacted: false,
            body_hash: "hash-secret".to_string(),
            created_at: now_ts(),
        })
        .expect("insert preview");
    storage
        .insert_request_log_payload_manifest(
            &manifest("trc_secret", now_ts(), &[&format!("\"{sentinel}\"")]),
            None,
        )
        .expect("insert manifest");
    storage.checkpoint_wal_truncate().expect("checkpoint seed");
    let contains = |haystack: &[u8]| {
        haystack
            .windows(sentinel.len())
            .any(|window| window == sentinel.as_bytes())
    };
    assert!(
        contains(&db.bytes("")),
        "sentinel must be on disk before clear"
    );

    storage.clear_request_logs().expect("clear");
    let outcome = storage.checkpoint_wal_truncate().expect("checkpoint");
    assert!(!outcome.busy);
    assert!(!contains(&db.bytes("")), "sentinel left in database file");
    assert!(!contains(&db.bytes("-wal")), "sentinel left in WAL file");
}

#[test]
fn batched_clear_purge_keeps_rows_written_after_the_clear() {
    let storage = memory_storage();
    let cleared_at = now_ts();
    storage
        .insert_request_log_payload_manifest(
            &manifest("trc_old", cleared_at - 10, &["shared", "old"]),
            None,
        )
        .expect("insert old");
    begin_clear_without_drain(&storage, cleared_at);

    // Written after the clear, before the purge ran: same conversation and a
    // reused blob. It must not chain onto the doomed manifest.
    let write = storage
        .insert_request_log_payload_manifest(
            &manifest("trc_new", cleared_at + 1, &["shared", "new"]),
            None,
        )
        .expect("insert new");
    assert!(write.inserted);
    assert_eq!(write.parent_trace_id, None);
    // Waiting for the purge already means invisible to readers.
    assert!(storage
        .find_request_log_payload_manifest("trc_old", "upstream")
        .unwrap()
        .is_none());
    assert!(storage
        .list_request_log_payload_manifest_stages("trc_old")
        .unwrap()
        .is_empty());
    assert_eq!(count(&storage, "request_log_payload_manifests"), 2);

    let progress = storage
        .drain_request_log_payload_purges(None)
        .expect("drain");
    assert!(!progress.pending);
    assert!(progress.deleted_rows > 0);
    assert!(storage
        .find_request_log_payload_manifest("trc_old", "upstream")
        .unwrap()
        .is_none());
    let full = storage
        .load_request_log_payload_full("trc_new", "upstream")
        .unwrap()
        .expect("new manifest survives");
    assert!(full.complete);
    assert_eq!(full.items, vec!["shared", "new"]);
    // model + shared + new survive, "old" is collected.
    assert_eq!(count(&storage, "request_log_payload_blobs"), 3);
    assert!(!storage.request_log_payload_purge_pending().unwrap());
}

#[test]
fn reused_rowid_after_clear_is_not_purged() {
    let storage = memory_storage();
    let cleared_at = now_ts();
    storage
        .insert_request_log_payload_manifest(&manifest("trc_old", cleared_at - 5, &["a"]), None)
        .expect("insert old");
    begin_clear_without_drain(&storage, cleared_at);
    // Another path removed the newest old row, so SQLite may hand out the
    // same rowid again. The later created_at keeps the new row safe.
    storage
        .conn
        .execute("DELETE FROM request_log_payload_manifests", [])
        .unwrap();
    storage
        .insert_request_log_payload_manifest(&manifest("trc_reuse", cleared_at + 5, &["b"]), None)
        .expect("insert reused rowid");
    storage
        .drain_request_log_payload_purges(None)
        .expect("drain");
    assert!(storage
        .find_request_log_payload_manifest("trc_reuse", "upstream")
        .unwrap()
        .is_some());
}

#[test]
fn purge_respects_time_budget_and_resumes() {
    let storage = memory_storage();
    let cleared_at = now_ts();
    for index in 0..5 {
        storage
            .insert_request_log_payload_manifest(
                &manifest(
                    &format!("trc_{index}"),
                    cleared_at - 1,
                    &[&format!("i{index}")],
                ),
                None,
            )
            .unwrap();
    }
    begin_clear_without_drain(&storage, cleared_at);
    let progress = storage
        .drain_request_log_payload_purges(Some(std::time::Duration::ZERO))
        .unwrap();
    assert!(progress.pending);
    assert!(storage.request_log_payload_purge_pending().unwrap());
    assert_eq!(count(&storage, "request_log_payload_manifests"), 5);

    let progress = storage.drain_request_log_payload_purges(None).unwrap();
    assert!(!progress.pending);
    assert_eq!(count(&storage, "request_log_payload_manifests"), 0);
    assert_eq!(count(&storage, "request_log_payload_blobs"), 0);
    assert!(!storage.request_log_payload_purge_pending().unwrap());
}

#[test]
fn retention_prune_rebases_survivors_and_never_reuses_expired_parents() {
    let storage = memory_storage();
    storage
        .insert_request_log_payload_manifest(&manifest("trc_old", 100, &["u1"]), None)
        .unwrap();
    storage
        .insert_request_log_payload_manifest(&manifest("trc_mid", 150, &["u1", "a1"]), None)
        .unwrap();
    storage
        .insert_request_log_payload_manifest(&manifest("trc_new", 300, &["u1", "a1", "u2"]), None)
        .unwrap();

    storage.prune_request_logs_before(200).expect("prune");
    assert_eq!(count(&storage, "request_log_payload_manifests"), 1);
    let survivor = storage
        .find_request_log_payload_manifest("trc_new", "upstream")
        .unwrap()
        .expect("survivor");
    assert_eq!(survivor.parent_trace_id, None);
    let full = storage
        .load_request_log_payload_full("trc_new", "upstream")
        .unwrap()
        .unwrap();
    assert!(full.complete);
    assert_eq!(full.items, vec!["u1", "a1", "u2"]);

    // A late row below the cutoff (written through the unguarded API) must
    // never become the parent of new data, even when the writer's cache
    // still hints at it.
    let late = storage
        .insert_request_log_payload_manifest(
            &manifest("trc_late", 180, &["u1", "a1", "u2", "x"]),
            None,
        )
        .unwrap();
    let hint = RequestLogPayloadParentHint {
        trace_id: "trc_late".to_string(),
        stage: "upstream".to_string(),
        item_blob_ids: late.item_blob_ids.clone(),
    };
    let write = storage
        .insert_request_log_payload_manifest(
            &manifest("trc_next", 400, &["u1", "a1", "u2", "x", "y"]),
            Some(&hint),
        )
        .unwrap();
    assert_eq!(write.parent_trace_id.as_deref(), Some("trc_new"));
    assert_eq!(write.shared_prefix_len, 3);
}

#[test]
fn clear_rejects_stale_jobs_and_finishes_purge_synchronously() {
    let storage = memory_storage();
    let generation = storage.request_log_payload_generation().unwrap();
    storage
        .insert_request_log_payload_manifest(&manifest("trc_before", now_ts(), &["p"]), None)
        .unwrap();
    storage.clear_request_logs().unwrap();
    assert_eq!(count(&storage, "request_log_payload_manifests"), 0);
    assert!(
        !storage
            .insert_request_log_payload_manifest_if_current(
                &manifest("trc_stale", now_ts(), &["s"]),
                None,
                generation
            )
            .unwrap()
            .inserted
    );
    assert_eq!(count(&storage, "request_log_payload_purges"), 0);
}

#[test]
fn a_second_drain_on_the_same_file_returns_immediately() {
    let db = TempDb::new("single-drain");
    let storage = db.open(true);
    let cleared_at = now_ts();
    storage
        .insert_request_log_payload_manifest(&manifest("trc_busy", cleared_at - 1, &["b"]), None)
        .unwrap();
    begin_clear_without_drain(&storage, cleared_at);
    let slot = DrainSlot::acquire(storage.conn.path()).expect("first drain slot");
    let progress = storage.drain_request_log_payload_purges(None).unwrap();
    assert!(progress.pending);
    assert_eq!(progress.deleted_rows, 0);
    assert_eq!(count(&storage, "request_log_payload_manifests"), 1);
    drop(slot);
    let progress = storage.drain_request_log_payload_purges(None).unwrap();
    assert!(!progress.pending);
    assert_eq!(count(&storage, "request_log_payload_manifests"), 0);
}

#[test]
fn rebuild_is_refused_while_a_purge_is_pending() {
    let db = TempDb::new("rebuild-guard");
    let storage = db.open(true);
    storage
        .insert_request_log_payload_manifest(&manifest("trc_r", now_ts() - 1, &["r"]), None)
        .unwrap();
    begin_clear_without_drain(&storage, now_ts());
    assert!(storage.convert_to_incremental_auto_vacuum().is_err());
    storage.drain_request_log_payload_purges(None).unwrap();
    assert!(storage.convert_to_incremental_auto_vacuum().unwrap());
}

#[test]
fn preview_of_a_request_spanning_the_retention_cutoff_is_hidden_before_the_sweep() {
    let storage = memory_storage();
    let preview = |trace: &str, created_at: i64| RequestLogPayload {
        trace_id: trace.to_string(),
        stage: PAYLOAD_STAGE_CLIENT.to_string(),
        payload: "{}".to_string(),
        payload_bytes: 2,
        payload_truncated: false,
        redacted: true,
        body_hash: format!("hash-{trace}"),
        created_at,
    };
    storage
        .insert_request_log_payload(&preview("trc_span", 150))
        .unwrap();
    storage
        .insert_request_log_payload(&preview("trc_keep", 250))
        .unwrap();
    storage
        .conn
        .execute(
            "UPDATE request_log_payload_state SET retention_cutoff = 200 WHERE id = 1",
            [],
        )
        .unwrap();
    assert!(storage
        .find_request_log_payload_by_trace_id("trc_span", PAYLOAD_STAGE_CLIENT)
        .unwrap()
        .is_none());
    assert!(storage
        .find_request_log_payload_body_hash("trc_span", PAYLOAD_STAGE_CLIENT)
        .unwrap()
        .is_none());
    assert!(storage
        .find_request_log_payload_by_trace_id("trc_keep", PAYLOAD_STAGE_CLIENT)
        .unwrap()
        .is_some());
    storage.drain_request_log_payload_purges(None).unwrap();
    assert_eq!(count(&storage, "request_log_payloads"), 1);
}
