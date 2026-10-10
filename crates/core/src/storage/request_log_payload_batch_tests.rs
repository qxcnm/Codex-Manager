use super::super::{
    is_sqlite_busy_error, now_ts, RequestLogPayload, RequestLogPayloadManifestInput,
    RequestLogPayloadPart, RequestLogUpstreamAttempt, Storage, PAYLOAD_STAGE_CLIENT,
    PAYLOAD_STAGE_UPSTREAM,
};

fn storage() -> Storage {
    let storage = Storage::open_in_memory().expect("open in-memory storage");
    storage.init().expect("run storage migrations");
    storage
}

fn part(content: &str) -> RequestLogPayloadPart {
    RequestLogPayloadPart {
        hash: format!("h:{content}"),
        content: content.to_string(),
    }
}

fn manifest(
    trace_id: &str,
    stage: &str,
    created_at: i64,
    items: &[&str],
) -> RequestLogPayloadManifestInput {
    RequestLogPayloadManifestInput {
        trace_id: trace_id.to_string(),
        stage: stage.to_string(),
        body_hash: format!("body:{trace_id}:{}", items.join(",")),
        body_kind: "json_list".to_string(),
        list_field: Some("input".to_string()),
        conversation_key: Some("gk:conv-batch".to_string()),
        previous_response_id: None,
        payload_bytes: 10,
        redacted: false,
        created_at,
        fields: vec![("model".to_string(), part("\"gpt-6-astra\""))],
        items: items.iter().map(|item| part(item)).collect(),
    }
}

fn preview(trace_id: &str, created_at: i64) -> RequestLogPayload {
    RequestLogPayload {
        trace_id: trace_id.to_string(),
        stage: PAYLOAD_STAGE_CLIENT.to_string(),
        payload: "{}".to_string(),
        payload_bytes: 2,
        payload_truncated: false,
        redacted: true,
        body_hash: format!("hash:{trace_id}"),
        created_at,
    }
}

fn attempt(trace_id: &str, created_at: i64) -> RequestLogUpstreamAttempt {
    RequestLogUpstreamAttempt {
        trace_id: trace_id.to_string(),
        stage: PAYLOAD_STAGE_UPSTREAM.to_string(),
        method: "POST".to_string(),
        url: "http://127.0.0.1/v1/responses".to_string(),
        transport: "http".to_string(),
        content_encoding: None,
        wire_sha256: "wire".to_string(),
        identical_to_client: false,
        created_at,
    }
}

#[test]
fn request_log_payload_batch_commits_many_jobs_and_shares_prefix_within_batch() {
    let storage = storage();
    let created = now_ts();
    let generation = storage.request_log_payload_generation().unwrap();
    let writes = storage
        .write_request_log_payload_batch(|batch| {
            assert_eq!(batch.current_generation().unwrap(), generation);
            let first = batch
                .entry(|batch| {
                    batch.insert_request_log_payload_manifest_if_current(
                        &manifest("trc_b1", PAYLOAD_STAGE_CLIENT, created, &["u1"]),
                        None,
                        generation,
                    )
                })
                .unwrap();
            let second = batch
                .entry(|batch| {
                    batch.insert_request_log_payload_manifest_if_current(
                        &manifest("trc_b2", PAYLOAD_STAGE_CLIENT, created, &["u1", "a1", "u2"]),
                        None,
                        generation,
                    )
                })
                .unwrap();
            let preview_written = batch
                .entry(|batch| {
                    batch.insert_request_log_payload_if_current(
                        &preview("trc_p", created),
                        generation,
                    )
                })
                .unwrap();
            let attempt_written = batch
                .entry(|batch| {
                    batch.record_request_log_upstream_attempt_if_current(
                        &attempt("trc_b2", created),
                        generation,
                    )
                })
                .unwrap();
            (first, second, preview_written, attempt_written)
        })
        .unwrap();
    assert!(writes.0.inserted);
    assert!(writes.1.inserted);
    assert_eq!(writes.1.parent_trace_id.as_deref(), Some("trc_b1"));
    assert_eq!(writes.1.shared_prefix_len, 1);
    assert!(writes.2 && writes.3);
    let full = storage
        .load_request_log_payload_full("trc_b2", PAYLOAD_STAGE_CLIENT)
        .unwrap()
        .unwrap();
    assert!(full.complete);
    assert_eq!(full.items, vec!["u1", "a1", "u2"]);
    assert!(storage
        .find_request_log_payload_by_trace_id("trc_p", PAYLOAD_STAGE_CLIENT)
        .unwrap()
        .is_some());
    assert_eq!(
        storage
            .list_request_log_upstream_attempt_stages("trc_b2")
            .unwrap(),
        vec![PAYLOAD_STAGE_UPSTREAM.to_string()]
    );
}

#[test]
fn request_log_payload_batch_failed_entry_rolls_back_only_itself() {
    let storage = storage();
    let created = now_ts();
    let generation = storage.request_log_payload_generation().unwrap();
    storage
        .write_request_log_payload_batch(|batch| {
            let failed: rusqlite::Result<()> = batch.entry(|batch| {
                batch.insert_request_log_payload_if_current(
                    &preview("trc_fail", created),
                    generation,
                )?;
                Err(rusqlite::Error::SqliteFailure(
                    (),
                    Some("simulated".to_string()),
                ))
            });
            assert!(failed.is_err());
            batch
                .entry(|batch| {
                    batch.insert_request_log_payload_if_current(
                        &preview("trc_keep", created),
                        generation,
                    )
                })
                .unwrap();
        })
        .unwrap();
    assert!(storage
        .find_request_log_payload_by_trace_id("trc_fail", PAYLOAD_STAGE_CLIENT)
        .unwrap()
        .is_none());
    assert!(storage
        .find_request_log_payload_by_trace_id("trc_keep", PAYLOAD_STAGE_CLIENT)
        .unwrap()
        .is_some());
}

#[test]
fn request_log_payload_batch_rejects_jobs_from_before_a_clear() {
    let storage = storage();
    let created = now_ts();
    let stale = storage.request_log_payload_generation().unwrap();
    storage.clear_request_logs().unwrap();
    let current = storage.request_log_payload_generation().unwrap();
    assert!(current > stale);
    let outcome = storage
        .write_request_log_payload_batch(|batch| {
            assert_eq!(batch.current_generation().unwrap(), current);
            (
                batch
                    .entry(|batch| {
                        batch.insert_request_log_payload_if_current(
                            &preview("trc_old", created),
                            stale,
                        )
                    })
                    .unwrap(),
                batch
                    .entry(|batch| {
                        batch.insert_request_log_payload_manifest_if_current(
                            &manifest("trc_old_m", PAYLOAD_STAGE_CLIENT, created, &["secret"]),
                            None,
                            stale,
                        )
                    })
                    .unwrap()
                    .inserted,
                batch
                    .entry(|batch| {
                        batch.record_request_log_upstream_attempt_if_current(
                            &attempt("trc_old", created),
                            stale,
                        )
                    })
                    .unwrap(),
            )
        })
        .unwrap();
    assert_eq!(outcome, (false, false, false));
    assert!(storage
        .find_request_log_payload_manifest("trc_old_m", PAYLOAD_STAGE_CLIENT)
        .unwrap()
        .is_none());
}

#[test]
fn request_log_payload_batch_sees_client_capture_written_earlier_in_same_batch() {
    let storage = storage();
    let created = now_ts();
    let generation = storage.request_log_payload_generation().unwrap();
    let client = manifest("trc_same", PAYLOAD_STAGE_CLIENT, created, &["u1"]);
    let mut upstream = client.clone();
    upstream.stage = PAYLOAD_STAGE_UPSTREAM.to_string();
    let write = storage
        .write_request_log_payload_batch(|batch| {
            batch
                .entry(|batch| {
                    batch.insert_request_log_payload_manifest_if_current(&client, None, generation)
                })
                .unwrap();
            let found = batch
                .find_request_log_payload_manifest("trc_same", PAYLOAD_STAGE_CLIENT)
                .unwrap();
            assert!(found.is_some());
            batch
                .entry(|batch| {
                    batch
                        .insert_request_log_payload_manifest_if_current(&upstream, None, generation)
                })
                .unwrap()
        })
        .unwrap();
    assert!(write.identical_to_client);
    assert!(!write.inserted);
}

#[test]
fn request_log_payload_batch_busy_errors_are_classified() {
    let busy = |message: &str| {
        is_sqlite_busy_error(&rusqlite::Error::SqliteFailure(
            (),
            Some(message.to_string()),
        ))
    };
    assert!(busy(
        "error returned from database: (code: 5) database is locked"
    ));
    assert!(busy(
        "error returned from database: (code: 517) database is locked"
    ));
    assert!(busy(
        "error returned from database: (code: 6) database table is locked"
    ));
    assert!(busy("database is locked"));
    assert!(!busy(
        "error returned from database: (code: 19) UNIQUE constraint failed"
    ));
    assert!(!busy(
        "error returned from database: (code: 13) database or disk is full"
    ));
    assert!(!busy("disk I/O error"));
    assert!(!is_sqlite_busy_error(&rusqlite::Error::QueryReturnedNoRows));
}

#[test]
fn request_log_payload_batch_job_is_current_matches_clear_and_retention() {
    let storage = storage();
    let created = now_ts();
    let generation = storage.request_log_payload_generation().unwrap();
    storage
        .write_request_log_payload_batch(|batch| {
            assert!(batch.job_is_current(generation, created).unwrap());
            assert!(!batch.job_is_current(generation + 1, created).unwrap());
        })
        .unwrap();
    storage.prune_request_logs_before(created + 1).unwrap();
    storage
        .write_request_log_payload_batch(|batch| {
            assert!(!batch.job_is_current(generation, created).unwrap());
            assert!(batch.job_is_current(generation, created + 5).unwrap());
        })
        .unwrap();
}

#[test]
fn request_log_payload_batch_reports_busy_while_another_connection_writes() {
    let dir =
        std::env::temp_dir().join(format!("cm-batch-busy-{}-{}", std::process::id(), now_ts()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("busy.db");
    let storage = Storage::open(&path).expect("open file storage");
    storage.init().expect("init");
    let holder = rusqlite::Connection::open(&path).expect("open holder");
    holder
        .execute_batch("BEGIN IMMEDIATE")
        .expect("hold write lock");
    let err = storage
        .write_request_log_payload_batch(|_| ())
        .expect_err("write lock is held by another connection");
    assert!(is_sqlite_busy_error(&err), "{err}");
    holder.execute_batch("COMMIT").unwrap();
    storage
        .write_request_log_payload_batch(|_| ())
        .expect("lock released");
    drop(holder);
    drop(storage);
    let _ = std::fs::remove_dir_all(&dir);
}
