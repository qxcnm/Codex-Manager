//! Background SQLite storage maintenance.
//!
//! Retention pruning, batched purges of cleared request payloads, WAL
//! checkpoint retries and incremental space reclamation run on this dedicated
//! thread instead of the gateway request path, which previously executed the
//! retention prune inline while writing a request log. Remote (SeaORM)
//! storage keeps its own maintenance and is skipped here.

use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

use codexmanager_core::storage::{
    now_ts, wal_checkpoint_pending, DatabaseSpaceUsage, AUTO_VACUUM_FULL, AUTO_VACUUM_INCREMENTAL,
};
use serde_json::{json, Value};

use crate::storage_helpers::{open_storage, seaorm_enabled};

const TICK: Duration = Duration::from_secs(30);
const PURGE_BUDGET_PER_TICK: Duration = Duration::from_secs(5);
const AUTO_RECLAIM_BUDGET_PER_TICK: Duration = Duration::from_secs(5);
/// Automatic reclamation only starts when at least this much is free...
const AUTO_RECLAIM_MIN_FREE_BYTES: i64 = 64 * 1024 * 1024;
/// ...and free pages make up at least this share of the file.
const AUTO_RECLAIM_MIN_FREE_PERCENT: i64 = 10;
const RECLAIM_INITIAL_PAGES: i64 = 256;
const RECLAIM_MIN_PAGES: i64 = 16;
const RECLAIM_MAX_PAGES: i64 = 8192;
/// Each `incremental_vacuum` step holds the write lock, keep it short.
const RECLAIM_TARGET_STEP: Duration = Duration::from_millis(50);
const RECLAIM_PAUSE: Duration = Duration::from_millis(200);
const REBUILD_DISK_MARGIN_BYTES: u64 = 256 * 1024 * 1024;

static STARTED: AtomicBool = AtomicBool::new(false);
static RECLAIM_REQUESTED: AtomicBool = AtomicBool::new(false);
static RECLAIM_RUNNING: AtomicBool = AtomicBool::new(false);
static REBUILD_RUNNING: AtomicBool = AtomicBool::new(false);

fn wake_signal() -> &'static (Mutex<bool>, Condvar) {
    static WAKE: OnceLock<(Mutex<bool>, Condvar)> = OnceLock::new();
    WAKE.get_or_init(|| (Mutex::new(false), Condvar::new()))
}

fn last_error() -> &'static Mutex<Option<String>> {
    static LAST_ERROR: OnceLock<Mutex<Option<String>>> = OnceLock::new();
    LAST_ERROR.get_or_init(|| Mutex::new(None))
}

fn set_last_error(error: Option<String>) {
    *last_error()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = error;
}

fn read_last_error() -> Option<String> {
    last_error()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

/// Resets a running flag even when the guarded work panics.
struct FlagGuard(&'static AtomicBool);

impl Drop for FlagGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// Start the maintenance thread once. Safe to call repeatedly.
pub(crate) fn ensure_storage_maintenance() {
    if seaorm_enabled() {
        return;
    }
    if STARTED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("storage-maintenance".to_string())
        .spawn(run_maintenance_loop);
    if let Err(err) = spawned {
        STARTED.store(false, Ordering::Release);
        log::warn!("event=storage_maintenance_spawn_failed err={err}");
    }
}

/// When the background thread runs, the request path skips inline
/// retention maintenance.
pub(crate) fn background_maintenance_active() -> bool {
    STARTED.load(Ordering::Acquire)
}

pub(crate) fn wake_storage_maintenance() {
    let (lock, signal) = wake_signal();
    *lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = true;
    signal.notify_all();
}

fn run_maintenance_loop() {
    loop {
        if std::panic::catch_unwind(run_maintenance_tick).is_err() {
            log::error!("event=storage_maintenance_tick_panicked");
        }
        let (lock, signal) = wake_signal();
        let mut woken = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if !*woken {
            woken = match signal.wait_timeout(woken, TICK) {
                Ok((guard, _)) => guard,
                Err(poisoned) => poisoned.into_inner().0,
            };
        }
        *woken = false;
    }
}

fn run_maintenance_tick() {
    if seaorm_enabled() {
        return;
    }
    if let Some(storage) = open_storage() {
        if let Err(err) = storage.maybe_run_observability_maintenance(now_ts()) {
            log::warn!("event=storage_maintenance_retention_failed err={err}");
        }
        match storage.drain_request_log_payload_purges(Some(PURGE_BUDGET_PER_TICK)) {
            Ok(progress) if progress.deleted_rows > 0 || progress.pending => log::info!(
                "event=storage_maintenance_payload_purge deleted_rows={} pending={}",
                progress.deleted_rows,
                progress.pending
            ),
            Ok(_) => {}
            Err(err) => log::warn!("event=storage_maintenance_payload_purge_failed err={err}"),
        }
        if wal_checkpoint_pending() {
            match storage.retry_pending_wal_checkpoint() {
                Ok(true) => log::info!("event=storage_maintenance_wal_checkpoint_completed"),
                Ok(false) => log::debug!("event=storage_maintenance_wal_checkpoint_busy"),
                Err(err) => log::warn!("event=storage_maintenance_wal_checkpoint_failed err={err}"),
            }
        }
    }
    reclaim_free_pages();
}

fn auto_reclaim_worthwhile(usage: &DatabaseSpaceUsage) -> bool {
    usage.reclaimable_bytes >= AUTO_RECLAIM_MIN_FREE_BYTES
        && usage.freelist_count.saturating_mul(100)
            >= usage
                .page_count
                .saturating_mul(AUTO_RECLAIM_MIN_FREE_PERCENT)
}

fn next_step_pages(pages: i64, elapsed: Duration) -> i64 {
    if elapsed > RECLAIM_TARGET_STEP.saturating_mul(2) {
        (pages / 2).max(RECLAIM_MIN_PAGES)
    } else if elapsed < RECLAIM_TARGET_STEP / 2 {
        pages.saturating_mul(2).min(RECLAIM_MAX_PAGES)
    } else {
        pages
    }
}

/// Return free pages to the OS in small steps. Runs automatically when a lot
/// of space is free, or to completion when an administrator asked for it.
fn reclaim_free_pages() {
    let manual = RECLAIM_REQUESTED.swap(false, Ordering::AcqRel);
    let Some(usage) = open_storage().and_then(|storage| storage.database_space_usage().ok()) else {
        return;
    };
    if usage.auto_vacuum != AUTO_VACUUM_INCREMENTAL || usage.freelist_count == 0 {
        return;
    }
    if !manual && !auto_reclaim_worthwhile(&usage) {
        return;
    }
    if RECLAIM_RUNNING
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }
    let _running = FlagGuard(&RECLAIM_RUNNING);
    let started = Instant::now();
    let mut pages = RECLAIM_INITIAL_PAGES;
    let mut freed_total = 0_i64;
    loop {
        // A fresh handle per step returns the connection to the pool between
        // steps, so this never starves request handling.
        let Some(storage) = open_storage() else {
            break;
        };
        let step_started = Instant::now();
        let freed = match storage.incremental_vacuum_step(pages) {
            Ok(freed) => freed,
            Err(err) => {
                log::warn!("event=storage_reclaim_step_failed err={err}");
                set_last_error(Some(format!("incremental vacuum failed: {err}")));
                break;
            }
        };
        drop(storage);
        pages = next_step_pages(pages, step_started.elapsed());
        freed_total = freed_total.saturating_add(freed);
        if freed == 0 || (!manual && started.elapsed() >= AUTO_RECLAIM_BUDGET_PER_TICK) {
            break;
        }
        std::thread::sleep(RECLAIM_PAUSE);
    }
    if freed_total > 0 {
        log::info!(
            "event=storage_reclaim freed_pages={} page_size={} manual={}",
            freed_total,
            usage.page_size,
            manual
        );
    }
}

fn auto_vacuum_label(mode: i64) -> &'static str {
    match mode {
        AUTO_VACUUM_INCREMENTAL => "incremental",
        AUTO_VACUUM_FULL => "full",
        _ => "none",
    }
}

/// Space usage of the local database for the settings page.
pub(crate) fn space_usage_snapshot() -> Result<Value, String> {
    if seaorm_enabled() {
        return Ok(json!({ "backend": "remote" }));
    }
    let storage = open_storage().ok_or_else(|| "open storage failed".to_string())?;
    let usage = storage
        .database_space_usage()
        .map_err(|err| format!("read database space usage failed: {err}"))?;
    let purge_pending = storage.request_log_payload_purge_pending().unwrap_or(false);
    Ok(json!({
        "backend": "sqlite",
        "pageSize": usage.page_size,
        "pageCount": usage.page_count,
        "freelistCount": usage.freelist_count,
        "usedBytes": usage.used_bytes,
        "reclaimableBytes": usage.reclaimable_bytes,
        "fileBytes": usage.file_bytes,
        "walBytes": usage.wal_bytes,
        "autoVacuum": auto_vacuum_label(usage.auto_vacuum),
        "purgePending": purge_pending,
        "checkpointPending": wal_checkpoint_pending(),
        "reclaimRunning": RECLAIM_RUNNING.load(Ordering::Acquire)
            || RECLAIM_REQUESTED.load(Ordering::Acquire),
        "rebuildRunning": REBUILD_RUNNING.load(Ordering::Acquire),
        "lastError": read_last_error(),
    }))
}

/// Administrator triggered reclamation. Incremental databases reclaim in
/// the background; older databases need `rebuild = true`, which runs a
/// one-time full rebuild on its own thread.
pub(crate) fn request_reclaim(rebuild: bool) -> Result<Value, String> {
    if seaorm_enabled() {
        return Err(crate::gateway::bilingual_error(
            "远程数据库模式下不支持回收本地数据库空间",
            "reclaiming database space requires the local SQLite backend",
        ));
    }
    if REBUILD_RUNNING.load(Ordering::Acquire) {
        return Err(crate::gateway::bilingual_error(
            "数据库整理正在进行",
            "a database rebuild is already running",
        ));
    }
    let usage = open_storage()
        .ok_or_else(|| "open storage failed".to_string())?
        .database_space_usage()
        .map_err(|err| format!("read database space usage failed: {err}"))?;
    match usage.auto_vacuum {
        AUTO_VACUUM_INCREMENTAL => {
            set_last_error(None);
            RECLAIM_REQUESTED.store(true, Ordering::Release);
            ensure_storage_maintenance();
            wake_storage_maintenance();
            Ok(json!({ "mode": "incremental", "started": true }))
        }
        AUTO_VACUUM_FULL => Ok(json!({ "mode": "full", "started": false })),
        _ if !rebuild => Err(crate::gateway::bilingual_error(
            "当前数据库未启用增量回收，需要先整理数据库",
            "this database needs a one-time rebuild before space can be reclaimed",
        )),
        _ => {
            let purge_pending = open_storage()
                .ok_or_else(|| "open storage failed".to_string())?
                .request_log_payload_purge_pending()
                .map_err(|err| format!("read purge state failed: {err}"))?;
            if purge_pending {
                wake_storage_maintenance();
                return Err(crate::gateway::bilingual_error(
                    "已清空的请求内容仍在后台分批删除，完成后再整理数据库",
                    "cleared request payloads are still being removed in the background, rebuild after that finishes",
                ));
            }
            ensure_rebuild_disk_space(&usage)?;
            start_rebuild()?;
            Ok(json!({ "mode": "rebuild", "started": true }))
        }
    }
}

fn start_rebuild() -> Result<(), String> {
    if REBUILD_RUNNING
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err(crate::gateway::bilingual_error(
            "数据库整理正在进行",
            "a database rebuild is already running",
        ));
    }
    set_last_error(None);
    let spawned = std::thread::Builder::new()
        .name("storage-rebuild".to_string())
        .spawn(|| {
            let _running = FlagGuard(&REBUILD_RUNNING);
            let started = Instant::now();
            let result = open_storage()
                .ok_or_else(|| "open storage failed".to_string())
                .and_then(|storage| {
                    storage
                        .convert_to_incremental_auto_vacuum()
                        .map_err(|err| format!("database rebuild failed: {err}"))
                });
            match result {
                Ok(_) => log::info!(
                    "event=storage_rebuild_completed elapsed_ms={}",
                    started.elapsed().as_millis()
                ),
                Err(err) => {
                    log::warn!("event=storage_rebuild_failed err={err}");
                    set_last_error(Some(err));
                }
            }
        });
    if let Err(err) = spawned {
        REBUILD_RUNNING.store(false, Ordering::Release);
        return Err(format!("start database rebuild failed: {err}"));
    }
    Ok(())
}

/// The rebuild writes a transient copy of the live data into SQLite's
/// temporary directory and the rebuilt pages through the WAL next to the
/// database, so both locations need about the live data size.
fn ensure_rebuild_disk_space(usage: &DatabaseSpaceUsage) -> Result<(), String> {
    let Some(db_path) = std::env::var_os("CODEXMANAGER_DB_PATH").map(PathBuf::from) else {
        return Ok(());
    };
    let live = usage.used_bytes.max(0) as u64;
    let needed = live
        .saturating_add(live / 5)
        .saturating_add(REBUILD_DISK_MARGIN_BYTES);
    let temp_dir = std::env::var_os("SQLITE_TMPDIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let disks = sysinfo::Disks::new_with_refreshed_list();
    let mounts = disks
        .list()
        .iter()
        .map(|disk| (disk.mount_point().to_path_buf(), disk.available_space()))
        .collect::<Vec<_>>();
    let data = disk_for_path(&mounts, &db_path);
    let temp = disk_for_path(&mounts, &temp_dir);
    let mut requirements: Vec<(PathBuf, u64, u64)> = Vec::new();
    for (mount, available) in [data, temp].into_iter().flatten() {
        if let Some(entry) = requirements.iter_mut().find(|entry| entry.0 == mount) {
            entry.2 = entry.2.saturating_add(needed);
        } else {
            requirements.push((mount, available, needed));
        }
    }
    if requirements.is_empty() {
        log::warn!(
            "event=storage_rebuild_disk_unknown db_path={}",
            db_path.display()
        );
    }
    for (mount, available, required) in requirements {
        if available < required {
            return Err(crate::gateway::bilingual_error(
                format!(
                    "磁盘空间不足：整理数据库需要 {} 可用空间，{} 当前只有 {}",
                    format_bytes(required),
                    mount.display(),
                    format_bytes(available)
                ),
                format!(
                    "not enough free disk space: the rebuild needs {} on {} but only {} is available",
                    format_bytes(required),
                    mount.display(),
                    format_bytes(available)
                ),
            ));
        }
    }
    Ok(())
}

fn normalized_components(path: &Path) -> Vec<String> {
    let resolved = std::fs::canonicalize(path)
        .or_else(|_| {
            path.parent()
                .map(std::fs::canonicalize)
                .unwrap_or_else(|| Ok(path.to_path_buf()))
        })
        .unwrap_or_else(|_| path.to_path_buf());
    resolved
        .components()
        .filter_map(|component| match component {
            Component::Prefix(prefix) => {
                // `\\?\C:` (canonicalize on Windows) and `C:` refer to the
                // same volume, compare the drive part only.
                let raw = prefix.as_os_str().to_string_lossy().to_string();
                Some(raw.trim_start_matches(r"\\?\").to_ascii_lowercase())
            }
            Component::RootDir => Some(String::from("/")),
            Component::Normal(part) => Some(part.to_string_lossy().to_string()),
            _ => None,
        })
        .collect()
}

/// The mount point that contains `path` (longest matching prefix).
fn disk_for_path(mounts: &[(PathBuf, u64)], path: &Path) -> Option<(PathBuf, u64)> {
    let target = normalized_components(path);
    mounts
        .iter()
        .filter_map(|(mount, available)| {
            let prefix = normalized_components(mount);
            (target.len() >= prefix.len() && target[..prefix.len()] == prefix[..])
                .then(|| (prefix.len(), mount.clone(), *available))
        })
        .max_by_key(|(depth, _, _)| *depth)
        .map(|(_, mount, available)| (mount, available))
}

fn format_bytes(bytes: u64) -> String {
    const MIB: f64 = 1024.0 * 1024.0;
    if (bytes as f64) >= 1024.0 * MIB {
        format!("{:.1} GiB", bytes as f64 / (1024.0 * MIB))
    } else {
        format!("{:.0} MiB", bytes as f64 / MIB)
    }
}

#[cfg(test)]
#[path = "maintenance_tests.rs"]
mod tests;
