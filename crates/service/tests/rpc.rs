use codexmanager_core::rpc::types::JsonRpcRequest;
use codexmanager_core::storage::{
    now_ts, Account, Event, ProxyProfileCreateInput, RequestLog, RequestTokenStat, Storage, Token,
    UsageSnapshotRecord,
};
use std::fs;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::MutexGuard;
use std::thread;
use std::time::Duration;
use tiny_http::{Header, Response, Server, StatusCode};

mod support;
use support::{test_env_guard, EnvGuard};
static RPC_TEST_DIR_SEQ: AtomicUsize = AtomicUsize::new(0);

/// 函数 `new_test_dir`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - prefix: 参数 prefix
///
/// # 返回
/// 返回函数执行结果
fn new_test_dir(prefix: &str) -> PathBuf {
    // 中文注释：用进程号 + 自增序号构造临时目录，避免 Windows 复用旧目录导致脏数据串用。
    let seq = RPC_TEST_DIR_SEQ.fetch_add(1, Ordering::Relaxed);
    let mut dir = std::env::temp_dir();
    dir.push(format!("{prefix}-{}-{seq}", std::process::id()));
    let _ = fs::create_dir_all(&dir);
    dir
}

struct RpcTestContext {
    _env_lock: MutexGuard<'static, ()>,
    _db_path_guard: EnvGuard,
    _auto_usage_refresh_guard: EnvGuard,
    dir: PathBuf,
}

impl RpcTestContext {
    /// 函数 `new`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - prefix: 参数 prefix
    ///
    /// # 返回
    /// 返回函数执行结果
    fn new(prefix: &str) -> Self {
        let env_lock = test_env_guard();
        let dir = new_test_dir(prefix);
        let db_path = dir.join("codexmanager.db");
        let db_path_guard =
            EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());
        let auto_usage_refresh_guard =
            EnvGuard::set("CODEXMANAGER_AUTO_USAGE_REFRESH_AFTER_ACCOUNT_ADD", "0");
        Self {
            _env_lock: env_lock,
            _db_path_guard: db_path_guard,
            _auto_usage_refresh_guard: auto_usage_refresh_guard,
            dir,
        }
    }

    /// 函数 `db_path`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    ///
    /// # 返回
    /// 返回函数执行结果
    fn db_path(&self) -> PathBuf {
        self.dir.join("codexmanager.db")
    }

    /// 函数 `seed_accounts`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - count: 参数 count
    ///
    /// # 返回
    /// 无
    fn seed_accounts(&self, count: usize) {
        let storage = Storage::open(self.db_path()).expect("open db");
        storage.init().expect("init schema");
        let now = now_ts();
        for idx in 0..count {
            let sort = idx as i64;
            storage
                .insert_account(&Account {
                    id: format!("acc-{idx}"),
                    label: format!("Account {idx}"),
                    issuer: "https://auth.openai.com".to_string(),
                    chatgpt_account_id: Some(format!("chatgpt-{idx}")),
                    workspace_id: Some(format!("workspace-{idx}")),
                    group_name: Some(format!("group-{}", idx % 2)),
                    sort,
                    status: "active".to_string(),
                    created_at: now + sort,
                    updated_at: now + sort,
                })
                .expect("insert account");
        }
    }
}

impl Drop for RpcTestContext {
    /// 函数 `drop`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    ///
    /// # 返回
    /// 无
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

/// 函数 `post_rpc_raw`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - addr: 参数 addr
/// - body: 参数 body
/// - headers: 参数 headers
///
/// # 返回
/// 返回函数执行结果
fn post_rpc_raw(addr: &str, body: &str, headers: &[(&str, &str)]) -> (u16, String) {
    let mut stream = TcpStream::connect(addr).expect("connect server");
    let mut request = format!("POST /rpc HTTP/1.1\r\nHost: {addr}\r\n");
    for (name, value) in headers {
        request.push_str(name);
        request.push_str(": ");
        request.push_str(value);
        request.push_str("\r\n");
    }
    request.push_str(&format!("Content-Length: {}\r\n\r\n{}", body.len(), body));
    stream.write_all(request.as_bytes()).expect("write");
    stream.shutdown(std::net::Shutdown::Write).ok();

    let mut buf = String::new();
    stream.read_to_string(&mut buf).expect("read");
    let status = buf
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|value| value.parse::<u16>().ok())
        .expect("status");
    let body = buf.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
    (status, body)
}

/// 函数 `post_rpc`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - addr: 参数 addr
/// - body: 参数 body
///
/// # 返回
/// 返回函数执行结果
fn post_rpc(addr: &str, body: &str) -> serde_json::Value {
    let token = codexmanager_service::rpc_auth_token().to_string();
    let (status, body) = post_rpc_raw(
        addr,
        body,
        &[
            ("Content-Type", "application/json"),
            ("X-CodexManager-Rpc-Token", token.as_str()),
        ],
    );
    assert_eq!(status, 200, "unexpected status {status}: {body}");
    serde_json::from_str(&body).expect("parse response")
}

fn post_rpc_method(
    addr: &str,
    id: i64,
    method: &str,
    params: Option<serde_json::Value>,
) -> serde_json::Value {
    let req = JsonRpcRequest {
        id: id.into(),
        method: method.to_string(),
        params,
        trace: None,
    };
    let json = serde_json::to_string(&req).expect("serialize rpc request");
    post_rpc(addr, &json)
}

fn wait_for_proxy_test_job(job_id: &str) -> serde_json::Value {
    for idx in 0..120 {
        let server = codexmanager_service::start_one_shot_server().expect("start server");
        let resp = post_rpc_method(
            &server.addr,
            9_000 + idx,
            "system/proxy/test-job",
            Some(serde_json::json!({ "jobId": job_id })),
        );
        let result = resp
            .get("result")
            .cloned()
            .expect("job result payload");
        let status = result
            .get("status")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        if matches!(status, "completed" | "failed" | "cancelled") {
            return result;
        }
        thread::sleep(Duration::from_millis(100));
    }
    panic!("proxy test job did not reach terminal state: {job_id}");
}

fn wait_for_account_proxy_test_job(account_id: &str, job_id: &str) -> serde_json::Value {
    for idx in 0..120 {
        let server = codexmanager_service::start_one_shot_server().expect("start server");
        let resp = post_rpc_method(
            &server.addr,
            10_000 + idx,
            "account/proxy/test-job",
            Some(serde_json::json!({
                "accountId": account_id,
                "jobId": job_id,
            })),
        );
        let result = resp
            .get("result")
            .cloned()
            .expect("job result payload");
        let status = result
            .get("status")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        if matches!(status, "completed" | "failed" | "cancelled") {
            return result;
        }
        thread::sleep(Duration::from_millis(100));
    }
    panic!("account proxy test job did not reach terminal state: {job_id}");
}

/// 函数 `encode_base64url`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - bytes: 参数 bytes
///
/// # 返回
/// 返回函数执行结果
fn encode_base64url(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    let mut index = 0;
    while index + 3 <= bytes.len() {
        let chunk = ((bytes[index] as u32) << 16)
            | ((bytes[index + 1] as u32) << 8)
            | (bytes[index + 2] as u32);
        out.push(TABLE[((chunk >> 18) & 0x3f) as usize] as char);
        out.push(TABLE[((chunk >> 12) & 0x3f) as usize] as char);
        out.push(TABLE[((chunk >> 6) & 0x3f) as usize] as char);
        out.push(TABLE[(chunk & 0x3f) as usize] as char);
        index += 3;
    }
    match bytes.len().saturating_sub(index) {
        1 => {
            let chunk = (bytes[index] as u32) << 16;
            out.push(TABLE[((chunk >> 18) & 0x3f) as usize] as char);
            out.push(TABLE[((chunk >> 12) & 0x3f) as usize] as char);
        }
        2 => {
            let chunk = ((bytes[index] as u32) << 16) | ((bytes[index + 1] as u32) << 8);
            out.push(TABLE[((chunk >> 18) & 0x3f) as usize] as char);
            out.push(TABLE[((chunk >> 12) & 0x3f) as usize] as char);
            out.push(TABLE[((chunk >> 6) & 0x3f) as usize] as char);
        }
        _ => {}
    }
    out
}

/// 函数 `build_access_token`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - subject: 参数 subject
/// - email: 参数 email
/// - chatgpt_account_id: 参数 chatgpt_account_id
/// - plan_type: 参数 plan_type
///
/// # 返回
/// 返回函数执行结果
fn build_access_token(
    subject: &str,
    email: &str,
    chatgpt_account_id: &str,
    plan_type: &str,
) -> String {
    let header = encode_base64url(br#"{"alg":"none","typ":"JWT"}"#);
    let payload = serde_json::json!({
        "sub": subject,
        "email": email,
        "workspace_id": chatgpt_account_id,
        "https://api.openai.com/auth": {
            "chatgpt_account_id": chatgpt_account_id,
            "chatgpt_plan_type": plan_type
        }
    });
    let payload = encode_base64url(
        serde_json::to_string(&payload)
            .expect("serialize jwt payload")
            .as_bytes(),
    );
    format!("{header}.{payload}.sig")
}

/// 函数 `start_mock_oauth_token_server`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - status: 参数 status
/// - response_body: 参数 response_body
///
/// # 返回
/// 返回函数执行结果
fn start_mock_oauth_token_server(
    status: u16,
    response_body: String,
) -> (
    String,
    std::sync::mpsc::Receiver<String>,
    thread::JoinHandle<()>,
) {
    let server = Server::http("127.0.0.1:0").expect("start mock oauth server");
    let addr = format!("http://{}", server.server_addr());
    let (tx, rx) = std::sync::mpsc::channel();
    let handle = thread::spawn(move || {
        let mut request = server.recv().expect("receive oauth request");
        let mut body = String::new();
        request
            .as_reader()
            .read_to_string(&mut body)
            .expect("read oauth request body");
        tx.send(body).expect("send oauth request body");
        let response = Response::from_string(response_body)
            .with_status_code(StatusCode(status))
            .with_header(
                Header::from_bytes("Content-Type", "application/json")
                    .expect("content-type header"),
            );
        request.respond(response).expect("respond oauth request");
    });
    (addr, rx, handle)
}

#[derive(Debug)]
struct RecordedHeaderRequest {
    path: String,
    authorization: Option<String>,
    chatgpt_account_id: Option<String>,
}

fn start_mock_subscription_server(
    status: u16,
    response_body: String,
) -> (
    String,
    std::sync::mpsc::Receiver<RecordedHeaderRequest>,
    thread::JoinHandle<()>,
) {
    let server = Server::http("127.0.0.1:0").expect("start mock subscription server");
    let addr = format!("http://{}", server.server_addr());
    let (tx, rx) = std::sync::mpsc::channel();
    let handle = thread::spawn(move || {
        let request = server.recv().expect("receive subscription request");
        let path = request.url().to_string();
        let authorization = request
            .headers()
            .iter()
            .find(|header| header.field.equiv("Authorization"))
            .map(|header| header.value.as_str().to_string());
        let chatgpt_account_id = request
            .headers()
            .iter()
            .find(|header| header.field.equiv("ChatGPT-Account-ID"))
            .map(|header| header.value.as_str().to_string());
        tx.send(RecordedHeaderRequest {
            path: path.clone(),
            authorization,
            chatgpt_account_id,
        })
        .expect("send subscription request");
        let body = match path.as_str() {
            "/accounts/check/v4-2023-04-27" => response_body,
            other => panic!("unexpected subscription path: {other}"),
        };
        let response = Response::from_string(body)
            .with_status_code(StatusCode(status))
            .with_header(
                Header::from_bytes("Content-Type", "application/json")
                    .expect("content-type header"),
            );
        request
            .respond(response)
            .expect("respond subscription request");
    });
    (addr, rx, handle)
}

fn start_mock_usage_refresh_server(
    accounts_check_response_body: String,
    usage_response_body: String,
) -> (
    String,
    std::sync::mpsc::Receiver<RecordedHeaderRequest>,
    thread::JoinHandle<()>,
) {
    let server = Server::http("127.0.0.1:0").expect("start mock usage refresh server");
    let addr = format!("http://{}", server.server_addr());
    let (tx, rx) = std::sync::mpsc::channel();
    let handle = thread::spawn(move || {
        for _ in 0..2 {
            let request = server
                .recv_timeout(Duration::from_secs(5))
                .expect("usage refresh server timeout")
                .expect("receive usage refresh request");
            let path = request.url().to_string();
            let authorization = request
                .headers()
                .iter()
                .find(|header| header.field.equiv("Authorization"))
                .map(|header| header.value.as_str().to_string());
            let chatgpt_account_id = request
                .headers()
                .iter()
                .find(|header| header.field.equiv("ChatGPT-Account-ID"))
                .map(|header| header.value.as_str().to_string());
            tx.send(RecordedHeaderRequest {
                path: path.clone(),
                authorization,
                chatgpt_account_id,
            })
            .expect("send usage refresh request");

            let response_body = match path.as_str() {
                "/accounts/check/v4-2023-04-27" => accounts_check_response_body.clone(),
                "/api/codex/usage" => usage_response_body.clone(),
                other => panic!("unexpected usage refresh path: {other}"),
            };
            let response = Response::from_string(response_body)
                .with_status_code(StatusCode(200))
                .with_header(
                    Header::from_bytes("Content-Type", "application/json")
                        .expect("content-type header"),
                );
            request
                .respond(response)
                .expect("respond usage refresh request");
        }
    });
    (addr, rx, handle)
}

#[derive(Debug)]
struct RecordedRequest {
    path: String,
    body: String,
}

/// 函数 `start_mock_device_login_server`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 返回函数执行结果
fn start_mock_device_login_server() -> (
    String,
    std::sync::mpsc::Receiver<RecordedRequest>,
    thread::JoinHandle<()>,
) {
    let server = Server::http("127.0.0.1:0").expect("start mock device server");
    let addr = format!("http://{}", server.server_addr());
    let (tx, rx) = std::sync::mpsc::channel();
    let handle = thread::spawn(move || {
        for _ in 0..4 {
            let mut request = server
                .recv_timeout(Duration::from_secs(5))
                .expect("device login server timeout")
                .expect("receive device request");
            let path = request.url().to_string();
            let mut body = String::new();
            request
                .as_reader()
                .read_to_string(&mut body)
                .expect("read device request body");
            tx.send(RecordedRequest {
                path: path.clone(),
                body: body.clone(),
            })
            .expect("send device request body");

            let response_body = match path.as_str() {
                "/api/accounts/deviceauth/usercode" => serde_json::json!({
                    "device_auth_id": "device-auth-123",
                    "user_code": "ABCD-1234",
                    "interval": 1
                })
                .to_string(),
                "/api/accounts/deviceauth/token" => {
                    assert!(body.contains("\"device_auth_id\":\"device-auth-123\""));
                    assert!(body.contains("\"user_code\":\"ABCD-1234\""));
                    serde_json::json!({
                        "authorization_code": "auth-code-device-123",
                        "code_challenge": "challenge-device-123",
                        "code_verifier": "verifier-device-123"
                    })
                    .to_string()
                }
                "/oauth/token" => {
                    if body.contains("grant_type=authorization_code") {
                        assert!(body.contains("code=auth-code-device-123"));
                        assert!(body.contains("redirect_uri=http%3A%2F%2F127.0.0.1"));
                        serde_json::json!({
                            "id_token": build_access_token(
                                "sub-device",
                                "device@example.com",
                                "org-device",
                                "pro"
                            ),
                            "access_token": build_access_token(
                                "sub-device",
                                "device@example.com",
                                "org-device",
                                "pro"
                            ),
                            "refresh_token": "refresh-device-123"
                        })
                        .to_string()
                    } else {
                        assert!(body.contains(
                            "grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Atoken-exchange"
                        ));
                        serde_json::json!({
                            "access_token": "api-access-token-device-123"
                        })
                        .to_string()
                    }
                }
                other => panic!("unexpected device login path: {other}"),
            };
            let response = Response::from_string(response_body)
                .with_status_code(StatusCode(200))
                .with_header(
                    Header::from_bytes("Content-Type", "application/json")
                        .expect("content-type header"),
                );
            request.respond(response).expect("respond device request");
        }
    });
    (addr, rx, handle)
}

fn start_mock_proxy_response_server(
    response: &'static str,
) -> (
    String,
    std::sync::mpsc::Receiver<String>,
    thread::JoinHandle<()>,
) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind fake proxy");
    let addr = format!("http://{}", listener.local_addr().expect("fake proxy addr"));
    // Set nonblocking so accept() returns WouldBlock instead of blocking forever.
    listener.set_nonblocking(true).expect("set nonblocking");
    let (tx, rx) = std::sync::mpsc::channel();
    let handle = thread::spawn(move || {
        // Accept up to 15 connections: warmup + 10 latency samples + spare attempts.
        // Only the first request is forwarded through `tx`; all subsequent connections
        // silently receive the same response so the latency job finishes its sample loop.
        let mut first_sent = false;
        let mut accepted = 0usize;
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        while accepted < 15 && std::time::Instant::now() < deadline {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    accepted += 1;
                    // Switch the accepted stream to blocking mode for reliable reads.
                    // On Windows, sockets accepted from a nonblocking listener are also
                    // nonblocking, so we must explicitly switch before calling read().
                    let _ = stream.set_nonblocking(false);
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                    let mut buffer = vec![0_u8; 8192];
                    if let Ok(size) = stream.read(&mut buffer) {
                        if size > 0 {
                            let req = String::from_utf8_lossy(&buffer[..size]).to_string();
                            if !first_sent {
                                let _ = tx.send(req);
                                first_sent = true;
                            }
                        }
                    }
                    let _ = stream.write_all(response.as_bytes());
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(_) => break,
            }
        }
    });
    (addr, rx, handle)
}


fn start_mock_proxy_speed_server(
    download_response: &'static str,
    download_body: &'static [u8],
    upload_response: &'static str,
) -> (
    String,
    std::sync::mpsc::Receiver<String>,
    std::sync::mpsc::Receiver<String>,
    thread::JoinHandle<()>,
) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind fake proxy");
    let addr = format!("http://{}", listener.local_addr().expect("fake proxy addr"));
    let (download_tx, download_rx) = std::sync::mpsc::channel();
    let (upload_tx, upload_rx) = std::sync::mpsc::channel();
    let handle = thread::spawn(move || {
        let (mut download_stream, _) = listener.accept().expect("accept download connection");
        let mut download_buffer = vec![0_u8; 8192];
        let download_size = download_stream
            .read(&mut download_buffer)
            .expect("read download request");
        download_tx
            .send(String::from_utf8_lossy(&download_buffer[..download_size]).to_string())
            .expect("send download request");
        download_stream
            .write_all(download_response.as_bytes())
            .expect("write download response");
        download_stream
            .write_all(download_body)
            .expect("write download body");

        let (mut upload_stream, _) = listener.accept().expect("accept upload connection");
        let mut upload_buffer = vec![0_u8; 8192];
        let upload_size = upload_stream
            .read(&mut upload_buffer)
            .expect("read upload request");
        upload_tx
            .send(String::from_utf8_lossy(&upload_buffer[..upload_size]).to_string())
            .expect("send upload request");
        let mut body_tail = Vec::new();
        let mut drain = [0_u8; 4096];
        loop {
            let size = upload_stream.read(&mut drain).unwrap_or(0);
            if size == 0 {
                break;
            }
            body_tail.extend_from_slice(&drain[..size]);
            if body_tail.windows(5).any(|window| window == b"0\r\n\r\n") {
                break;
            }
            if body_tail.len() > 10 {
                let keep_from = body_tail.len() - 10;
                body_tail.drain(0..keep_from);
            }
        }
        let _ = upload_stream.write_all(upload_response.as_bytes());
    });
    (addr, download_rx, upload_rx, handle)
}

/// 函数 `rpc_initialize_roundtrip`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn rpc_initialize_roundtrip() {
    let _ctx = RpcTestContext::new("rpc-initialize");
    let server = codexmanager_service::start_one_shot_server().expect("start server");

    let req = JsonRpcRequest {
        id: 1.into(),
        method: "initialize".to_string(),
        params: None,
        trace: None,
    };
    let json = serde_json::to_string(&req).expect("serialize");
    let v = post_rpc(&server.addr, &json);
    let result = v.get("result").expect("result");
    assert!(result.get("serverName").is_none());
    assert!(result
        .get("codexHome")
        .and_then(|value| value.as_str())
        .is_some());
    assert!(result
        .get("platformFamily")
        .and_then(|value| value.as_str())
        .is_some());
    assert!(result
        .get("platformOs")
        .and_then(|value| value.as_str())
        .is_some());
    assert!(result
        .get("userAgent")
        .and_then(|value| value.as_str())
        .is_some());
}

/// 函数 `rpc_account_list_empty_uses_default_pagination`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn rpc_account_list_empty_uses_default_pagination() {
    let _ctx = RpcTestContext::new("rpc-account-list-empty");
    let server = codexmanager_service::start_one_shot_server().expect("start server");

    let req = JsonRpcRequest {
        id: 2.into(),
        method: "account/list".to_string(),
        params: None,
        trace: None,
    };
    let json = serde_json::to_string(&req).expect("serialize");
    let v = post_rpc(&server.addr, &json);
    let result = v.get("result").expect("result");

    let items = result
        .get("items")
        .and_then(|value| value.as_array())
        .expect("items array");
    assert!(items.is_empty(), "expected empty items, got: {result}");
    assert_eq!(
        result.get("total").and_then(|value| value.as_i64()),
        Some(0)
    );
    assert_eq!(result.get("page").and_then(|value| value.as_i64()), Some(1));
    assert_eq!(
        result.get("pageSize").and_then(|value| value.as_i64()),
        Some(5)
    );
}

/// 函数 `rpc_account_list_returns_all_accounts`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn rpc_account_list_returns_all_accounts() {
    let ctx = RpcTestContext::new("rpc-account-list-all");
    ctx.seed_accounts(7);
    let server = codexmanager_service::start_one_shot_server().expect("start server");

    let req = JsonRpcRequest {
        id: 3.into(),
        method: "account/list".to_string(),
        params: None,
        trace: None,
    };
    let json = serde_json::to_string(&req).expect("serialize");
    let v = post_rpc(&server.addr, &json);
    let result = v.get("result").expect("result");

    let items = result
        .get("items")
        .and_then(|value| value.as_array())
        .expect("items array");
    assert_eq!(items.len(), 7, "unexpected account count: {result}");
    assert_eq!(
        result.get("total").and_then(|value| value.as_i64()),
        Some(7)
    );
    assert_eq!(result.get("page").and_then(|value| value.as_i64()), Some(1));
    assert_eq!(
        result.get("pageSize").and_then(|value| value.as_i64()),
        Some(7)
    );

    let ids = items
        .iter()
        .map(|value| {
            value
                .get("id")
                .and_then(|value| value.as_str())
                .expect("item id")
        })
        .collect::<Vec<_>>();
    assert_eq!(
        ids,
        vec!["acc-0", "acc-1", "acc-2", "acc-3", "acc-4", "acc-5", "acc-6"]
    );
    assert_eq!(
        items[0].get("status").and_then(|value| value.as_str()),
        Some("active")
    );
    assert!(
        items[0].get("planType").is_some(),
        "missing planType field: {result}"
    );
}

/// 函数 `rpc_account_list_includes_account_plan_type`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn rpc_account_list_includes_account_plan_type() {
    let ctx = RpcTestContext::new("rpc-account-list-plan-type");
    let storage = Storage::open(ctx.db_path()).expect("open db");
    storage.init().expect("init schema");
    let now = now_ts();
    storage
        .insert_account(&Account {
            id: "acc-plan-team".to_string(),
            label: "Team Account".to_string(),
            issuer: "https://auth.openai.com".to_string(),
            chatgpt_account_id: Some("org-team".to_string()),
            workspace_id: Some("org-team".to_string()),
            group_name: Some("team".to_string()),
            sort: 0,
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
        })
        .expect("insert account");
    storage
        .insert_token(&Token {
            account_id: "acc-plan-team".to_string(),
            id_token: build_access_token("sub-team", "team@example.com", "org-team", "team"),
            access_token: build_access_token("sub-team", "team@example.com", "org-team", "team"),
            refresh_token: "refresh-team".to_string(),
            api_key_access_token: None,
            last_refresh: now,
        })
        .expect("insert token");
    storage
        .upsert_account_metadata("acc-plan-team", Some("主账号"), Some("高频,团队A"))
        .expect("insert account metadata");

    let server = codexmanager_service::start_one_shot_server().expect("start server");
    let req = JsonRpcRequest {
        id: 76.into(),
        method: "account/list".to_string(),
        params: None,
        trace: None,
    };
    let json = serde_json::to_string(&req).expect("serialize");
    let v = post_rpc(&server.addr, &json);
    let item = v
        .get("result")
        .and_then(|value| value.get("items"))
        .and_then(|value| value.as_array())
        .and_then(|items| items.first())
        .expect("account item");

    assert_eq!(
        item.get("planType").and_then(|value| value.as_str()),
        Some("team")
    );
    assert_eq!(
        item.get("note").and_then(|value| value.as_str()),
        Some("主账号")
    );
    assert_eq!(
        item.get("tags").and_then(|value| value.as_str()),
        Some("高频,团队A")
    );
    assert!(
        item.get("planTypeRaw").is_some(),
        "missing planTypeRaw field: {item}"
    );
}

#[test]
fn rpc_account_list_prefers_free_subscription_result_over_token_plan() {
    let ctx = RpcTestContext::new("rpc-account-list-subscription-free-plan");
    let storage = Storage::open(ctx.db_path()).expect("open db");
    storage.init().expect("init schema");
    let now = now_ts();
    storage
        .insert_account(&Account {
            id: "acc-subscription-free".to_string(),
            label: "Subscription Free Account".to_string(),
            issuer: "https://auth.openai.com".to_string(),
            chatgpt_account_id: Some("org-subscription-free".to_string()),
            workspace_id: Some("org-subscription-free".to_string()),
            group_name: None,
            sort: 0,
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
        })
        .expect("insert account");
    storage
        .insert_token(&Token {
            account_id: "acc-subscription-free".to_string(),
            id_token: build_access_token(
                "sub-subscription-free",
                "subscription-free@example.com",
                "org-subscription-free",
                "plus",
            ),
            access_token: build_access_token(
                "sub-subscription-free",
                "subscription-free@example.com",
                "org-subscription-free",
                "plus",
            ),
            refresh_token: "refresh-subscription-free".to_string(),
            api_key_access_token: None,
            last_refresh: now,
        })
        .expect("insert token");
    storage
        .upsert_account_subscription(
            "acc-subscription-free",
            false,
            Some("free"),
            Some("free"),
            None,
            None,
        )
        .expect("insert subscription result");

    let server = codexmanager_service::start_one_shot_server().expect("start server");
    let req = JsonRpcRequest {
        id: 77.into(),
        method: "account/list".to_string(),
        params: None,
        trace: None,
    };
    let json = serde_json::to_string(&req).expect("serialize");
    let v = post_rpc(&server.addr, &json);
    let item = v
        .get("result")
        .and_then(|value| value.get("items"))
        .and_then(|value| value.as_array())
        .and_then(|items| items.first())
        .expect("account item");

    assert_eq!(
        item.get("planType").and_then(|value| value.as_str()),
        Some("free")
    );
    assert_eq!(
        item.get("hasSubscription")
            .and_then(|value| value.as_bool()),
        Some(false)
    );
    assert_eq!(
        item.get("subscriptionPlan")
            .and_then(|value| value.as_str()),
        Some("free")
    );
}

#[test]
fn rpc_account_list_prefers_accounts_check_plan_over_subscription_plan() {
    let ctx = RpcTestContext::new("rpc-account-list-accounts-check-plan");
    let storage = Storage::open(ctx.db_path()).expect("open db");
    storage.init().expect("init schema");
    let now = now_ts();
    storage
        .insert_account(&Account {
            id: "acc-accounts-check-plan".to_string(),
            label: "Accounts Check Plan Account".to_string(),
            issuer: "https://auth.openai.com".to_string(),
            chatgpt_account_id: Some("org-accounts-check-plan".to_string()),
            workspace_id: Some("org-accounts-check-plan".to_string()),
            group_name: None,
            sort: 0,
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
        })
        .expect("insert account");
    storage
        .insert_token(&Token {
            account_id: "acc-accounts-check-plan".to_string(),
            id_token: build_access_token(
                "sub-accounts-check-plan",
                "accounts-check-plan@example.com",
                "org-accounts-check-plan",
                "plus",
            ),
            access_token: build_access_token(
                "sub-accounts-check-plan",
                "accounts-check-plan@example.com",
                "org-accounts-check-plan",
                "plus",
            ),
            refresh_token: "refresh-accounts-check-plan".to_string(),
            api_key_access_token: None,
            last_refresh: now,
        })
        .expect("insert token");
    storage
        .upsert_account_subscription(
            "acc-accounts-check-plan",
            true,
            Some("free"),
            Some("plus"),
            Some(1_778_038_289),
            Some(1_776_655_889),
        )
        .expect("insert subscription result");

    let server = codexmanager_service::start_one_shot_server().expect("start server");
    let req = JsonRpcRequest {
        id: 78.into(),
        method: "account/list".to_string(),
        params: None,
        trace: None,
    };
    let json = serde_json::to_string(&req).expect("serialize");
    let v = post_rpc(&server.addr, &json);
    let item = v
        .get("result")
        .and_then(|value| value.get("items"))
        .and_then(|value| value.as_array())
        .and_then(|items| items.first())
        .expect("account item");

    assert_eq!(
        item.get("planType").and_then(|value| value.as_str()),
        Some("free")
    );
    assert_eq!(
        item.get("subscriptionPlan")
            .and_then(|value| value.as_str()),
        Some("plus")
    );
}

/// 函数 `rpc_account_update_profile_updates_label_note_tags_and_sort`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn rpc_account_update_profile_updates_label_note_tags_and_sort() {
    let ctx = RpcTestContext::new("rpc-account-update-profile");
    ctx.seed_accounts(1);

    let server = codexmanager_service::start_one_shot_server().expect("start server");
    let req = JsonRpcRequest {
        id: 78.into(),
        method: "account/update".to_string(),
        params: Some(serde_json::json!({
            "accountId": "acc-0",
            "label": "主账号A",
            "note": "团队共享主号",
            "tags": "高频,团队A",
            "sort": 7
        })),
        trace: None,
    };
    let json = serde_json::to_string(&req).expect("serialize");
    let v = post_rpc(&server.addr, &json);
    let result = v.get("result").expect("result");
    assert_eq!(
        result.get("ok").and_then(|value| value.as_bool()),
        Some(true)
    );

    let storage = Storage::open(ctx.db_path()).expect("open db");
    let account = storage
        .find_account_by_id("acc-0")
        .expect("find account")
        .expect("account exists");
    assert_eq!(account.label, "主账号A");
    assert_eq!(account.sort, 7);

    let metadata = storage
        .find_account_metadata("acc-0")
        .expect("find account metadata")
        .expect("metadata exists");
    assert_eq!(metadata.note.as_deref(), Some("团队共享主号"));
    assert_eq!(metadata.tags.as_deref(), Some("高频,团队A"));
}

#[test]
fn rpc_system_proxy_crud_roundtrip() {
    let _ctx = RpcTestContext::new("rpc-system-proxy-crud");

    let create_server = codexmanager_service::start_one_shot_server().expect("start server");
    let create_req = JsonRpcRequest {
        id: 79.into(),
        method: "system/proxy/create".to_string(),
        params: Some(serde_json::json!({
            "name": "RPC Proxy",
            "proxyUrl": "http://user:pass@example.com:8080/private",
            "enabled": true,
            "tagsJson": "[\"rpc\"]",
            "notes": "Created via rpc"
        })),
        trace: None,
    };
    let create_json = serde_json::to_string(&create_req).expect("serialize create");
    let create_resp = post_rpc(&create_server.addr, &create_json);
    let create_result = create_resp.get("result").expect("create result");
    assert_eq!(
        create_result.get("status").and_then(|value| value.as_str()),
        Some("unchecked")
    );
    assert_eq!(
        create_result
            .get("proxyUrlRedacted")
            .and_then(|value| value.as_str()),
        Some("http://example.com:8080")
    );
    let proxy_id = create_result
        .get("id")
        .and_then(|value| value.as_str())
        .expect("proxy id")
        .to_string();

    let list_server = codexmanager_service::start_one_shot_server().expect("start server");
    let list_req = JsonRpcRequest {
        id: 80.into(),
        method: "system/proxy/list".to_string(),
        params: Some(serde_json::json!({})),
        trace: None,
    };
    let list_json = serde_json::to_string(&list_req).expect("serialize list");
    let list_resp = post_rpc(&list_server.addr, &list_json);
    let items = list_resp["result"]["items"].as_array().expect("items");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["id"], proxy_id);

    let update_server = codexmanager_service::start_one_shot_server().expect("start server");
    let update_req = JsonRpcRequest {
        id: 81.into(),
        method: "system/proxy/update".to_string(),
        params: Some(serde_json::json!({
            "id": proxy_id,
            "name": "RPC Proxy Updated",
            "proxyUrl": "socks5h://proxy.example:1080",
            "enabled": false
        })),
        trace: None,
    };
    let update_json = serde_json::to_string(&update_req).expect("serialize update");
    let update_resp = post_rpc(&update_server.addr, &update_json);
    let update_result = update_resp.get("result").expect("update result");
    assert_eq!(
        update_result.get("name").and_then(|value| value.as_str()),
        Some("RPC Proxy Updated")
    );
    assert_eq!(
        update_result
            .get("proxyUrlRedacted")
            .and_then(|value| value.as_str()),
        Some("socks5h://proxy.example:1080")
    );
    assert_eq!(
        update_result
            .get("enabled")
            .and_then(|value| value.as_bool()),
        Some(false)
    );

    let delete_server = codexmanager_service::start_one_shot_server().expect("start server");
    let delete_req = JsonRpcRequest {
        id: 82.into(),
        method: "system/proxy/delete".to_string(),
        params: Some(serde_json::json!({ "id": proxy_id })),
        trace: None,
    };
    let delete_json = serde_json::to_string(&delete_req).expect("serialize delete");
    let delete_resp = post_rpc(&delete_server.addr, &delete_json);
    assert_eq!(delete_resp["result"]["ok"].as_bool(), Some(true));
}

#[test]
fn rpc_system_proxy_test_presets_returns_expected_defaults_and_upload_status() {
    let _ctx = RpcTestContext::new("rpc-system-proxy-test-presets");
    std::env::set_var(
        "CODEXMANAGER_PROXY_TEST_UPLOAD_URL",
        "https://upload.example.com/proxy-test",
    );

    let server = codexmanager_service::start_one_shot_server().expect("start server");
    let req = JsonRpcRequest {
        id: 87.into(),
        method: "system/proxy/test-presets".to_string(),
        params: Some(serde_json::json!({})),
        trace: None,
    };
    let json = serde_json::to_string(&req).expect("serialize test-presets");
    let resp = post_rpc(&server.addr, &json);
    let result = resp.get("result").expect("result");

    assert_eq!(
        result["defaults"]["latencyPresetId"].as_str(),
        Some("google_gstatic_204")
    );
    assert_eq!(
        result["defaults"]["speedProviderId"].as_str(),
        Some("cloudflare_http_rust")
    );
    assert_eq!(result["defaults"]["fileSizeId"].as_str(), Some("size_25mb"));

    let cachefly = result["speedProviders"]
        .as_array()
        .expect("speed providers")
        .iter()
        .find(|item| item["id"].as_str() == Some("cachefly"))
        .expect("cachefly preset");
    assert_eq!(
        cachefly["files"].as_array().map(|items| items.len()),
        Some(3)
    );

    let hetzner = result["speedProviders"]
        .as_array()
        .expect("speed providers")
        .iter()
        .find(|item| item["id"].as_str() == Some("hetzner_fsn1"))
        .expect("hetzner preset");
    let hetzner_cap = hetzner["files"]
        .as_array()
        .expect("hetzner files")
        .iter()
        .find(|item| item["fileSizeId"].as_str() == Some("size_500mb_cap"))
        .expect("hetzner 500mb cap");
    assert_eq!(hetzner_cap["readLimitBytes"].as_i64(), Some(500_000_000));
    assert_eq!(
        hetzner_cap["downloadUrl"].as_str(),
        Some("https://fsn1-speed.hetzner.com/1GB.bin")
    );

    let upload = result["uploadEndpoint"]
        .as_object()
        .expect("upload endpoint");
    assert_eq!(
        upload.get("configured").and_then(|v| v.as_bool()),
        Some(true)
    );
    assert_eq!(upload.get("source").and_then(|v| v.as_str()), Some("env"));
    assert_eq!(
        upload.get("url").and_then(|v| v.as_str()),
        Some("https://upload.example.com/proxy-test")
    );

    std::env::remove_var("CODEXMANAGER_PROXY_TEST_UPLOAD_URL");
}

#[test]
fn rpc_system_proxy_speed_test_fails_when_upload_not_configured() {
    let ctx = RpcTestContext::new("rpc-system-proxy-speed-test-no-upload");
    let storage = Storage::open(ctx.db_path()).expect("open db");
    storage.init().expect("init schema");

    std::env::remove_var("CODEXMANAGER_PROXY_TEST_UPLOAD_URL");

    storage
        .create_proxy_profile(&ProxyProfileCreateInput {
            id: "pp_speed_fail".to_string(),
            name: "Speed Fail".to_string(),
            proxy_url: "http://127.0.0.1:3128".to_string(),
            enabled: true,
            tags_json: None,
            notes: None,
        })
        .expect("create proxy profile");

    let server = codexmanager_service::start_one_shot_server().expect("start server");
    let resp = post_rpc_method(
        &server.addr,
        90,
        "system/proxy/speed-test",
        Some(serde_json::json!({
            "id": "pp_speed_fail",
            "providerId": "cachefly"
        })),
    );

    let result = resp.get("result").expect("should return result");
    let job_id = result
        .get("jobId")
        .and_then(|value| value.as_str())
        .expect("speed test should return job id");
    let final_job = wait_for_proxy_test_job(job_id);
    let error = final_job
        .get("error")
        .and_then(|value| value.as_str())
        .unwrap_or("");

    assert_eq!(final_job.get("status").and_then(|value| value.as_str()), Some("failed"));
    assert!(
        error.contains("upload_endpoint_not_configured")
            || error.contains("not configured"),
        "error: {error}"
    );

    let updated = storage
        .find_proxy_profile("pp_speed_fail")
        .expect("find proxy profile")
        .expect("proxy exists");
    assert_eq!(updated.status, "failed");
    assert!(updated.last_error.unwrap_or_default().contains("Upload endpoint is not configured"));
}

#[test]
fn rpc_system_proxy_test_latency_204_success_updates_profile_and_history() {
    let ctx = RpcTestContext::new("rpc-system-proxy-test-latency-ok");
    let storage = Storage::open(ctx.db_path()).expect("open db");
    storage.init().expect("init schema");
    let (proxy_url, rx, handle) = start_mock_proxy_response_server(
        "HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    );
    storage
        .create_proxy_profile(&ProxyProfileCreateInput {
            id: "pp_latency_ok".to_string(),
            name: "Latency OK".to_string(),
            proxy_url: proxy_url.clone(),
            enabled: true,
            tags_json: None,
            notes: None,
        })
        .expect("create proxy profile");

    let server = codexmanager_service::start_one_shot_server().expect("start server");
    let resp = post_rpc_method(
        &server.addr,
        88,
        "system/proxy/test-latency",
        Some(serde_json::json!({
            "id": "pp_latency_ok",
        })),
    );
    let result = resp.get("result").expect("latency test result");
    let job_id = result
        .get("jobId")
        .and_then(|value| value.as_str())
        .expect("latency test should return job id");

    let request_line = rx.recv().expect("proxy request");
    handle.join().expect("join fake proxy");
    let final_job = wait_for_proxy_test_job(job_id);

    assert!(request_line.starts_with("GET http://cp.cloudflare.com/generate_204 HTTP/1.1"));
    assert_eq!(result["status"].as_str(), Some("queued"));
    assert_eq!(final_job["status"].as_str(), Some("completed"));
    assert_eq!(final_job["kind"].as_str(), Some("latency"));
    assert!(final_job["latencyMs"].as_i64().is_some());

    let updated = storage
        .find_proxy_profile("pp_latency_ok")
        .expect("find updated proxy")
        .expect("proxy exists");
    assert_eq!(updated.status, "ok");
    assert_eq!(updated.last_error, None);
    assert_eq!(updated.last_url_latency_ms, final_job["latencyMs"].as_i64());
    assert!(updated.last_tested_at.is_some());

    let history = storage
        .list_proxy_profile_url_tests("pp_latency_ok", 10)
        .expect("list proxy url tests");
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].status, "ok");
    assert_eq!(history[0].status_code, Some(204));
    assert_eq!(history[0].test_url, "http://cp.cloudflare.com/generate_204");
    assert!(!history[0].redirected);
    assert_eq!(history[0].error_code, None);
}

#[test]
fn rpc_system_proxy_test_latency_reports_redirect_and_persists_history() {
    let ctx = RpcTestContext::new("rpc-system-proxy-test-latency-redirect");
    let storage = Storage::open(ctx.db_path()).expect("open db");
    storage.init().expect("init schema");
    let (proxy_url, _rx, handle) = start_mock_proxy_response_server(
        "HTTP/1.1 302 Found\r\nLocation: /login\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    );
    storage
        .create_proxy_profile(&ProxyProfileCreateInput {
            id: "pp_latency_redirect".to_string(),
            name: "Latency Redirect".to_string(),
            proxy_url: proxy_url,
            enabled: true,
            tags_json: None,
            notes: None,
        })
        .expect("create proxy profile");

    let server = codexmanager_service::start_one_shot_server().expect("start server");
    let resp = post_rpc_method(
        &server.addr,
        89,
        "system/proxy/test-latency",
        Some(serde_json::json!({
            "id": "pp_latency_redirect",
        })),
    );
    let result = resp.get("result").expect("latency test result");
    let job_id = result
        .get("jobId")
        .and_then(|value| value.as_str())
        .expect("latency test should return job id");

    handle.join().expect("join fake proxy");
    let final_job = wait_for_proxy_test_job(job_id);

    assert_eq!(result["status"].as_str(), Some("queued"));
    assert_eq!(final_job["status"].as_str(), Some("failed"));
    assert_eq!(final_job["kind"].as_str(), Some("latency"));

    let updated = storage
        .find_proxy_profile("pp_latency_redirect")
        .expect("find updated proxy")
        .expect("proxy exists");
    assert_eq!(updated.status, "failed");
    assert_eq!(
        updated.last_error.as_deref(),
        Some("redirect detected: http://cp.cloudflare.com/login")
    );
    assert!(updated.last_tested_at.is_some());
    assert!(updated.last_url_latency_ms.is_some());

    let history = storage
        .list_proxy_profile_url_tests("pp_latency_redirect", 10)
        .expect("list proxy url tests");
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].status, "failed");
    assert_eq!(history[0].status_code, Some(302));
    assert!(history[0].redirected);
    assert_eq!(
        history[0].final_url.as_deref(),
        Some("http://cp.cloudflare.com/login")
    );
    assert_eq!(history[0].error_code.as_deref(), Some("redirect_detected"));
}

#[test]
fn rpc_account_proxy_can_bind_proxy_profile_and_fail_closed_when_profile_disabled() {
    let ctx = RpcTestContext::new("rpc-account-proxy-profile-binding");
    ctx.seed_accounts(1);

    let create_proxy_server = codexmanager_service::start_one_shot_server().expect("start server");
    let create_proxy_req = JsonRpcRequest {
        id: 83.into(),
        method: "system/proxy/create".to_string(),
        params: Some(serde_json::json!({
            "name": "Account Bound Proxy",
            "proxyUrl": "http://127.0.0.1:7891",
            "enabled": true
        })),
        trace: None,
    };
    let create_proxy_json =
        serde_json::to_string(&create_proxy_req).expect("serialize proxy create");
    let create_proxy_resp = post_rpc(&create_proxy_server.addr, &create_proxy_json);
    let proxy_id = create_proxy_resp["result"]["id"]
        .as_str()
        .expect("proxy id")
        .to_string();

    let set_proxy_server = codexmanager_service::start_one_shot_server().expect("start server");
    let set_proxy_req = JsonRpcRequest {
        id: 84.into(),
        method: "account/proxy/set".to_string(),
        params: Some(serde_json::json!({
            "accountId": "acc-0",
            "enabled": true,
            "source": "profile",
            "proxyProfileId": proxy_id,
            "proxyUrl": "http://127.0.0.1:7999"
        })),
        trace: None,
    };
    let set_proxy_json = serde_json::to_string(&set_proxy_req).expect("serialize account proxy");
    let set_proxy_resp = post_rpc(&set_proxy_server.addr, &set_proxy_json);
    let set_result = set_proxy_resp.get("result").expect("set result");
    assert_eq!(
        set_result.get("source").and_then(|value| value.as_str()),
        Some("profile")
    );
    assert_eq!(
        set_result
            .get("proxyProfileId")
            .and_then(|value| value.as_str()),
        Some(proxy_id.as_str())
    );
    assert_eq!(
        set_result
            .get("proxyUrlRedacted")
            .and_then(|value| value.as_str()),
        Some("http://127.0.0.1:7891")
    );

    let disable_proxy_server = codexmanager_service::start_one_shot_server().expect("start server");
    let disable_proxy_req = JsonRpcRequest {
        id: 85.into(),
        method: "system/proxy/update".to_string(),
        params: Some(serde_json::json!({
            "id": proxy_id,
            "enabled": false
        })),
        trace: None,
    };
    let disable_proxy_json =
        serde_json::to_string(&disable_proxy_req).expect("serialize proxy disable");
    let disable_proxy_resp = post_rpc(&disable_proxy_server.addr, &disable_proxy_json);
    assert_eq!(
        disable_proxy_resp["result"]["enabled"].as_bool(),
        Some(false)
    );

    let test_proxy_server = codexmanager_service::start_one_shot_server().expect("start server");
    let test_proxy_req = JsonRpcRequest {
        id: 86.into(),
        method: "account/proxy/test".to_string(),
        params: Some(serde_json::json!({
            "accountId": "acc-0",
            "enabled": true,
            "source": "profile",
            "proxyProfileId": proxy_id
        })),
        trace: None,
    };
    let test_proxy_json =
        serde_json::to_string(&test_proxy_req).expect("serialize account proxy test");
    let test_proxy_resp = post_rpc(&test_proxy_server.addr, &test_proxy_json);
    let test_result = test_proxy_resp.get("result").expect("test result");
    assert_eq!(
        test_result.get("status").and_then(|value| value.as_str()),
        Some("invalid_url")
    );
    let last_error = test_result
        .get("lastError")
        .and_then(|value| value.as_str())
        .expect("lastError");
    assert!(last_error.contains("fail-closed"), "{last_error}");
    assert!(last_error.contains("disabled"), "{last_error}");
}

#[test]
fn rpc_account_proxy_latency_test_uses_profile_binding_and_keeps_system_history_separate() {
    let ctx = RpcTestContext::new("rpc-account-proxy-latency-profile");
    ctx.seed_accounts(1);
    let storage = Storage::open(ctx.db_path()).expect("open db");
    storage.init().expect("init schema");

    let (proxy_url, rx, handle) = start_mock_proxy_response_server(
        "HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    );

    storage
        .create_proxy_profile(&ProxyProfileCreateInput {
            id: "pp_account_latency".to_string(),
            name: "Account Latency".to_string(),
            proxy_url,
            enabled: true,
            tags_json: None,
            notes: None,
        })
        .expect("create proxy profile");
    storage
        .upsert_account_proxy_settings(
            "acc-0",
            true,
            Some("profile"),
            Some("pp_account_latency"),
            None,
            "unchecked",
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .expect("set account proxy settings");

    let server = codexmanager_service::start_one_shot_server().expect("start server");
    let resp = post_rpc_method(
        &server.addr,
        186,
        "account/proxy/latency-test",
        Some(serde_json::json!({
            "accountId": "acc-0",
            "presetId": "custom",
            "customUrl": "http://example.test/generate_204",
        })),
    );
    let result = resp.get("result").expect("latency test result");
    let job_id = result
        .get("jobId")
        .and_then(|value| value.as_str())
        .expect("latency test should return job id");

    let final_job = wait_for_account_proxy_test_job("acc-0", job_id);
    let request_line = rx.recv().expect("proxy request");
    handle.join().expect("join fake proxy");

    assert!(request_line.starts_with("GET http://cp.cloudflare.com/generate_204 HTTP/1.1"));
    assert_eq!(final_job["scope"].as_str(), Some("account_proxy"));
    assert_eq!(final_job["kind"].as_str(), Some("latency"));
    assert_eq!(final_job["status"].as_str(), Some("completed"));
    assert!(final_job["latencyMs"].as_i64().is_some());

    let updated = storage
        .find_account_proxy_settings("acc-0")
        .expect("find account proxy settings")
        .expect("account proxy settings exist");
    assert_eq!(updated.status, "ok");
    assert!(updated.latency_ms.is_some());
    assert_eq!(updated.last_download_mbps, None);
    assert_eq!(updated.last_upload_mbps, None);
    assert!(updated.last_check_at.is_some());

    let profile = storage
        .find_proxy_profile("pp_account_latency")
        .expect("find proxy profile")
        .expect("proxy profile exists");
    assert_eq!(profile.status, "unchecked");
    let history = storage
        .list_proxy_profile_url_tests("pp_account_latency", 10)
        .expect("list system history");
    assert!(history.is_empty(), "account tests must not write system history");
}

#[test]
fn rpc_account_proxy_speed_test_uses_custom_proxy_and_updates_latest_only_fields() {
    let ctx = RpcTestContext::new("rpc-account-proxy-speed-custom");
    ctx.seed_accounts(1);
    let storage = Storage::open(ctx.db_path()).expect("open db");
    storage.init().expect("init schema");

    let _upload_guard = EnvGuard::set(
        "CODEXMANAGER_PROXY_TEST_UPLOAD_URL",
        "http://example.test/proxy-test-upload",
    );
    let (proxy_url, download_rx, upload_rx, handle) = start_mock_proxy_speed_server(
        "HTTP/1.1 200 OK\r\nContent-Length: 10\r\nConnection: close\r\n\r\n",
        b"abcdefghij",
        "HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n",
    );

    storage
        .upsert_account_proxy_settings(
            "acc-0",
            true,
            Some("custom"),
            None,
            Some(proxy_url.as_str()),
            "unchecked",
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .expect("set custom account proxy");

    let server = codexmanager_service::start_one_shot_server().expect("start server");
    let resp = post_rpc_method(
        &server.addr,
        187,
        "account/proxy/speed-test",
        Some(serde_json::json!({
            "accountId": "acc-0",
            "providerId": "cachefly",
            "fileSizeId": "size_1mb",
        })),
    );
    let result = resp.get("result").expect("speed test result");
    let job_id = result
        .get("jobId")
        .and_then(|value| value.as_str())
        .expect("speed test should return job id");

    let final_job = wait_for_account_proxy_test_job("acc-0", job_id);
    let download_request = download_rx.recv().expect("download request");
    let upload_request = upload_rx.recv().expect("upload request");
    handle.join().expect("join fake proxy");

    assert!(download_request.contains("GET http://cachefly.cachefly.net/1mb.test"));
    assert!(upload_request.contains("POST http://example.test/proxy-test-upload"));
    assert_eq!(final_job["scope"].as_str(), Some("account_proxy"));
    assert_eq!(final_job["kind"].as_str(), Some("speed"));
    assert_eq!(final_job["status"].as_str(), Some("completed"));
    assert!(final_job["downloadMbps"].as_f64().unwrap_or_default() > 0.0);
    assert!(final_job["uploadMbps"].as_f64().unwrap_or_default() > 0.0);

    let updated = storage
        .find_account_proxy_settings("acc-0")
        .expect("find account proxy settings")
        .expect("account proxy settings exist");
    assert_eq!(updated.status, "ok");
    assert!(updated.last_download_mbps.unwrap_or_default() > 0.0);
    assert!(updated.last_upload_mbps.unwrap_or_default() > 0.0);
    assert!(updated.last_check_at.is_some());
}

#[test]
fn rpc_account_proxy_latency_test_fails_closed_for_disabled_profile() {
    let ctx = RpcTestContext::new("rpc-account-proxy-latency-disabled-profile");
    ctx.seed_accounts(1);
    let storage = Storage::open(ctx.db_path()).expect("open db");
    storage.init().expect("init schema");

    storage
        .create_proxy_profile(&ProxyProfileCreateInput {
            id: "pp_account_disabled".to_string(),
            name: "Disabled".to_string(),
            proxy_url: "http://127.0.0.1:7891".to_string(),
            enabled: false,
            tags_json: None,
            notes: None,
        })
        .expect("create disabled proxy profile");
    storage
        .upsert_account_proxy_settings(
            "acc-0",
            true,
            Some("profile"),
            Some("pp_account_disabled"),
            None,
            "unchecked",
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .expect("set profile-based account proxy");

    let server = codexmanager_service::start_one_shot_server().expect("start server");
    let resp = post_rpc_method(
        &server.addr,
        188,
        "account/proxy/latency-test",
        Some(serde_json::json!({
            "accountId": "acc-0",
        })),
    );
    let error = resp["result"]["error"]
        .as_str()
        .expect("latency test error");
    assert!(error.contains("fail-closed"), "{error}");
    assert!(error.contains("disabled"), "{error}");
}

/// 函数 `rpc_app_settings_set_invalid_payload_returns_structured_error`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn rpc_app_settings_set_invalid_payload_returns_structured_error() {
    let _ctx = RpcTestContext::new("rpc-app-settings-invalid-payload");
    let server = codexmanager_service::start_one_shot_server().expect("start server");

    let req = JsonRpcRequest {
        id: 30.into(),
        method: "appSettings/set".to_string(),
        params: Some(serde_json::json!("invalid-payload")),
        trace: None,
    };
    let json = serde_json::to_string(&req).expect("serialize");
    let v = post_rpc(&server.addr, &json);
    let result = v.get("result").expect("result");

    let message = result
        .get("error")
        .and_then(|value| value.as_str())
        .expect("error message");
    assert!(
        message.starts_with("invalid app settings payload:"),
        "unexpected message: {message}"
    );
    assert_eq!(
        result.get("errorCode").and_then(|value| value.as_str()),
        Some("invalid_settings_payload")
    );
    let detail = result.get("errorDetail").expect("errorDetail");
    assert_eq!(
        detail.get("code").and_then(|value| value.as_str()),
        Some("invalid_settings_payload")
    );
    assert_eq!(
        detail.get("message").and_then(|value| value.as_str()),
        Some(message)
    );
}

/// 函数 `rpc_app_settings_can_roundtrip_free_account_max_model`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn rpc_app_settings_can_roundtrip_free_account_max_model() {
    let _ctx = RpcTestContext::new("rpc-app-settings-free-max-model");
    let set_server = codexmanager_service::start_one_shot_server().expect("start server");

    let set_req = JsonRpcRequest {
        id: 31.into(),
        method: "appSettings/set".to_string(),
        params: Some(serde_json::json!({
            "freeAccountMaxModel": "gpt-5.3-codex"
        })),
        trace: None,
    };
    let set_json = serde_json::to_string(&set_req).expect("serialize");
    let set_resp = post_rpc(&set_server.addr, &set_json);
    let set_result = set_resp.get("result").expect("result");
    assert_eq!(
        set_result
            .get("freeAccountMaxModel")
            .and_then(|value| value.as_str()),
        Some("gpt-5.3-codex")
    );

    let get_server = codexmanager_service::start_one_shot_server().expect("start server");
    let get_req = JsonRpcRequest {
        id: 32.into(),
        method: "appSettings/get".to_string(),
        params: None,
        trace: None,
    };
    let get_json = serde_json::to_string(&get_req).expect("serialize");
    let get_resp = post_rpc(&get_server.addr, &get_json);
    let get_result = get_resp.get("result").expect("result");
    assert_eq!(
        get_result
            .get("freeAccountMaxModel")
            .and_then(|value| value.as_str()),
        Some("gpt-5.3-codex")
    );
}

/// 函数 `rpc_account_delete_many_deletes_requested_accounts`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn rpc_account_delete_many_deletes_requested_accounts() {
    let ctx = RpcTestContext::new("rpc-account-delete-many");
    ctx.seed_accounts(4);
    let server = codexmanager_service::start_one_shot_server().expect("start server");

    let req = JsonRpcRequest {
        id: 11.into(),
        method: "account/deleteMany".to_string(),
        params: Some(serde_json::json!({
            "accountIds": ["acc-1", "acc-3", "missing"]
        })),
        trace: None,
    };
    let json = serde_json::to_string(&req).expect("serialize");
    let v = post_rpc(&server.addr, &json);
    let result = v.get("result").expect("result");

    assert_eq!(
        result.get("requested").and_then(|value| value.as_u64()),
        Some(3)
    );
    assert_eq!(
        result.get("deleted").and_then(|value| value.as_u64()),
        Some(2)
    );
    assert_eq!(
        result.get("failed").and_then(|value| value.as_u64()),
        Some(1)
    );
    let deleted = result
        .get("deletedAccountIds")
        .and_then(|value| value.as_array())
        .expect("deleted ids");
    assert_eq!(deleted.len(), 2);

    let storage = Storage::open(ctx.db_path()).expect("open db");
    let remaining = storage.list_accounts().expect("list remaining");
    let ids = remaining
        .into_iter()
        .map(|item| item.id)
        .collect::<Vec<_>>();
    assert_eq!(ids, vec!["acc-0", "acc-2"]);
}

/// 函数 `rpc_account_delete_unavailable_free_removes_refresh_invalid_free_accounts`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn rpc_account_delete_unavailable_free_removes_refresh_invalid_free_accounts() {
    let ctx = RpcTestContext::new("rpc-account-delete-unavailable-free");
    let storage = Storage::open(ctx.db_path()).expect("open db");
    storage.init().expect("init schema");
    let now = now_ts();
    storage
        .insert_account(&Account {
            id: "acc-free-invalid".to_string(),
            label: "Free Invalid".to_string(),
            issuer: "https://auth.openai.com".to_string(),
            chatgpt_account_id: Some("org-free-invalid".to_string()),
            workspace_id: Some("org-free-invalid".to_string()),
            group_name: None,
            sort: 0,
            status: "unavailable".to_string(),
            created_at: now,
            updated_at: now,
        })
        .expect("insert banned free account");
    storage
        .insert_token(&Token {
            account_id: "acc-free-invalid".to_string(),
            id_token: build_access_token(
                "sub-free-invalid",
                "free-invalid@example.com",
                "org-free-invalid",
                "free",
            ),
            access_token: build_access_token(
                "sub-free-invalid",
                "free-invalid@example.com",
                "org-free-invalid",
                "free",
            ),
            refresh_token: "refresh-free-invalid".to_string(),
            api_key_access_token: None,
            last_refresh: now,
        })
        .expect("insert free token");
    storage
        .insert_event(&Event {
            account_id: Some("acc-free-invalid".to_string()),
            event_type: "account_status_update".to_string(),
            message: "status=banned reason=account_deactivated".to_string(),
            created_at: now,
        })
        .expect("insert status reason");

    storage
        .insert_account(&Account {
            id: "acc-free-unavailable".to_string(),
            label: "Free Unavailable".to_string(),
            issuer: "https://auth.openai.com".to_string(),
            chatgpt_account_id: Some("org-free-unavailable".to_string()),
            workspace_id: Some("org-free-unavailable".to_string()),
            group_name: None,
            sort: 1,
            status: "unavailable".to_string(),
            created_at: now + 1,
            updated_at: now + 1,
        })
        .expect("insert unavailable free account");
    storage
        .insert_token(&Token {
            account_id: "acc-free-unavailable".to_string(),
            id_token: build_access_token(
                "sub-free-unavailable",
                "free-unavailable@example.com",
                "org-free-unavailable",
                "free",
            ),
            access_token: build_access_token(
                "sub-free-unavailable",
                "free-unavailable@example.com",
                "org-free-unavailable",
                "free",
            ),
            refresh_token: "refresh-free-unavailable".to_string(),
            api_key_access_token: None,
            last_refresh: now + 1,
        })
        .expect("insert unavailable token");

    storage
        .insert_account(&Account {
            id: "acc-pro-invalid".to_string(),
            label: "Pro Invalid".to_string(),
            issuer: "https://auth.openai.com".to_string(),
            chatgpt_account_id: Some("org-pro-invalid".to_string()),
            workspace_id: Some("org-pro-invalid".to_string()),
            group_name: None,
            sort: 1,
            status: "unavailable".to_string(),
            created_at: now + 1,
            updated_at: now + 1,
        })
        .expect("insert unavailable pro account");
    storage
        .insert_token(&Token {
            account_id: "acc-pro-invalid".to_string(),
            id_token: build_access_token(
                "sub-pro-invalid",
                "pro-invalid@example.com",
                "org-pro-invalid",
                "pro",
            ),
            access_token: build_access_token(
                "sub-pro-invalid",
                "pro-invalid@example.com",
                "org-pro-invalid",
                "pro",
            ),
            refresh_token: "refresh-pro-invalid".to_string(),
            api_key_access_token: None,
            last_refresh: now + 1,
        })
        .expect("insert pro token");

    let server = codexmanager_service::start_one_shot_server().expect("start server");
    let req = JsonRpcRequest {
        id: 77.into(),
        method: "account/deleteUnavailableFree".to_string(),
        params: None,
        trace: None,
    };
    let json = serde_json::to_string(&req).expect("serialize delete");
    let v = post_rpc(&server.addr, &json);
    let result = v.get("result").expect("result");

    assert_eq!(
        result.get("deleted").and_then(|value| value.as_u64()),
        Some(2)
    );
    let deleted_ids = result
        .get("deletedAccountIds")
        .and_then(|value| value.as_array())
        .expect("deleted ids");
    assert_eq!(deleted_ids.len(), 2);
    assert_eq!(deleted_ids[0].as_str(), Some("acc-free-invalid"));
    assert_eq!(deleted_ids[1].as_str(), Some("acc-free-unavailable"));

    let remaining = storage.list_accounts().expect("list accounts");
    let remaining_ids = remaining
        .into_iter()
        .map(|item| item.id)
        .collect::<Vec<_>>();
    assert_eq!(remaining_ids, vec!["acc-pro-invalid"]);
}

/// 函数 `rpc_account_delete_by_statuses_deletes_only_selected_statuses`
///
/// 作者: gaohongshun
///
/// 时间: 2026-05-04
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn rpc_account_delete_by_statuses_deletes_only_selected_statuses() {
    let ctx = RpcTestContext::new("rpc-account-delete-by-statuses");
    let storage = Storage::open(ctx.db_path()).expect("open db");
    storage.init().expect("init schema");
    let now = now_ts();

    for (idx, (id, status)) in [
        ("acc-active", "active"),
        ("acc-unavailable", "unavailable"),
        ("acc-banned", "banned"),
        ("acc-limited", "limited"),
        ("acc-disabled", "disabled"),
        ("acc-inactive", "inactive"),
    ]
    .into_iter()
    .enumerate()
    {
        storage
            .insert_account(&Account {
                id: id.to_string(),
                label: id.to_string(),
                issuer: "https://auth.openai.com".to_string(),
                chatgpt_account_id: None,
                workspace_id: None,
                group_name: None,
                sort: idx as i64,
                status: status.to_string(),
                created_at: now,
                updated_at: now,
            })
            .expect("insert account");
    }

    let server = codexmanager_service::start_one_shot_server().expect("start server");
    let req = JsonRpcRequest {
        id: 78.into(),
        method: "account/deleteByStatuses".to_string(),
        params: Some(serde_json::json!({
            "statuses": ["banned", "limited", "inactive"]
        })),
        trace: None,
    };
    let json = serde_json::to_string(&req).expect("serialize delete");
    let v = post_rpc(&server.addr, &json);
    let result = v.get("result").expect("result");

    assert_eq!(
        result.get("deleted").and_then(|value| value.as_u64()),
        Some(3)
    );
    let target_statuses = result
        .get("targetStatuses")
        .and_then(|value| value.as_array())
        .expect("target statuses");
    assert_eq!(
        target_statuses
            .iter()
            .filter_map(|value| value.as_str())
            .collect::<Vec<_>>(),
        vec!["banned", "limited", "inactive"]
    );
    let deleted_ids = result
        .get("deletedAccountIds")
        .and_then(|value| value.as_array())
        .expect("deleted ids");
    assert_eq!(
        deleted_ids
            .iter()
            .filter_map(|value| value.as_str())
            .collect::<Vec<_>>(),
        vec!["acc-banned", "acc-limited", "acc-inactive"]
    );

    let remaining = storage.list_accounts().expect("list accounts");
    let remaining_ids = remaining
        .into_iter()
        .map(|item| item.id)
        .collect::<Vec<_>>();
    assert_eq!(
        remaining_ids,
        vec!["acc-active", "acc-unavailable", "acc-disabled"]
    );
}

/// 函数 `rpc_account_delete_by_statuses_deletes_unknown_status`
///
/// 作者: gaohongshun
///
/// 时间: 2026-05-04
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn rpc_account_delete_by_statuses_deletes_unknown_status() {
    let ctx = RpcTestContext::new("rpc-account-delete-by-statuses-unknown");
    let storage = Storage::open(ctx.db_path()).expect("open db");
    storage.init().expect("init schema");
    let now = now_ts();
    for (idx, (id, status)) in [("acc-active", "active"), ("acc-unknown", "unknown")]
        .into_iter()
        .enumerate()
    {
        storage
            .insert_account(&Account {
                id: id.to_string(),
                label: id.to_string(),
                issuer: "https://auth.openai.com".to_string(),
                chatgpt_account_id: None,
                workspace_id: None,
                group_name: None,
                sort: idx as i64,
                status: status.to_string(),
                created_at: now,
                updated_at: now,
            })
            .expect("insert account");
    }

    let server = codexmanager_service::start_one_shot_server().expect("start server");
    let req = JsonRpcRequest {
        id: 79.into(),
        method: "account/deleteByStatuses".to_string(),
        params: Some(serde_json::json!({
            "statuses": ["unknown"]
        })),
        trace: None,
    };
    let json = serde_json::to_string(&req).expect("serialize delete");
    let v = post_rpc(&server.addr, &json);
    let result = v.get("result").expect("result");

    assert_eq!(
        result.get("deleted").and_then(|value| value.as_u64()),
        Some(1)
    );
    let target_statuses = result
        .get("targetStatuses")
        .and_then(|value| value.as_array())
        .expect("target statuses");
    assert_eq!(
        target_statuses
            .iter()
            .filter_map(|value| value.as_str())
            .collect::<Vec<_>>(),
        vec!["unknown"]
    );
    let remaining = storage.list_accounts().expect("list accounts");
    let remaining_ids = remaining
        .into_iter()
        .map(|item| item.id)
        .collect::<Vec<_>>();
    assert_eq!(remaining_ids, vec!["acc-active"]);
}

/// 函数 `rpc_account_update_status_toggles_manual_enable_disable`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn rpc_account_update_status_toggles_manual_enable_disable() {
    let ctx = RpcTestContext::new("rpc-account-update-status");
    ctx.seed_accounts(1);

    let disable_server = codexmanager_service::start_one_shot_server().expect("start server");
    let disable_req = JsonRpcRequest {
        id: 12.into(),
        method: "account/update".to_string(),
        params: Some(serde_json::json!({
            "accountId": "acc-0",
            "status": "disabled"
        })),
        trace: None,
    };
    let disable_json = serde_json::to_string(&disable_req).expect("serialize");
    let disable_resp = post_rpc(&disable_server.addr, &disable_json);
    let disable_result = disable_resp.get("result").expect("result");
    assert_eq!(
        disable_result.get("ok").and_then(|value| value.as_bool()),
        Some(true)
    );

    let storage = Storage::open(ctx.db_path()).expect("open db");
    let disabled = storage
        .find_account_by_id("acc-0")
        .expect("find account")
        .expect("account exists");
    assert_eq!(disabled.status, "disabled");

    let enable_server = codexmanager_service::start_one_shot_server().expect("start server");
    let enable_req = JsonRpcRequest {
        id: 13.into(),
        method: "account/update".to_string(),
        params: Some(serde_json::json!({
            "accountId": "acc-0",
            "status": "active"
        })),
        trace: None,
    };
    let enable_json = serde_json::to_string(&enable_req).expect("serialize");
    let enable_resp = post_rpc(&enable_server.addr, &enable_json);
    let enable_result = enable_resp.get("result").expect("result");
    assert_eq!(
        enable_result.get("ok").and_then(|value| value.as_bool()),
        Some(true)
    );

    let active = storage
        .find_account_by_id("acc-0")
        .expect("find account")
        .expect("account exists");
    assert_eq!(active.status, "active");
}

/// 函数 `rpc_login_start_returns_url`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn rpc_login_start_returns_url() {
    let _ctx = RpcTestContext::new("rpc-login-start");
    let _login_addr_guard = EnvGuard::set("CODEXMANAGER_LOGIN_ADDR", "127.0.0.1:0");
    let server = codexmanager_service::start_one_shot_server().expect("start server");

    let req = JsonRpcRequest {
        id: 4.into(),
        method: "account/login/start".to_string(),
        params: Some(serde_json::json!({"type": "chatgpt", "openBrowser": false})),
        trace: None,
    };
    let json = serde_json::to_string(&req).expect("serialize");
    let v = post_rpc(&server.addr, &json);
    let result = v.get("result").expect("result");
    assert_eq!(result.get("type").and_then(|v| v.as_str()), Some("chatgpt"));
    let auth_url = result.get("authUrl").and_then(|v| v.as_str()).unwrap();
    let login_id = result.get("loginId").and_then(|v| v.as_str()).unwrap();
    assert!(auth_url.contains("oauth/authorize"));
    assert!(!login_id.is_empty());
}

/// 函数 `rpc_login_start_returns_api_key_variant`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn rpc_login_start_returns_api_key_variant() {
    let _ctx = RpcTestContext::new("rpc-login-api-key");
    let server = codexmanager_service::start_one_shot_server().expect("start server");

    let req = JsonRpcRequest {
        id: 44.into(),
        method: "account/login/start".to_string(),
        params: Some(serde_json::json!({"type": "apiKey", "openBrowser": false})),
        trace: None,
    };
    let json = serde_json::to_string(&req).expect("serialize");
    let v = post_rpc(&server.addr, &json);
    let result = v.get("result").expect("result");
    assert_eq!(
        result.get("type").and_then(|value| value.as_str()),
        Some("apiKey")
    );
}

/// 函数 `rpc_login_start_chatgpt_device_code_returns_user_code`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn rpc_login_start_chatgpt_device_code_returns_user_code() {
    let _ctx = RpcTestContext::new("rpc-login-device-code");
    let (issuer, request_rx, request_join) = start_mock_device_login_server();
    let _issuer_guard = EnvGuard::set("CODEXMANAGER_ISSUER", &issuer);

    let server = codexmanager_service::start_one_shot_server().expect("start server");
    let req = JsonRpcRequest {
        id: 4.into(),
        method: "account/login/start".to_string(),
        params: Some(serde_json::json!({"type": "chatgptDeviceCode", "openBrowser": false})),
        trace: None,
    };
    let json = serde_json::to_string(&req).expect("serialize");
    let v = post_rpc(&server.addr, &json);
    let result = v.get("result").expect("result");
    assert_eq!(
        result.get("type").and_then(|v| v.as_str()),
        Some("chatgptDeviceCode")
    );
    assert!(result
        .get("verificationUrl")
        .and_then(|v| v.as_str())
        .is_some_and(|value| value.contains("/codex/device")));
    assert_eq!(
        result.get("userCode").and_then(|v| v.as_str()),
        Some("ABCD-1234")
    );
    let login_id = result
        .get("loginId")
        .and_then(|v| v.as_str())
        .expect("login id")
        .to_string();

    let mut requests = Vec::new();
    for _ in 0..4 {
        requests.push(
            request_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("receive device request"),
        );
    }
    request_join.join().expect("join mock device server");

    assert_eq!(
        requests[0].path, "/api/accounts/deviceauth/usercode",
        "unexpected first request: {requests:?}"
    );
    assert!(requests[0].body.contains("client_id"));
    assert_eq!(requests[1].path, "/api/accounts/deviceauth/token");
    assert!(requests[1]
        .body
        .contains("\"device_auth_id\":\"device-auth-123\""));
    assert!(requests[1].body.contains("\"user_code\":\"ABCD-1234\""));
    assert_eq!(requests[2].path, "/oauth/token");
    assert!(requests[2].body.contains("grant_type=authorization_code"));
    assert_eq!(requests[3].path, "/oauth/token");
    assert!(requests[3]
        .body
        .contains("grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Atoken-exchange"));

    let status_server = codexmanager_service::start_one_shot_server().expect("start server");
    let status_req = JsonRpcRequest {
        id: 5.into(),
        method: "account/login/status".to_string(),
        params: Some(serde_json::json!({ "loginId": login_id })),
        trace: None,
    };
    let status_json = serde_json::to_string(&status_req).expect("serialize status");
    let status_resp = post_rpc(&status_server.addr, &status_json);
    let status_result = status_resp.get("result").expect("status result");
    assert_eq!(
        status_result.get("status").and_then(|value| value.as_str()),
        Some("success")
    );

    let storage = Storage::open(_ctx.db_path()).expect("open db");
    let accounts = storage.list_accounts().expect("list accounts");
    assert!(
        accounts
            .iter()
            .any(|account| account.id.contains("sub-device")),
        "device login should persist an account: {accounts:?}"
    );
}

/// 函数 `rpc_chatgpt_auth_tokens_login_read_logout_roundtrip`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn rpc_chatgpt_auth_tokens_login_read_logout_roundtrip() {
    let ctx = RpcTestContext::new("rpc-chatgpt-auth-tokens-roundtrip");
    let access_token = build_access_token(
        "sub-external",
        "embedded@example.com",
        "org-embedded",
        "pro",
    );

    let login_req = JsonRpcRequest {
        id: 41.into(),
        method: "account/login/start".to_string(),
        params: Some(serde_json::json!({
            "type": "chatgptAuthTokens",
            "accessToken": access_token,
            "chatgptAccountId": "org-embedded",
            "chatgptPlanType": "pro"
        })),
        trace: None,
    };
    let login_json = serde_json::to_string(&login_req).expect("serialize login");
    let login_server = codexmanager_service::start_one_shot_server().expect("start server");
    let login_resp = post_rpc(&login_server.addr, &login_json);
    let login_result = login_resp.get("result").expect("login result");
    assert_eq!(
        login_result.get("type").and_then(|value| value.as_str()),
        Some("chatgptAuthTokens")
    );

    let read_req = JsonRpcRequest {
        id: 42.into(),
        method: "account/read".to_string(),
        params: Some(serde_json::json!({ "refreshToken": false })),
        trace: None,
    };
    let read_json = serde_json::to_string(&read_req).expect("serialize read");
    let read_server = codexmanager_service::start_one_shot_server().expect("start server");
    let read_resp = post_rpc(&read_server.addr, &read_json);
    let read_result = read_resp.get("result").expect("read result");
    let account = read_result.get("account").expect("current account");
    assert!(read_result.get("authMode").is_none());
    assert_eq!(
        account.get("email").and_then(|value| value.as_str()),
        Some("embedded@example.com")
    );
    assert_eq!(
        account.get("planType").and_then(|value| value.as_str()),
        Some("pro")
    );
    assert_eq!(
        account
            .get("chatgptAccountId")
            .and_then(|value| value.as_str()),
        Some("org-embedded")
    );

    let logout_req = JsonRpcRequest {
        id: 43.into(),
        method: "account/logout".to_string(),
        params: None,
        trace: None,
    };
    let logout_json = serde_json::to_string(&logout_req).expect("serialize logout");
    let logout_server = codexmanager_service::start_one_shot_server().expect("start server");
    let logout_resp = post_rpc(&logout_server.addr, &logout_json);
    let logout_result = logout_resp.get("result").expect("logout result");
    assert!(logout_result
        .as_object()
        .is_some_and(|value| value.is_empty()));

    let read_after_logout_server =
        codexmanager_service::start_one_shot_server().expect("start server");
    let read_after_logout = post_rpc(&read_after_logout_server.addr, &read_json);
    let read_after_logout_result = read_after_logout.get("result").expect("read result");
    assert!(read_after_logout_result.get("account").unwrap().is_null());

    let storage = Storage::open(ctx.db_path()).expect("open db");
    let account_id = storage
        .list_accounts()
        .expect("list accounts")
        .into_iter()
        .find(|account| account.chatgpt_account_id.as_deref() == Some("org-embedded"))
        .map(|account| account.id)
        .expect("account id");
    let account = storage
        .find_account_by_id(&account_id)
        .expect("find account")
        .expect("account exists");
    assert_eq!(account.status, "inactive");
}

#[test]
fn rpc_chatgpt_auth_tokens_login_enqueues_usage_refresh() {
    let ctx = RpcTestContext::new("rpc-chatgpt-auth-tokens-login-auto-usage-refresh");
    let access_token = build_access_token(
        "sub-usage-refresh",
        "usage-refresh@example.com",
        "org-usage-refresh",
        "pro",
    );
    let accounts_check_response = serde_json::json!({
        "accounts": {
            "org-usage-refresh": {
                "account": {
                    "plan_type": "pro",
                    "is_default": true
                },
                "entitlement": {
                    "subscription_plan": "plus",
                    "expires_at": "2026-05-06T03:31:29Z",
                    "next_renewal_at": "2026-04-20T03:31:29Z",
                    "has_active_subscription": true
                }
            }
        }
    });
    let usage_response = serde_json::json!({
        "rate_limit": {
            "primary_window": {
                "used_percent": 25.0,
                "limit_window_seconds": 18000,
                "reset_at": 1776655889
            },
            "secondary_window": {
                "used_percent": 10.0,
                "limit_window_seconds": 604800,
                "reset_at": 1778038289
            }
        }
    });
    let (usage_base_url, request_rx, request_join) = start_mock_usage_refresh_server(
        serde_json::to_string(&accounts_check_response)
            .expect("serialize auto usage refresh accounts check response"),
        serde_json::to_string(&usage_response).expect("serialize auto usage response"),
    );
    let _auto_refresh_guard =
        EnvGuard::set("CODEXMANAGER_AUTO_USAGE_REFRESH_AFTER_ACCOUNT_ADD", "1");
    let _usage_base_url_guard = EnvGuard::set("CODEXMANAGER_USAGE_BASE_URL", &usage_base_url);

    let login_req = JsonRpcRequest {
        id: 46.into(),
        method: "account/login/start".to_string(),
        params: Some(serde_json::json!({
            "type": "chatgptAuthTokens",
            "accessToken": access_token.clone(),
            "chatgptAccountId": "org-usage-refresh"
        })),
        trace: None,
    };
    let login_json = serde_json::to_string(&login_req).expect("serialize login");
    let login_server = codexmanager_service::start_one_shot_server().expect("start server");
    let login_resp = post_rpc(&login_server.addr, &login_json);
    let login_result = login_resp.get("result").expect("login result");
    assert_eq!(
        login_result.get("type").and_then(|value| value.as_str()),
        Some("chatgptAuthTokens")
    );

    let first_request = request_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("receive auto accounts check request");
    let second_request = request_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("receive auto usage request");
    request_join.join().expect("join auto usage refresh server");

    assert_eq!(first_request.path, "/accounts/check/v4-2023-04-27");
    assert_eq!(
        first_request.authorization.as_deref(),
        Some(format!("Bearer {access_token}").as_str())
    );
    assert_eq!(first_request.chatgpt_account_id, None);
    assert_eq!(second_request.path, "/api/codex/usage");
    assert_eq!(
        second_request.authorization.as_deref(),
        Some(format!("Bearer {access_token}").as_str())
    );
    assert_eq!(
        second_request.chatgpt_account_id.as_deref(),
        Some("org-usage-refresh")
    );

    let storage = Storage::open(ctx.db_path()).expect("open db");
    let account_id = storage
        .list_accounts()
        .expect("list accounts")
        .into_iter()
        .find(|account| account.chatgpt_account_id.as_deref() == Some("org-usage-refresh"))
        .map(|account| account.id)
        .expect("account id");
    let snapshot = storage
        .latest_usage_snapshot_for_account(&account_id)
        .expect("find usage snapshot")
        .expect("usage snapshot exists");
    assert_eq!(snapshot.used_percent, Some(25.0));
    assert_eq!(snapshot.secondary_used_percent, Some(10.0));
}

/// 函数 `rpc_chatgpt_auth_tokens_refresh_updates_access_token`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn rpc_chatgpt_auth_tokens_refresh_updates_access_token() {
    let _ctx = RpcTestContext::new("rpc-chatgpt-auth-tokens-refresh");
    let refreshed_access_token =
        build_access_token("sub-refresh", "refreshed@example.com", "org-refresh", "pro");
    let accounts_check_response = serde_json::json!({
        "accounts": {
            "org-refresh": {
                "account": {
                    "plan_type": "pro",
                    "is_default": true
                },
                "entitlement": {
                    "subscription_plan": "plus",
                    "expires_at": "2026-05-06T03:31:29Z",
                    "next_renewal_at": "2026-04-20T03:31:29Z",
                    "has_active_subscription": true
                }
            }
        }
    });
    let refresh_response = serde_json::json!({
        "access_token": refreshed_access_token,
        "refresh_token": "refresh-token-new"
    });
    let (issuer, refresh_rx, refresh_join) = start_mock_oauth_token_server(
        200,
        serde_json::to_string(&refresh_response).expect("serialize refresh response"),
    );
    let (usage_base_url, subscription_rx, subscription_join) = start_mock_subscription_server(
        200,
        serde_json::to_string(&accounts_check_response).expect("serialize subscription response"),
    );
    let _issuer_guard = EnvGuard::set("CODEXMANAGER_ISSUER", &issuer);
    let _client_id_guard = EnvGuard::set("CODEXMANAGER_CLIENT_ID", "client-test-rpc-refresh");
    let _usage_base_url_guard = EnvGuard::set("CODEXMANAGER_USAGE_BASE_URL", &usage_base_url);

    let login_req = JsonRpcRequest {
        id: 44.into(),
        method: "account/login/start".to_string(),
        params: Some(serde_json::json!({
            "type": "chatgptAuthTokens",
            "accessToken": build_access_token(
                "sub-refresh",
                "initial@example.com",
                "org-refresh",
                "pro"
            ),
            "refreshToken": "refresh-token-old",
            "chatgptAccountId": "org-refresh"
        })),
        trace: None,
    };
    let login_json = serde_json::to_string(&login_req).expect("serialize login");
    let login_server = codexmanager_service::start_one_shot_server().expect("start server");
    let login_resp = post_rpc(&login_server.addr, &login_json);
    let login_result = login_resp.get("result").expect("login result");
    assert_eq!(
        login_result.get("type").and_then(|value| value.as_str()),
        Some("chatgptAuthTokens")
    );

    let refresh_req = JsonRpcRequest {
        id: 45.into(),
        method: "account/chatgptAuthTokens/refresh".to_string(),
        params: Some(serde_json::json!({
            "reason": "unauthorized",
            "previousAccountId": "org-refresh"
        })),
        trace: None,
    };
    let refresh_json = serde_json::to_string(&refresh_req).expect("serialize refresh");
    let refresh_server = codexmanager_service::start_one_shot_server().expect("start server");
    let refresh_rpc_resp = post_rpc(&refresh_server.addr, &refresh_json);
    let refresh_result = refresh_rpc_resp.get("result").expect("refresh result");
    assert_eq!(
        refresh_result
            .get("chatgptAccountId")
            .and_then(|value| value.as_str()),
        Some("org-refresh")
    );
    assert_eq!(
        refresh_result
            .get("chatgptPlanType")
            .and_then(|value| value.as_str()),
        Some("pro")
    );
    assert_eq!(
        refresh_result
            .get("hasSubscription")
            .and_then(|value| value.as_bool()),
        Some(true)
    );
    assert_eq!(
        refresh_result
            .get("subscriptionPlan")
            .and_then(|value| value.as_str()),
        Some("plus")
    );
    assert_eq!(
        refresh_result
            .get("subscriptionExpiresAt")
            .and_then(|value| value.as_i64()),
        Some(1_778_038_289)
    );
    assert_eq!(
        refresh_result
            .get("subscriptionRenewsAt")
            .and_then(|value| value.as_i64()),
        Some(1_776_655_889)
    );
    assert_eq!(
        refresh_result
            .get("accessToken")
            .and_then(|value| value.as_str()),
        Some(refreshed_access_token.as_str())
    );

    let refresh_body = refresh_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("receive refresh request");
    let subscription_request = subscription_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("receive subscription request");
    refresh_join.join().expect("join mock oauth server");
    subscription_join
        .join()
        .expect("join mock subscription server");
    assert!(refresh_body.contains("grant_type=refresh_token"));
    assert!(refresh_body.contains("refresh_token=refresh-token-old"));
    assert!(refresh_body.contains("scope=openid+profile+email"));
    assert_eq!(subscription_request.path, "/accounts/check/v4-2023-04-27");
    assert_eq!(
        subscription_request.authorization.as_deref(),
        Some(format!("Bearer {refreshed_access_token}").as_str())
    );
    assert_eq!(subscription_request.chatgpt_account_id, None);

    let storage =
        Storage::open(std::env::var("CODEXMANAGER_DB_PATH").expect("db path")).expect("open db");
    let account_id = storage
        .list_accounts()
        .expect("list accounts")
        .into_iter()
        .find(|account| account.chatgpt_account_id.as_deref() == Some("org-refresh"))
        .map(|account| account.id)
        .expect("account id");
    let token = storage
        .find_token_by_account_id(&account_id)
        .expect("find token")
        .expect("token exists");
    assert_eq!(token.access_token, refreshed_access_token);
    assert_eq!(token.refresh_token, "refresh-token-new");
    let subscription = storage
        .find_account_subscription(&account_id)
        .expect("find subscription")
        .expect("subscription exists");
    assert!(subscription.has_subscription);
    assert_eq!(subscription.account_plan_type.as_deref(), Some("pro"));
    assert_eq!(subscription.plan_type.as_deref(), Some("plus"));
    assert_eq!(subscription.expires_at, Some(1_778_038_289));
    assert_eq!(subscription.renews_at, Some(1_776_655_889));
}

#[test]
fn rpc_usage_refresh_persists_subscription_fields() {
    let ctx = RpcTestContext::new("rpc-usage-refresh-subscription");
    let access_token = build_access_token(
        "sub-usage-refresh",
        "usage-refresh@example.com",
        "org-usage-refresh",
        "pro",
    );
    let accounts_check_response = serde_json::json!({
        "accounts": {
            "org-usage-refresh": {
                "account": {
                    "plan_type": "pro",
                    "is_default": true
                },
                "entitlement": {
                    "subscription_plan": "plus",
                    "expires_at": "2026-05-06T03:31:29Z",
                    "next_renewal_at": "2026-04-20T03:31:29Z",
                    "has_active_subscription": true
                }
            }
        }
    });
    let usage_response = serde_json::json!({
        "rate_limit": {
            "primary_window": {
                "used_percent": 25.0,
                "limit_window_seconds": 18000,
                "reset_at": 1776655889
            },
            "secondary_window": {
                "used_percent": 10.0,
                "limit_window_seconds": 604800,
                "reset_at": 1778038289
            }
        },
        "additional_rate_limits": [
            {
                "limit_name": "Spark",
                "metered_feature": "codex_other",
                "rate_limit": {
                    "allowed": true,
                    "limit_reached": false,
                    "primary_window": {
                        "used_percent": 40.0,
                        "limit_window_seconds": 86400,
                        "reset_at": 1776742289
                    }
                }
            }
        ]
    });
    let (usage_base_url, request_rx, request_join) = start_mock_usage_refresh_server(
        serde_json::to_string(&accounts_check_response)
            .expect("serialize usage refresh accounts check response"),
        serde_json::to_string(&usage_response).expect("serialize usage response"),
    );
    let _usage_base_url_guard = EnvGuard::set("CODEXMANAGER_USAGE_BASE_URL", &usage_base_url);

    let login_req = JsonRpcRequest {
        id: 46.into(),
        method: "account/login/start".to_string(),
        params: Some(serde_json::json!({
            "type": "chatgptAuthTokens",
            "accessToken": access_token.clone(),
            "chatgptAccountId": "org-usage-refresh"
        })),
        trace: None,
    };
    let login_json = serde_json::to_string(&login_req).expect("serialize login");
    let login_server = codexmanager_service::start_one_shot_server().expect("start server");
    let login_resp = post_rpc(&login_server.addr, &login_json);
    let login_result = login_resp.get("result").expect("login result");
    assert_eq!(
        login_result.get("type").and_then(|value| value.as_str()),
        Some("chatgptAuthTokens")
    );

    let storage = Storage::open(ctx.db_path()).expect("open db");
    let account_id = storage
        .list_accounts()
        .expect("list accounts")
        .into_iter()
        .find(|account| account.chatgpt_account_id.as_deref() == Some("org-usage-refresh"))
        .map(|account| account.id)
        .expect("account id");

    let refresh_req = JsonRpcRequest {
        id: 47.into(),
        method: "account/usage/refresh".to_string(),
        params: Some(serde_json::json!({
            "accountId": account_id.clone()
        })),
        trace: None,
    };
    let refresh_json = serde_json::to_string(&refresh_req).expect("serialize usage refresh");
    let refresh_server = codexmanager_service::start_one_shot_server().expect("start server");
    let refresh_resp = post_rpc(&refresh_server.addr, &refresh_json);
    assert_eq!(
        refresh_resp
            .get("result")
            .and_then(|value| value.get("ok"))
            .and_then(|value| value.as_bool()),
        Some(true)
    );

    let first_request = request_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("receive first usage refresh request");
    let second_request = request_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("receive second usage refresh request");
    request_join.join().expect("join usage refresh server");

    assert_eq!(first_request.path, "/accounts/check/v4-2023-04-27");
    assert_eq!(
        first_request.authorization.as_deref(),
        Some(format!("Bearer {access_token}").as_str())
    );
    assert_eq!(first_request.chatgpt_account_id, None);
    assert_eq!(second_request.path, "/api/codex/usage");
    assert_eq!(
        second_request.authorization.as_deref(),
        Some(format!("Bearer {access_token}").as_str())
    );
    assert_eq!(
        second_request.chatgpt_account_id.as_deref(),
        Some("org-usage-refresh")
    );

    let storage = Storage::open(ctx.db_path()).expect("open db");
    let subscription = storage
        .find_account_subscription(&account_id)
        .expect("find subscription")
        .expect("subscription exists");
    assert!(subscription.has_subscription);
    assert_eq!(subscription.account_plan_type.as_deref(), Some("pro"));
    assert_eq!(subscription.plan_type.as_deref(), Some("plus"));
    assert_eq!(subscription.expires_at, Some(1_778_038_289));
    assert_eq!(subscription.renews_at, Some(1_776_655_889));

    let snapshot = storage
        .latest_usage_snapshot_for_account(&account_id)
        .expect("find usage snapshot")
        .expect("usage snapshot exists");
    assert_eq!(snapshot.used_percent, Some(25.0));
    assert_eq!(snapshot.window_minutes, Some(300));
    assert_eq!(snapshot.secondary_used_percent, Some(10.0));
    assert_eq!(snapshot.secondary_window_minutes, Some(10080));
    let credits: serde_json::Value = serde_json::from_str(
        snapshot
            .credits_json
            .as_deref()
            .expect("credits json with extra rate limits"),
    )
    .expect("parse credits json");
    let extras = credits["_codexmanager_extra_rate_limits"]
        .as_array()
        .expect("extra rate limits array");
    assert_eq!(extras.len(), 1);
    assert_eq!(extras[0]["source_key"], "codex_other");
    assert_eq!(extras[0]["limit_name"], "Spark");
    assert_eq!(extras[0]["primary_window"]["used_percent"], 40.0);
}

/// 函数 `rpc_usage_read_empty`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn rpc_usage_read_empty() {
    let _ctx = RpcTestContext::new("rpc-usage-read");
    let server = codexmanager_service::start_one_shot_server().expect("start server");

    let req = JsonRpcRequest {
        id: 5.into(),
        method: "account/usage/read".to_string(),
        params: None,
        trace: None,
    };
    let json = serde_json::to_string(&req).expect("serialize");
    let v = post_rpc(&server.addr, &json);
    let result = v.get("result").expect("result");
    assert!(result.get("snapshot").is_some());
}

/// 函数 `rpc_login_status_pending`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn rpc_login_status_pending() {
    let _ctx = RpcTestContext::new("rpc-login-status");
    let server = codexmanager_service::start_one_shot_server().expect("start server");

    let req = JsonRpcRequest {
        id: 6.into(),
        method: "account/login/status".to_string(),
        params: Some(serde_json::json!({"loginId": "login-1"})),
        trace: None,
    };
    let json = serde_json::to_string(&req).expect("serialize");
    let v = post_rpc(&server.addr, &json);
    let result = v.get("result").expect("result");
    assert!(result.get("status").is_some());
}

/// 函数 `rpc_usage_list_empty`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn rpc_usage_list_empty() {
    let _ctx = RpcTestContext::new("rpc-usage-list");
    let server = codexmanager_service::start_one_shot_server().expect("start server");

    let req = JsonRpcRequest {
        id: 7.into(),
        method: "account/usage/list".to_string(),
        params: None,
        trace: None,
    };
    let json = serde_json::to_string(&req).expect("serialize");
    let v = post_rpc(&server.addr, &json);
    let result = v.get("result").expect("result");
    let items = result
        .get("items")
        .and_then(|value| value.as_array())
        .expect("items array");
    assert!(
        items.is_empty(),
        "expected empty usage items, got: {result}"
    );
}

#[test]
fn rpc_usage_list_limit_returns_recent_latest_snapshots() {
    let ctx = RpcTestContext::new("rpc-usage-list-limit");
    let storage = Storage::open(ctx.db_path()).expect("open db");
    storage.init().expect("init schema");
    let now = now_ts();
    for (account_id, captured_at, used_percent) in [
        ("acc-old", now, 10.0),
        ("acc-newest", now + 3, 30.0),
        ("acc-middle", now + 2, 20.0),
    ] {
        storage
            .insert_usage_snapshot(&UsageSnapshotRecord {
                account_id: account_id.to_string(),
                used_percent: Some(used_percent),
                window_minutes: Some(180),
                resets_at: None,
                secondary_used_percent: None,
                secondary_window_minutes: None,
                secondary_resets_at: None,
                credits_json: None,
                captured_at,
            })
            .expect("insert usage snapshot");
    }

    let server = codexmanager_service::start_one_shot_server().expect("start server");
    let req = JsonRpcRequest {
        id: 17.into(),
        method: "account/usage/list".to_string(),
        params: Some(serde_json::json!({ "limit": 2 })),
        trace: None,
    };
    let json = serde_json::to_string(&req).expect("serialize");
    let v = post_rpc(&server.addr, &json);
    let result = v.get("result").expect("result");
    let items = result
        .get("items")
        .and_then(|value| value.as_array())
        .expect("items array");

    assert_eq!(items.len(), 2);
    assert_eq!(
        items
            .iter()
            .filter_map(|item| item.get("accountId").and_then(|value| value.as_str()))
            .collect::<Vec<_>>(),
        vec!["acc-newest", "acc-middle"]
    );
}

#[test]
fn rpc_usage_list_zero_limit_returns_empty_without_unbounded_read() {
    let ctx = RpcTestContext::new("rpc-usage-list-zero-limit");
    let storage = Storage::open(ctx.db_path()).expect("open db");
    storage.init().expect("init schema");
    storage
        .insert_usage_snapshot(&UsageSnapshotRecord {
            account_id: "acc-hidden".to_string(),
            used_percent: Some(10.0),
            window_minutes: Some(180),
            resets_at: None,
            secondary_used_percent: None,
            secondary_window_minutes: None,
            secondary_resets_at: None,
            credits_json: None,
            captured_at: now_ts(),
        })
        .expect("insert usage snapshot");

    let server = codexmanager_service::start_one_shot_server().expect("start server");
    let req = JsonRpcRequest {
        id: 18.into(),
        method: "account/usage/list".to_string(),
        params: Some(serde_json::json!({ "limit": 0 })),
        trace: None,
    };
    let json = serde_json::to_string(&req).expect("serialize");
    let v = post_rpc(&server.addr, &json);
    let result = v.get("result").expect("result");
    let items = result
        .get("items")
        .and_then(|value| value.as_array())
        .expect("items array");

    assert!(items.is_empty(), "expected zero-limit usage list: {result}");
}

/// 函数 `rpc_usage_aggregate_returns_backend_summary`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn rpc_usage_aggregate_returns_backend_summary() {
    let ctx = RpcTestContext::new("rpc-usage-aggregate");
    let storage = Storage::open(ctx.db_path()).expect("open db");
    storage.init().expect("init schema");
    let now = now_ts();

    storage
        .insert_account(&Account {
            id: "acc-pro".to_string(),
            label: "Pro".to_string(),
            issuer: "https://auth.openai.com".to_string(),
            chatgpt_account_id: None,
            workspace_id: None,
            group_name: None,
            sort: 0,
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
        })
        .expect("insert pro account");
    storage
        .insert_account(&Account {
            id: "acc-free".to_string(),
            label: "Free".to_string(),
            issuer: "https://auth.openai.com".to_string(),
            chatgpt_account_id: None,
            workspace_id: None,
            group_name: None,
            sort: 1,
            status: "active".to_string(),
            created_at: now + 1,
            updated_at: now + 1,
        })
        .expect("insert free account");

    storage
        .insert_usage_snapshot(&UsageSnapshotRecord {
            account_id: "acc-pro".to_string(),
            used_percent: Some(10.0),
            window_minutes: Some(300),
            resets_at: None,
            secondary_used_percent: Some(40.0),
            secondary_window_minutes: Some(10080),
            secondary_resets_at: None,
            credits_json: None,
            captured_at: now,
        })
        .expect("insert pro usage");
    storage
        .insert_usage_snapshot(&UsageSnapshotRecord {
            account_id: "acc-free".to_string(),
            used_percent: Some(20.0),
            window_minutes: Some(10080),
            resets_at: None,
            secondary_used_percent: None,
            secondary_window_minutes: None,
            secondary_resets_at: None,
            credits_json: Some(r#"{"planType":"free"}"#.to_string()),
            captured_at: now,
        })
        .expect("insert free usage");

    let server = codexmanager_service::start_one_shot_server().expect("start server");
    let req = JsonRpcRequest {
        id: 71.into(),
        method: "account/usage/aggregate".to_string(),
        params: None,
        trace: None,
    };
    let json = serde_json::to_string(&req).expect("serialize");
    let v = post_rpc(&server.addr, &json);
    let result = v.get("result").expect("result");

    assert_eq!(
        result
            .get("primaryBucketCount")
            .and_then(|value| value.as_i64()),
        Some(1)
    );
    assert_eq!(
        result
            .get("primaryRemainPercent")
            .and_then(|value| value.as_i64()),
        Some(90)
    );
    assert_eq!(
        result
            .get("secondaryBucketCount")
            .and_then(|value| value.as_i64()),
        Some(2)
    );
    assert_eq!(
        result
            .get("secondaryRemainPercent")
            .and_then(|value| value.as_i64()),
        Some(70)
    );
}

/// 函数 `rpc_requestlog_list_and_summary_support_pagination`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn rpc_requestlog_list_and_summary_support_pagination() {
    let ctx = RpcTestContext::new("rpc-requestlog-page");
    let storage = Storage::open(ctx.db_path()).expect("open db");
    storage.init().expect("init schema");

    for index in 0..4_i64 {
        let created_at = now_ts() + index;
        let status_code = if index < 2 { Some(200) } else { Some(502) };
        let request_log_id = storage
            .insert_request_log(&RequestLog {
                trace_id: Some(format!("trc-page-{index}")),
                key_id: Some("gk-page".to_string()),
                account_id: Some("acc-page".to_string()),
                initial_account_id: Some("acc-free".to_string()),
                attempted_account_ids_json: Some(r#"["acc-free","acc-page"]"#.to_string()),
                request_path: "/v1/responses".to_string(),
                original_path: Some("/v1/responses".to_string()),
                adapted_path: Some("/v1/responses".to_string()),
                method: "POST".to_string(),
                model: Some("gpt-5".to_string()),
                reasoning_effort: Some("medium".to_string()),
                response_adapter: Some("Passthrough".to_string()),
                upstream_url: Some("https://chatgpt.com/backend-api/codex/responses".to_string()),
                aggregate_api_supplier_name: None,
                aggregate_api_url: None,
                status_code,
                duration_ms: Some(500 + index),
                first_response_ms: None,
                input_tokens: None,
                cached_input_tokens: None,
                output_tokens: None,
                total_tokens: None,
                reasoning_output_tokens: None,
                estimated_cost_usd: None,
                error: if status_code == Some(502) {
                    Some("stream interrupted".to_string())
                } else {
                    None
                },
                created_at,
                ..Default::default()
            })
            .expect("insert request log");
        storage
            .insert_request_token_stat(&RequestTokenStat {
                request_log_id,
                key_id: Some("gk-page".to_string()),
                account_id: Some("acc-page".to_string()),
                model: Some("gpt-5".to_string()),
                input_tokens: Some(10),
                cached_input_tokens: Some(1),
                output_tokens: Some(2),
                total_tokens: Some(20 + index),
                reasoning_output_tokens: Some(0),
                estimated_cost_usd: Some(0.01),
                created_at,
                ..RequestTokenStat::default()
            })
            .expect("insert token stat");
    }

    let server = codexmanager_service::start_one_shot_server().expect("start server");
    let list_req = JsonRpcRequest {
        id: 72.into(),
        method: "requestlog/list".to_string(),
        params: Some(serde_json::json!({
            "page": 2,
            "pageSize": 1,
            "statusFilter": "5xx"
        })),
        trace: None,
    };
    let list_json = serde_json::to_string(&list_req).expect("serialize requestlog list");
    let list_resp = post_rpc(&server.addr, &list_json);
    let list_result = list_resp.get("result").expect("requestlog list result");
    assert_eq!(
        list_result.get("total").and_then(|value| value.as_i64()),
        Some(2)
    );
    assert_eq!(
        list_result.get("page").and_then(|value| value.as_i64()),
        Some(2)
    );
    assert_eq!(
        list_result.get("pageSize").and_then(|value| value.as_i64()),
        Some(1)
    );
    let items = list_result
        .get("items")
        .and_then(|value| value.as_array())
        .expect("requestlog items");
    assert_eq!(items.len(), 1);
    assert_eq!(
        items[0].get("traceId").and_then(|value| value.as_str()),
        Some("trc-page-2")
    );
    assert_eq!(
        items[0]
            .get("initialAccountId")
            .and_then(|value| value.as_str()),
        Some("acc-free")
    );
    assert_eq!(
        items[0]
            .get("attemptedAccountIds")
            .and_then(|value| value.as_array())
            .map(|items| items.len()),
        Some(2)
    );

    let summary_server = codexmanager_service::start_one_shot_server().expect("start server");
    let summary_req = JsonRpcRequest {
        id: 73.into(),
        method: "requestlog/summary".to_string(),
        params: Some(serde_json::json!({
            "statusFilter": "5xx"
        })),
        trace: None,
    };
    let summary_json = serde_json::to_string(&summary_req).expect("serialize requestlog summary");
    let summary_resp = post_rpc(&summary_server.addr, &summary_json);
    let summary_result = summary_resp
        .get("result")
        .expect("requestlog summary result");
    assert_eq!(
        summary_result
            .get("totalCount")
            .and_then(|value| value.as_i64()),
        Some(4)
    );
    assert_eq!(
        summary_result
            .get("filteredCount")
            .and_then(|value| value.as_i64()),
        Some(2)
    );
    assert_eq!(
        summary_result
            .get("errorCount")
            .and_then(|value| value.as_i64()),
        Some(2)
    );
    assert_eq!(
        summary_result
            .get("totalTokens")
            .and_then(|value| value.as_i64()),
        Some(45)
    );
}

#[test]
fn rpc_apikey_create_accepts_custom_key_and_rejects_duplicate() {
    let _ctx = RpcTestContext::new("rpc-apikey-create-custom-key");
    let custom_key = "sk-codexmanager-custom-fixed";
    let server = codexmanager_service::start_one_shot_server().expect("start server");
    let create_req = JsonRpcRequest {
        id: 80.into(),
        method: "apikey/create".to_string(),
        params: Some(serde_json::json!({
            "name": "custom-key",
            "modelSlug": "gpt-5.4",
            "customKey": custom_key
        })),
        trace: None,
    };
    let create_json = serde_json::to_string(&create_req).expect("serialize apikey create");
    let create_resp = post_rpc(&server.addr, &create_json);
    let create_result = create_resp.get("result").expect("create result");
    assert_eq!(
        create_result.get("key").and_then(|value| value.as_str()),
        Some(custom_key)
    );
    let key_id = create_result
        .get("id")
        .and_then(|value| value.as_str())
        .expect("created key id")
        .to_string();

    let read_server = codexmanager_service::start_one_shot_server().expect("start server");
    let read_req = JsonRpcRequest {
        id: 81.into(),
        method: "apikey/readSecret".to_string(),
        params: Some(serde_json::json!({ "id": key_id })),
        trace: None,
    };
    let read_json = serde_json::to_string(&read_req).expect("serialize apikey read secret");
    let read_resp = post_rpc(&read_server.addr, &read_json);
    assert_eq!(
        read_resp
            .get("result")
            .and_then(|value| value.get("key"))
            .and_then(|value| value.as_str()),
        Some(custom_key)
    );

    let duplicate_server = codexmanager_service::start_one_shot_server().expect("start server");
    let duplicate_req = JsonRpcRequest {
        id: 82.into(),
        method: "apikey/create".to_string(),
        params: Some(serde_json::json!({
            "name": "duplicate-custom-key",
            "customKey": custom_key
        })),
        trace: None,
    };
    let duplicate_json =
        serde_json::to_string(&duplicate_req).expect("serialize duplicate apikey create");
    let duplicate_resp = post_rpc(&duplicate_server.addr, &duplicate_json);
    let duplicate_result = duplicate_resp.get("result").expect("duplicate result");
    let message = duplicate_result
        .get("error")
        .and_then(|value| value.as_str())
        .expect("duplicate error message");
    assert!(message.contains("custom api key already exists"));
}

/// 函数 `rpc_apikey_update_model_updates_name_with_chinese`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn rpc_apikey_update_model_updates_name_with_chinese() {
    let ctx = RpcTestContext::new("rpc-apikey-update-name");
    let storage = Storage::open(ctx.db_path()).expect("open db");
    storage.init().expect("init schema");
    storage
        .insert_api_key(&codexmanager_core::storage::ApiKey {
            id: "gk-update-name".to_string(),
            name: Some("old-name".to_string()),
            model_slug: Some("gpt-5.4".to_string()),
            reasoning_effort: Some("medium".to_string()),
            service_tier: None,
            rotation_strategy: "account_rotation".to_string(),
            aggregate_api_id: None,
            account_plan_filter: None,
            aggregate_api_url: None,
            client_type: "codex".to_string(),
            protocol_type: "openai_compat".to_string(),
            auth_scheme: "authorization_bearer".to_string(),
            upstream_base_url: None,
            static_headers_json: None,
            key_hash: "hash-update-name".to_string(),
            status: "active".to_string(),
            created_at: now_ts(),
            last_used_at: None,
        })
        .expect("insert api key");

    let server = codexmanager_service::start_one_shot_server().expect("start server");
    let update_req = JsonRpcRequest {
        id: 74.into(),
        method: "apikey/updateModel".to_string(),
        params: Some(serde_json::json!({
            "id": "gk-update-name",
            "name": "中文名称",
            "modelSlug": "gpt-5.4",
            "reasoningEffort": "medium"
        })),
        trace: None,
    };
    let update_json = serde_json::to_string(&update_req).expect("serialize apikey update");
    let update_resp = post_rpc(&server.addr, &update_json);
    assert_eq!(
        update_resp
            .get("result")
            .and_then(|value| value.get("ok"))
            .and_then(|value| value.as_bool()),
        Some(true)
    );

    let list_server = codexmanager_service::start_one_shot_server().expect("start server");
    let list_req = JsonRpcRequest {
        id: 75.into(),
        method: "apikey/list".to_string(),
        params: None,
        trace: None,
    };
    let list_json = serde_json::to_string(&list_req).expect("serialize apikey list");
    let list_resp = post_rpc(&list_server.addr, &list_json);
    let items = list_resp
        .get("result")
        .and_then(|value| value.get("items"))
        .and_then(|value| value.as_array())
        .expect("apikey items");
    let updated = items
        .iter()
        .find(|value| {
            value
                .get("id")
                .and_then(|item| item.as_str())
                .map(|id| id == "gk-update-name")
                .unwrap_or(false)
        })
        .expect("updated api key");
    assert_eq!(
        updated.get("name").and_then(|value| value.as_str()),
        Some("中文名称")
    );
}

/// 函数 `rpc_rejects_missing_token`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn rpc_rejects_missing_token() {
    let _ctx = RpcTestContext::new("rpc-missing-token");
    let server = codexmanager_service::start_one_shot_server().expect("start server");

    let req = JsonRpcRequest {
        id: 8.into(),
        method: "initialize".to_string(),
        params: None,
        trace: None,
    };
    let json = serde_json::to_string(&req).expect("serialize");
    let (status, _) = post_rpc_raw(&server.addr, &json, &[("Content-Type", "application/json")]);
    assert_eq!(status, 401);
}

/// 函数 `rpc_rejects_cross_site_origin`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn rpc_rejects_cross_site_origin() {
    let _ctx = RpcTestContext::new("rpc-cross-site-origin");
    let server = codexmanager_service::start_one_shot_server().expect("start server");

    let req = JsonRpcRequest {
        id: 9.into(),
        method: "initialize".to_string(),
        params: None,
        trace: None,
    };
    let json = serde_json::to_string(&req).expect("serialize");
    let token = codexmanager_service::rpc_auth_token().to_string();
    let (status, _) = post_rpc_raw(
        &server.addr,
        &json,
        &[
            ("Content-Type", "application/json"),
            ("X-CodexManager-Rpc-Token", token.as_str()),
            ("Origin", "https://evil.example"),
            ("Sec-Fetch-Site", "cross-site"),
        ],
    );
    assert_eq!(status, 403);
}

/// 函数 `rpc_accepts_loopback_origin`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn rpc_accepts_loopback_origin() {
    let _ctx = RpcTestContext::new("rpc-loopback-origin");
    let server = codexmanager_service::start_one_shot_server().expect("start server");

    let req = JsonRpcRequest {
        id: 10.into(),
        method: "initialize".to_string(),
        params: None,
        trace: None,
    };
    let json = serde_json::to_string(&req).expect("serialize");
    let token = codexmanager_service::rpc_auth_token().to_string();
    let (status, body) = post_rpc_raw(
        &server.addr,
        &json,
        &[
            ("Content-Type", "application/json"),
            ("X-CodexManager-Rpc-Token", token.as_str()),
            ("Origin", "http://localhost:5173"),
            ("Sec-Fetch-Site", "same-site"),
        ],
    );
    assert_eq!(status, 200, "unexpected status {status}: {body}");
}

#[test]
fn rpc_account_manager_assigns_key_and_bills_wallet() {
    let ctx = RpcTestContext::new("rpc-account-manager-billing");

    let call_rpc_response =
        |id: i64, method: &str, params: Option<serde_json::Value>| -> serde_json::Value {
            let server = codexmanager_service::start_one_shot_server().expect("start server");
            let req = JsonRpcRequest {
                id: id.into(),
                method: method.to_string(),
                params,
                trace: None,
            };
            let json = serde_json::to_string(&req).expect("serialize");
            post_rpc(&server.addr, &json)
        };
    let call_rpc =
        |id: i64, method: &str, params: Option<serde_json::Value>| -> serde_json::Value {
            call_rpc_response(id, method, params)
                .get("result")
                .cloned()
                .expect("result")
        };

    let settings = call_rpc(
        200,
        "appSettings/set",
        Some(serde_json::json!({
            "webAuthMode": "accounts",
            "distributionEnabled": true
        })),
    );
    assert_eq!(settings["webAuthMode"], "accounts");
    assert_eq!(settings["distributionEnabled"], true);

    let user = call_rpc(
        201,
        "accountManager/users/create",
        Some(serde_json::json!({
            "username": "member-one",
            "password": "password123",
            "displayName": "Member One",
            "role": "member"
        })),
    );
    let user_id = user["id"].as_str().expect("user id").to_string();
    assert_eq!(user["wallet"]["availableCreditMicros"], 0);

    let admin = call_rpc(
        206,
        "accountManager/users/create",
        Some(serde_json::json!({
            "username": "admin-one",
            "password": "password123",
            "displayName": "Admin One",
            "role": "admin"
        })),
    );
    let admin_id = admin["id"].as_str().expect("admin id").to_string();
    assert!(admin["wallet"].is_null());

    let api_key = call_rpc(
        202,
        "apikey/create",
        Some(serde_json::json!({
            "name": "Member key",
            "modelSlug": "gpt-5",
            "rotationStrategy": "account_rotation"
        })),
    );
    let key_id = api_key["id"].as_str().expect("key id").to_string();

    let storage = Storage::open(ctx.db_path()).expect("open storage");
    storage.init().expect("init storage");
    codexmanager_service::wallet_precheck_for_api_key(&storage, &key_id)
        .expect("unassigned api key should bypass wallet precheck");
    let missing_owner_charge = codexmanager_service::wallet_charge_for_request(
        &storage,
        Some(&key_id),
        41,
        0.25,
        None,
        Some("default"),
        None,
    )
    .expect("unassigned api key should bypass wallet charge");
    assert!(missing_owner_charge.is_none());

    let admin_owner_error = call_rpc(
        207,
        "accountManager/apiKeyOwners/set",
        Some(serde_json::json!({
            "keyId": key_id,
            "ownerKind": "user",
            "ownerUserId": admin_id.as_str()
        })),
    );
    assert!(admin_owner_error["error"]
        .as_str()
        .expect("admin owner error")
        .contains("管理员账号不参与额度分发"));

    let admin_wallet_error = call_rpc(
        208,
        "accountManager/wallet/topUp",
        Some(serde_json::json!({
            "ownerKind": "user",
            "ownerId": admin_id.as_str(),
            "amountCreditMicros": 1_000_000
        })),
    );
    assert!(admin_wallet_error["error"]
        .as_str()
        .expect("admin wallet error")
        .contains("管理员账号不参与额度分发"));

    let owner = call_rpc(
        203,
        "accountManager/apiKeyOwners/set",
        Some(serde_json::json!({
            "keyId": key_id,
            "ownerKind": "user",
            "ownerUserId": user_id
        })),
    );
    assert_eq!(owner["ownerKind"], "user");
    assert_eq!(owner["ownerUserId"], user_id);

    let zero_balance_error = codexmanager_service::wallet_precheck_for_api_key(&storage, &key_id)
        .expect_err("zero wallet should fail");
    assert!(zero_balance_error.contains("余额不足"));

    let wallet = call_rpc(
        204,
        "accountManager/wallet/topUp",
        Some(serde_json::json!({
            "ownerKind": "User",
            "ownerId": user_id,
            "amountCreditMicros": 1_000_000,
            "note": "test top up"
        })),
    );
    assert_eq!(wallet["ownerKind"], "user");
    assert_eq!(wallet["availableCreditMicros"], 1_000_000);
    codexmanager_service::wallet_precheck_for_api_key(&storage, &key_id)
        .expect("funded wallet should pass");

    let rules = call_rpc(
        209,
        "quota/billingRule/upsert",
        Some(serde_json::json!({
            "name": "Member markup",
            "multiplierMillis": 1500,
            "apiKeyId": key_id.as_str(),
            "priority": 10
        })),
    );
    let rule_id = rules["items"]
        .as_array()
        .expect("billing rules")
        .iter()
        .find(|item| item["name"].as_str() == Some("Member markup"))
        .and_then(|item| item["id"].as_str())
        .expect("rule id")
        .to_string();

    let charge_entry = codexmanager_service::wallet_charge_for_request(
        &storage,
        Some(&key_id),
        42,
        0.25,
        None,
        Some("default"),
        Some(r#"{"test":true}"#.to_string()),
    )
    .expect("charge wallet")
    .expect("charge entry");
    assert_eq!(
        charge_entry.pricing_rule_id.as_deref(),
        Some(rule_id.as_str())
    );
    let charged_wallet = storage
        .find_wallet_by_owner("user", &user_id)
        .expect("read wallet")
        .expect("wallet");
    assert_eq!(charged_wallet.balance_credit_micros, 625_000);

    let owners = call_rpc(205, "accountManager/apiKeyOwners/list", None);
    let owners = owners.as_array().expect("owners array");
    assert!(owners
        .iter()
        .any(|item| item["keyId"].as_str() == Some(key_id.as_str())));

    let distribution_error = call_rpc(
        210,
        "accountManager/distribution/set",
        Some(serde_json::json!({
            "enabled": false
        })),
    );
    assert!(distribution_error["error"]
        .as_str()
        .expect("distribution error")
        .contains("distribution_mode_locked"));

    let auth_mode_error = call_rpc(
        211,
        "accountManager/webAuthMode/set",
        Some(serde_json::json!({
            "mode": "none"
        })),
    );
    assert!(auth_mode_error["error"]
        .as_str()
        .expect("auth mode error")
        .contains("account_billing_mode_locked"));
}

fn start_mock_proxy_slow_server(
    response: &'static str,
    delay: Duration,
) -> (
    String,
    std::sync::mpsc::Receiver<String>,
    thread::JoinHandle<()>,
) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind fake proxy");
    let addr = format!("http://{}", listener.local_addr().expect("fake proxy addr"));
    let (tx, rx) = std::sync::mpsc::channel();
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept proxy connection");
        let mut buffer = vec![0_u8; 8192];
        let size = stream.read(&mut buffer).expect("read proxy request");
        tx.send(String::from_utf8_lossy(&buffer[..size]).to_string())
            .expect("send proxy request");
        thread::sleep(delay);
        let _ = stream.write_all(response.as_bytes());
    });
    (addr, rx, handle)
}

#[test]
fn rpc_system_proxy_jobs_flow() {
    let ctx = RpcTestContext::new("rpc-system-proxy-jobs");
    let storage = Storage::open(ctx.db_path()).expect("open db");
    storage.init().expect("init schema");

    let call_rpc = |id: i64, method: &str, params: Option<serde_json::Value>| {
        let server = codexmanager_service::start_one_shot_server().expect("start server");
        let req = JsonRpcRequest {
            id: id.into(),
            method: method.to_string(),
            params,
            trace: None,
        };
        let json = serde_json::to_string(&req).expect("serialize");
        post_rpc(&server.addr, &json)
    };

    // 1. Создаем прокси профиль
    let create_resp = call_rpc(
        1,
        "system/proxy/create",
        Some(serde_json::json!({
            "name": "Test Proxy",
            "proxyUrl": "http://127.0.0.1:12345",
            "enabled": true
        })),
    );
    let result = create_resp.get("result").expect("create result");
    let profile_id = result.get("id").unwrap().as_str().unwrap().to_string();

    // Запускаем медленный фейковый прокси
    let (proxy_addr, _rx, proxy_handle) = start_mock_proxy_slow_server(
        "HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n",
        Duration::from_millis(300),
    );

    // Обновляем прокси URL на правильный адрес фейкового прокси
    let update_resp = call_rpc(
        2,
        "system/proxy/update",
        Some(serde_json::json!({
            "id": &profile_id,
            "proxyUrl": &proxy_addr
        })),
    );
    assert!(update_resp.get("result").is_some());

    // 2. Запускаем latency test
    let test_resp = call_rpc(
        3,
        "system/proxy/test-latency",
        Some(serde_json::json!({
            "id": &profile_id,
        })),
    );
    let test_result = test_resp.get("result").expect("test result");
    let job_id = test_result.get("jobId").unwrap().as_str().unwrap().to_string();
    assert!(job_id.starts_with("job_lat_"));

    // 3. Сразу опрашиваем состояние - джоба должна быть либо queued, либо running
    let job_resp = call_rpc(
        4,
        "system/proxy/test-job",
        Some(serde_json::json!({
            "jobId": &job_id
        })),
    );
    let job_state = job_resp.get("result").expect("job state");
    let status = job_state.get("status").unwrap().as_str().unwrap();
    assert!(status == "queued" || status == "running");

    // 4. Отменяем джобу
    let cancel_resp = call_rpc(
        5,
        "system/proxy/cancel-test",
        Some(serde_json::json!({
            "jobId": &job_id
        })),
    );
    assert!(cancel_resp.get("result").is_some());

    // 5. Проверяем, что статус стал cancelled
    let mut cancelled = false;
    for idx in 0..30 {
        thread::sleep(Duration::from_millis(100));
        let job_resp2 = call_rpc(
            6 + idx,
            "system/proxy/test-job",
            Some(serde_json::json!({
                "jobId": &job_id
            })),
        );
        let job_state2 = job_resp2.get("result").expect("job state");
        let status2 = job_state2.get("status").unwrap().as_str().unwrap();
        if status2 == "cancelled" {
            cancelled = true;
            break;
        }
    }
    assert!(cancelled, "Job was not cancelled successfully");

    let _ = proxy_handle.join();
}
