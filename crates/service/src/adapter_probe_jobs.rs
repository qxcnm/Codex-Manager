use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        Arc, Mutex, OnceLock,
    },
    thread,
    time::{Duration, Instant},
};

use codexmanager_core::storage::now_ts;

const MAX_JOB_ITEMS: usize = 10_000;
const MAX_RETAINED_JOBS: usize = 50;
const MAX_RETAINED_RESULTS: usize = 2_000;

type ProbeFn = dyn Fn(&str, &str) -> Result<(), String> + Send + Sync + 'static;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AdapterProbeJobItemResult {
    pub credential_id: String,
    pub status: String,
    pub error_code: Option<String>,
    pub attempts: usize,
    pub latency_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AdapterProbeJobSnapshot {
    pub id: String,
    pub pool_id: String,
    pub status: String,
    pub requested: usize,
    pub completed: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub cancelled: usize,
    pub concurrency: usize,
    pub created_at: i64,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
    pub results: Vec<AdapterProbeJobItemResult>,
}

struct AdapterProbeJobEntry {
    snapshot: AdapterProbeJobSnapshot,
    cancel: Arc<AtomicBool>,
    credential_ids: HashSet<String>,
}

static JOBS: OnceLock<Mutex<HashMap<String, AdapterProbeJobEntry>>> = OnceLock::new();
static NEXT_JOB_ID: AtomicU64 = AtomicU64::new(1);

fn jobs() -> &'static Mutex<HashMap<String, AdapterProbeJobEntry>> {
    JOBS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn lock_jobs() -> std::sync::MutexGuard<'static, HashMap<String, AdapterProbeJobEntry>> {
    jobs()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn normalized_pool_id(pool_id: &str) -> Result<&'static str, String> {
    match pool_id.trim().to_ascii_lowercase().as_str() {
        "codex" | "gpt" | "openai" => Ok("codex"),
        "kiro" => Ok("kiro"),
        "grok" => Ok("grok"),
        _ => Err("adapter_probe_pool_not_supported".into()),
    }
}

fn concurrency_limits(pool_id: &str) -> (usize, usize) {
    match pool_id {
        "codex" => (4, 8),
        "kiro" => (3, 6),
        "grok" => (2, 4),
        _ => (1, 1),
    }
}

fn dedupe_ids(ids: Vec<String>) -> Result<Vec<String>, String> {
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for id in ids {
        let id = id.trim();
        if id.is_empty() || !seen.insert(id.to_string()) {
            continue;
        }
        result.push(id.to_string());
        if result.len() > MAX_JOB_ITEMS {
            return Err("adapter_probe_job_too_large".into());
        }
    }
    if result.is_empty() {
        return Err("adapter_probe_credentials_required".into());
    }
    Ok(result)
}

pub(crate) fn start_adapter_probe_job(
    pool_id: &str,
    credential_ids: Vec<String>,
    requested_concurrency: Option<usize>,
) -> Result<AdapterProbeJobSnapshot, String> {
    start_adapter_probe_job_with(
        pool_id,
        credential_ids,
        requested_concurrency,
        Arc::new(probe_one),
    )
}

fn start_adapter_probe_job_with(
    pool_id: &str,
    credential_ids: Vec<String>,
    requested_concurrency: Option<usize>,
    probe: Arc<ProbeFn>,
) -> Result<AdapterProbeJobSnapshot, String> {
    let pool_id = normalized_pool_id(pool_id)?;
    let credential_ids = dedupe_ids(credential_ids)?;
    let (default_concurrency, max_concurrency) = concurrency_limits(pool_id);
    let concurrency = requested_concurrency
        .unwrap_or(default_concurrency)
        .clamp(1, max_concurrency)
        .min(credential_ids.len());

    let now = now_ts();
    let sequence = NEXT_JOB_ID.fetch_add(1, Ordering::Relaxed);
    let job_id = format!("probe-{pool_id}-{now}-{sequence}");
    let snapshot = AdapterProbeJobSnapshot {
        id: job_id.clone(),
        pool_id: pool_id.to_string(),
        status: "running".into(),
        requested: credential_ids.len(),
        completed: 0,
        succeeded: 0,
        failed: 0,
        cancelled: 0,
        concurrency,
        created_at: now,
        started_at: Some(now),
        finished_at: None,
        results: Vec::new(),
    };
    let cancel = Arc::new(AtomicBool::new(false));
    let requested_ids = credential_ids.iter().cloned().collect::<HashSet<_>>();
    {
        let mut guard = lock_jobs();
        prune_jobs(&mut guard);
        if let Some(existing) = guard.values().find(|entry| {
            entry.snapshot.pool_id == pool_id
                && matches!(
                    entry.snapshot.status.as_str(),
                    "queued" | "running" | "cancelling"
                )
        }) {
            if requested_ids.is_subset(&existing.credential_ids) {
                return Ok(existing.snapshot.clone());
            }
            return Err("adapter_probe_job_already_running".into());
        }
        guard.insert(
            job_id.clone(),
            AdapterProbeJobEntry {
                snapshot: snapshot.clone(),
                cancel: cancel.clone(),
                credential_ids: requested_ids,
            },
        );
    }

    let queue = Arc::new(Mutex::new(VecDeque::from(credential_ids)));
    let active_workers = Arc::new(AtomicUsize::new(concurrency));
    for worker_index in 0..concurrency {
        let queue = queue.clone();
        let cancel = cancel.clone();
        let worker_counter = active_workers.clone();
        let probe = probe.clone();
        let job_id = job_id.clone();
        let pool_id = pool_id.to_string();
        let spawn_failure_job_id = job_id.clone();
        let spawn_failure_cancel = cancel.clone();
        let spawned = thread::Builder::new()
            .name(format!("openruntime-probe-{pool_id}-{worker_index}"))
            .spawn(move || {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    run_worker(&job_id, &pool_id, queue, cancel.clone(), probe);
                }));
                if worker_counter.fetch_sub(1, Ordering::AcqRel) == 1 {
                    finish_job(&job_id, cancel.load(Ordering::Acquire));
                }
            });
        if spawned.is_err() && active_workers.fetch_sub(1, Ordering::AcqRel) == 1 {
            finish_job(
                &spawn_failure_job_id,
                spawn_failure_cancel.load(Ordering::Acquire),
            );
        }
    }
    Ok(snapshot)
}

fn run_worker(
    job_id: &str,
    pool_id: &str,
    queue: Arc<Mutex<VecDeque<String>>>,
    cancel: Arc<AtomicBool>,
    probe: Arc<ProbeFn>,
) {
    loop {
        if cancel.load(Ordering::Acquire) {
            return;
        }
        let credential_id = queue
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .pop_front();
        let Some(credential_id) = credential_id else {
            return;
        };
        if cancel.load(Ordering::Acquire) {
            return;
        }
        let started = Instant::now();
        let mut attempts = 1;
        let mut result = probe(pool_id, &credential_id);
        if result.as_ref().is_err_and(|error| retryable_error(error))
            && !cancel.load(Ordering::Acquire)
        {
            thread::sleep(Duration::from_millis(250));
            if !cancel.load(Ordering::Acquire) {
                attempts = 2;
                result = probe(pool_id, &credential_id);
            }
        }
        record_item_result(
            job_id,
            AdapterProbeJobItemResult {
                credential_id,
                status: if result.is_ok() {
                    "available"
                } else {
                    "failed"
                }
                .into(),
                error_code: result.err().map(|error| classify_error(&error)),
                attempts,
                latency_ms: started.elapsed().as_millis().min(u64::MAX as u128) as u64,
            },
        );
    }
}

fn probe_one(pool_id: &str, credential_id: &str) -> Result<(), String> {
    match pool_id {
        "codex" => crate::usage_refresh::probe_codex_account_availability(credential_id),
        "kiro" => {
            let storage = crate::storage_helpers::open_storage()
                .ok_or_else(|| "storage_unavailable".to_string())?;
            let summary = crate::kiro::runtime::probe_credential_models(&storage, credential_id)?;
            if summary.available_models.is_empty() {
                Err("no_available_kiro_model".into())
            } else {
                Ok(())
            }
        }
        "grok" => {
            let storage = crate::storage_helpers::open_storage()
                .ok_or_else(|| "storage_unavailable".to_string())?;
            let summary = crate::grok::runtime::probe_credential_models(&storage, credential_id)?;
            if summary.available_models.is_empty() {
                Err("no_available_grok_model".into())
            } else {
                Ok(())
            }
        }
        _ => Err("adapter_probe_pool_not_supported".into()),
    }
}

fn retryable_error(error: &str) -> bool {
    let error = error.to_ascii_lowercase();
    error.contains("timeout")
        || error.contains("timed out")
        || error.contains("429")
        || error.contains("rate_limited")
        || error.contains("http_5")
        || error.contains("server_error")
        || error.contains("temporarily_unavailable")
}

fn classify_error(error: &str) -> String {
    let normalized = error.to_ascii_lowercase();
    if normalized.contains("timeout") || normalized.contains("timed out") {
        "timeout".into()
    } else if normalized.contains("429") || normalized.contains("rate_limited") {
        "rate_limited".into()
    } else if normalized.contains("unauthorized")
        || normalized.contains("forbidden")
        || normalized.contains("invalid_grant")
        || normalized.contains("not_active")
    {
        "credential_invalid".into()
    } else if normalized.contains("quota") || normalized.contains("exhausted") {
        "quota_exhausted".into()
    } else if normalized.contains("http_5") || normalized.contains("server_error") {
        "upstream_error".into()
    } else if normalized.contains("no_available") || normalized.contains("unsupported") {
        "no_available_model".into()
    } else {
        "probe_failed".into()
    }
}

fn record_item_result(job_id: &str, result: AdapterProbeJobItemResult) {
    let mut guard = lock_jobs();
    let Some(entry) = guard.get_mut(job_id) else {
        return;
    };
    entry.snapshot.completed = entry.snapshot.completed.saturating_add(1);
    if result.error_code.is_none() {
        entry.snapshot.succeeded = entry.snapshot.succeeded.saturating_add(1);
    } else {
        entry.snapshot.failed = entry.snapshot.failed.saturating_add(1);
    }
    if entry.snapshot.results.len() < MAX_RETAINED_RESULTS {
        entry.snapshot.results.push(result);
    }
}

fn finish_job(job_id: &str, was_cancelled: bool) {
    let mut guard = lock_jobs();
    let Some(entry) = guard.get_mut(job_id) else {
        return;
    };
    entry.snapshot.cancelled = entry
        .snapshot
        .requested
        .saturating_sub(entry.snapshot.completed);
    entry.snapshot.status = if was_cancelled {
        "cancelled".into()
    } else {
        "completed".into()
    };
    entry.snapshot.finished_at = Some(now_ts());
}

fn prune_jobs(guard: &mut HashMap<String, AdapterProbeJobEntry>) {
    if guard.len() < MAX_RETAINED_JOBS {
        return;
    }
    let mut completed = guard
        .iter()
        .filter(|(_, entry)| matches!(entry.snapshot.status.as_str(), "completed" | "cancelled"))
        .map(|(id, entry)| (id.clone(), entry.snapshot.created_at))
        .collect::<Vec<_>>();
    completed.sort_by_key(|(_, created_at)| *created_at);
    for (id, _) in completed
        .into_iter()
        .take(guard.len().saturating_sub(MAX_RETAINED_JOBS - 1))
    {
        guard.remove(&id);
    }
}

pub(crate) fn get_adapter_probe_job(job_id: &str) -> Result<AdapterProbeJobSnapshot, String> {
    lock_jobs()
        .get(job_id)
        .map(|entry| entry.snapshot.clone())
        .ok_or_else(|| "adapter_probe_job_not_found".into())
}

pub(crate) fn latest_adapter_probe_job(
    pool_id: &str,
) -> Result<Option<AdapterProbeJobSnapshot>, String> {
    let pool_id = normalized_pool_id(pool_id)?;
    Ok(lock_jobs()
        .values()
        .filter(|entry| entry.snapshot.pool_id == pool_id)
        .max_by_key(|entry| (entry.snapshot.created_at, entry.snapshot.id.clone()))
        .map(|entry| entry.snapshot.clone()))
}

pub(crate) fn cancel_adapter_probe_job(job_id: &str) -> Result<AdapterProbeJobSnapshot, String> {
    let mut guard = lock_jobs();
    let entry = guard
        .get_mut(job_id)
        .ok_or_else(|| "adapter_probe_job_not_found".to_string())?;
    if matches!(entry.snapshot.status.as_str(), "running" | "queued") {
        entry.cancel.store(true, Ordering::Release);
        entry.snapshot.status = "cancelling".into();
    }
    Ok(entry.snapshot.clone())
}

#[cfg(test)]
pub(crate) fn clear_adapter_probe_jobs_for_tests() {
    lock_jobs().clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wait_for_terminal(id: &str) -> AdapterProbeJobSnapshot {
        for _ in 0..200 {
            let snapshot = get_adapter_probe_job(id).unwrap();
            if matches!(snapshot.status.as_str(), "completed" | "cancelled") {
                return snapshot;
            }
            thread::sleep(Duration::from_millis(5));
        }
        panic!("probe job did not finish");
    }

    #[test]
    fn job_limits_concurrency_and_isolates_item_failures() {
        let _guard = crate::test_env_guard();
        clear_adapter_probe_jobs_for_tests();
        let current = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let probe: Arc<ProbeFn> = {
            let current = current.clone();
            let peak = peak.clone();
            Arc::new(move |_, id| {
                let active = current.fetch_add(1, Ordering::AcqRel) + 1;
                peak.fetch_max(active, Ordering::AcqRel);
                thread::sleep(Duration::from_millis(10));
                current.fetch_sub(1, Ordering::AcqRel);
                if id == "bad" {
                    Err("credential_unauthorized".into())
                } else {
                    Ok(())
                }
            })
        };
        let job = start_adapter_probe_job_with(
            "kiro",
            vec!["one".into(), "bad".into(), "two".into(), "three".into()],
            Some(2),
            probe,
        )
        .unwrap();
        let done = wait_for_terminal(&job.id);
        assert_eq!(done.status, "completed");
        assert_eq!((done.requested, done.completed), (4, 4));
        assert_eq!((done.succeeded, done.failed), (3, 1));
        assert!(peak.load(Ordering::Acquire) <= 2);
    }

    #[test]
    fn cancellation_stops_queued_items() {
        let _guard = crate::test_env_guard();
        clear_adapter_probe_jobs_for_tests();
        let probe: Arc<ProbeFn> = Arc::new(|_, _| {
            thread::sleep(Duration::from_millis(40));
            Ok(())
        });
        let job = start_adapter_probe_job_with(
            "grok",
            (0..20).map(|value| value.to_string()).collect(),
            Some(1),
            probe,
        )
        .unwrap();
        thread::sleep(Duration::from_millis(10));
        cancel_adapter_probe_job(&job.id).unwrap();
        let done = wait_for_terminal(&job.id);
        assert_eq!(done.status, "cancelled");
        assert!(done.completed < done.requested);
        assert_eq!(done.cancelled, done.requested - done.completed);
    }

    #[test]
    fn running_job_only_reuses_requests_it_already_covers() {
        let _guard = crate::test_env_guard();
        clear_adapter_probe_jobs_for_tests();
        let probe: Arc<ProbeFn> = Arc::new(|_, _| {
            thread::sleep(Duration::from_millis(80));
            Ok(())
        });
        let job = start_adapter_probe_job_with(
            "codex",
            vec!["one".into(), "two".into()],
            Some(1),
            probe.clone(),
        )
        .unwrap();
        let reused =
            start_adapter_probe_job_with("codex", vec!["one".into()], Some(1), probe.clone())
                .unwrap();
        assert_eq!(reused.id, job.id);

        let conflict = start_adapter_probe_job_with("codex", vec!["three".into()], Some(1), probe)
            .unwrap_err();
        assert_eq!(conflict, "adapter_probe_job_already_running");
        cancel_adapter_probe_job(&job.id).unwrap();
        let _ = wait_for_terminal(&job.id);
    }
}
