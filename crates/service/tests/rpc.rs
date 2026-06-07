use codexmanager_core::rpc::types::JsonRpcRequest;
use codexmanager_core::storage::{
    now_ts, Account, Event, RequestLog, RequestTokenStat, Storage, Token, UsageSnapshotRecord,
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

/// 函数 `rpc_account_delete_by_statuses_rejects_unknown_status`
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
fn rpc_account_delete_by_statuses_rejects_unknown_status() {
    let _ctx = RpcTestContext::new("rpc-account-delete-by-statuses-unknown");
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
    let message = result
        .get("error")
        .and_then(|value| value.as_str())
        .expect("error message");

    assert!(message.contains("unsupported cleanup statuses"));
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
