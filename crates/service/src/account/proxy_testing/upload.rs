use std::future::Future;
use std::pin::pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use futures_util::stream::unfold;
use tokio::runtime::{Builder, Runtime};

use super::errors::{
    map_proxy_test_reqwest_error, proxy_test_result_error_code, proxy_test_result_status,
    ProxyTestError,
};

const CANCEL_POLL_INTERVAL: Duration = Duration::from_millis(100);

static PROXY_UPLOAD_TEST_RUNTIME: OnceLock<Runtime> = OnceLock::new();

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct ProxyUploadTestOutcome {
    pub status: String,
    pub upload_url: Option<String>,
    pub size_bytes: i64,
    pub bytes_written: i64,
    pub duration_ms: i64,
    pub upload_mbps: Option<f64>,
    pub status_code: Option<i64>,
    pub tested_at: i64,
    pub cancelled: bool,
    pub error_code: Option<String>,
    pub error: Option<String>,
}

#[allow(dead_code)]
pub(crate) fn run_proxy_upload_test(
    proxy_url: &str,
    upload_bytes: u64,
) -> ProxyUploadTestOutcome {
    run_proxy_upload_test_with_cancel(proxy_url, upload_bytes, || false, |_| {})
}

pub(crate) fn run_proxy_upload_test_with_cancel<F, P>(
    proxy_url: &str,
    upload_bytes: u64,
    should_cancel: F,
    on_progress: P,
) -> ProxyUploadTestOutcome
where
    F: Fn() -> bool,
    P: Fn(u64) + Send + Sync + Clone + 'static,
{
    let endpoint_status = super::presets::upload_endpoint_status();
    if !endpoint_status.configured {
        let err = super::errors::upload_endpoint_not_configured_error();
        return ProxyUploadTestOutcome {
            status: proxy_test_result_status(&err).to_string(),
            upload_url: None,
            size_bytes: upload_bytes as i64,
            bytes_written: 0,
            duration_ms: 0,
            upload_mbps: None,
            status_code: None,
            tested_at: codexmanager_core::storage::now_ts(),
            cancelled: false,
            error_code: Some(proxy_test_result_error_code(&err).to_string()),
            error: Some(err.message),
        };
    }
    let upload_url = endpoint_status.url.unwrap();

    run_proxy_test_future(async move {
        let started_at = Instant::now();
        if should_cancel() {
            return cancelled_outcome(&upload_url, upload_bytes as i64, 0, started_at);
        }

        let (client, _) = match super::client::build_proxy_test_client(
            proxy_url,
            super::client::ProxyTestRedirectPolicy::Limited(10),
            false,
        ) {
            Ok(result) => result,
            Err(err) => return builder_error_to_outcome(&upload_url, upload_bytes as i64, err, started_at),
        };

        let bytes_written = Arc::new(AtomicU64::new(0));
        let bytes_written_clone = bytes_written.clone();
        let chunk_size = 65536;

        let stream = unfold(0u64, move |sent| {
            let bytes_written_clone = bytes_written_clone.clone();
            let on_progress = on_progress.clone();
            async move {
                if sent >= upload_bytes {
                    return None;
                }
                let len = (upload_bytes - sent).min(chunk_size as u64) as usize;
                let chunk = bytes::Bytes::from(vec![0u8; len]);
                let written = bytes_written_clone.fetch_add(len as u64, Ordering::SeqCst) + len as u64;
                on_progress(written);
                Some((Ok::<_, std::io::Error>(chunk), sent + len as u64))
            }
        });

        let body = reqwest::Body::wrap_stream(stream);

        let response_future = client
            .post(upload_url.as_str())
            .body(body)
            .send();

        let send_result = poll_future_with_cancel(response_future, &should_cancel).await;

        let final_bytes = bytes_written.load(Ordering::SeqCst) as i64;
        let duration_ms = elapsed_ms(started_at);

        match send_result {
            PollWithCancel::Ready(Ok(response)) => {
                let status = response.status();
                let status_code = Some(i64::from(status.as_u16()));
                if !status.is_success() {
                    return ProxyUploadTestOutcome {
                        status: "failed".to_string(),
                        upload_url: Some(upload_url.clone()),
                        size_bytes: upload_bytes as i64,
                        bytes_written: final_bytes,
                        duration_ms,
                        upload_mbps: None,
                        status_code,
                        tested_at: codexmanager_core::storage::now_ts(),
                        cancelled: false,
                        error_code: Some("http_status_error".to_string()),
                        error: Some(format!("upload URL returned HTTP {}", status)),
                    };
                }

                ProxyUploadTestOutcome {
                    status: "ok".to_string(),
                    upload_url: Some(upload_url.clone()),
                    size_bytes: upload_bytes as i64,
                    bytes_written: final_bytes,
                    duration_ms,
                    upload_mbps: compute_mbps(final_bytes, duration_ms),
                    status_code,
                    tested_at: codexmanager_core::storage::now_ts(),
                    cancelled: false,
                    error_code: None,
                    error: None,
                }
            }
            PollWithCancel::Ready(Err(err)) => {
                reqwest_error_to_outcome(
                    "proxy upload test",
                    proxy_url,
                    &upload_url,
                    upload_bytes as i64,
                    final_bytes,
                    err,
                    started_at,
                )
            }
            PollWithCancel::Cancelled => {
                cancelled_outcome(&upload_url, upload_bytes as i64, final_bytes, started_at)
            }
        }
    })
}

fn proxy_test_runtime() -> &'static Runtime {
    PROXY_UPLOAD_TEST_RUNTIME.get_or_init(|| {
        Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .thread_name("proxy-upload-test")
            .build()
            .unwrap_or_else(|err| panic!("build proxy upload test runtime failed: {err}"))
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
    upload_url: &str,
    size_bytes: i64,
    err: ProxyTestError,
    started_at: Instant,
) -> ProxyUploadTestOutcome {
    ProxyUploadTestOutcome {
        status: proxy_test_result_status(&err).to_string(),
        upload_url: Some(upload_url.to_string()),
        size_bytes,
        bytes_written: 0,
        duration_ms: elapsed_ms(started_at),
        upload_mbps: None,
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
    upload_url: &str,
    size_bytes: i64,
    bytes_written: i64,
    err: reqwest::Error,
    started_at: Instant,
) -> ProxyUploadTestOutcome {
    let mapped = map_proxy_test_reqwest_error(action, proxy_url, err);
    let duration_ms = elapsed_ms(started_at);
    ProxyUploadTestOutcome {
        status: proxy_test_result_status(&mapped).to_string(),
        upload_url: Some(upload_url.to_string()),
        size_bytes,
        bytes_written,
        duration_ms,
        upload_mbps: compute_mbps(bytes_written, duration_ms),
        status_code: None,
        tested_at: codexmanager_core::storage::now_ts(),
        cancelled: false,
        error_code: Some(proxy_test_result_error_code(&mapped).to_string()),
        error: Some(mapped.message),
    }
}

fn cancelled_outcome(
    upload_url: &str,
    size_bytes: i64,
    bytes_written: i64,
    started_at: Instant,
) -> ProxyUploadTestOutcome {
    let duration_ms = elapsed_ms(started_at);
    ProxyUploadTestOutcome {
        status: "cancelled".to_string(),
        upload_url: Some(upload_url.to_string()),
        size_bytes,
        bytes_written,
        duration_ms,
        upload_mbps: compute_mbps(bytes_written, duration_ms),
        status_code: None,
        tested_at: codexmanager_core::storage::now_ts(),
        cancelled: true,
        error_code: Some("cancelled".to_string()),
        error: Some("proxy upload test cancelled".to_string()),
    }
}

fn elapsed_ms(started_at: Instant) -> i64 {
    started_at.elapsed().as_millis().min(i64::MAX as u128) as i64
}

fn compute_mbps(bytes_written: i64, duration_ms: i64) -> Option<f64> {
    if bytes_written <= 0 || duration_ms <= 0 {
        return None;
    }
    let seconds = duration_ms as f64 / 1000.0;
    if seconds <= 0.0 {
        return None;
    }
    Some((bytes_written as f64 * 8.0) / seconds / 1_000_000.0)
}

#[cfg(test)]
mod tests {
    use super::run_proxy_upload_test_with_cancel;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn upload_test_fails_when_endpoint_not_configured() {
        let _guard = crate::test_env_guard();
        std::env::remove_var("CODEXMANAGER_PROXY_TEST_UPLOAD_URL");

        let result = run_proxy_upload_test_with_cancel("socks5h://127.0.0.1:1080", 1000, || false, |_| {});

        assert_eq!(result.status, "failed");
        assert_eq!(result.error_code.as_deref(), Some("upload_endpoint_not_configured"));
        assert!(result.error.unwrap().contains("Upload endpoint is not configured"));
    }

    #[test]
    fn upload_test_streams_to_configured_endpoint_and_reports_bytes_mbps() {
        let _guard = crate::test_env_guard();

        let (proxy_url, rx, handle) = start_fake_proxy_upload_endpoint(
            "HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n",
            Duration::from_millis(50),
        );

        std::env::set_var("CODEXMANAGER_PROXY_TEST_UPLOAD_URL", "http://example.com/proxy-test-upload");

        let result = run_proxy_upload_test_with_cancel(proxy_url.as_str(), 100000, || false, |_| {});

        let request = rx.recv().expect("captured request");
        handle.join().expect("join fake proxy");

        std::env::remove_var("CODEXMANAGER_PROXY_TEST_UPLOAD_URL");

        assert_eq!(result.status, "ok");
        assert_eq!(result.status_code, Some(204));
        assert_eq!(result.bytes_written, 100000);
        assert!(result.duration_ms >= 50);
        assert!(result.upload_mbps.unwrap_or_default() > 0.0);
        assert_eq!(result.error_code, None);
        assert!(request.contains("POST"));
    }

    fn start_fake_proxy_upload_endpoint(
        response: &'static str,
        delay: Duration,
    ) -> (String, mpsc::Receiver<String>, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake proxy");
        let addr = listener.local_addr().expect("fake proxy addr");
        let proxy_url = format!("http://{addr}");
        let (tx, rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept proxy connection");
            let request = read_request_headers(&mut stream);
            tx.send(request).expect("send request");

            // Читаем все тело chunked до конца
            read_request_body_chunked(&mut stream);

            thread::sleep(delay);

            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
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

    fn read_request_body_chunked(stream: &mut std::net::TcpStream) {
        let mut buffer = [0_u8; 8192];
        let mut temp = Vec::new();
        loop {
            match stream.read(&mut buffer) {
                Ok(0) => break, // EOF
                Ok(size) => {
                    temp.extend_from_slice(&buffer[..size]);
                    // Ищем терминатор chunked-тела "0\r\n\r\n"
                    if temp.windows(5).any(|window| window == b"0\r\n\r\n") {
                        break;
                    }
                    if temp.len() > 10 {
                        let drain_len = temp.len() - 10;
                        temp.drain(0..drain_len);
                    }
                }
                Err(_) => break,
            }
        }
    }
}
