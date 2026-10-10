use futures_util::FutureExt;
use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::Ordering;
#[cfg(test)]
use std::sync::mpsc as std_mpsc;
use std::sync::{Arc, Mutex, OnceLock};
use tokio::sync::{mpsc, Mutex as AsyncMutex};

use super::{ensure_background_tasks_config_loaded, USAGE_REFRESH_WORKERS};

const USAGE_REFRESH_QUEUE_CAPACITY: usize = 1024;
static PENDING_USAGE_REFRESH_TASKS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
static USAGE_REFRESH_EXECUTOR: Mutex<Option<UsageRefreshExecutor>> = Mutex::new(None);

type UsageRefreshFuture = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

pub(super) fn enqueue_usage_refresh_async<F, Fut>(account_id: &str, worker: F) -> bool
where
    F: FnOnce(String) -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    let id = account_id.trim();
    if crate::shutdown_requested() || id.is_empty() || !mark_usage_refresh_task_pending(id) {
        return false;
    }
    let task = UsageRefreshTask {
        account_id: id.to_string(),
        worker: Box::new(move |id| Box::pin(worker(id))),
    };
    let sent = {
        let mut slot =
            crate::lock_utils::lock_recover(&USAGE_REFRESH_EXECUTOR, "usage_refresh_executor");
        if slot
            .as_ref()
            .is_none_or(|executor| executor.sender.is_closed())
        {
            *slot = UsageRefreshExecutor::new().ok();
        }
        slot.as_ref()
            .is_some_and(|executor| executor.sender.try_send(task).is_ok())
    };
    if !sent {
        clear_usage_refresh_task_pending(id);
    }
    sent
}

// Existing queue behavior tests use deliberately blocking callbacks. Production
// callbacks return futures, so socket waits never occupy a blocking worker.
#[cfg(test)]
pub(crate) fn enqueue_usage_refresh_with_worker<F>(account_id: &str, worker: F) -> bool
where
    F: FnOnce(String) + Send + 'static,
{
    enqueue_usage_refresh_async(account_id, move |id| async move {
        let _ = tokio::task::spawn_blocking(move || worker(id)).await;
    })
}

struct UsageRefreshTask {
    account_id: String,
    worker: Box<dyn FnOnce(String) -> UsageRefreshFuture + Send + 'static>,
}

struct UsageRefreshExecutor {
    sender: mpsc::Sender<UsageRefreshTask>,
    #[cfg(test)]
    worker_count: usize,
    #[cfg(test)]
    worker_finished: std_mpsc::Receiver<()>,
}

impl UsageRefreshExecutor {
    fn new() -> Result<Self, String> {
        ensure_background_tasks_config_loaded();
        let worker_count = USAGE_REFRESH_WORKERS.load(Ordering::Relaxed).max(1);
        let (sender, receiver) = mpsc::channel::<UsageRefreshTask>(USAGE_REFRESH_QUEUE_CAPACITY);
        let receiver = Arc::new(AsyncMutex::new(receiver));
        #[cfg(test)]
        let (worker_finished_tx, worker_finished) = std_mpsc::channel();
        for _ in 0..worker_count {
            let receiver = Arc::clone(&receiver);
            #[cfg(test)]
            let worker_finished_tx = worker_finished_tx.clone();
            super::background::spawn(async move {
                loop {
                    let task = tokio::select! {
                        biased;
                        _ = super::runner::wait_for_shutdown() => break,
                        task = async { receiver.lock().await.recv().await } => match task {
                            Some(task) => task,
                            None => break,
                        },
                    };
                    let _guard = PendingGuard(task.account_id.clone());
                    let future = async move { (task.worker)(task.account_id).await };
                    tokio::select! {
                        biased;
                        _ = super::runner::wait_for_shutdown() => break,
                        _ = std::panic::AssertUnwindSafe(future).catch_unwind() => {},
                    }
                }
                // Shutdown releases queued deduplication entries too.
                let mut receiver = receiver.lock().await;
                while let Ok(task) = receiver.try_recv() {
                    clear_usage_refresh_task_pending(&task.account_id);
                }
                #[cfg(test)]
                let _ = worker_finished_tx.send(());
            })?;
        }
        Ok(Self {
            sender,
            #[cfg(test)]
            worker_count,
            #[cfg(test)]
            worker_finished,
        })
    }
}

struct PendingGuard(String);
impl Drop for PendingGuard {
    fn drop(&mut self) {
        clear_usage_refresh_task_pending(&self.0);
    }
}

/// 函数 `mark_usage_refresh_task_pending`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - account_id: 参数 account_id
///
/// # 返回
/// 返回函数执行结果
fn mark_usage_refresh_task_pending(account_id: &str) -> bool {
    let mutex = PENDING_USAGE_REFRESH_TASKS.get_or_init(|| Mutex::new(HashSet::new()));
    let mut pending = crate::lock_utils::lock_recover(mutex, "pending_usage_refresh_tasks");
    pending.insert(account_id.to_string())
}

/// 函数 `clear_usage_refresh_task_pending`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - account_id: 参数 account_id
///
/// # 返回
/// 无
fn clear_usage_refresh_task_pending(account_id: &str) {
    let Some(mutex) = PENDING_USAGE_REFRESH_TASKS.get() else {
        return;
    };
    let mut pending = crate::lock_utils::lock_recover(mutex, "pending_usage_refresh_tasks");
    pending.remove(account_id);
}

/// 函数 `reset_usage_refresh_executor_for_tests`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 无
#[cfg(test)]
pub(crate) fn reset_usage_refresh_executor_for_tests() {
    let executor = {
        let mut slot =
            crate::lock_utils::lock_recover(&USAGE_REFRESH_EXECUTOR, "usage_refresh_executor");
        slot.take()
    };
    if let Some(executor) = executor {
        drop(executor.sender);
        for _ in 0..executor.worker_count {
            executor
                .worker_finished
                .recv()
                .expect("usage refresh worker exits after its queue is drained");
        }
    }

    if let Some(mutex) = PENDING_USAGE_REFRESH_TASKS.get() {
        let pending = crate::lock_utils::lock_recover(mutex, "pending_usage_refresh_tasks");
        assert!(
            pending.is_empty(),
            "usage refresh executor reset left pending accounts: {pending:?}"
        );
    }
}
