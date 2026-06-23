use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tiny_http::{Header, Response, Server, StatusCode};
use url::Url;

use super::config::CfStyleConfig;
use super::runner::run_cf_style_speed_test;
use super::model::CfStyleStatus;

fn spawn_mock_server() -> (String, thread::JoinHandle<()>, Arc<AtomicBool>) {
    let server = Server::http("127.0.0.1:0").unwrap();
    let addr = format!("http://{}", server.server_addr());
    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_clone = shutdown.clone();

    let handle = thread::spawn(move || {
        while !shutdown_clone.load(Ordering::Relaxed) {
            if let Ok(Some(mut request)) = server.recv_timeout(Duration::from_millis(50)) {
                let request_url = request.url();
                let parsed = if request_url.starts_with('/') {
                    Url::parse(&format!("http://127.0.0.1:{}", request_url)).unwrap()
                } else {
                    Url::parse(request_url).unwrap()
                };

                match parsed.path() {
                    "/meta" => {
                        let body = r#"{"clientIp": "127.0.0.1", "country": "US", "colo": "SFO"}"#;
                        let response = Response::from_string(body)
                            .with_header(Header::from_bytes("Content-Type", "application/json").unwrap())
                            .with_status_code(StatusCode(200));
                        let _ = request.respond(response);
                    }
                    "/cdn-cgi/trace" => {
                        let body = "ip=127.0.0.1\nloc=US\ncolo=SFO\n";
                        let response = Response::from_string(body)
                            .with_header(Header::from_bytes("Content-Type", "text/plain").unwrap())
                            .with_status_code(StatusCode(200));
                        let _ = request.respond(response);
                    }
                    "/__down" => {
                        let bytes_len = parsed.query_pairs()
                            .find(|(k, _)| k == "bytes")
                            .and_then(|(_, v)| v.parse::<usize>().ok())
                            .unwrap_or(0);
                        let body = vec![0u8; bytes_len];
                        let response = Response::from_data(body)
                            .with_header(Header::from_bytes("Content-Type", "application/octet-stream").unwrap())
                            .with_status_code(StatusCode(200));
                        let _ = request.respond(response);
                    }
                    "/__up" => {
                        let mut reader = request.as_reader();
                        let mut buf = vec![0u8; 4096];
                        while let Ok(n) = std::io::Read::read(&mut reader, &mut buf) {
                            if n == 0 {
                                break;
                            }
                        }
                        let response = Response::empty(200);
                        let _ = request.respond(response);
                    }
                    _ => {
                        let response = Response::empty(404);
                        let _ = request.respond(response);
                    }
                }
            }
        }
    });

    (addr, handle, shutdown)
}

#[tokio::test]
async fn test_cf_style_speed_test_success() {
    let (server_addr, handle, shutdown) = spawn_mock_server();

    // Use Quick preset but configure base_url to point to mock server
    let config = CfStyleConfig {
        download_preset: Some("1mb".to_string()),
        upload_preset: Some("1mb".to_string()),
        timeout_secs: 10,
        run_upload: Some(true),
        base_url: server_addr.clone(),
    };

    let result = run_cf_style_speed_test(
        &server_addr,
        &config,
        || false,
        |_phase| {},
        |_bytes, _mbps| {},
        |_bytes, _mbps| {},
    )
    .await;

    shutdown.store(true, Ordering::Relaxed);
    let _ = handle.join();

    assert_eq!(result.status, CfStyleStatus::Ok);
    assert!(result.errors.is_empty());

    let latency = result.latency.expect("latency result");
    assert!(latency.median_ms >= 0.0);
    assert_eq!(latency.raw_samples_ms.len(), 5);

    let download = result.download.expect("download result");
    assert!(download.final_mbps >= 0.0);
    assert!(!download.runs.is_empty());

    let upload = result.upload.expect("upload result");
    assert!(upload.final_mbps >= 0.0);
    assert!(!upload.runs.is_empty());

    let endpoint = result.endpoint_info;
    assert_eq!(endpoint.observed_ip.as_deref(), Some("127.0.0.1"));
    assert_eq!(endpoint.observed_country.as_deref(), Some("US"));
    assert_eq!(endpoint.observed_colo.as_deref(), Some("SFO"));
}

#[tokio::test]
async fn test_cf_style_speed_test_cancel() {
    let (server_addr, handle, shutdown) = spawn_mock_server();

    let config = CfStyleConfig {
        download_preset: Some("1mb".to_string()),
        upload_preset: Some("1mb".to_string()),
        timeout_secs: 10,
        run_upload: Some(true),
        base_url: server_addr.clone(),
    };

    // Cancel instantly (should_cancel returns true)
    let result = run_cf_style_speed_test(
        &server_addr,
        &config,
        || true,
        |_phase| {},
        |_bytes, _mbps| {},
        |_bytes, _mbps| {},
    )
    .await;

    shutdown.store(true, Ordering::Relaxed);
    let _ = handle.join();

    assert_eq!(result.status, CfStyleStatus::Cancelled);
    assert!(result.latency.is_none());
    assert!(result.download.is_none());
}

#[tokio::test]
async fn test_cf_style_speed_test_preflight_fail_but_continues() {
    // Start mock server that returns 500 for preflight but works for others
    let server = Server::http("127.0.0.1:0").unwrap();
    let server_addr = format!("http://{}", server.server_addr());
    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_clone = shutdown.clone();

    let srv_handle = thread::spawn(move || {
        while !shutdown_clone.load(Ordering::Relaxed) {
            if let Ok(Some(mut request)) = server.recv_timeout(Duration::from_millis(50)) {
                let request_url = request.url();
                let parsed = if request_url.starts_with('/') {
                    Url::parse(&format!("http://127.0.0.1:{}", request_url)).unwrap()
                } else {
                    Url::parse(request_url).unwrap()
                };

                match parsed.path() {
                    "/meta" | "/cdn-cgi/trace" => {
                        let response = Response::empty(500);
                        let _ = request.respond(response);
                    }
                    "/__down" => {
                        let bytes_len = parsed.query_pairs()
                            .find(|(k, _)| k == "bytes")
                            .and_then(|(_, v)| v.parse::<usize>().ok())
                            .unwrap_or(0);
                        let body = vec![0u8; bytes_len];
                        let response = Response::from_data(body)
                            .with_status_code(StatusCode(200));
                        let _ = request.respond(response);
                    }
                    "/__up" => {
                        let mut reader = request.as_reader();
                        let mut buf = vec![0u8; 4096];
                        while let Ok(n) = std::io::Read::read(&mut reader, &mut buf) {
                            if n == 0 {
                                break;
                            }
                        }
                        let response = Response::empty(200);
                        let _ = request.respond(response);
                    }
                    _ => {
                        let response = Response::empty(404);
                        let _ = request.respond(response);
                    }
                }
            }
        }
    });

    let config = CfStyleConfig {
        download_preset: Some("1mb".to_string()),
        upload_preset: Some("1mb".to_string()),
        timeout_secs: 10,
        run_upload: Some(true),
        base_url: server_addr.clone(),
    };

    let result = run_cf_style_speed_test(
        &server_addr,
        &config,
        || false,
        |_phase| {},
        |_bytes, _mbps| {},
        |_bytes, _mbps| {},
    )
    .await;

    shutdown.store(true, Ordering::Relaxed);
    let _ = srv_handle.join();

    // The test status should be Partial since preflight failed but download/upload ran and succeeded
    assert_eq!(result.status, CfStyleStatus::Partial);
    assert!(!result.errors.is_empty());
    assert_eq!(result.errors[0].phase, "preflight");

    assert!(result.latency.is_some());
    assert!(result.download.is_some());
    assert!(result.upload.is_some());
}
