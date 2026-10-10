//! Fixed workers for synchronous ABIs which may wait on async domain work.
//! Keeping these off Tokio's blocking pool prevents a Rhai/RPC waiter from
//! starving the file/SQLite/DNS work its own future is waiting for.
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

type Work = Box<dyn FnOnce() + Send>;
static WORKERS: OnceLock<Result<crossbeam_channel::Sender<Work>, String>> = OnceLock::new();
static ACTIVE: Mutex<usize> = Mutex::new(0);
static CHANGED: tokio::sync::Notify = tokio::sync::Notify::const_new();
const CAPACITY: usize = 40;

struct Active;
impl Drop for Active {
    fn drop(&mut self) {
        *crate::lock_utils::lock_recover(&ACTIVE, "compatibility_workers") -= 1;
        CHANGED.notify_waiters();
    }
}

fn workers() -> Result<&'static crossbeam_channel::Sender<Work>, String> {
    WORKERS
        .get_or_init(|| {
            let (send, receive) = crossbeam_channel::bounded::<Work>(CAPACITY);
            for index in 0..8 {
                let receive = receive.clone();
                std::thread::Builder::new()
                    .name(format!("service-sync-{index}"))
                    .spawn(move || {
                        for work in receive {
                            work();
                        }
                    })
                    .map_err(|_| "could not start compatibility workers".to_owned())?;
            }
            Ok(send)
        })
        .as_ref()
        .map_err(Clone::clone)
}

pub(crate) async fn run<T: Send + 'static>(
    name: &'static str,
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T, String> {
    let workers = workers()?;
    let guard = {
        let mut active = crate::lock_utils::lock_recover(&ACTIVE, "compatibility_workers");
        if crate::shutdown_requested() {
            return Err("service is shutting down".to_owned());
        }
        if *active >= CAPACITY {
            return Err("compatibility workers busy".to_owned());
        }
        *active += 1;
        Active
    };
    let queued_at = Instant::now();
    let (send, receive) = tokio::sync::oneshot::channel();
    workers
        .try_send(Box::new(move || {
            let _guard = guard;
            let started = Instant::now();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(work))
                .map_err(|_| "compatibility worker panicked".to_owned());
            tracing::debug!(
                task = name,
                wait_us = started.duration_since(queued_at).as_micros() as u64,
                run_us = started.elapsed().as_micros() as u64,
                failed = result.is_err(),
                "compatibility task finished"
            );
            let _ = send.send(result);
        }))
        .map_err(|_| "compatibility workers busy".to_owned())?;
    receive
        .await
        .map_err(|_| "compatibility worker interrupted".to_owned())?
}

/// Already admitted state changes finish even when their HTTP receiver drops.
pub(crate) async fn drain() {
    loop {
        let changed = CHANGED.notified();
        tokio::pin!(changed);
        changed.as_mut().enable();
        if *crate::lock_utils::lock_recover(&ACTIVE, "compatibility_workers") == 0 {
            return;
        }
        changed.await;
    }
}

#[cfg(test)]
mod tests {
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cancelled_receiver_keeps_admitted_work_until_drain_and_restart() {
        let _guard = crate::test_env_guard();
        crate::clear_shutdown_flag();
        let (started, ready) = tokio::sync::oneshot::channel();
        let (release, wait) = std::sync::mpsc::channel();
        let finished = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let completed = finished.clone();
        let caller = tokio::spawn(super::run("cancelled-receiver", move || {
            let _ = started.send(());
            wait.recv().unwrap();
            completed.store(true, std::sync::atomic::Ordering::Release);
        }));
        ready.await.unwrap();
        caller.abort();
        let _ = caller.await;
        crate::request_shutdown("");
        assert!(super::run("rejected", || ())
            .await
            .unwrap_err()
            .contains("shutting down"));
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(25), super::drain())
                .await
                .is_err()
        );
        release.send(()).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(2), super::drain())
            .await
            .unwrap();
        assert!(finished.load(std::sync::atomic::Ordering::Acquire));
        crate::clear_shutdown_flag();
        assert_eq!(super::run("restart", || 1).await.unwrap(), 1);
    }

    #[tokio::test]
    async fn synchronous_abi_can_await_nested_blocking_work_and_survives_panic() {
        let _guard = crate::test_env_guard();
        crate::clear_shutdown_flag();
        let value = super::run("nested", || {
            crate::runtime::service_runtime::run_sync(async {
                tokio::task::spawn_blocking(|| 42).await.unwrap()
            })
            .unwrap()
        })
        .await
        .unwrap();
        assert_eq!(value, 42);
        assert!(super::run("panic", || panic!("fixture"))
            .await
            .unwrap_err()
            .contains("panicked"));
        assert_eq!(super::run("recovery", || 43).await.unwrap(), 43);
        super::drain().await;
    }
}
