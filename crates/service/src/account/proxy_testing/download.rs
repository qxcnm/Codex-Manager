use std::future::Future;
use std::pin::pin;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use tokio::runtime::{Builder, Runtime};

use super::errors::{
    map_proxy_test_reqwest_error, proxy_test_result_error_code, proxy_test_result_status,
    ProxyTestError,
};
use super::presets::ResolvedDownloadTestTarget;

const CANCEL_POLL_INTERVAL: Duration = Duration::from_millis(100);

static PROXY_DOWNLOAD_TEST_RUNTIME: OnceLock<Runtime> = OnceLock::new();

#[derive(Debug, Clone)]
pub(crate) struct ProxyDownloadTestOutcome {
    pub status: String,
    pub provider_id: String,
    pub provider_family: String,
    pub file_size_id: String,
    pub download_url: String,
    pub size_bytes: i64,
    pub read_limit_bytes: Option<i64>,
    pub bytes_read: i64,
    pub ttfb_ms: Option<i64>,
    pub duration_ms: i64,
    pub download_mbps: Option<f64>,
    pub status_code: Option<i64>,
    pub tested_at: i64,
    pub cancelled: bool,
    pub error_code: Option<String>,
    pub error: Option<String>,
}

pub(crate) fn run_proxy_download_test(
    proxy_url: &str,
    target: &ResolvedDownloadTestTarget,
) -> ProxyDownloadTestOutcome {
    run_proxy_download_test_with_cancel(proxy_url, target, || false, |_| {})
}

pub(crate) fn run_proxy_download_test_with_cancel<F, P>(
    proxy_url: &str,
    target: &ResolvedDownloadTestTarget,
    should_cancel: F,
    on_progress: P,
) -> ProxyDownloadTestOutcome
where
    F: Fn() -> bool,
    P: Fn(u64),
{
    run_proxy_test_future(async move {
        let started_at = Instant::now();
        if should_cancel() {
            return cancelled_outcome(target, None, 0, None, started_at);
        }

        let (client, _) = match super::client::build_proxy_test_client(
            proxy_url,
            super::client::ProxyTestRedirectPolicy::Limited(10),
            false,
        ) {
            Ok(result) => result,
            Err(err) => return builder_error_to_outcome(target, err, started_at),
        };

        let response = match poll_future_with_cancel(
            client
                .get(target.download_url.as_str())
                .header(reqwest::header::ACCEPT_ENCODING, "identity")
                .send(),
            &should_cancel,
        )
        .await
        {
            PollWithCancel::Ready(Ok(response)) => response,
            PollWithCancel::Ready(Err(err)) => {
                return reqwest_error_to_outcome(
                    "proxy download test",
                    proxy_url,
                    target,
                    err,
                    started_at,
                )
            }
            PollWithCancel::Cancelled => {
                return cancelled_outcome(target, None, 0, None, started_at)
            }
        };

        let status = response.status();
        let status_code = Some(i64::from(status.as_u16()));
        if !status.is_success() {
            return ProxyDownloadTestOutcome {
                status: "failed".to_string(),
                provider_id: target.provider_id.clone(),
                provider_family: target.provider_family.clone(),
                file_size_id: target.file_size_id.clone(),
                download_url: target.download_url.clone(),
                size_bytes: target.size_bytes,
                read_limit_bytes: target.read_limit_bytes,
                bytes_read: 0,
                ttfb_ms: None,
                duration_ms: elapsed_ms(started_at),
                download_mbps: None,
                status_code,
                tested_at: codexmanager_core::storage::now_ts(),
                cancelled: false,
                error_code: Some("http_status_error".to_string()),
                error: Some(format!("download URL returned HTTP {}", status)),
            };
        }

        let mut stream = response.bytes_stream();
        let mut bytes_read = 0_i64;
        let mut ttfb_ms = None;
        loop {
            let next_item = match poll_future_with_cancel(stream.next(), &should_cancel).await {
                PollWithCancel::Ready(item) => item,
                PollWithCancel::Cancelled => {
                    return cancelled_outcome(target, status_code, bytes_read, ttfb_ms, started_at)
                }
            };

            match next_item {
                Some(Ok(bytes)) => {
                    if ttfb_ms.is_none() {
                        ttfb_ms = Some(elapsed_ms(started_at));
                    }

                    if let Some(limit) = target.read_limit_bytes {
                        if bytes_read >= limit {
                            break;
                        }
                        let remaining = (limit - bytes_read) as usize;
                        let taken = bytes.len().min(remaining);
                        bytes_read += taken as i64;
                        on_progress(bytes_read as u64);
                        if taken < bytes.len() {
                            break;
                        }
                    } else {
                        bytes_read += bytes.len() as i64;
                        on_progress(bytes_read as u64);
                    }
                }
                Some(Err(err)) => {
                    return reqwest_stream_error_to_outcome(
                        proxy_url,
                        target,
                        err,
                        status_code,
                        bytes_read,
                        ttfb_ms,
                        started_at,
                    )
                }
                None => break,
            }
        }

        let duration_ms = elapsed_ms(started_at);
        ProxyDownloadTestOutcome {
            status: "ok".to_string(),
            provider_id: target.provider_id.clone(),
            provider_family: target.provider_family.clone(),
            file_size_id: target.file_size_id.clone(),
            download_url: target.download_url.clone(),
            size_bytes: target.size_bytes,
            read_limit_bytes: target.read_limit_bytes,
            bytes_read,
            ttfb_ms,
            duration_ms,
            download_mbps: compute_mbps(bytes_read, duration_ms),
            status_code,
            tested_at: codexmanager_core::storage::now_ts(),
            cancelled: false,
            error_code: None,
            error: None,
        }
    })
}

fn proxy_test_runtime() -> &'static Runtime {
    PROXY_DOWNLOAD_TEST_RUNTIME.get_or_init(|| {
        Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .thread_name("proxy-download-test")
            .build()
            .unwrap_or_else(|err| panic!("build proxy download test runtime failed: {err}"))
    })
}

fn run_proxy_test_future<F>(future: F) -> F::Output
where
    F: Future,
{
    proxy_test_runtime().block_on(future)
}

enum PollWithCancel<T> {
    Ready(T),
    Cancelled,
}

async fn poll_future_with_cancel<F, C>(future: F, should_cancel: &C) -> PollWithCancel<F::Output>
where
    F: Future,
    C: Fn() -> bool,
{
    let mut future = pin!(future);
    loop {
        if should_cancel() {
            return PollWithCancel::Cancelled;
        }
        match tokio::time::timeout(CANCEL_POLL_INTERVAL, future.as_mut()).await {
            Ok(output) => return PollWithCancel::Ready(output),
            Err(_) => {
                if should_cancel() {
                    return PollWithCancel::Cancelled;
                }
            }
        }
    }
}

fn builder_error_to_outcome(
    target: &ResolvedDownloadTestTarget,
    err: ProxyTestError,
    started_at: Instant,
) -> ProxyDownloadTestOutcome {
    ProxyDownloadTestOutcome {
        status: proxy_test_result_status(&err).to_string(),
        provider_id: target.provider_id.clone(),
        provider_family: target.provider_family.clone(),
        file_size_id: target.file_size_id.clone(),
        download_url: target.download_url.clone(),
        size_bytes: target.size_bytes,
        read_limit_bytes: target.read_limit_bytes,
        bytes_read: 0,
        ttfb_ms: None,
        duration_ms: elapsed_ms(started_at),
        download_mbps: None,
        status_code: None,
        tested_at: codexmanager_core::storage::now_ts(),
        cancelled: false,
        error_code: Some(proxy_test_result_error_code(&err).to_string()),
        error: Some(err.message),
    }
}

fn reqwest_error_to_outcome(
    action: &str,
    proxy_url: &str,
    target: &ResolvedDownloadTestTarget,
    err: reqwest::Error,
    started_at: Instant,
) -> ProxyDownloadTestOutcome {
    let mapped = map_proxy_test_reqwest_error(action, proxy_url, err);
    ProxyDownloadTestOutcome {
        status: proxy_test_result_status(&mapped).to_string(),
        provider_id: target.provider_id.clone(),
        provider_family: target.provider_family.clone(),
        file_size_id: target.file_size_id.clone(),
        download_url: target.download_url.clone(),
        size_bytes: target.size_bytes,
        read_limit_bytes: target.read_limit_bytes,
        bytes_read: 0,
        ttfb_ms: None,
        duration_ms: elapsed_ms(started_at),
        download_mbps: None,
        status_code: None,
        tested_at: codexmanager_core::storage::now_ts(),
        cancelled: false,
        error_code: Some(proxy_test_result_error_code(&mapped).to_string()),
        error: Some(mapped.message),
    }
}

fn reqwest_stream_error_to_outcome(
    proxy_url: &str,
    target: &ResolvedDownloadTestTarget,
    err: reqwest::Error,
    status_code: Option<i64>,
    bytes_read: i64,
    ttfb_ms: Option<i64>,
    started_at: Instant,
) -> ProxyDownloadTestOutcome {
    let mapped = map_proxy_test_reqwest_error("proxy download test body read", proxy_url, err);
    ProxyDownloadTestOutcome {
        status: proxy_test_result_status(&mapped).to_string(),
        provider_id: target.provider_id.clone(),
        provider_family: target.provider_family.clone(),
        file_size_id: target.file_size_id.clone(),
        download_url: target.download_url.clone(),
        size_bytes: target.size_bytes,
        read_limit_bytes: target.read_limit_bytes,
        bytes_read,
        ttfb_ms,
        duration_ms: elapsed_ms(started_at),
        download_mbps: compute_mbps(bytes_read, elapsed_ms(started_at)),
        status_code,
        tested_at: codexmanager_core::storage::now_ts(),
        cancelled: false,
        error_code: Some(proxy_test_result_error_code(&mapped).to_string()),
        error: Some(mapped.message),
    }
}

fn cancelled_outcome(
    target: &ResolvedDownloadTestTarget,
    status_code: Option<i64>,
    bytes_read: i64,
    ttfb_ms: Option<i64>,
    started_at: Instant,
) -> ProxyDownloadTestOutcome {
    let duration_ms = elapsed_ms(started_at);
    ProxyDownloadTestOutcome {
        status: "cancelled".to_string(),
        provider_id: target.provider_id.clone(),
        provider_family: target.provider_family.clone(),
        file_size_id: target.file_size_id.clone(),
        download_url: target.download_url.clone(),
        size_bytes: target.size_bytes,
        read_limit_bytes: target.read_limit_bytes,
        bytes_read,
        ttfb_ms,
        duration_ms,
        download_mbps: compute_mbps(bytes_read, duration_ms),
        status_code,
        tested_at: codexmanager_core::storage::now_ts(),
        cancelled: true,
        error_code: Some("cancelled".to_string()),
        error: Some("proxy download test cancelled".to_string()),
    }
}

fn elapsed_ms(started_at: Instant) -> i64 {
    started_at.elapsed().as_millis().min(i64::MAX as u128) as i64
}

fn compute_mbps(bytes_read: i64, duration_ms: i64) -> Option<f64> {
    if bytes_read <= 0 || duration_ms <= 0 {
        return None;
    }
    let seconds = duration_ms as f64 / 1000.0;
    if seconds <= 0.0 {
        return None;
    }
    Some((bytes_read as f64 * 8.0) / seconds / 1_000_000.0)
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{mpsc, Arc};
    use std::thread;
    use std::time::Duration;

    use super::run_proxy_download_test_with_cancel;
    use crate::account::proxy_testing::presets::resolve_download_test_target;

    #[test]
    fn download_test_streams_with_identity_encoding_and_reports_bytes_mbps() {
        let body_chunks = vec![b"hello".to_vec(), b"world".to_vec()];
        let (proxy_url, rx, handle) = start_fake_proxy_download_response(
            "HTTP/1.1 200 OK\r\nContent-Length: 10\r\nConnection: close\r\n\r\n",
            body_chunks,
            Duration::from_millis(50),
            Duration::from_millis(10),
        );
        let target =
            resolve_download_test_target(Some("cachefly"), Some("size_1mb")).expect("target");

        let result = run_proxy_download_test_with_cancel(proxy_url.as_str(), &target, || false, |_| {});

        let request = rx.recv().expect("captured request");
        handle.join().expect("join fake proxy");

        assert!(request
            .to_ascii_lowercase()
            .contains("accept-encoding: identity"));
        assert_eq!(result.status, "ok");
        assert_eq!(result.status_code, Some(200));
        assert_eq!(result.bytes_read, 10);
        assert!(result.ttfb_ms.unwrap_or_default() >= 25);
        assert!(result.duration_ms >= result.ttfb_ms.unwrap_or_default());
        assert!(result.download_mbps.unwrap_or_default() > 0.0);
        assert_eq!(result.error_code, None);
    }

    #[test]
    fn download_test_stops_at_read_limit() {
        let body_chunks = vec![b"abcdef".to_vec(), b"ghijkl".to_vec()];
        let (proxy_url, _rx, handle) = start_fake_proxy_download_response(
            "HTTP/1.1 200 OK\r\nContent-Length: 12\r\nConnection: close\r\n\r\n",
            body_chunks,
            Duration::from_millis(0),
            Duration::from_millis(0),
        );
        let target = crate::account::proxy_testing::presets::ResolvedDownloadTestTarget {
            provider_id: "test".to_string(),
            provider_family: "test".to_string(),
            file_size_id: "size_500mb_cap".to_string(),
            size_bytes: 500_000_000,
            download_url: "http://example.test/1gb.bin".to_string(),
            read_limit_bytes: Some(5),
        };

        let result = run_proxy_download_test_with_cancel(proxy_url.as_str(), &target, || false, |_| {});

        handle.join().expect("join fake proxy");

        assert_eq!(result.status, "ok");
        assert_eq!(result.bytes_read, 5);
        assert_eq!(result.read_limit_bytes, Some(5));
    }

    #[test]
    fn download_test_honors_cancel_hook_while_waiting_for_first_chunk() {
        let body_chunks = vec![b"abcdefghij".to_vec()];
        let (proxy_url, _rx, handle) = start_fake_proxy_download_response(
            "HTTP/1.1 200 OK\r\nContent-Length: 10\r\nConnection: close\r\n\r\n",
            body_chunks,
            Duration::from_millis(250),
            Duration::from_millis(0),
        );
        let target =
            resolve_download_test_target(Some("cachefly"), Some("size_1mb")).expect("target");
        let cancelled = Arc::new(AtomicBool::new(false));
        let cancel_flag = Arc::clone(&cancelled);
        let signal = Arc::clone(&cancelled);
        let cancel_thread = thread::spawn(move || {
            thread::sleep(Duration::from_millis(120));
            signal.store(true, Ordering::SeqCst);
        });

        let result = run_proxy_download_test_with_cancel(proxy_url.as_str(), &target, move || {
            cancel_flag.load(Ordering::SeqCst)
        }, |_| {});

        cancel_thread.join().expect("join cancel thread");
        handle.join().expect("join fake proxy");

        assert_eq!(result.status, "cancelled");
        assert!(result.cancelled);
        assert_eq!(result.bytes_read, 0);
        assert_eq!(result.error_code.as_deref(), Some("cancelled"));
    }

    fn start_fake_proxy_download_response(
        response_headers: &'static str,
        body_chunks: Vec<Vec<u8>>,
        first_chunk_delay: Duration,
        between_chunks_delay: Duration,
    ) -> (String, mpsc::Receiver<String>, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake proxy");
        let addr = listener.local_addr().expect("fake proxy addr");
        let proxy_url = format!("http://{addr}");
        let (tx, rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept proxy connection");
            let request = read_request_headers(&mut stream);
            tx.send(request).expect("send request");
            stream
                .write_all(response_headers.as_bytes())
                .expect("write response headers");
            stream.flush().expect("flush response headers");
            thread::sleep(first_chunk_delay);
            let chunk_count = body_chunks.len();
            for (index, chunk) in body_chunks.into_iter().enumerate() {
                if stream.write_all(chunk.as_slice()).is_err() {
                    return;
                }
                let _ = stream.flush();
                if index + 1 < chunk_count {
                    thread::sleep(between_chunks_delay);
                }
            }
        });
        (proxy_url, rx, handle)
    }

    fn read_request_headers(stream: &mut std::net::TcpStream) -> String {
        let mut buffer = vec![0_u8; 8192];
        let mut request = Vec::new();
        loop {
            let size = stream.read(&mut buffer).expect("read request");
            if size == 0 {
                break;
            }
            request.extend_from_slice(&buffer[..size]);
            if request.windows(4).any(|window| window == b"\r\n\r\n") {
                break;
            }
        }
        String::from_utf8_lossy(request.as_slice()).to_string()
    }
}
