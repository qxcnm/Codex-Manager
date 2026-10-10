use super::super::clear::{self as payload_clear, ClearState};
use super::super::{
    bytes_hash, set_test_pipeline, store_client_request_log_payload, OutboundAttemptCapture,
    RequestLogPayloadJob,
};
use super::*;
use bytes::Bytes;
use codexmanager_core::request_log_spill::record::{SpillRecordMeta, SpillRecordRef};
use codexmanager_core::request_log_spill::segment::{list_segments, segment_path};
use codexmanager_core::storage::{now_ts, PAYLOAD_STAGE_CLIENT, PAYLOAD_STAGE_UPSTREAM};
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicU64 as TestCounter;
use std::time::Instant;

static NEXT_DIR: TestCounter = TestCounter::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "cm-payload-queue-{name}-{}-{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&root);
        Self(root.join(SPILL_DIR_NAME))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        if let Some(parent) = self.0.parent() {
            let _ = std::fs::remove_dir_all(parent);
        }
    }
}

/// Writer gate: every batch consumes one permit.
#[derive(Default)]
struct Gate {
    permits: Mutex<u64>,
    cv: Condvar,
}

impl Gate {
    fn wait(&self) {
        let mut permits = self.permits.lock().unwrap();
        while *permits == 0 {
            permits = self.cv.wait(permits).unwrap();
        }
        *permits -= 1;
    }

    fn grant(&self, count: u64) {
        *self.permits.lock().unwrap() += count;
        self.cv.notify_all();
    }
}

struct TestPipeline {
    shared: Arc<PipelineShared>,
    gate: Arc<Gate>,
}

impl Drop for TestPipeline {
    fn drop(&mut self) {
        self.gate.grant(1 << 40);
        self.shared.shutdown();
    }
}

fn storage() -> Storage {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    storage
}

fn private_clear_state() -> &'static ClearState {
    Box::leak(Box::new(ClearState::new()))
}

fn budget(bytes: u64) -> QueueBudget {
    QueueBudget {
        bytes,
        effective_memory: None,
        source: QueueBudgetSource::Override,
        invalid_override: false,
    }
}

fn start(
    storage: &Storage,
    budget_bytes: u64,
    spill: SpillSetup,
    clear_state: &'static ClearState,
    before_write: Option<HookFn>,
    before_preprocess: Option<JobHookFn>,
) -> TestPipeline {
    start_full(
        storage,
        budget_bytes,
        spill,
        clear_state,
        before_write,
        before_preprocess,
        None,
    )
}

fn start_full(
    storage: &Storage,
    budget_bytes: u64,
    spill: SpillSetup,
    clear_state: &'static ClearState,
    before_write: Option<HookFn>,
    before_preprocess: Option<JobHookFn>,
    before_spill: Option<JobHookFn>,
) -> TestPipeline {
    let writer_storage = storage.shared_handle();
    let open: OpenStorageFn =
        Arc::new(move || Some(Box::new(Box::new(writer_storage.shared_handle())) as StorageRef));
    start_with_open(
        open,
        budget_bytes,
        spill,
        clear_state,
        before_write,
        before_preprocess,
        before_spill,
    )
}

fn start_with_open(
    open: OpenStorageFn,
    budget_bytes: u64,
    spill: SpillSetup,
    clear_state: &'static ClearState,
    before_write: Option<HookFn>,
    before_preprocess: Option<JobHookFn>,
    before_spill: Option<JobHookFn>,
) -> TestPipeline {
    let gate = Arc::new(Gate::default());
    let gate_hook = gate.clone();
    let before_write = before_write.unwrap_or_else(|| Arc::new(move || gate_hook.wait()) as HookFn);
    let shared = PipelineShared::start(PipelineOptions {
        clear: clear_state,
        budget: budget(budget_bytes),
        spill,
        workers: 2,
        hooks: PipelineHooks {
            open,
            before_write: Some(before_write),
            before_preprocess,
            before_spill,
        },
    });
    TestPipeline { shared, gate }
}

fn job(trace_id: &str, body: &str, preview: bool) -> RequestLogPayloadJob {
    RequestLogPayloadJob {
        trace_id: trace_id.to_string(),
        stage: PAYLOAD_STAGE_CLIENT.to_string(),
        body: Bytes::from(body.to_string()),
        conversation_key: Some("gk_q|conv".to_string()),
        redact: false,
        preview,
        created_at: now_ts(),
        generation: payload_clear::GENERATION_UNRESOLVED,
        clear_epoch: 0,
        boot_id: 0,
        attempt: None,
        original: None,
    }
}

fn wait_until(what: &str, condition: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if condition() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("timed out waiting for {what}");
}

fn stored(storage: &Storage, trace_id: &str) -> bool {
    storage
        .find_request_log_payload_by_trace_id(trace_id, PAYLOAD_STAGE_CLIENT)
        .unwrap()
        .is_some()
        || storage
            .find_request_log_payload_manifest(trace_id, PAYLOAD_STAGE_CLIENT)
            .unwrap()
            .is_some()
}

fn segments(dir: &TempDir) -> usize {
    list_segments(&dir.0).unwrap().len()
}

#[test]
fn request_log_payload_queue_hot_path_returns_immediately_while_writer_is_paused() {
    let storage = storage();
    let dir = TempDir::new("hot");
    let clear_state = private_clear_state();
    clear_state.observe_generation(0, None);
    let body = "x".repeat(16 * 1024);
    let per_job = job_accounted_bytes(&job("trc_hot_00", &body, true));
    let budget_bytes = per_job * 3;
    let store = SpillStore::open(&dir.0, SEGMENT_MAX_BYTES).unwrap();
    let pipeline = start(
        &storage,
        budget_bytes,
        SpillSetup::Opened(store),
        clear_state,
        None,
        None,
    );
    let shared = pipeline.shared.clone();
    set_test_pipeline(Some(shared.clone()));
    let calls_before = crate::storage_helpers::open_storage_calls_on_this_thread();
    let payload = Bytes::from(body.clone());
    let started = Instant::now();
    for index in 0..20 {
        let call = Instant::now();
        store_client_request_log_payload(
            &format!("trc_hot_{index:02}"),
            &payload,
            Some("gk_q|conv".to_string()),
        );
        assert!(
            call.elapsed() < Duration::from_millis(500),
            "capture blocked the hot path"
        );
    }
    assert!(started.elapsed() < Duration::from_secs(2));
    set_test_pipeline(None);
    assert_eq!(
        crate::storage_helpers::open_storage_calls_on_this_thread(),
        calls_before,
        "the capture hot path must never acquire storage"
    );

    let stats = shared.snapshot_stats(None);
    assert!(stats.queued_bytes <= budget_bytes, "{stats:?}");
    assert_eq!(stats.queued_bytes, per_job * 3);
    wait_until("spill hand-off drained", || {
        shared.snapshot_stats(None).spill_pending_jobs == 0
    });
    let stats = shared.snapshot_stats(None);
    assert!(stats.spilled_total >= 1, "{stats:?}");
    // Every over-budget job is either on disk or dropped and counted
    // (disk_full only when the CI disk itself is nearly full).
    assert_eq!(
        stats.spilled_total
            + stats.dropped_by_reason["disk_slow"]
            + stats.dropped_by_reason["disk_full"],
        17,
        "{stats:?}"
    );
    assert!(stats.queued_bytes <= budget_bytes);

    pipeline.gate.grant(1 << 20);
    let expected = 3 + stats.spilled_total;
    wait_until("all accepted jobs written", || {
        shared.snapshot_stats(None).written_total == expected
    });
    for index in 0..20 {
        let trace_id = format!("trc_hot_{index:02}");
        assert_ne!(
            stored(&storage, &trace_id),
            shared.drop_reason_for_trace(&trace_id).is_some(),
            "{trace_id}: stored xor dropped"
        );
    }
    wait_until("queue back in memory mode", || {
        let stats = shared.snapshot_stats(None);
        stats.queued_bytes == 0 && !stats.spilling
    });
    wait_until("consumed segments deleted", || segments(&dir) == 0);
}

#[test]
fn request_log_payload_queue_drops_with_reason_when_spill_dir_is_locked() {
    let storage = storage();
    let dir = TempDir::new("locked");
    let _owner = SpillStore::open(&dir.0, SEGMENT_MAX_BYTES).unwrap();
    let setup = open_spill_setup(Some(dir.0.clone()), SEGMENT_MAX_BYTES);
    assert!(matches!(
        setup,
        SpillSetup::Disabled(DropReason::SpillLocked)
    ));
    let first = job("trc_lock_a", "{\"input\":\"a\"}", true);
    let budget_bytes = job_accounted_bytes(&first);
    let pipeline = start(
        &storage,
        budget_bytes,
        setup,
        private_clear_state(),
        None,
        None,
    );
    let shared = pipeline.shared.clone();
    shared.submit(first);
    shared.submit(job("trc_lock_b", "{\"input\":\"b\"}", true));
    shared.submit(job("trc_lock_c", "{\"input\":\"c\"}", true));
    let stats = shared.snapshot_stats(Some("trc_lock_b"));
    assert_eq!(stats.dropped_by_reason["spill_locked"], 2, "{stats:?}");
    assert_eq!(stats.dropped_total, 2);
    assert_eq!(stats.trace_drop_reason.as_deref(), Some("spill_locked"));
    assert!(!stats.spill_available);
    assert_eq!(stats.spill_blocked_reason.as_deref(), Some("spill_locked"));
    assert_eq!(
        shared.snapshot_stats(Some("trc_lock_a")).trace_drop_reason,
        None
    );
    pipeline.gate.grant(10);
    wait_until("memory job written", || {
        shared.snapshot_stats(None).written_total == 1
    });
    assert!(stored(&storage, "trc_lock_a"));
    assert!(!stored(&storage, "trc_lock_b"));
}

#[test]
fn request_log_payload_queue_keeps_capture_order_across_spill() {
    let storage = storage();
    let dir = TempDir::new("order");
    let o1 = job(
        "trc_order_1",
        r#"{"input":[{"role":"user","content":"u1"}]}"#,
        false,
    );
    let budget_bytes = job_accounted_bytes(&o1);
    let pipeline = start(
        &storage,
        budget_bytes,
        SpillSetup::Opened(SpillStore::open(&dir.0, SEGMENT_MAX_BYTES).unwrap()),
        private_clear_state(),
        None,
        None,
    );
    let shared = pipeline.shared.clone();
    shared.submit(o1);
    shared.submit(job(
        "trc_order_2",
        r#"{"input":[{"role":"user","content":"u1"},{"role":"assistant","content":"a1"},{"role":"user","content":"u2"}]}"#,
        false,
    ));
    wait_until("second job spilled", || {
        let stats = shared.snapshot_stats(None);
        stats.spilled_total == 1 && stats.spill_pending_jobs == 0
    });
    // Let only the first (memory) batch through: memory is free again, but
    // the spilled job is still unread.
    pipeline.gate.grant(1);
    wait_until("first job written", || {
        shared.snapshot_stats(None).written_total == 1
    });
    shared.submit(job(
        "trc_order_3",
        r#"{"input":[{"role":"user","content":"u1"},{"role":"assistant","content":"a1"},{"role":"user","content":"u2"},{"role":"assistant","content":"a2"},{"role":"user","content":"u3"}]}"#,
        false,
    ));
    let stats = shared.snapshot_stats(None);
    assert_eq!(
        stats.queued_jobs, 0,
        "a later job must not overtake: {stats:?}"
    );
    wait_until("third job spilled behind the second", || {
        let stats = shared.snapshot_stats(None);
        stats.spilled_total == 2 && stats.spill_pending_jobs == 0
    });
    pipeline.gate.grant(100);
    wait_until("all jobs written", || {
        shared.snapshot_stats(None).written_total == 3
    });
    let second = storage
        .find_request_log_payload_manifest("trc_order_2", PAYLOAD_STAGE_CLIENT)
        .unwrap()
        .unwrap();
    assert_eq!(second.parent_trace_id.as_deref(), Some("trc_order_1"));
    assert_eq!(second.shared_prefix_len, 1);
    let third = storage
        .find_request_log_payload_manifest("trc_order_3", PAYLOAD_STAGE_CLIENT)
        .unwrap()
        .unwrap();
    assert_eq!(third.parent_trace_id.as_deref(), Some("trc_order_2"));
    assert_eq!(third.shared_prefix_len, 3);
    wait_until("memory mode resumed", || {
        !shared.snapshot_stats(None).spilling
    });
}

fn spill_meta(trace_id: &str, generation: Option<i64>, boot_id: u64) -> SpillRecordMeta {
    SpillRecordMeta {
        trace_id: trace_id.to_string(),
        stage: PAYLOAD_STAGE_CLIENT.to_string(),
        generation,
        clear_epoch: 0,
        boot_id,
        redact: false,
        preview: true,
        conversation_key: Some("gk_q|conv".to_string()),
        attempt: None,
        created_at: now_ts(),
        original: None,
    }
}

#[test]
fn request_log_payload_queue_replay_rejects_jobs_from_before_a_clear() {
    let storage = storage();
    let dir = TempDir::new("replay");
    {
        let mut store = SpillStore::open(&dir.0, SEGMENT_MAX_BYTES).unwrap();
        let append = |store: &mut SpillStore, meta: SpillRecordMeta| {
            store
                .append(&SpillRecordRef {
                    meta: &meta,
                    body: br#"{"input":"secret before clear"}"#,
                    wire_body: None,
                })
                .unwrap();
        };
        append(&mut store, spill_meta("trc_replay_stale", Some(0), 77));
        append(
            &mut store,
            spill_meta(
                "trc_replay_foreign",
                None,
                payload_clear::boot_id().wrapping_add(1),
            ),
        );
        storage.clear_request_logs().unwrap();
        let current = storage.request_log_payload_generation().unwrap();
        append(
            &mut store,
            spill_meta("trc_replay_fresh", Some(current), 77),
        );
        append(
            &mut store,
            spill_meta("trc_replay_local", None, payload_clear::boot_id()),
        );
        store.close_active().unwrap();
    }
    assert!(segments(&dir) > 0);
    let pipeline = start(
        &storage,
        1 << 20,
        SpillSetup::Opened(SpillStore::open(&dir.0, SEGMENT_MAX_BYTES).unwrap()),
        private_clear_state(),
        None,
        None,
    );
    pipeline.gate.grant(100);
    let shared = pipeline.shared.clone();
    wait_until("leftover segments replayed", || {
        shared.snapshot_stats(None).replayed_total == 4
    });
    let stats = shared.snapshot_stats(None);
    assert_eq!(stats.written_total, 2, "{stats:?}");
    assert_eq!(stats.stale_rejected_total, 2, "{stats:?}");
    assert!(stored(&storage, "trc_replay_fresh"));
    assert!(stored(&storage, "trc_replay_local"));
    assert!(!stored(&storage, "trc_replay_stale"));
    assert!(!stored(&storage, "trc_replay_foreign"));
    assert_eq!(
        shared.drop_reason_for_trace("trc_replay_stale"),
        Some(TraceDropReason::Stale)
    );
    let stats = shared.snapshot_stats(Some("trc_replay_foreign"));
    assert_eq!(stats.trace_drop_reason.as_deref(), Some("stale_generation"));
    assert_eq!(
        stats.dropped_total, 0,
        "stale jobs are not counted as dropped"
    );
    wait_until("replayed segments deleted", || segments(&dir) == 0);
}

#[test]
fn request_log_payload_queue_clear_deletes_old_spill_and_keeps_jobs_captured_during_clear() {
    let storage = storage();
    let dir = TempDir::new("clear");
    let clear_state = private_clear_state();
    clear_state.observe_generation(storage.request_log_payload_generation().unwrap(), None);
    let first = job("trc_clear_a", "{\"input\":\"a\"}", true);
    let budget_bytes = job_accounted_bytes(&first);
    let pipeline = start(
        &storage,
        budget_bytes,
        SpillSetup::Opened(SpillStore::open(&dir.0, SEGMENT_MAX_BYTES).unwrap()),
        clear_state,
        None,
        None,
    );
    let shared = pipeline.shared.clone();
    shared.submit(first);
    shared.submit(job("trc_clear_b", "{\"input\":\"b\"}", true));
    wait_until("b spilled", || {
        let stats = shared.snapshot_stats(None);
        stats.spilled_total == 1 && stats.spill_pending_jobs == 0
    });

    // The clear starts: the spill thread rolls to a new segment.
    let seq = shared.begin_clear();
    clear_state.begin_clear();
    storage.clear_request_logs().unwrap();
    let generation = storage.request_log_payload_generation().unwrap();
    // What the writer's periodic refresh does (the writer is gated here).
    clear_state.observe_generation(generation, None);
    // Captured while the clear is still running (e.g. batched deletes).
    shared.submit(job("trc_clear_c", "{\"input\":\"c\"}", true));
    wait_until("c spilled after the roll", || {
        let stats = shared.snapshot_stats(None);
        stats.spilled_total == 2 && stats.spill_pending_jobs == 0
    });
    assert!(lock_inner(&shared).clear_boundaries.contains_key(&seq));
    assert_eq!(segments(&dir), 2, "the clear boundary rolled the segment");

    shared.purge_after_clear(Some(seq), Some(generation));
    clear_state.finish_clear();
    wait_until("pre-clear spill segment deleted", || {
        segments(&dir) == 1 && lock_inner(&shared).purges.is_empty()
    });
    pipeline.gate.grant(100);
    wait_until("job captured during the clear written", || {
        let stats = shared.snapshot_stats(None);
        stats.written_total == 1 && stats.stale_rejected_total + stats.discarded_by_clear_total >= 1
    });
    shared.submit(job("trc_clear_d", "{\"input\":\"d\"}", true));
    wait_until("job captured after the clear written", || {
        shared.snapshot_stats(None).written_total == 2
    });
    assert!(stored(&storage, "trc_clear_c"));
    assert!(stored(&storage, "trc_clear_d"));
    for trace_id in ["trc_clear_a", "trc_clear_b"] {
        assert!(
            !stored(&storage, trace_id),
            "{trace_id} restored after clear"
        );
    }
    assert_eq!(shared.snapshot_stats(None).replayed_total, 1);
    wait_until("consumed segment deleted", || segments(&dir) == 0);
}

#[test]
fn request_log_payload_queue_writer_refreshes_generation_mirror_within_interval() {
    let storage = storage();
    let clear_state = private_clear_state();
    let pipeline = start(
        &storage,
        1 << 20,
        SpillSetup::Disabled(DropReason::IoError),
        clear_state,
        Some(Arc::new(|| {}) as HookFn),
        None,
    );
    wait_until("mirror initialized by the writer", || {
        clear_state.mirror_value() == storage.request_log_payload_generation().unwrap()
    });
    let calls_before = crate::storage_helpers::open_storage_calls_on_this_thread();
    storage.clear_request_logs().unwrap();
    let generation = storage.request_log_payload_generation().unwrap();
    let cleared = Instant::now();
    wait_until("mirror refreshed after the clear committed", || {
        clear_state.mirror_value() == generation
    });
    assert!(
        cleared.elapsed() < Duration::from_secs(2),
        "refresh took {:?}",
        cleared.elapsed()
    );
    let shared = pipeline.shared.clone();
    shared.submit(job("trc_refresh", "{\"input\":\"r\"}", true));
    wait_until("job captured after refresh written", || {
        shared.snapshot_stats(None).written_total == 1
    });
    assert!(stored(&storage, "trc_refresh"));
    assert_eq!(
        crate::storage_helpers::open_storage_calls_on_this_thread(),
        calls_before
    );
}

#[test]
fn request_log_payload_queue_preprocess_panic_does_not_kill_writer() {
    let storage = storage();
    let hook: JobHookFn = Arc::new(|job: &RequestLogPayloadJob| {
        if job.trace_id == "trc_panic" {
            panic!("injected preprocessing panic");
        }
    });
    let pipeline = start(
        &storage,
        1 << 20,
        SpillSetup::Disabled(DropReason::IoError),
        private_clear_state(),
        None,
        Some(hook),
    );
    pipeline.gate.grant(1 << 20);
    let shared = pipeline.shared.clone();
    shared.submit(job("trc_ok_1", "{\"input\":\"1\"}", true));
    shared.submit(job("trc_panic", "{\"input\":\"p\"}", true));
    shared.submit(job("trc_ok_2", "{\"input\":\"2\"}", false));
    wait_until("healthy jobs written", || {
        shared.snapshot_stats(None).written_total == 2
    });
    let stats = shared.snapshot_stats(Some("trc_panic"));
    assert_eq!(stats.preprocess_panics, 1);
    assert_eq!(
        stats.trace_drop_reason.as_deref(),
        Some("writer_unavailable")
    );
    assert_eq!(stats.writer_restarts, 0);
    shared.submit(job("trc_ok_3", "{\"input\":\"3\"}", true));
    wait_until("writer still alive", || {
        shared.snapshot_stats(None).written_total == 3
    });
    assert!(stored(&storage, "trc_ok_3"));
    assert!(!stored(&storage, "trc_panic"));
}

#[test]
fn request_log_payload_queue_writer_panic_restarts_and_releases_budget() {
    let storage = storage();
    let panicked = Arc::new(AtomicBool::new(false));
    let panic_flag = panicked.clone();
    let hook: HookFn = Arc::new(move || {
        if !panic_flag.swap(true, Ordering::SeqCst) {
            panic!("injected writer panic");
        }
    });
    let pipeline = start(
        &storage,
        1 << 20,
        SpillSetup::Disabled(DropReason::IoError),
        private_clear_state(),
        Some(hook),
        None,
    );
    let shared = pipeline.shared.clone();
    shared.submit(job("trc_restart_1", "{\"input\":\"1\"}", true));
    wait_until("writer restarted", || {
        shared.snapshot_stats(None).writer_restarts == 1
    });
    let stats = shared.snapshot_stats(Some("trc_restart_1"));
    assert_eq!(stats.queued_bytes, 0, "in-flight budget released");
    assert_eq!(
        stats.trace_drop_reason.as_deref(),
        Some("writer_unavailable")
    );
    shared.submit(job("trc_restart_2", "{\"input\":\"2\"}", true));
    wait_until("restarted writer persists", || {
        shared.snapshot_stats(None).written_total == 1
    });
    assert!(stored(&storage, "trc_restart_2"));
}

#[test]
fn request_log_payload_queue_wire_body_shared_with_logical_body_is_counted_once() {
    let body = Bytes::from_static(b"{\"input\":\"shared\"}");
    let mut shared_job = job("trc_wire", "", true);
    shared_job.body = body.clone();
    shared_job.attempt = Some(super::super::OutboundAttemptCapture {
        method: "POST".to_string(),
        url: "u".to_string(),
        transport: "http".to_string(),
        content_encoding: None,
        wire_body: body.clone(),
    });
    let mut separate_job = shared_job.clone();
    if let Some(attempt) = separate_job.attempt.as_mut() {
        attempt.wire_body = Bytes::from(body.to_vec());
    }
    assert_eq!(
        job_accounted_bytes(&separate_job),
        job_accounted_bytes(&shared_job) + body.len() as u64
    );
}

#[test]
fn request_log_payload_queue_stats_before_start_are_zero() {
    let stats = RequestLogPayloadQueueStats::default();
    assert_eq!(stats.dropped_total, 0);
    let value = serde_json::to_value(&stats).unwrap();
    assert!(value.get("queuedBytes").is_some());
    assert!(value.get("droppedByReason").is_some());
    assert!(value.get("traceDropReason").is_some());
}

fn spill_files_contain(dir: &TempDir, needle: &[u8]) -> bool {
    list_segments(&dir.0).unwrap().iter().any(|(id, _)| {
        let bytes = std::fs::read(segment_path(&dir.0, *id)).unwrap_or_default();
        bytes.windows(needle.len()).any(|window| window == needle)
    })
}

#[test]
fn request_log_payload_queue_spill_panic_releases_handoff_and_resumes_memory() {
    let storage = storage();
    let dir = TempDir::new("spill-panic");
    let first = job("trc_sp_a", "{\"input\":\"a\"}", true);
    let budget_bytes = job_accounted_bytes(&first);
    let hook: JobHookFn = Arc::new(|job: &RequestLogPayloadJob| {
        if job.trace_id == "trc_sp_panic" {
            panic!("injected spill panic");
        }
    });
    let pipeline = start_full(
        &storage,
        budget_bytes,
        SpillSetup::Opened(SpillStore::open(&dir.0, SEGMENT_MAX_BYTES).unwrap()),
        private_clear_state(),
        None,
        None,
        Some(hook),
    );
    let shared = pipeline.shared.clone();
    shared.submit(first);
    shared.submit(job("trc_sp_panic", "{\"input\":\"p\"}", true));
    wait_until("spill thread restarted and released the job", || {
        let stats = shared.snapshot_stats(None);
        stats.spill_restarts == 1 && stats.spill_pending_jobs == 0 && stats.spill_pending_bytes == 0
    });
    let stats = shared.snapshot_stats(Some("trc_sp_panic"));
    assert_eq!(
        stats.trace_drop_reason.as_deref(),
        Some("writer_unavailable")
    );
    assert_eq!(stats.spilled_total, 0);
    pipeline.gate.grant(100);
    wait_until("queue back in memory mode", || {
        let stats = shared.snapshot_stats(None);
        stats.written_total == 1 && !stats.spilling
    });
    // Same size as the first job, so it fits the budget and must not spill.
    let after = job("trc_sp_b", "{\"input\":\"b\"}", true);
    assert!(job_accounted_bytes(&after) <= budget_bytes);
    shared.submit(after);
    wait_until("later job written from memory", || {
        shared.snapshot_stats(None).written_total == 2
    });
    assert!(stored(&storage, "trc_sp_b"));
    assert_eq!(shared.snapshot_stats(None).spilled_total, 0);
}

#[test]
fn request_log_payload_queue_waits_out_a_busy_database_without_dropping() {
    let root = std::env::temp_dir().join(format!(
        "cm-payload-busy-{}-{}",
        std::process::id(),
        NEXT_DIR.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let db_path = root.join("busy.db");
    let storage = Storage::open(&db_path).expect("open file storage");
    storage.init().expect("init");
    let holder = rusqlite::Connection::open(&db_path).expect("open holder");
    holder
        .execute_batch("BEGIN IMMEDIATE")
        .expect("hold the write lock");
    let pipeline = start(
        &storage,
        1 << 20,
        SpillSetup::Disabled(DropReason::IoError),
        private_clear_state(),
        Some(Arc::new(|| {}) as HookFn),
        None,
    );
    let shared = pipeline.shared.clone();
    shared.submit(job("trc_busy", "{\"input\":\"busy\"}", true));
    // Longer than the old 3 x busy_timeout retry budget.
    std::thread::sleep(Duration::from_secs(10));
    let stats = shared.snapshot_stats(Some("trc_busy"));
    assert_eq!(stats.dropped_total, 0, "{stats:?}");
    assert_eq!(stats.written_total, 0);
    assert!(stats.queued_bytes > 0, "accounting kept while waiting");
    assert_eq!(stats.trace_drop_reason, None);
    holder
        .execute_batch("COMMIT")
        .expect("release the write lock");
    // The write counter is published before the in-flight guard releases memory.
    wait_until("job written and memory budget released", || {
        let stats = shared.snapshot_stats(None);
        stats.written_total == 1 && stats.queued_bytes == 0 && stats.queued_jobs == 0
    });
    let stats = shared.snapshot_stats(None);
    assert!(stats.db_busy_retries >= 1, "{stats:?}");
    assert_eq!(stats.dropped_total, 0);
    assert_eq!(stats.queued_bytes, 0);
    assert!(stored(&storage, "trc_busy"));
    drop(pipeline);
    drop(holder);
    drop(storage);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn request_log_payload_queue_holds_pre_clear_handoff_jobs_off_disk() {
    let storage = storage();
    let dir = TempDir::new("hold");
    let clear_state = private_clear_state();
    clear_state.observe_generation(storage.request_log_payload_generation().unwrap(), None);
    // The first job fills the memory budget, so the next ones spill. It is
    // large enough that the hand-off budget (half the queue budget) holds the
    // held pre-clear job and the post-clear job together.
    let first = job(
        "trc_hold_a",
        &format!("{{\"input\":\"{}\"}}", "a".repeat(4096)),
        true,
    );
    let budget_bytes = job_accounted_bytes(&first);
    let before = job("trc_hold_b", "{\"input\":\"secret-before-clear\"}", true);
    let after = job("trc_hold_c", "{\"input\":\"after-clear\"}", true);
    assert!(
        job_accounted_bytes(&before) + job_accounted_bytes(&after)
            <= spill_handoff_budget(budget_bytes)
    );
    let pipeline = start(
        &storage,
        budget_bytes,
        SpillSetup::Opened(SpillStore::open(&dir.0, SEGMENT_MAX_BYTES).unwrap()),
        clear_state,
        None,
        None,
    );
    let shared = pipeline.shared.clone();
    shared.submit(first);
    let seq = shared.begin_clear();
    wait_until("roll recorded", || {
        lock_inner(&shared).clear_boundaries.contains_key(&seq)
    });
    // Captured before the clear committed: must never reach the disk.
    shared.submit(before);
    wait_until("pre-clear job held", || lock_inner(&shared).held.len() == 1);
    storage.clear_request_logs().unwrap();
    let generation = storage.request_log_payload_generation().unwrap();
    clear_state.observe_generation(generation, None);
    shared.submit(after);
    wait_until("post-clear job spilled", || {
        shared.snapshot_stats(None).spilled_total == 1
    });
    assert!(spill_files_contain(&dir, b"after-clear"));
    assert!(!spill_files_contain(&dir, b"secret-before-clear"));
    shared.purge_after_clear(Some(seq), Some(generation));
    wait_until("held job discarded", || {
        let stats = shared.snapshot_stats(None);
        stats.spill_pending_jobs == 0 && lock_inner(&shared).held.is_empty()
    });
    // The held job is discarded by the purge. The first job is discarded
    // too if it is still queued, or rejected by the database if the gated
    // writer already took it.
    assert!(shared.snapshot_stats(None).discarded_by_clear_total >= 1);
    assert_eq!(
        shared.drop_reason_for_trace("trc_hold_b"),
        Some(TraceDropReason::Stale)
    );
    pipeline.gate.grant(100);
    wait_until("post-clear job written, pre-clear jobs gone", || {
        let stats = shared.snapshot_stats(None);
        stats.written_total == 1 && stats.discarded_by_clear_total + stats.stale_rejected_total == 2
    });
    assert!(stored(&storage, "trc_hold_c"));
    assert!(!stored(&storage, "trc_hold_a"));
    assert!(!stored(&storage, "trc_hold_b"));
    wait_until("spill files deleted", || segments(&dir) == 0);
}

#[test]
fn request_log_payload_queue_failed_clear_requeues_held_jobs() {
    let storage = storage();
    let dir = TempDir::new("hold-abandon");
    let clear_state = private_clear_state();
    clear_state.observe_generation(storage.request_log_payload_generation().unwrap(), None);
    let first = job("trc_abandon_a", "{\"input\":\"a\"}", true);
    let budget_bytes = job_accounted_bytes(&first);
    let pipeline = start(
        &storage,
        budget_bytes,
        SpillSetup::Opened(SpillStore::open(&dir.0, SEGMENT_MAX_BYTES).unwrap()),
        clear_state,
        None,
        None,
    );
    let shared = pipeline.shared.clone();
    shared.submit(first);
    let seq = shared.begin_clear();
    shared.submit(job("trc_abandon_b", "{\"input\":\"b\"}", true));
    wait_until("job held during the clear", || {
        lock_inner(&shared).held.len() == 1
    });
    shared.abandon_clear(seq);
    pipeline.gate.grant(100);
    wait_until("both jobs written after the failed clear", || {
        shared.snapshot_stats(None).written_total == 2
    });
    assert!(stored(&storage, "trc_abandon_a"));
    assert!(stored(&storage, "trc_abandon_b"));
    assert_eq!(shared.snapshot_stats(None).discarded_by_clear_total, 0);
}

#[test]
fn request_log_payload_queue_clear_before_store_install_keeps_new_segments() {
    let storage = storage();
    let dir = TempDir::new("late-install");
    {
        let mut leftover = SpillStore::open(&dir.0, SEGMENT_MAX_BYTES).unwrap();
        leftover
            .append(&SpillRecordRef {
                meta: &spill_meta("trc_late_leftover", Some(0), 5),
                body: b"{\"input\":\"leftover\"}",
                wire_body: None,
            })
            .unwrap();
        leftover.close_active().unwrap();
    }
    assert_eq!(segments(&dir), 1);
    let clear_state = private_clear_state();
    clear_state.observe_generation(0, None);
    let first = job("trc_late_a", "{\"input\":\"a\"}", true);
    let second = job("trc_late_c", "{\"input\":\"c\"}", true);
    // Both jobs fit in memory and together in the hand-off area: they spill
    // only because of the leftover segment.
    let budget_bytes = 8 * job_accounted_bytes(&first);
    assert!(
        job_accounted_bytes(&first) + job_accounted_bytes(&second)
            <= spill_handoff_budget(budget_bytes)
    );
    let pipeline = start(
        &storage,
        budget_bytes,
        SpillSetup::Disabled(DropReason::IoError),
        clear_state,
        None,
        None,
    );
    let shared = pipeline.shared.clone();
    let seq = shared.begin_clear();
    storage.clear_request_logs().unwrap();
    let generation = storage.request_log_payload_generation().unwrap();
    clear_state.observe_generation(generation, None);
    super::super::spill::install_store(
        &shared,
        SpillStore::open(&dir.0, SEGMENT_MAX_BYTES).unwrap(),
    );
    shared.submit(first);
    shared.submit(second);
    // Leftover segments put the queue in spilling mode: both jobs spill.
    wait_until("jobs spilled while the clear runs", || {
        let stats = shared.snapshot_stats(None);
        stats.spilled_total == 2 && stats.spill_pending_jobs == 0
    });
    assert_eq!(segments(&dir), 2);
    shared.purge_after_clear(Some(seq), Some(generation));
    wait_until("only the leftover segment deleted", || {
        segments(&dir) == 1 && lock_inner(&shared).purges.is_empty()
    });
    pipeline.gate.grant(100);
    wait_until("jobs captured during the clear written", || {
        shared.snapshot_stats(None).written_total == 2
    });
    assert!(stored(&storage, "trc_late_a"));
    assert!(stored(&storage, "trc_late_c"));
    assert!(!stored(&storage, "trc_late_leftover"));
}

#[test]
fn request_log_payload_queue_hot_path_never_spawns_when_writer_is_down() {
    let storage = storage();
    let pipeline = start(
        &storage,
        1 << 20,
        SpillSetup::Disabled(DropReason::IoError),
        private_clear_state(),
        None,
        None,
    );
    let shared = pipeline.shared.clone();
    let threads_before = lock_recover(&shared.threads).len();
    lock_inner(&shared).writer_alive = false;
    let started = Instant::now();
    for index in 0..50 {
        shared.submit(job(&format!("trc_down_{index}"), "{}", true));
    }
    assert!(started.elapsed() < Duration::from_secs(1));
    assert_eq!(lock_recover(&shared.threads).len(), threads_before);
    let stats = shared.snapshot_stats(Some("trc_down_7"));
    assert_eq!(stats.dropped_by_reason["writer_unavailable"], 50);
    assert_eq!(
        stats.trace_drop_reason.as_deref(),
        Some("writer_unavailable")
    );
    lock_inner(&shared).writer_alive = true;
}

#[test]
fn request_log_payload_queue_retries_unavailable_storage_without_dropping() {
    let storage = storage();
    let handle = storage.shared_handle();
    let failures_left = Arc::new(Mutex::new(6_u64));
    let attempts = Arc::new(TestCounter::new(0));
    let open: OpenStorageFn = {
        let failures_left = failures_left.clone();
        let attempts = attempts.clone();
        Arc::new(move || {
            attempts.fetch_add(1, Ordering::SeqCst);
            let failing = {
                let mut left = failures_left.lock().unwrap();
                let failing = *left > 0;
                *left = left.saturating_sub(1);
                failing
            };
            if failing {
                None
            } else {
                Some(Box::new(Box::new(handle.shared_handle())) as StorageRef)
            }
        })
    };
    let first = job("trc_unavail_1", "{\"input\":\"1\"}", true);
    let per_job = job_accounted_bytes(&first);
    let pipeline = start_with_open(
        open,
        1 << 20,
        SpillSetup::Disabled(DropReason::IoError),
        private_clear_state(),
        Some(Arc::new(|| {}) as HookFn),
        None,
        None,
    );
    let shared = pipeline.shared.clone();
    shared.submit(first);
    shared.submit(job("trc_unavail_2", "{\"input\":\"2\"}", false));
    wait_until("several failed acquisitions", || {
        shared.snapshot_stats(None).storage_unavailable_retries >= 3
    });
    let stats = shared.snapshot_stats(None);
    assert_eq!(stats.dropped_total, 0, "{stats:?}");
    assert_eq!(stats.written_total, 0);
    assert!(stats.queued_bytes >= per_job, "accounting kept: {stats:?}");
    wait_until("written once the storage recovers", || {
        shared.snapshot_stats(None).written_total == 2
    });
    let stats = shared.snapshot_stats(None);
    assert_eq!(stats.dropped_total, 0, "{stats:?}");
    assert_eq!(stats.queued_bytes, 0, "accounting released after the write");
    assert!(stats.storage_unavailable_retries >= 3);
    assert_eq!(*failures_left.lock().unwrap(), 0);
    assert!(attempts.load(Ordering::SeqCst) > 6);
    assert!(stored(&storage, "trc_unavail_1"));
    assert!(stored(&storage, "trc_unavail_2"));
    assert_eq!(
        shared
            .snapshot_stats(Some("trc_unavail_1"))
            .trace_drop_reason,
        None
    );
}

fn redacting_upstream_job(
    trace_id: &str,
    body: &str,
    preview: bool,
    wire: Bytes,
) -> RequestLogPayloadJob {
    let mut job = job(trace_id, body, preview);
    job.stage = PAYLOAD_STAGE_UPSTREAM.to_string();
    job.redact = true;
    job.attempt = Some(OutboundAttemptCapture {
        method: "POST".to_string(),
        url: "https://upstream.invalid/v1/responses".to_string(),
        transport: "http".to_string(),
        content_encoding: Some("zstd".to_string()),
        wire_body: wire,
    });
    job
}

#[test]
fn request_log_payload_queue_redacts_spilled_jobs_before_they_reach_the_disk() {
    let storage = storage();
    let dir = TempDir::new("redact");
    let first = job("trc_red_a", "{\"input\":\"a\"}", true);
    let budget_bytes = job_accounted_bytes(&first);
    let pipeline = start(
        &storage,
        budget_bytes,
        SpillSetup::Opened(SpillStore::open(&dir.0, SEGMENT_MAX_BYTES).unwrap()),
        private_clear_state(),
        None,
        None,
    );
    let shared = pipeline.shared.clone();
    // Fills the memory budget; the gated writer holds it, so the next jobs
    // spill.
    shared.submit(first);
    let body = r#"{"model":"m","api_key":"sk-spill-body-secret","input":[{"role":"user","content":"hi","authorization":"Bearer sk-spill-item-secret"}]}"#;
    // A separate wire body (e.g. compressed) that also holds the secret.
    let wire = Bytes::from_static(b"\x28\xb5\x2f\xfd compressed sk-spill-wire-secret");
    shared.submit(redacting_upstream_job(
        "trc_red_preview",
        body,
        true,
        wire.clone(),
    ));
    wait_until("preview job spilled", || {
        shared.snapshot_stats(None).spilled_total == 1
    });
    shared.submit(redacting_upstream_job(
        "trc_red_full",
        body,
        false,
        wire.clone(),
    ));
    wait_until("full job spilled", || {
        shared.snapshot_stats(None).spilled_total == 2
    });
    for secret in [
        &b"sk-spill-body-secret"[..],
        b"sk-spill-item-secret",
        b"sk-spill-wire-secret",
    ] {
        assert!(
            !spill_files_contain(&dir, secret),
            "{} reached the spill files",
            String::from_utf8_lossy(secret)
        );
    }
    assert!(spill_files_contain(&dir, b"[REDACTED]"));

    pipeline.gate.grant(100);
    wait_until("spilled jobs replayed", || {
        shared.snapshot_stats(None).written_total == 3
    });
    let preview = storage
        .find_request_log_payload_by_trace_id("trc_red_preview", PAYLOAD_STAGE_UPSTREAM)
        .unwrap()
        .expect("preview replayed");
    assert!(preview.redacted);
    assert!(!preview.payload.contains("sk-spill"), "{}", preview.payload);
    assert!(preview.payload.contains("[REDACTED]"));
    assert_eq!(preview.body_hash, bytes_hash(body.as_bytes()));
    assert_eq!(preview.payload_bytes, body.len() as i64);
    let full = storage
        .load_request_log_payload_full("trc_red_full", PAYLOAD_STAGE_UPSTREAM)
        .unwrap()
        .expect("manifest replayed");
    let stored_text = format!("{:?}{:?}", full.fields, full.items);
    assert!(!stored_text.contains("sk-spill"), "{stored_text}");
    assert!(stored_text.contains("[REDACTED]"));
    assert_eq!(full.manifest.body_hash, bytes_hash(body.as_bytes()));
    assert_eq!(full.manifest.payload_bytes, body.len() as i64);
    for trace_id in ["trc_red_preview", "trc_red_full"] {
        let attempt = storage
            .find_request_log_upstream_attempt(trace_id, PAYLOAD_STAGE_UPSTREAM)
            .unwrap()
            .expect("attempt replayed");
        assert_eq!(attempt.wire_sha256, bytes_hash(&wire), "{trace_id}");
    }
    wait_until("replayed segments deleted", || segments(&dir) == 0);
}
