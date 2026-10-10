//! The service owns one process-lifetime executor, including pooled connection
//! drivers. Stopping a listener drains its tasks, never destroys the executor.
use std::future::Future;
use std::sync::OnceLock;
use tokio::runtime::{Runtime, RuntimeFlavor};

static RUNTIME: OnceLock<Result<Runtime, String>> = OnceLock::new();
// Only legacy synchronous ABIs use this bridge. Native HTTP handlers await.
static SYNC_BRIDGES: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(32);

pub fn process_runtime() -> Result<&'static Runtime, String> {
    RUNTIME
        .get_or_init(|| {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(crate::http::proxy_runtime::front_proxy_worker_threads())
                .max_blocking_threads(
                    crate::http::proxy_runtime::front_proxy_max_blocking_threads(),
                )
                .thread_name("codexmanager-runtime")
                .enable_all()
                .build()
                .map_err(|_| "could not initialize service runtime".to_owned())
        })
        .as_ref()
        .map_err(Clone::clone)
}

/// Compatibility for synchronous desktop APIs and Rhai workers. Admission is
/// bounded even for current-thread test callers, which need a scoped OS worker
/// because borrowed futures cannot be spawned as 'static tasks. No async
/// production route may use this instead of awaiting its domain operation.
pub(crate) fn run_sync<F>(future: F) -> Result<F::Output, String>
where
    F: Future + Send,
    F::Output: Send,
{
    let _permit = SYNC_BRIDGES
        .try_acquire()
        .map_err(|_| "synchronous compatibility capacity exhausted".to_owned())?;
    let runtime = process_runtime()?;
    let wait = || runtime.block_on(future);
    match tokio::runtime::Handle::try_current() {
        Ok(handle) if handle.runtime_flavor() == RuntimeFlavor::MultiThread => {
            Ok(tokio::task::block_in_place(wait))
        }
        Ok(_) => std::thread::scope(|scope| {
            scope
                .spawn(wait)
                .join()
                .map_err(|_| "synchronous compatibility worker panicked".to_owned())
        }),
        Err(_) => Ok(wait()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_domains_share_the_process_executor() {
        let runtime = process_runtime().unwrap();
        assert!(std::ptr::eq(
            runtime,
            crate::account::background::runtime().unwrap()
        ));
        assert!(std::ptr::eq(
            runtime,
            crate::http::proxy_runtime::front_proxy_runtime().unwrap()
        ));
        let first = runtime.spawn(async {
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
            7
        });
        assert_eq!(run_sync(first).unwrap().unwrap(), 7);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn compatibility_is_bounded_and_does_not_nest_a_runtime() {
        assert_eq!(
            run_sync(async {
                tokio::time::sleep(std::time::Duration::from_millis(1)).await;
                11
            })
            .unwrap(),
            11
        );
        let permits = SYNC_BRIDGES.acquire_many(32).await.unwrap();
        assert!(run_sync(async {}).unwrap_err().contains("capacity"));
        drop(permits);
        assert_eq!(run_sync(async { 12 }).unwrap(), 12);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn compatibility_callers_return_capacity_errors_without_polling_network_work() {
        let permits = SYNC_BRIDGES.acquire_many(32).await.unwrap();
        let polled = std::sync::atomic::AtomicBool::new(false);
        let result = crate::auth_tokens::run_auth_future(async {
            polled.store(true, std::sync::atomic::Ordering::SeqCst);
            Ok::<_, String>(())
        });
        assert!(result.unwrap_err().contains("capacity exhausted"));
        assert!(!polled.load(std::sync::atomic::Ordering::SeqCst));
        assert!(crate::aggregate_api::run_aggregate_future(async { Ok(()) })
            .unwrap_err()
            .contains("capacity exhausted"));
        // These URLs cannot be reached: admission must reject before polling.
        let error = crate::usage_http::fetch_reset_credits_snapshot(
            "http://127.0.0.1:1",
            "fixture-only",
            None,
        )
        .unwrap_err();
        assert_eq!(error.status, None);
        assert!(error.message.contains("capacity exhausted"));
        assert!(crate::usage_http::refresh_access_token(
            "http://127.0.0.1:1",
            "fixture-only",
            "fixture-only",
        )
        .err()
        .expect("capacity must reject token refresh")
        .contains("capacity exhausted"));
        drop(permits);
        assert_eq!(
            crate::auth_tokens::run_auth_future(async { Ok::<_, String>(13) }).unwrap(),
            13
        );
    }
}
