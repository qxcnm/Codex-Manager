use super::*;
use std::net::Shutdown;

/// A real HTTP provider socket that emits a delta, then keeps the SSE body open
/// without terminal events or usage. Only cancellation closes this response.
struct OpenStreamProvider {
    addr: String,
    cancel: mpsc::Sender<()>,
    finished: Receiver<bool>,
    join: Option<thread::JoinHandle<()>>,
}

impl OpenStreamProvider {
    fn start() -> Self {
        let listener = bind_test_listener("disconnect provider");
        let addr = listener.local_addr().unwrap().to_string();
        listener.set_nonblocking(true).unwrap();
        let (cancel, cancelled) = mpsc::channel();
        let (done, finished) = mpsc::channel();
        let join = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(20);
            let mut stream = loop {
                if cancelled.try_recv().is_ok() || Instant::now() >= deadline {
                    let _ = done.send(false);
                    return;
                }
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => panic!("accept open provider request: {error}"),
                }
            };
            stream
                .set_read_timeout(Some(Duration::from_millis(100)))
                .unwrap();
            stream
                .set_write_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            let mut input = Vec::new();
            let mut bytes = [0u8; 4096];
            loop {
                let count = stream.read(&mut bytes).expect("read provider request");
                assert!(count > 0, "provider request closed before headers");
                input.extend_from_slice(&bytes[..count]);
                if let Some(end) = input.windows(4).position(|chunk| chunk == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&input[..end]);
                    assert!(headers.lines().next().unwrap().contains("/responses "));
                    let length: usize = headers
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse().unwrap())
                        })
                        .expect("provider request content length");
                    while input.len() < end + 4 + length {
                        let count = stream.read(&mut bytes).expect("read provider request body");
                        assert!(count > 0);
                        input.extend_from_slice(&bytes[..count]);
                    }
                    break;
                }
                assert!(input.len() < 128 * 1024, "fixture request header bound");
            }
            stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n").unwrap();
            let frame = b"data: {\"type\":\"response.output_text.delta\",\"delta\":\"disconnect fixture delta\"}\n\n";
            write!(stream, "{:x}\r\n", frame.len()).unwrap();
            stream.write_all(frame).unwrap();
            stream.write_all(b"\r\n").unwrap();
            stream.flush().unwrap();
            let disconnected = loop {
                if cancelled.try_recv().is_ok() || Instant::now() >= deadline {
                    break false;
                }
                match stream.read(&mut bytes) {
                    Ok(0) => break true,
                    Ok(_) => continue,
                    Err(error)
                        if matches!(
                            error.kind(),
                            std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                        ) =>
                    {
                        continue
                    }
                    Err(_) => break true,
                }
            };
            let _ = done.send(disconnected);
        });
        Self {
            addr,
            cancel,
            finished,
            join: Some(join),
        }
    }

    fn finish(mut self) {
        assert!(
            self.finished
                .recv_timeout(Duration::from_secs(5))
                .expect("upstream cancellation timeout"),
            "gateway must cancel the open upstream SSE body"
        );
        self.join
            .take()
            .unwrap()
            .join()
            .expect("provider thread did not panic");
    }
}

impl Drop for OpenStreamProvider {
    fn drop(&mut self) {
        let _ = self.cancel.send(());
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn exercise(
    client: &Client,
    runtime: &tokio::runtime::Runtime,
    kind: StorageBackendKind,
    url: &str,
    platform_key: &str,
    key_id: &str,
    model: &str,
    wallet_id: &str,
    stamp: u128,
) {
    let provider = OpenStreamProvider::start();
    let _provider_url = EnvGuard::set(
        "CODEXMANAGER_UPSTREAM_BASE_URL",
        &format!("http://{}/backend-api/codex", provider.addr),
    );
    let server = PersistentServer::start(client);
    let request_trace_id = format!("fixture-disconnect-{stamp}");
    let body = serde_json::json!({"model":model,"input":"disconnect before EOS","stream":true})
        .to_string();
    let mut stream = TcpStream::connect(("127.0.0.1", server.port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    write!(stream, "POST /v1/responses HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nAuthorization: Bearer {}\r\nX-Request-Id: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", server.addr, platform_key, request_trace_id, body.len(), body).unwrap();
    stream.flush().unwrap();
    let mut response = Vec::new();
    let mut chunk = [0u8; 4096];
    let trace_id = loop {
        let count = stream.read(&mut chunk).expect("receive live SSE delta");
        assert!(count > 0, "SSE ended before client cancellation");
        response.extend_from_slice(&chunk[..count]);
        let text = String::from_utf8_lossy(&response);
        if text.contains("disconnect fixture delta") {
            assert!(text.starts_with("HTTP/1.1 200"));
            assert!(text.contains("text/event-stream"));
            assert!(!text.contains("response.completed"));
            let request_id = text
                .lines()
                .skip(1)
                .take_while(|line| !line.trim().is_empty())
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("x-request-id")
                        .then(|| value.trim())
                })
                .expect("transport request id header");
            assert_eq!(
                request_id, request_trace_id,
                "transport correlation keeps caller id"
            );
            // Gateway request_logs use the independent internal trace header;
            // transport x-request-id remains caller-controlled.
            break text
                .lines()
                .skip(1)
                .take_while(|line| !line.trim().is_empty())
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("x-codexmanager-trace-id")
                        .then(|| value.trim().to_owned())
                })
                .filter(|value| !value.is_empty())
                .expect("server-generated gateway trace header");
        }
        assert!(response.len() < 128 * 1024, "fixture response bound");
    };
    stream.shutdown(Shutdown::Both).unwrap();
    drop(stream);
    // No DB polling or grace period: server shutdown must drain the body-drop
    // finalizer and commit the cancellation log and its existing billing policy.
    server.stop();
    provider.finish();
    let remote = runtime.block_on(SeaOrmStorage::connect(kind, url)).unwrap();
    runtime.block_on(async {
        let scope = RequestLogFilter {
            key_ids: Some(vec![key_id.to_owned()]),
            ..Default::default()
        };
        let logs = RequestLogsRepository::list_filtered(remote.connection(), &scope, 0, 10)
            .await
            .unwrap();
        assert_eq!(logs.len(), 5);
        let cancelled = logs
            .iter()
            .find(|log| log.trace_id.as_deref() == Some(trace_id.as_str()))
            .expect("durable cancellation log");
        assert_eq!(cancelled.status_code, Some(499));
        assert!(cancelled.error.is_some());
        let entries = BillingRepository::ledger(remote.connection(), wallet_id, 10)
            .await
            .unwrap();
        assert_eq!(
            entries.len(),
            5,
            "existing policy charges estimated usage on client cancellation"
        );
        let mut cancellation_charge = None;
        for entry in &entries {
            let request = entry.request_log_id.expect("ledger request reference");
            let log = RequestLogsRepository::get(remote.connection(), request)
                .await
                .unwrap()
                .unwrap();
            if log.log.trace_id.as_deref() != Some(trace_id.as_str()) {
                continue;
            }
            let snapshot = BillingRepository::snapshot(remote.connection(), request)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(snapshot.usage_source, "estimated");
            assert!(snapshot.input_tokens > 0);
            assert_eq!(snapshot.output_tokens, 0);
            assert_eq!(snapshot.rate_multiplier_millis, 1500);
            assert_eq!(snapshot.base_cost_microusd, snapshot.input_tokens * 2);
            assert_eq!(snapshot.charged_cost_microusd, snapshot.input_tokens * 3);
            assert_eq!(entry.entry_kind, "request_charge");
            assert_eq!(entry.amount_credit_micros, -snapshot.charged_cost_microusd);
            cancellation_charge = Some(snapshot.charged_cost_microusd);
        }
        let charge = cancellation_charge.expect("cancellation charge snapshot and ledger entry");
        let wallet = BillingRepository::wallet(remote.connection(), wallet_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(wallet.balance_credit_micros, 1_000_000 - 48 - charge);
    });
}
