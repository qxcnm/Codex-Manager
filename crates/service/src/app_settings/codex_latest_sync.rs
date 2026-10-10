use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

const CODEX_LATEST_SYNC_INTERVAL_ENV: &str = "CODEXMANAGER_CODEX_LATEST_SYNC_INTERVAL_SECS";
const DEFAULT_CODEX_LATEST_SYNC_INTERVAL_SECS: u64 = 6 * 60 * 60;
const MIN_CODEX_LATEST_SYNC_INTERVAL_SECS: u64 = 60;

static CODEX_LATEST_SYNC_STARTED: AtomicBool = AtomicBool::new(false);

struct LatestSyncLease;

impl Drop for LatestSyncLease {
    fn drop(&mut self) {
        CODEX_LATEST_SYNC_STARTED.store(false, Ordering::Release);
    }
}

pub(crate) fn ensure_codex_latest_version_sync() {
    if CODEX_LATEST_SYNC_STARTED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }
    let lease = LatestSyncLease;
    if let Err(error) = crate::account::background::spawn("codex-latest-version-sync", async move {
        let _lease = lease;
        codex_latest_version_sync_loop().await;
    }) {
        log::warn!("codex latest client_version sync task failed to start: {error}");
    }
}

async fn codex_latest_version_sync_loop() {
    loop {
        match super::sync_gateway_user_agent_version_from_codex_latest_async().await {
            Ok(version) => {
                log::info!("codex latest client_version synced: version={}", version);
            }
            Err(err) => {
                log::warn!("codex latest client_version sync failed: {err}");
            }
        }
        tokio::time::sleep(Duration::from_secs(codex_latest_sync_interval_secs())).await;
    }
}

fn codex_latest_sync_interval_secs() -> u64 {
    std::env::var(CODEX_LATEST_SYNC_INTERVAL_ENV)
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_CODEX_LATEST_SYNC_INTERVAL_SECS)
        .max(MIN_CODEX_LATEST_SYNC_INTERVAL_SECS)
}

#[cfg(test)]
#[path = "codex_latest_sync_tests.rs"]
mod tests;
