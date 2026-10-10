use std::sync::{Arc, OnceLock};

use std::future::Future;
use tokio::runtime::Runtime;
use tokio::sync::Semaphore;

use super::AsyncStreamRequestError;

const DEFAULT_WORKERS: usize = 32;
const MAX_WORKERS: usize = 256;
const WORKERS_ENV: &str = "CODEXMANAGER_GATEWAY_ASYNC_STREAM_WORKERS";
static WORKERS: OnceLock<Arc<Semaphore>> = OnceLock::new();

pub(in crate::gateway) fn upstream_runtime() -> Result<&'static Runtime, AsyncStreamRequestError> {
    crate::runtime::service_runtime::process_runtime()
        .map_err(AsyncStreamRequestError::WorkerUnavailable)
}

pub(super) fn spawn_http_worker<F>(work: F) -> Result<(), AsyncStreamRequestError>
where
    F: Future<Output = ()> + Send + 'static,
{
    let workers = WORKERS.get_or_init(|| {
        let configured = std::env::var(WORKERS_ENV)
            .ok()
            .and_then(|value| value.trim().parse::<usize>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(DEFAULT_WORKERS)
            .min(MAX_WORKERS);
        Arc::new(Semaphore::new(configured))
    });
    spawn_bounded_worker(upstream_runtime()?, workers, work)
}

fn spawn_bounded_worker<F>(
    runtime: &Runtime,
    workers: &Arc<Semaphore>,
    work: F,
) -> Result<(), AsyncStreamRequestError>
where
    F: Future<Output = ()> + Send + 'static,
{
    let permit = Arc::clone(workers)
        .try_acquire_owned()
        .map_err(|_| AsyncStreamRequestError::WorkerLimit)?;
    runtime.spawn(async move {
        // Admission follows the asynchronous response lifetime, including
        // backpressure and cancellation; it does not reserve a blocking thread.
        let _permit = permit;
        work.await;
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn async_http_transport_reuses_runtime_and_bounds_live_tasks() {
        let runtime = upstream_runtime().unwrap();
        assert!(std::ptr::eq(runtime, upstream_runtime().unwrap()));
        let workers = Arc::new(Semaphore::new(1));
        let (release, receive) = tokio::sync::oneshot::channel();
        let (started, has_started) = tokio::sync::oneshot::channel();
        spawn_bounded_worker(runtime, &workers, async move {
            started.send(()).unwrap();
            receive.await.unwrap();
        })
        .unwrap();
        runtime.block_on(async {
            tokio::time::timeout(Duration::from_secs(1), has_started)
                .await
                .unwrap()
                .unwrap()
        });
        assert!(matches!(
            spawn_bounded_worker(runtime, &workers, async {}),
            Err(AsyncStreamRequestError::WorkerLimit)
        ));
        release.send(()).unwrap();
        runtime.block_on(async {
            let permit = tokio::time::timeout(Duration::from_secs(1), workers.acquire())
                .await
                .expect("worker releases permit")
                .unwrap();
            drop(permit);
        });
    }
}
