//! Tracks scheduler/queue tasks so service shutdown can wait for cancellation
//! and a later start in the same process can create fresh workers.
use std::future::Future;
use std::sync::atomic::{AtomicUsize, Ordering};

static REGISTRATION: std::sync::Mutex<()> = std::sync::Mutex::new(());
static ACTIVE: AtomicUsize = AtomicUsize::new(0);
static CHANGED: tokio::sync::Notify = tokio::sync::Notify::const_new();
struct TaskGuard;
impl Drop for TaskGuard {
    fn drop(&mut self) {
        ACTIVE.fetch_sub(1, Ordering::AcqRel);
        CHANGED.notify_waiters();
    }
}

pub(super) fn spawn<F>(future: F) -> Result<(), String>
where
    F: Future<Output = ()> + Send + 'static,
{
    let runtime = crate::account::background::runtime()?;
    let registration =
        crate::lock_utils::lock_recover(&REGISTRATION, "usage_background_registration");
    if crate::shutdown_requested() {
        return Err("usage background task rejected during shutdown".into());
    }
    ACTIVE.fetch_add(1, Ordering::AcqRel);
    let guard = TaskGuard;
    drop(registration);
    runtime.spawn(async move {
        let _guard = guard;
        future.await;
    });
    Ok(())
}

pub(crate) async fn drain_usage_background_tasks() {
    loop {
        let changed = CHANGED.notified();
        tokio::pin!(changed);
        changed.as_mut().enable();
        let active = {
            let _registration =
                crate::lock_utils::lock_recover(&REGISTRATION, "usage_background_registration");
            ACTIVE.load(Ordering::Acquire)
        };
        if active == 0 {
            return;
        }
        changed.await;
    }
}
