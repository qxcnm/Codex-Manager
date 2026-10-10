use std::future::Future;
use std::sync::Mutex;
use std::time::Duration;
use tokio::runtime::Runtime;

/// Long-lived account jobs share asynchronous socket workers. Callers bound
/// task admission independently so a busy test queue cannot starve reset work.
pub(crate) fn runtime() -> Result<&'static Runtime, String> {
    crate::runtime::service_runtime::process_runtime()
}

static ACTIVE: Mutex<usize> = Mutex::new(0);
static CHANGED: tokio::sync::Notify = tokio::sync::Notify::const_new();

struct TaskGuard;
impl Drop for TaskGuard {
    fn drop(&mut self) {
        let mut active = crate::lock_utils::lock_recover(&ACTIVE, "account_background_tasks");
        *active -= 1;
        CHANGED.notify_waiters();
    }
}

/// Register before scheduling, so shutdown also waits for tasks not yet polled.
/// This is for cancellable jobs; token rotation uses its own completion tracker
/// because a received rotated credential must be persisted even after shutdown.
pub(crate) fn spawn<F>(name: &'static str, future: F) -> Result<(), String>
where
    F: Future<Output = ()> + Send + 'static,
{
    use futures_util::FutureExt;
    let runtime = runtime()?;
    let guard = {
        let mut active = crate::lock_utils::lock_recover(&ACTIVE, "account_background_tasks");
        if crate::shutdown_requested() {
            return Err("account background task rejected during shutdown".into());
        }
        *active += 1;
        TaskGuard
    };
    runtime.spawn(async move {
        let _guard = guard;
        let shutdown = async {
            while !crate::shutdown_requested() {
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        };
        tokio::select! {
            biased;
            _ = shutdown => {}
            result = std::panic::AssertUnwindSafe(future).catch_unwind() => {
                if result.is_err() {
                    log::error!("account background task failed unexpectedly: {name}");
                }
            }
        }
    });
    Ok(())
}

/// Call only after global shutdown is set. Holding the admission lock while
/// checking zero closes the race with a task registering during shutdown.
pub(crate) async fn drain_account_background_tasks() {
    loop {
        let changed = CHANGED.notified();
        tokio::pin!(changed);
        changed.as_mut().enable();
        if *crate::lock_utils::lock_recover(&ACTIVE, "account_background_tasks") == 0 {
            return;
        }
        changed.await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    struct ResetShutdown;
    impl Drop for ResetShutdown {
        fn drop(&mut self) {
            crate::clear_shutdown_flag();
        }
    }
    struct Dropped(Arc<AtomicUsize>);
    impl Drop for Dropped {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn shutdown_drains_socket_cancellation_before_allowing_a_fresh_start() {
        let _guard = crate::test_env_guard();
        let _reset = ResetShutdown;
        crate::clear_shutdown_flag();
        for headers in ["", "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n"] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}/pending", listener.local_addr().unwrap());
            let (ready, accepted) = tokio::sync::oneshot::channel();
            let provider = tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut buffer = [0; 4096];
                assert!(socket.read(&mut buffer).await.unwrap() > 0);
                socket.write_all(headers.as_bytes()).await.unwrap();
                ready.send(()).unwrap();
                assert_eq!(
                    tokio::time::timeout(Duration::from_secs(2), socket.read(&mut buffer))
                        .await
                        .unwrap()
                        .unwrap(),
                    0
                );
            });
            let dropped = Arc::new(AtomicUsize::new(0));
            let cleanup = Dropped(dropped.clone());
            spawn("cancel-socket-test", async move {
                let _cleanup = cleanup;
                let client = reqwest::Client::builder().no_proxy().build().unwrap();
                let _ = client.get(url).send().await.unwrap().bytes().await;
            })
            .unwrap();
            tokio::time::timeout(Duration::from_secs(2), accepted)
                .await
                .unwrap()
                .unwrap();
            crate::request_shutdown("");
            tokio::time::timeout(Duration::from_secs(2), drain_account_background_tasks())
                .await
                .unwrap();
            assert_eq!(
                dropped.load(Ordering::SeqCst),
                1,
                "drain must include resource cleanup"
            );
            assert!(spawn("rejected-during-shutdown", async {}).is_err());
            provider.await.unwrap();
            crate::clear_shutdown_flag();
        }
        let (completed, completion) = tokio::sync::oneshot::channel();
        spawn("restart-test", async move {
            completed.send(()).unwrap();
        })
        .unwrap();
        tokio::time::timeout(Duration::from_secs(2), completion)
            .await
            .unwrap()
            .unwrap();
        crate::request_shutdown("");
        tokio::time::timeout(Duration::from_secs(2), drain_account_background_tasks())
            .await
            .unwrap();
    }
}
