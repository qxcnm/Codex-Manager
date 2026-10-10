use super::*;

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

fn manifest(trace_id: &str, created_at: i64, items: &[&str]) -> RequestLogPayloadManifestInput {
    RequestLogPayloadManifestInput {
        trace_id: trace_id.to_string(),
        stage: "upstream".to_string(),
        body_hash: String::new(),
        body_kind: "json_list".to_string(),
        list_field: Some("input".to_string()),
        conversation_key: Some("gk:conv-1".to_string()),
        previous_response_id: None,
        payload_bytes: 100,
        redacted: false,
        created_at,
        fields: vec![
            ("model".to_string(), part("\"gpt-6-astra\"")),
            ("tools".to_string(), part("[{\"type\":\"function\"}]")),
        ],
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

fn own_item_rows(storage: &Storage, trace_id: &str) -> i64 {
    storage
        .conn
        .query_row(
            "SELECT COUNT(1) FROM request_log_payload_manifest_items WHERE trace_id = ?1",
            [trace_id],
            |row| row.get(0),
        )
        .expect("count own rows")
}

#[test]
fn child_request_only_stores_new_tail_and_rebuilds_full_list() {
    let storage = storage();
    let first = storage
        .insert_request_log_payload_manifest(&manifest("trc_1", 100, &["u1"]), None)
        .expect("insert first");
    assert!(first.inserted);
    assert_eq!(first.parent_trace_id, None);

    let second = storage
        .insert_request_log_payload_manifest(&manifest("trc_2", 101, &["u1", "a1", "u2"]), None)
        .expect("insert second");
    assert_eq!(second.parent_trace_id.as_deref(), Some("trc_1"));
    assert_eq!(second.shared_prefix_len, 1);
    assert_eq!(own_item_rows(&storage, "trc_2"), 2);

    let third = storage
        .insert_request_log_payload_manifest(
            &manifest("trc_3", 102, &["u1", "a1", "u2", "a2", "u3"]),
            None,
        )
        .expect("insert third");
    assert_eq!(third.parent_trace_id.as_deref(), Some("trc_2"));
    assert_eq!(third.shared_prefix_len, 3);
    assert_eq!(own_item_rows(&storage, "trc_3"), 2);

    let full = storage
        .load_request_log_payload_full("trc_3", "upstream")
        .expect("load")
        .expect("manifest exists");
    assert!(full.complete);
    assert_eq!(full.items, vec!["u1", "a1", "u2", "a2", "u3"]);
    assert_eq!(
        full.fields,
        vec![
            ("model".to_string(), "\"gpt-6-astra\"".to_string()),
            ("tools".to_string(), "[{\"type\":\"function\"}]".to_string()),
        ]
    );
    // model, tools, u1, a1, u2, a2, u3 — each stored exactly once.
    assert_eq!(count(&storage, "request_log_payload_blobs"), 7);
}

#[test]
fn rewritten_history_forks_at_the_first_divergent_item() {
    let storage = storage();
    storage
        .insert_request_log_payload_manifest(&manifest("trc_1", 100, &["u1", "a1", "u2"]), None)
        .expect("insert first");
    let compacted = storage
        .insert_request_log_payload_manifest(
            &manifest("trc_2", 101, &["u1", "summary", "u3"]),
            None,
        )
        .expect("insert compacted");
    assert_eq!(compacted.parent_trace_id.as_deref(), Some("trc_1"));
    assert_eq!(compacted.shared_prefix_len, 1);

    let unrelated = storage
        .insert_request_log_payload_manifest(&manifest("trc_3", 102, &["other"]), None)
        .expect("insert unrelated");
    assert_eq!(unrelated.parent_trace_id, None);
    assert_eq!(unrelated.shared_prefix_len, 0);

    let full = storage
        .load_request_log_payload_full("trc_2", "upstream")
        .expect("load")
        .expect("manifest exists");
    assert_eq!(full.items, vec!["u1", "summary", "u3"]);
}

#[test]
fn parent_hint_is_used_and_duplicate_trace_is_ignored() {
    let storage = storage();
    let first = storage
        .insert_request_log_payload_manifest(&manifest("trc_1", 100, &["u1", "a1"]), None)
        .expect("insert first");
    let hint = RequestLogPayloadParentHint {
        trace_id: "trc_1".to_string(),
        stage: "upstream".to_string(),
        item_blob_ids: first.item_blob_ids.clone(),
    };
    let mut input = manifest("trc_2", 101, &["u1", "a1", "u2"]);
    input.conversation_key = None;
    let second = storage
        .insert_request_log_payload_manifest(&input, Some(&hint))
        .expect("insert second");
    assert_eq!(second.parent_trace_id.as_deref(), Some("trc_1"));
    assert_eq!(second.shared_prefix_len, 2);

    let again = storage
        .insert_request_log_payload_manifest(&input, Some(&hint))
        .expect("insert duplicate");
    assert!(!again.inserted);
    assert_eq!(count(&storage, "request_log_payload_manifests"), 2);
}

#[test]
fn pruning_a_parent_rebases_children_and_collects_unused_blobs() {
    let storage = storage();
    storage
        .insert_request_log_payload_manifest(&manifest("trc_old", 100, &["u1", "gone"]), None)
        .expect("insert old");
    storage
        .insert_request_log_payload_manifest(&manifest("trc_mid", 150, &["u1", "gone", "a1"]), None)
        .expect("insert mid");
    storage
        .insert_request_log_payload_manifest(
            &manifest("trc_new", 300, &["u1", "gone", "a1", "u2"]),
            None,
        )
        .expect("insert new");

    // Delete trc_old and trc_mid; trc_new must survive with its full list.
    let removed = storage
        .prune_request_log_payload_store_before(200)
        .expect("prune");
    assert_eq!(removed, 2);

    let rebased = storage
        .find_request_log_payload_manifest("trc_new", "upstream")
        .expect("read manifest")
        .expect("manifest kept");
    assert_eq!(rebased.parent_trace_id, None);
    assert_eq!(rebased.shared_prefix_len, 0);
    let full = storage
        .load_request_log_payload_full("trc_new", "upstream")
        .expect("load")
        .expect("manifest exists");
    assert!(full.complete);
    assert_eq!(full.items, vec!["u1", "gone", "a1", "u2"]);
    // model, tools, u1, gone, a1, u2 are all still referenced by trc_new.
    assert_eq!(count(&storage, "request_log_payload_blobs"), 6);

    storage
        .prune_request_log_payload_store_before(400)
        .expect("prune all");
    assert_eq!(count(&storage, "request_log_payload_manifests"), 0);
    assert_eq!(count(&storage, "request_log_payload_manifest_items"), 0);
    assert_eq!(count(&storage, "request_log_payload_blobs"), 0);
}

fn seed_log(storage: &Storage, trace_id: &str, key_id: &str) {
    storage
        .insert_request_log_with_token_stat(
            &super::super::RequestLog {
                trace_id: Some(trace_id.to_string()),
                key_id: Some(key_id.to_string()),
                request_path: "/v1/responses".to_string(),
                method: "POST".to_string(),
                created_at: super::super::now_ts(),
                ..Default::default()
            },
            &super::super::RequestTokenStat::default(),
        )
        .expect("insert request log");
}

#[test]
fn response_id_link_never_uses_temporal_proximity_or_other_keys() {
    let storage = storage();
    seed_log(&storage, "a-root", "gk_shared");
    seed_log(&storage, "b-unrelated", "gk_shared");
    seed_log(&storage, "a-cont", "gk_shared");
    storage
        .record_request_log_response_id("gk_shared", "resp_from_a_root", "a-root")
        .expect("record A response");
    storage
        .record_request_log_response_id("gk_shared", "resp_from_b", "b-unrelated")
        .expect("record B response");
    assert_eq!(
        storage
            .find_request_log_trace_for_response_id("gk_shared", "resp_from_a_root")
            .unwrap(),
        Some("a-root".to_string())
    );
    assert_eq!(
        storage
            .find_request_log_trace_for_response_id("gk_other", "resp_from_a_root")
            .unwrap(),
        None
    );
    assert_eq!(
        storage
            .find_request_log_trace_for_response_id("gk_shared", "resp_unknown")
            .unwrap(),
        None
    );
    // Even a guessed ID cannot be attached to a log belonging to another key.
    storage
        .record_request_log_response_id("gk_other", "resp_fake", "a-root")
        .unwrap();
    assert_eq!(
        storage
            .find_request_log_trace_for_response_id("gk_other", "resp_fake")
            .unwrap(),
        None
    );
    storage.clear_request_log_payloads().expect("clear");
    assert_eq!(count(&storage, "request_log_response_links"), 0);
}

#[test]
fn clear_generation_and_retention_reject_stale_jobs_atomically() {
    let storage = storage();
    let before = storage
        .request_log_payload_generation()
        .expect("read generation");
    let created = super::super::now_ts();
    let mut full = manifest("trc_old", created, &["secret"]);
    full.created_at = created;
    let preview = super::super::RequestLogPayload {
        trace_id: "trc_old".to_string(),
        stage: PAYLOAD_STAGE_CLIENT.to_string(),
        payload: "secret".to_string(),
        payload_bytes: 6,
        payload_truncated: false,
        redacted: false,
        body_hash: "hash-old".to_string(),
        created_at: created,
    };
    storage.clear_request_logs().expect("clear");
    assert!(!storage
        .insert_request_log_payload_if_current(&preview, before)
        .unwrap());
    assert!(
        !storage
            .insert_request_log_payload_manifest_if_current(&full, None, before)
            .unwrap()
            .inserted
    );
    assert_eq!(count(&storage, "request_log_payload_blobs"), 0);
    let current = storage.request_log_payload_generation().unwrap();
    assert!(storage
        .insert_request_log_payload_if_current(&preview, current)
        .unwrap());
    assert!(
        storage
            .insert_request_log_payload_manifest_if_current(&full, None, current)
            .unwrap()
            .inserted
    );
    storage
        .prune_request_logs_before(created + 1)
        .expect("prune");
    assert!(!storage
        .insert_request_log_payload_if_current(&preview, current)
        .unwrap());
    assert!(
        !storage
            .insert_request_log_payload_manifest_if_current(&full, None, current)
            .unwrap()
            .inserted
    );
    assert_eq!(count(&storage, "request_log_payload_blobs"), 0);
    assert_eq!(count(&storage, "request_log_payload_manifests"), 0);
}

#[test]
fn request_payload_migrations_are_idempotent_and_keep_clear_generation() {
    let storage = storage();
    storage.clear_request_logs().expect("advance generation");
    let generation = storage.request_log_payload_generation().unwrap();
    assert_eq!(generation, 1);
    storage
        .init()
        .expect("reapply migrations on existing database");
    assert_eq!(
        storage.request_log_payload_generation().unwrap(),
        generation
    );
    assert_eq!(count(&storage, "request_log_upstream_attempts"), 0);
    assert_eq!(count(&storage, "request_log_response_links"), 0);
}

#[test]
fn attempt_metadata_uses_same_clear_and_retention_barriers() {
    let storage = storage();
    let created = super::super::now_ts();
    let generation = storage.request_log_payload_generation().unwrap();
    let attempt = RequestLogUpstreamAttempt {
        trace_id: "trc_attempt".to_string(),
        stage: "upstream:00000000000000000002".to_string(),
        method: "POST".to_string(),
        url: "http://127.0.0.1/v1/responses".to_string(),
        transport: "http".to_string(),
        content_encoding: None,
        wire_sha256: "sample-hash".to_string(),
        identical_to_client: false,
        created_at: created,
    };
    assert!(storage
        .record_request_log_upstream_attempt_if_current(&attempt, generation)
        .unwrap());
    assert_eq!(
        storage
            .list_request_log_upstream_attempt_stages("trc_attempt")
            .unwrap(),
        vec![attempt.stage.clone()]
    );
    assert_eq!(
        storage
            .find_request_log_upstream_attempt("trc_attempt", &attempt.stage)
            .unwrap()
            .unwrap()
            .wire_sha256,
        "sample-hash"
    );
    storage.clear_request_logs().unwrap();
    assert!(storage
        .list_request_log_upstream_attempt_stages("trc_attempt")
        .unwrap()
        .is_empty());
    assert!(!storage
        .record_request_log_upstream_attempt_if_current(&attempt, generation)
        .unwrap());
    let new_generation = storage.request_log_payload_generation().unwrap();
    assert!(storage
        .record_request_log_upstream_attempt_if_current(&attempt, new_generation)
        .unwrap());
    storage.prune_request_logs_before(created + 1).unwrap();
    assert!(storage
        .list_request_log_upstream_attempt_stages("trc_attempt")
        .unwrap()
        .is_empty());
    assert!(!storage
        .record_request_log_upstream_attempt_if_current(&attempt, new_generation)
        .unwrap());
}

#[test]
fn identical_upstream_capture_is_skipped() {
    let storage = storage();
    let mut client = manifest("trc_1", 100, &["u1", "a1"]);
    client.stage = PAYLOAD_STAGE_CLIENT.to_string();
    client.body_hash = "hash-same".to_string();
    let written = storage
        .insert_request_log_payload_manifest(&client, None)
        .expect("insert client");
    assert!(written.inserted);

    let mut upstream = manifest("trc_1", 100, &["u1", "a1"]);
    upstream.body_hash = "hash-same".to_string();
    let skipped = storage
        .insert_request_log_payload_manifest(&upstream, None)
        .expect("insert upstream");
    assert!(!skipped.inserted);
    assert!(skipped.identical_to_client);
    assert_eq!(count(&storage, "request_log_payload_manifests"), 1);

    // A rewritten upstream body is stored as its own stage.
    let mut rewritten = manifest("trc_1", 100, &["u1", "a1", "extra"]);
    rewritten.body_hash = "hash-rewritten".to_string();
    let stored = storage
        .insert_request_log_payload_manifest(&rewritten, None)
        .expect("insert rewritten");
    assert!(stored.inserted);
    assert!(!stored.identical_to_client);
    assert_eq!(
        storage
            .list_request_log_payload_manifest_stages("trc_1")
            .unwrap(),
        vec![
            PAYLOAD_STAGE_CLIENT.to_string(),
            PAYLOAD_STAGE_UPSTREAM.to_string()
        ]
    );
    let client_full = storage
        .load_request_log_payload_full("trc_1", PAYLOAD_STAGE_CLIENT)
        .expect("load client")
        .expect("client exists");
    assert_eq!(client_full.items, vec!["u1", "a1"]);
    let upstream_full = storage
        .load_request_log_payload_full("trc_1", PAYLOAD_STAGE_UPSTREAM)
        .expect("load upstream")
        .expect("upstream exists");
    assert_eq!(upstream_full.items, vec!["u1", "a1", "extra"]);
}

#[test]
fn stages_keep_independent_conversation_chains() {
    let storage = storage();
    let mut client_first = manifest("trc_c1", 100, &["u1"]);
    client_first.stage = PAYLOAD_STAGE_CLIENT.to_string();
    storage
        .insert_request_log_payload_manifest(&client_first, None)
        .expect("insert client first");

    // Same conversation, first upstream capture: it must not chain onto the
    // client manifest.
    let upstream_first = manifest("trc_u1", 101, &["u1"]);
    let written = storage
        .insert_request_log_payload_manifest(&upstream_first, None)
        .expect("insert upstream first");
    assert_eq!(written.parent_trace_id, None);

    // Second upstream capture chains onto the first upstream one.
    let upstream_second = manifest("trc_u2", 102, &["u1", "a1"]);
    let written = storage
        .insert_request_log_payload_manifest(&upstream_second, None)
        .expect("insert upstream second");
    assert_eq!(written.parent_trace_id.as_deref(), Some("trc_u1"));
}
