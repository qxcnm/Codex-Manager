use super::{store, types::Source};
use codexmanager_core::{rpc::types::AggregateApiAssociateModelsResult, storage::now_ts};
use std::{
    collections::{BTreeMap, HashMap},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, LazyLock, Mutex, MutexGuard,
    },
    time::Duration,
};

static STARTED: AtomicBool = AtomicBool::new(false);
static CHANGED: tokio::sync::Notify = tokio::sync::Notify::const_new();
static CONTROL: LazyLock<Mutex<Control>> = LazyLock::new(|| Mutex::new(Control::default()));
const MAX_CONCURRENT: usize = 2;

#[derive(Default)]
pub(super) struct Control {
    pub active: HashMap<String, Active>,
    pub cooldowns: HashMap<String, i64>,
}

pub(super) struct Active {
    pub cancelled: Arc<AtomicBool>,
}

pub(super) fn control() -> MutexGuard<'static, Control> {
    crate::lock_utils::lock_recover(&CONTROL, "model_discovery_control")
}

pub(super) fn wake() {
    CHANGED.notify_one();
}

struct SchedulerLease;
impl Drop for SchedulerLease {
    fn drop(&mut self) {
        STARTED.store(false, Ordering::Release);
    }
}

pub(super) struct RunLease {
    pub source: Source,
    pub generation: u64,
    pub cancelled: Arc<AtomicBool>,
    finished: AtomicBool,
}

impl Drop for RunLease {
    fn drop(&mut self) {
        let mut control = control();
        let key = self.source.key();
        if control
            .active
            .get(&key)
            .is_some_and(|active| Arc::ptr_eq(&active.cancelled, &self.cancelled))
        {
            control.active.remove(&key);
            if !self.finished.load(Ordering::Acquire)
                && !self.cancelled.load(Ordering::Acquire)
                && !crate::shutdown_requested()
            {
                // A failed completion write must not turn into another upstream request.
                control.cooldowns.insert(key, now_ts().saturating_add(300));
            }
        }
        wake();
    }
}

pub(crate) fn ensure_model_discovery() {
    if STARTED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }
    let lease = SchedulerLease;
    if crate::account::background::spawn("model-discovery-scheduler", async move {
        let _lease = lease;
        loop {
            match crate::runtime::blocking::run("model-discovery-claim", claim_due).await {
                Ok(Ok(runs)) => {
                    for run in runs {
                        if crate::account::background::spawn(
                            "model-discovery-source",
                            run_task(run),
                        )
                        .is_err()
                        {
                            log::warn!("event=model_discovery_admission_failed");
                        }
                    }
                }
                _ => log::warn!("event=model_discovery_schedule_storage_failed"),
            }
            tokio::select! {
                _ = CHANGED.notified() => {},
                _ = tokio::time::sleep(Duration::from_secs(5)) => {},
            }
        }
    })
    .is_err()
    {
        log::warn!("event=model_discovery_scheduler_start_failed");
    }
}

/// Read metadata only. No tokens or provider secrets are needed to decide eligibility.
pub(super) fn check_source(source: &Source) -> Result<(), String> {
    let handle = crate::storage_helpers::open_storage()
        .ok_or_else(|| "source storage unavailable".to_owned())?;
    let storage = crate::account::remote_storage::AccountStorage::new(&handle);
    let status = match source.source_kind.as_str() {
        "openai_account" => storage.find_account_status_by_id(&source.source_id),
        "aggregate_api" => storage.find_aggregate_api_status_by_id(&source.source_id),
        _ => return Err("unsupported model discovery sourceKind".into()),
    }
    .map_err(|_| "source status could not be read".to_owned())?
    .ok_or_else(|| "model discovery source no longer exists".to_owned())?;
    if matches!(
        status.trim().to_ascii_lowercase().as_str(),
        "disabled" | "inactive"
    ) {
        return Err("model discovery source is disabled".into());
    }
    Ok(())
}

fn unavailable_source(error: &str) -> bool {
    matches!(
        error,
        "model discovery source no longer exists" | "model discovery source is disabled"
    )
}

pub(super) fn claim_due() -> Result<Vec<RunLease>, String> {
    let mut control = control();
    let now = now_ts();
    control.cooldowns.retain(|_, retry_at| *retry_at > now);
    let mut claimed = Vec::new();
    for mut record in store::load()? {
        let key = record.config.source.key();
        if !record.config.enabled {
            continue;
        }
        if let Err(error) = check_source(&record.config.source) {
            if unavailable_source(&error) {
                if record.update(false, None, now).is_err() {
                    continue;
                }
                record.status.last_error = Some(error);
                record.status.discovered_count = 0;
                record.status.created_model_count = 0;
                record.status.added_route_count = 0;
                if store::save(&record, true).is_err() {
                    continue;
                }
                if let Some(active) = control.active.get(&key) {
                    active.cancelled.store(true, Ordering::Release);
                }
            }
            // Transient storage errors are not evidence the source was removed.
            continue;
        }
        if control.active.contains_key(&key) {
            continue;
        }
        if control.cooldowns.contains_key(&key) {
            continue;
        }
        if record.status.running {
            record.status.running = false;
            record.status.last_finished_at = Some(now);
            record.status.last_warning =
                Some("previous model discovery was interrupted; retrying".into());
            record.status.next_run_at = Some(now);
        }
        if record.status.next_run_at.is_some_and(|next| next > now) {
            continue;
        }
        if control.active.len() >= MAX_CONCURRENT {
            continue;
        }
        record.status.running = true;
        record.status.last_started_at = Some(now);
        record.status.last_error = None;
        record.status.next_run_at = None;
        if store::save(&record, false).is_err() {
            log::warn!("event=model_discovery_claim_storage_failed");
            continue;
        }
        let cancelled = Arc::new(AtomicBool::new(false));
        control.active.insert(
            key,
            Active {
                cancelled: cancelled.clone(),
            },
        );
        claimed.push(RunLease {
            source: record.config.source,
            generation: record.config.generation,
            cancelled,
            finished: AtomicBool::new(false),
        });
    }
    Ok(claimed)
}

#[derive(Default)]
pub(super) struct Fetched {
    pub models: Vec<String>,
    pub names: BTreeMap<String, String>,
}

async fn fetch(source: &Source) -> Result<Fetched, String> {
    let items: Vec<(String, Option<String>)> = if source.source_kind == "openai_account" {
        crate::account::models::fetch_account_models_async(&source.source_id)
            .await
            .map_err(|_| {
                "model discovery fetch failed; check source authorization and connectivity"
                    .to_owned()
            })?
            .items
            .into_iter()
            .map(|item| (item.upstream_model, item.display_name))
            .collect()
    } else {
        crate::aggregate_api::fetch_aggregate_api_models_async(&source.source_id)
            .await
            .map_err(|_| {
                "model discovery fetch failed; check source authorization and connectivity"
                    .to_owned()
            })?
            .items
            .into_iter()
            .map(|item| (item.upstream_model, item.display_name))
            .collect()
    };
    let mut fetched = Fetched::default();
    for (model, name) in items {
        if let Some(name) = name {
            fetched.names.insert(model.clone(), name);
        }
        fetched.models.push(model);
    }
    Ok(fetched)
}

async fn cancelled(flag: &AtomicBool) {
    while !flag.load(Ordering::Acquire) {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

pub(super) async fn fetch_until_cancelled(
    flag: &AtomicBool,
    future: impl std::future::Future<Output = Result<Fetched, String>>,
) -> Option<Result<Fetched, String>> {
    if flag.load(Ordering::Acquire) {
        return None;
    }
    tokio::select! {
        biased;
        _ = cancelled(flag) => None,
        result = tokio::time::timeout(Duration::from_secs(90), future) => {
            Some(result.unwrap_or_else(|_| Err("model discovery fetch timed out".into())))
        }
    }
}

async fn run_task(run: RunLease) {
    let source = run.source.clone();
    let ready =
        crate::runtime::blocking::run("model-discovery-preflight", move || check_source(&source))
            .await;
    if !matches!(ready, Ok(Ok(()))) {
        let _ = crate::runtime::blocking::run("model-discovery-source-unavailable", move || {
            complete(
                &run,
                Err("model discovery source could not be checked".into()),
            );
        })
        .await;
        return;
    }
    let Some(result) = fetch_until_cancelled(&run.cancelled, fetch(&run.source)).await else {
        return;
    };
    if crate::runtime::blocking::run("model-discovery-complete", move || complete(&run, result))
        .await
        .is_err()
    {
        log::warn!("event=model_discovery_completion_worker_failed");
    }
}

fn associate(
    source: &Source,
    fetched: Fetched,
) -> Result<AggregateApiAssociateModelsResult, String> {
    if source.source_kind == "openai_account" {
        crate::account::models::associate_account_models(
            &source.source_id,
            fetched.models,
            fetched.names,
        )
    } else {
        crate::aggregate_api::associate_aggregate_api_models(
            &source.source_id,
            fetched.models,
            fetched.names,
        )
    }
    .map_err(|_| "model discovery catalog update failed".to_owned())
}

fn sync_profile() -> Result<(), String> {
    let storage =
        crate::storage_helpers::open_storage().ok_or_else(|| "storage unavailable".to_owned())?;
    crate::codex_profile::sync_active_gateway_profile_after_model_changes(&storage, Vec::new())
        .map(|_| ())
        .map_err(|_| "models were saved but the active Codex profile could not be refreshed".into())
}

fn complete(run: &RunLease, result: Result<Fetched, String>) {
    if complete_with(run, result, associate, sync_profile).is_err() {
        log::warn!("event=model_discovery_completion_storage_failed");
    }
}

/// The same lock orders disable/update and the final generation check + catalog write.
/// Once a disable RPC returns, no old generation can begin another catalog write.
pub(super) fn complete_with(
    run: &RunLease,
    result: Result<Fetched, String>,
    associate: impl FnOnce(&Source, Fetched) -> Result<AggregateApiAssociateModelsResult, String>,
    sync_profile: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    let _control = control();
    if run.cancelled.load(Ordering::Acquire) || crate::shutdown_requested() {
        return Ok(());
    }
    let Some(mut record) = store::load()?
        .into_iter()
        .find(|record| record.config.source == run.source)
    else {
        return Ok(());
    };
    if !record.config.enabled || record.config.generation != run.generation {
        return Ok(());
    }
    let now = now_ts();
    if let Err(error) = check_source(&run.source) {
        if unavailable_source(&error) {
            record.update(false, None, now)?;
        }
        record.status.discovered_count = 0;
        record.finish_error(&error, now);
        let saved = store::save(&record, true);
        if saved.is_ok() {
            run.finished.store(true, Ordering::Release);
        }
        return saved;
    }
    match result {
        Err(error) => {
            record.status.discovered_count = 0;
            record.finish_error(
                if error == "model discovery fetch timed out" {
                    "model discovery fetch timed out"
                } else {
                    "model discovery fetch failed; check source authorization and connectivity"
                },
                now,
            );
        }
        Ok(fetched) => {
            record.status.discovered_count = fetched.models.len();
            match associate(&run.source, fetched) {
                Err(_) => record.finish_error("model discovery catalog update failed", now),
                Ok(result) => {
                    record.status.running = false;
                    record.status.last_finished_at = Some(now);
                    record.status.last_success_at = Some(now);
                    record.status.next_run_at =
                        Some(now.saturating_add((record.config.interval_minutes * 60) as i64));
                    record.status.last_error = None;
                    record.status.last_warning = None;
                    record.status.consecutive_failures = 0;
                    record.status.created_model_count = result.created_models.len();
                    record.status.added_route_count = result.added_routes.len();
                    if (!result.created_models.is_empty() || !result.added_routes.is_empty())
                        && sync_profile().is_err()
                    {
                        record.status.last_warning = Some(
                            "models were saved but the active Codex profile could not be refreshed"
                                .into(),
                        );
                    }
                }
            }
        }
    }
    let saved = store::save(&record, false);
    if saved.is_ok() {
        run.finished.store(true, Ordering::Release);
    }
    saved
}
