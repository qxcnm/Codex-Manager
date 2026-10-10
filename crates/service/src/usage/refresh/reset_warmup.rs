use codexmanager_core::storage::{now_ts, AccountResetWarmupTarget, Storage};
use futures_util::FutureExt;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{
    mpsc::{channel as bounded, Receiver, Sender},
    Mutex as AsyncMutex,
};

#[cfg(test)]
use crate::account_warmup::AccountWarmupItemResult;
use crate::storage_helpers::open_storage;

const RESET_WARMUP_POLL_INTERVAL: Duration = Duration::from_secs(5);
const RESET_WARMUP_WORKERS: usize = 4;
const RESET_WARMUP_QUEUE_CAPACITY: usize = 128;
type PendingTasks = Arc<Mutex<HashSet<(String, i64)>>>;

/// Quota reset deadlines have their own clock so slow usage polling, polling
/// failures and the optional user cron never delay the start of a new window.
pub(super) async fn reset_warmup_loop() {
    let executor = match ResetWarmupExecutor::new() {
        Ok(executor) => executor,
        Err(err) => {
            log::error!("account reset warmup workers unavailable: {err}");
            return;
        }
    };
    while !crate::shutdown_requested() {
        if let Err(err) = enqueue_due_reset_warmups(&executor) {
            log::warn!("account reset warmup scheduling failed: {err}");
        }
        let deadline = std::time::Instant::now() + RESET_WARMUP_POLL_INTERVAL;
        while !crate::shutdown_requested() && std::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }
}

fn enqueue_due_reset_warmups(executor: &ResetWarmupExecutor) -> Result<(), String> {
    let storage = open_storage()
        .map(|storage| storage.shared_handle())
        .ok_or_else(|| "storage unavailable".to_string())?;
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    let targets = storage
        .list_account_reset_warmup_targets(now_ts(), RESET_WARMUP_QUEUE_CAPACITY)
        .map_err(|err| err.to_string())?;
    for target in targets {
        if crate::shutdown_requested() {
            break;
        }
        executor.enqueue(target);
    }
    Ok(())
}

struct ResetWarmupExecutor {
    sender: Sender<AccountResetWarmupTarget>,
    pending: PendingTasks,
}

impl ResetWarmupExecutor {
    fn new() -> Result<Self, String> {
        let (sender, receiver) = bounded(RESET_WARMUP_QUEUE_CAPACITY);
        let pending = Arc::new(Mutex::new(HashSet::new()));
        let receiver = Arc::new(AsyncMutex::new(receiver));
        for _ in 0..RESET_WARMUP_WORKERS {
            super::background::spawn(reset_warmup_worker(
                Arc::clone(&receiver),
                Arc::clone(&pending),
            ))?;
        }
        Ok(Self { sender, pending })
    }

    fn enqueue(&self, target: AccountResetWarmupTarget) -> bool {
        let key = (target.account_id.clone(), target.reset_at);
        if !crate::lock_utils::lock_recover(&self.pending, "pending_reset_warmups")
            .insert(key.clone())
        {
            return false;
        }
        if self.sender.try_send(target).is_err() {
            crate::lock_utils::lock_recover(&self.pending, "pending_reset_warmups").remove(&key);
            return false;
        }
        true
    }
}

struct PendingGuard {
    pending: PendingTasks,
    key: (String, i64),
}
impl Drop for PendingGuard {
    fn drop(&mut self) {
        crate::lock_utils::lock_recover(&self.pending, "pending_reset_warmups").remove(&self.key);
    }
}

async fn wait_for_shutdown() {
    while !crate::shutdown_requested() {
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

async fn reset_warmup_worker(
    receiver: Arc<AsyncMutex<Receiver<AccountResetWarmupTarget>>>,
    pending: PendingTasks,
) {
    loop {
        let target = tokio::select! {
            biased;
            _ = wait_for_shutdown() => break,
            target = async { receiver.lock().await.recv().await } => match target {
                Some(target) => target,
                None => break,
            },
        };
        let _pending_guard = PendingGuard {
            pending: Arc::clone(&pending),
            key: (target.account_id.clone(), target.reset_at),
        };
        let task = async {
            let storage = open_storage()
                .map(|storage| storage.shared_handle())
                .ok_or_else(|| "storage unavailable".to_string())?;
            if !claim_reset_warmup_task(&storage, &target, now_ts())? {
                return Ok(None);
            }
            crate::account_warmup::warmup_account_after_reset(&storage, &target.account_id)
                .await
                .map(Some)
        };
        let outcome = tokio::select! {
            biased;
            _ = wait_for_shutdown() => break,
            result = std::panic::AssertUnwindSafe(task).catch_unwind() => result,
        };
        match outcome {
            Ok(Ok(Some(item))) => log::info!(
                "account reset warmup finished: account_id={} reset_at={} ok={}",
                item.account_id,
                target.reset_at,
                item.ok
            ),
            Ok(Ok(None)) => {}
            Ok(Err(err)) => log::warn!(
                "account reset warmup failed: account_id={} reset_at={} err={}",
                target.account_id,
                target.reset_at,
                err
            ),
            Err(_) => log::error!(
                "account reset warmup worker panicked: account_id={} reset_at={}",
                target.account_id,
                target.reset_at
            ),
        }
    }
}

#[cfg(test)]
fn run_reset_warmup_task<F>(
    storage: &Storage,
    target: &AccountResetWarmupTarget,
    now: i64,
    send: F,
) -> Result<Option<AccountWarmupItemResult>, String>
where
    F: FnOnce(&Storage, &str) -> Result<AccountWarmupItemResult, String>,
{
    if !claim_reset_warmup_task(storage, target, now)? {
        return Ok(None);
    }
    send(storage, &target.account_id).map(Some)
}

fn claim_reset_warmup_task(
    storage: &Storage,
    target: &AccountResetWarmupTarget,
    now: i64,
) -> Result<bool, String> {
    if target.due_at > now || crate::shutdown_requested() {
        return Ok(false);
    }
    // Claim at execution. The database transaction rechecks switch, quota and
    // cycle and records the attempt before HTTP, including uncertain sends.
    crate::account::remote_storage::AccountStorage::new(storage)
        .claim_account_reset_warmup(&target.account_id, target.reset_at, now)
        .map_err(|error| error.to_string())
}

#[cfg(test)]
#[path = "reset_warmup_tests.rs"]
mod tests;
