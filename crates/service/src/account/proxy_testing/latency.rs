use codexmanager_core::rpc::types::ProxyProfileUrlTestEntry;
use std::future::Future;
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use tokio::runtime::{Builder, Runtime};

use super::errors::{
    map_proxy_test_reqwest_error, proxy_test_result_error_code, proxy_test_result_status,
    ProxyTestError,
};

static PROXY_TEST_RUNTIME: OnceLock<Runtime> = OnceLock::new();

#[derive(Debug, Clone)]
pub(crate) struct ProxyLatencyTestOutcome {
    pub status: String,
    pub url_latency_ms: Option<i64>,
    pub status_code: Option<i64>,
    pub test_url: String,
    pub final_url: Option<String>,
    pub redirected: bool,
    pub tested_at: i64,
    pub error_code: Option<String>,
    pub error: Option<String>,
}

async fn measure_single_latency(
    client: &reqwest::Client,
    target_url: &str,
    expect_http_204: bool,
) -> Result<u64, String> {
    let request_started = Instant::now();
    let response = client.get(target_url).send().await
        .map_err(|err| format!("request error: {err}"))?;

    let latency_ms = request_started.elapsed().as_millis() as u64;

    let status = response.status();
    if status.is_redirection() {
        return Err("redirect detected".to_string());
    }

    let body = response.bytes().await
        .map_err(|err| format!("body read error: {err}"))?;

    if expect_http_204 {
        if status == reqwest::StatusCode::NO_CONTENT && body.is_empty() {
            return Ok(latency_ms);
        }
        return Err(format!("expected HTTP 204 with empty body, got HTTP {}", status));
    }

    if status.is_success() {
        return Ok(latency_ms);
    }

    Err(format!("HTTP status error: {}", status))
}

pub(crate) fn run_proxy_latency_test(
    proxy_url: &str,
    target_url: &str,
    expect_http_204: bool,
) -> ProxyLatencyTestOutcome {
    run_proxy_test_future(async move {
        let (client, _) = match super::client::build_proxy_test_client(
            proxy_url,
            super::client::ProxyTestRedirectPolicy::None,
            false,
        ) {
            Ok(result) => result,
            Err(err) => return builder_error_to_outcome(target_url, err),
        };

        // 1. Warmup request (checks redirection)
        let tested_at = codexmanager_core::storage::now_ts();
        let warmup_started = Instant::now();
        let warmup_resp = match client.get(target_url).send().await {
            Ok(resp) => resp,
            Err(err) => return reqwest_error_to_outcome("proxy latency test warmup", proxy_url, target_url, err),
        };
        let warmup_latency_ms = warmup_started.elapsed().as_millis() as i64;

        let status = warmup_resp.status();
        if status.is_redirection() {
            let status_code = Some(i64::from(status.as_u16()));
            let final_url = resolve_redirect_location(warmup_resp.url().as_str(), warmup_resp.headers());
            return ProxyLatencyTestOutcome {
                status: "failed".to_string(),
                url_latency_ms: Some(warmup_latency_ms),
                status_code,
                test_url: target_url.to_string(),
                final_url: final_url.clone(),
                redirected: true,
                tested_at,
                error_code: Some("redirect_detected".to_string()),
                error: Some(match final_url {
                    Some(url) => format!("redirect detected: {url}"),
                    None => "redirect detected".to_string(),
                }),
            };
        }

        let num_samples = 10;
        let mut successful_samples = Vec::with_capacity(num_samples);

        let tested_at = codexmanager_core::storage::now_ts();

        for i in 0..num_samples {
            if i > 0 {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }

            match measure_single_latency(&client, target_url, expect_http_204).await {
                Ok(latency_ms) => {
                    successful_samples.push(latency_ms);
                }
                Err(err) => {
                    log::warn!("Proxy latency sample {} failed: {}", i + 1, err);
                }
            }
        }

        if successful_samples.is_empty() {
            let single_test = client.get(target_url).send().await;
            match single_test {
                Ok(resp) => {
                    let status = resp.status();
                    return ProxyLatencyTestOutcome {
                        status: "failed".to_string(),
                        url_latency_ms: None,
                        status_code: Some(status.as_u16() as i64),
                        test_url: target_url.to_string(),
                        final_url: None,
                        redirected: status.is_redirection(),
                        tested_at,
                        error_code: Some("all_samples_failed".to_string()),
                        error: Some("All 10 latency test samples failed".to_string()),
                    };
                }
                Err(err) => {
                    return reqwest_error_to_outcome("proxy latency test", proxy_url, target_url, err);
                }
            }
        }

        successful_samples.sort_unstable();
        let median_latency = if successful_samples.len() % 2 == 1 {
            successful_samples[successful_samples.len() / 2]
        } else {
            let mid = successful_samples.len() / 2;
            (successful_samples[mid - 1] + successful_samples[mid]) / 2
        };

        ProxyLatencyTestOutcome {
            status: "ok".to_string(),
            url_latency_ms: Some(median_latency as i64),
            status_code: Some(if expect_http_204 { 204 } else { 200 }),
            test_url: target_url.to_string(),
            final_url: None,
            redirected: false,
            tested_at,
            error_code: None,
            error: None,
        }
    })
}

#[allow(dead_code)]
pub(crate) fn proxy_profile_url_test_entry(
    test: codexmanager_core::storage::ProxyProfileUrlTest,
) -> ProxyProfileUrlTestEntry {
    ProxyProfileUrlTestEntry {
        id: test.id,
        proxy_profile_id: test.proxy_profile_id,
        status: test.status,
        url_latency_ms: test.url_latency_ms,
        status_code: test.status_code,
        test_url: test.test_url,
        final_url: test.final_url,
        redirected: test.redirected,
        tested_at: test.tested_at,
        error_code: test.error_code,
        error: test.error,
    }
}

fn proxy_test_runtime() -> &'static Runtime {
    PROXY_TEST_RUNTIME.get_or_init(|| {
        Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .thread_name("proxy-test-http")
            .build()
            .unwrap_or_else(|err| panic!("build proxy test runtime failed: {err}"))
    })
}

fn run_proxy_test_future<F>(future: F) -> F::Output
where
    F: Future,
{
    proxy_test_runtime().block_on(future)
}

fn builder_error_to_outcome(target_url: &str, err: ProxyTestError) -> ProxyLatencyTestOutcome {
    ProxyLatencyTestOutcome {
        status: proxy_test_result_status(&err).to_string(),
        url_latency_ms: None,
        status_code: None,
        test_url: target_url.to_string(),
        final_url: None,
        redirected: false,
        tested_at: codexmanager_core::storage::now_ts(),
        error_code: Some(proxy_test_result_error_code(&err).to_string()),
        error: Some(err.message),
    }
}

fn reqwest_error_to_outcome(
    action: &str,
    proxy_url: &str,
    target_url: &str,
    err: reqwest::Error,
) -> ProxyLatencyTestOutcome {
    let mapped = map_proxy_test_reqwest_error(action, proxy_url, err);
    ProxyLatencyTestOutcome {
        status: proxy_test_result_status(&mapped).to_string(),
        url_latency_ms: None,
        status_code: None,
        test_url: target_url.to_string(),
        final_url: None,
        redirected: false,
        tested_at: codexmanager_core::storage::now_ts(),
        error_code: Some(proxy_test_result_error_code(&mapped).to_string()),
        error: Some(mapped.message),
    }
}

fn resolve_redirect_location(
    request_url: &str,
    headers: &reqwest::header::HeaderMap,
) -> Option<String> {
    let location = headers
        .get(reqwest::header::LOCATION)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())?;

    match url::Url::parse(request_url) {
        Ok(base) => base
            .join(location)
            .map(|resolved| resolved.to_string())
            .ok()
            .or_else(|| Some(location.to_string())),
        Err(_) => Some(location.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::run_proxy_latency_test;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;
    use std::thread;

    #[test]
    fn latency_test_reports_redirect_without_following_it() {
        let (proxy_url, _rx, handle) = start_fake_proxy_response(
            "HTTP/1.1 302 Found\r\nLocation: /login\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        );

        let result =
            run_proxy_latency_test(proxy_url.as_str(), "http://example.test/generate_204", true);

        handle.join().expect("join fake proxy");

        assert_eq!(result.status, "failed");
        assert_eq!(result.status_code, Some(302));
        assert!(result.redirected);
        assert_eq!(
            result.final_url.as_deref(),
            Some("http://example.test/login")
        );
        assert_eq!(result.error_code.as_deref(), Some("redirect_detected"));
    }

    #[test]
    fn latency_test_accepts_204_empty_body_success() {
        let (proxy_url, _rx, handle) = start_fake_proxy_response(
            "HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        );

        let result =
            run_proxy_latency_test(proxy_url.as_str(), "http://example.test/generate_204", true);

        handle.join().expect("join fake proxy");

        assert_eq!(result.status, "ok");
        assert_eq!(result.status_code, Some(204));
        assert!(!result.redirected);
        assert!(result.url_latency_ms.is_some());
        assert_eq!(result.error_code, None);
    }

    fn start_fake_proxy_response(
        response: &'static str,
    ) -> (String, mpsc::Receiver<String>, thread::JoinHandle<()>) {
        use std::time::{Duration, Instant};
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake proxy");
        let addr = listener.local_addr().expect("fake proxy addr");
        let proxy_url = format!("http://{addr}");
        let (tx, rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            let _ = listener.set_nonblocking(true);
            for _ in 0..10 {
                let mut stream_opt = None;
                let start_wait = Instant::now();
                while start_wait.elapsed() < Duration::from_millis(500) {
                    if let Ok((stream, _)) = listener.accept() {
                        stream_opt = Some(stream);
                        break;
                    }
                    thread::sleep(Duration::from_millis(10));
                }

                let Some(mut stream) = stream_opt else {
                    break;
                };

                let mut buffer = vec![0_u8; 8192];
                if let Ok(size) = stream.read(&mut buffer) {
                    let request = String::from_utf8_lossy(&buffer[..size]).to_string();
                    if tx.send(request).is_err() {
                        break;
                    }
                    let _ = stream.write_all(response.as_bytes());
                }
            }
        });
        (proxy_url, rx, handle)
    }
}
