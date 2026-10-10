#[path = "../support.rs"]
mod shared;

pub(super) use shared::{test_env_guard, EnvGuard};

pub(super) use codexmanager_core::storage::{
    now_ts, Account, ApiKey, ApiKeyOwner, AppUser, ManagedModelV2Upsert, ModelRouteV2,
    ModelSourceMapping, ModelSourceModel, Storage, Token, UserModelGroup,
};
pub(super) use sha2::{Digest, Sha256};
pub(super) use std::collections::HashMap;
pub(super) use std::fs;
pub(super) use std::io::{Read, Write};
pub(super) use std::net::TcpListener;
pub(super) use std::net::TcpStream;
pub(super) use std::path::PathBuf;
pub(super) use std::sync::atomic::{AtomicUsize, Ordering};
pub(super) use std::sync::mpsc::{self, Receiver};
pub(super) use std::thread;
pub(super) use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub(super) static TEST_DIR_SEQ: AtomicUsize = AtomicUsize::new(0);
pub(super) static TEST_PORT_SEQ: AtomicUsize = AtomicUsize::new(41000);

pub(super) fn wait_for_upstream_attempts(
    storage: &Storage,
    trace_id: &str,
    count: usize,
) -> Vec<codexmanager_core::storage::RequestLogUpstreamAttempt> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let stages = storage
            .list_request_log_upstream_attempt_stages(trace_id)
            .expect("list request attempts");
        if stages.len() >= count {
            return stages
                .iter()
                .map(|stage| {
                    storage
                        .find_request_log_upstream_attempt(trace_id, stage)
                        .expect("read attempt")
                        .expect("attempt exists")
                })
                .collect();
        }
        assert!(
            Instant::now() < deadline,
            "request capture writer did not persist {count} attempt(s)"
        );
        thread::sleep(Duration::from_millis(20));
    }
}

/// 函数 `new_test_dir`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - super: 参数 super
///
/// # 返回
/// 返回函数执行结果
pub(super) fn new_test_dir(prefix: &str) -> PathBuf {
    // 中文注释：进程 ID 可能被复用；时间戳与递增序号共同避免复用旧测试数据库。
    let seq = TEST_DIR_SEQ.fetch_add(1, Ordering::Relaxed);
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let mut dir = std::env::temp_dir();
    dir.push(format!("{prefix}-{}-{timestamp}-{seq}", std::process::id()));
    let _ = fs::create_dir_all(&dir);
    // 在公开进程级 DB 环境变量前完成迁移，避免残留后台线程同时初始化同一临时库。
    let db_path = dir.join("codexmanager.db");
    let storage = Storage::open(&db_path).expect("open gateway test db");
    storage.init().expect("init gateway test db");
    drop(storage);
    dir
}

/// 函数 `bind_test_listener`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - super: 参数 super
///
/// # 返回
/// 返回函数执行结果
pub(super) fn bind_test_listener(label: &str) -> TcpListener {
    // Базовый порт смещается в зависимости от PID процесса для предотвращения пересечения портов при параллельном запуске в nextest.
    // Используем диапазон 15000-30000, чтобы не наткнуться на зарезервированные системой Windows порты (>49152).
    let base_port = 15000 + (std::process::id() % 15000) as usize;
    for _ in 0..1024 {
        let port = (base_port + TEST_PORT_SEQ.fetch_add(1, Ordering::Relaxed) % 1000) as u16;
        match TcpListener::bind(("127.0.0.1", port)) {
            Ok(listener) => return listener,
            Err(err) if err.kind() == std::io::ErrorKind::AddrInUse => continue,
            Err(err) => panic!("bind {label} port {port} failed: {err}"),
        }
    }
    panic!("exhausted test ports for {label}");
}

/// 函数 `decode_chunked_body_if_needed`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - body: 参数 body
///
/// # 返回
/// 返回函数执行结果
fn decode_chunked_body_if_needed(body: &str) -> String {
    let normalized = body.replace("\r\n", "\n");
    let bytes = normalized.as_bytes();
    let mut idx = 0usize;
    let mut out = Vec::new();
    let mut saw_chunk = false;

    while idx < bytes.len() {
        let size_end = match bytes[idx..].iter().position(|b| *b == b'\n') {
            Some(rel) => idx + rel,
            None => bytes.len(),
        };
        let size_text = std::str::from_utf8(&bytes[idx..size_end])
            .ok()
            .map(str::trim);
        let Some(size_text) = size_text else {
            return normalized;
        };
        let Ok(size) = usize::from_str_radix(size_text, 16) else {
            return normalized;
        };
        saw_chunk = true;
        idx = if size_end < bytes.len() {
            size_end + 1
        } else {
            size_end
        };
        if size == 0 {
            break;
        }
        if idx + size > bytes.len() {
            return normalized;
        }
        out.extend_from_slice(&bytes[idx..idx + size]);
        idx += size;
        if idx >= bytes.len() || bytes[idx] != b'\n' {
            return normalized;
        }
        idx += 1;
    }

    if !saw_chunk {
        return normalized;
    }
    String::from_utf8(out).unwrap_or(normalized)
}

/// 函数 `post_http_raw`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - super: 参数 super
///
/// # 返回
/// 返回函数执行结果
pub(super) fn post_http_raw(
    addr: &str,
    path: &str,
    body: &str,
    headers: &[(&str, &str)],
) -> (u16, String) {
    post_http_raw_with_read_timeout(addr, path, body, headers, Duration::from_secs(2))
}

pub(super) fn post_http_raw_with_read_timeout(
    addr: &str,
    path: &str,
    body: &str,
    headers: &[(&str, &str)],
    read_timeout: Duration,
) -> (u16, String) {
    let mut last_raw = String::new();
    for _ in 0..20 {
        let mut stream = TcpStream::connect(addr).expect("connect server");
        let _ = stream.set_read_timeout(Some(read_timeout));
        let mut request = format!("POST {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n");
        for (name, value) in headers {
            request.push_str(name);
            request.push_str(": ");
            request.push_str(value);
            request.push_str("\r\n");
        }
        request.push_str(&format!("Content-Length: {}\r\n\r\n{}", body.len(), body));
        stream.write_all(request.as_bytes()).expect("write");

        let mut buf = String::new();
        stream.read_to_string(&mut buf).expect("read");
        if let Some(status) = buf
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .and_then(|value| value.parse::<u16>().ok())
        {
            let body_raw = buf.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
            let body = decode_chunked_body_if_needed(&body_raw);
            return (status, body);
        }
        last_raw = buf;
        thread::sleep(Duration::from_millis(50));
    }
    panic!("status parse failed, raw response: {last_raw:?}");
}

/// 函数 `hash_platform_key_for_test`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - super: 参数 super
///
/// # 返回
/// 返回函数执行结果
pub(super) fn hash_platform_key_for_test(key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    let digest = hasher.finalize();
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

pub(super) struct GatewayModelForwardRulesResetGuard;

impl GatewayModelForwardRulesResetGuard {
    pub(super) fn reset() -> Self {
        reset_gateway_model_forward_rules_for_test();
        Self
    }
}

impl Drop for GatewayModelForwardRulesResetGuard {
    fn drop(&mut self) {
        reset_gateway_model_forward_rules_for_test();
    }
}

fn reset_gateway_model_forward_rules_for_test() {
    codexmanager_service::set_gateway_model_forward_rules("")
        .expect("reset gateway model forward rules");
}

/// 函数 `seed_model_catalog_models`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - super: 参数 super
///
/// # 返回
/// 无
pub(super) fn seed_model_catalog_models(storage: &Storage, models: &[&str]) {
    for (index, slug) in models.iter().enumerate() {
        let mut model = test_managed_model(storage, slug);
        model.enabled = true;
        model.supported_in_api = true;
        model.visibility = "list".to_string();
        model.sort_order = index as i64;
        model.routes = vec![ModelRouteV2 {
            id: String::new(),
            source_kind: "account_pool".to_string(),
            source_id: "default".to_string(),
            upstream_model: (*slug).to_string(),
            enabled: true,
            priority: 0,
            weight: 1,
        }];
        storage
            .upsert_managed_model_v2(&ManagedModelV2Upsert {
                previous_slug: None,
                model,
            })
            .expect("upsert V2 test model");
    }
}

pub(super) fn seed_model_catalog_route(
    storage: &Storage,
    model_slug: &str,
    source_kind: &str,
    source_id: &str,
    upstream_model: &str,
    priority: i64,
) {
    if storage
        .get_managed_model_v2(model_slug)
        .expect("get V2 test model")
        .is_none()
    {
        seed_model_catalog_models(storage, &[model_slug]);
    }
    let mut model = storage
        .get_managed_model_v2(model_slug)
        .expect("get V2 test model")
        .expect("V2 test model exists");
    model
        .routes
        .retain(|route| route.source_kind != source_kind || route.source_id != source_id);
    model.routes.push(ModelRouteV2 {
        id: String::new(),
        source_kind: source_kind.to_string(),
        source_id: source_id.to_string(),
        upstream_model: upstream_model.to_string(),
        enabled: true,
        priority,
        weight: 1,
    });
    storage
        .upsert_managed_model_v2(&ManagedModelV2Upsert {
            previous_slug: None,
            model,
        })
        .expect("upsert V2 test model route");
}

fn test_managed_model(storage: &Storage, slug: &str) -> codexmanager_core::storage::ManagedModelV2 {
    if let Some(model) = storage
        .get_managed_model_v2(slug)
        .expect("get existing V2 test model")
    {
        return model;
    }
    let mut model = storage
        .get_managed_model_v2("gpt-6-luna")
        .expect("get V2 test model template")
        .expect("seeded V2 test model template");
    model.id.clear();
    model.slug = slug.to_string();
    model.display_name = slug.to_string();
    model.origin = "custom".to_string();
    model.builtin_revision = None;
    model.user_edited = false;
    model.price.price_status = "custom".to_string();
    model.price.price_source = Some("gateway integration test".to_string());
    model.permission_group_ids.clear();
    model.routes.clear();
    model.created_at = 0;
    model.updated_at = 0;
    model
}

/// 函数 `decode_upstream_request_body`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - super: 参数 super
///
/// # 返回
/// 返回函数执行结果
pub(super) fn decode_upstream_request_body(captured: &CapturedUpstreamRequest) -> Vec<u8> {
    if captured
        .headers
        .get("content-encoding")
        .is_some_and(|value| value.eq_ignore_ascii_case("zstd"))
    {
        zstd::stream::decode_all(std::io::Cursor::new(captured.body.as_slice()))
            .expect("decode zstd upstream payload")
    } else {
        captured.body.clone()
    }
}

#[derive(Debug)]
pub(super) struct CapturedUpstreamRequest {
    pub(super) path: String,
    pub(super) headers: HashMap<String, String>,
    pub(super) body: Vec<u8>,
}

/// 函数 `try_read_http_request_once`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - stream: 参数 stream
///
/// # 返回
/// 返回函数执行结果
fn try_read_http_request_once(stream: &mut TcpStream) -> Option<CapturedUpstreamRequest> {
    // 中文注释：部分测试会命中 reqwest keep-alive 复用，下一轮 mock listener 可能先收到
    // 一个“已建立但没有发任何 HTTP 头”的残留连接；这里把它视作噪声并忽略。
    let _ = stream.set_read_timeout(Some(Duration::from_millis(300)));

    let mut raw = Vec::new();
    let mut buf = [0u8; 4096];
    let mut header_end = None;
    while header_end.is_none() {
        let read = match stream.read(&mut buf) {
            Ok(read) => read,
            Err(err)
                if matches!(
                    err.kind(),
                    std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                ) =>
            {
                return None;
            }
            Err(_) => return None,
        };
        if read == 0 {
            return None;
        }
        raw.extend_from_slice(&buf[..read]);
        header_end = raw
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .map(|idx| idx + 4);
    }
    let header_end = header_end?;
    let header_text = String::from_utf8_lossy(&raw[..header_end]).to_string();
    let mut lines = header_text.split("\r\n").filter(|line| !line.is_empty());
    let request_line = lines.next()?;
    let path = request_line
        .split_whitespace()
        .nth(1)
        .unwrap_or("/")
        .to_string();

    let mut headers = HashMap::new();
    let mut content_length = 0usize;
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            let name = name.trim().to_ascii_lowercase();
            let value = value.trim().to_string();
            if name == "content-length" {
                content_length = value.parse::<usize>().unwrap_or(0);
            }
            headers.insert(name, value);
        }
    }

    while raw.len() < header_end + content_length {
        let read = match stream.read(&mut buf) {
            Ok(read) => read,
            Err(err)
                if matches!(
                    err.kind(),
                    std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                ) =>
            {
                return None;
            }
            Err(_) => return None,
        };
        if read == 0 {
            return None;
        }
        raw.extend_from_slice(&buf[..read]);
    }
    let body_end = (header_end + content_length).min(raw.len());
    let body = raw[header_end..body_end].to_vec();

    Some(CapturedUpstreamRequest {
        path,
        headers,
        body,
    })
}

/// 函数 `accept_http_request`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - listener: 参数 listener
/// - idle_timeout: 参数 idle_timeout
///
/// # 返回
/// 返回函数执行结果
fn accept_http_request(
    listener: &TcpListener,
    idle_timeout: Duration,
) -> Option<(TcpStream, CapturedUpstreamRequest)> {
    listener
        .set_nonblocking(true)
        .expect("set nonblocking listener");
    let deadline = Instant::now() + idle_timeout;
    loop {
        match listener.accept() {
            Ok((mut stream, _)) => {
                let _ = stream.set_nonblocking(false);
                if let Some(captured) = try_read_http_request_once(&mut stream) {
                    return Some((stream, captured));
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return None;
                }
                thread::sleep(Duration::from_millis(10));
            }
            Err(_) => return None,
        }
    }
}

/// 函数 `start_mock_upstream_once`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - super: 参数 super
///
/// # 返回
/// 返回函数执行结果
pub(super) fn start_mock_upstream_once(
    response_json: &str,
) -> (
    String,
    Receiver<CapturedUpstreamRequest>,
    thread::JoinHandle<()>,
) {
    start_mock_upstream_once_with_content_type(response_json, "application/json")
}

/// 函数 `start_mock_upstream_once_with_content_type`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - super: 参数 super
///
/// # 返回
/// 返回函数执行结果
pub(super) fn start_mock_upstream_once_with_content_type(
    response_body: &str,
    content_type: &str,
) -> (
    String,
    Receiver<CapturedUpstreamRequest>,
    thread::JoinHandle<()>,
) {
    let listener = bind_test_listener("mock upstream");
    let addr = listener.local_addr().expect("mock upstream addr");
    let response = response_body.as_bytes().to_vec();
    let content_type = content_type.to_string();
    let (tx, rx) = mpsc::channel();

    let join = thread::spawn(move || {
        let (mut stream, captured) = accept_http_request(&listener, Duration::from_secs(3))
            .expect("accept upstream http request");
        let _ = tx.send(captured);

        let header = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            response.len()
        );
        stream
            .write_all(header.as_bytes())
            .expect("write upstream status");
        stream.write_all(&response).expect("write upstream body");
        let _ = stream.flush();
    });

    (addr.to_string(), rx, join)
}

/// 函数 `start_mock_upstream_sequence`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - super: 参数 super
///
/// # 返回
/// 返回函数执行结果
pub(super) fn start_mock_upstream_sequence(
    responses: Vec<(u16, String)>,
) -> (
    String,
    Receiver<CapturedUpstreamRequest>,
    thread::JoinHandle<()>,
) {
    start_mock_upstream_sequence_lenient(responses, Duration::from_secs(3))
}

/// 函数 `start_mock_upstream_sequence_lenient`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - super: 参数 super
///
/// # 返回
/// 返回函数执行结果
pub(super) fn start_mock_upstream_sequence_lenient(
    responses: Vec<(u16, String)>,
    idle_timeout: Duration,
) -> (
    String,
    Receiver<CapturedUpstreamRequest>,
    thread::JoinHandle<()>,
) {
    let typed = responses
        .into_iter()
        .map(|(status, body)| (status, body, "application/json".to_string()))
        .collect();
    start_mock_upstream_sequence_lenient_with_content_types(typed, idle_timeout)
}

pub(super) fn start_mock_upstream_sequence_lenient_with_content_types(
    responses: Vec<(u16, String, String)>,
    idle_timeout: Duration,
) -> (
    String,
    Receiver<CapturedUpstreamRequest>,
    thread::JoinHandle<()>,
) {
    let listener = bind_test_listener("mock upstream");
    let addr = listener.local_addr().expect("mock upstream addr");
    let (tx, rx) = mpsc::channel();

    let join = thread::spawn(move || {
        let mut idx = 0usize;
        let fallback_body =
            "{\"error\":{\"message\":\"unexpected extra upstream request\",\"type\":\"server_error\"}}"
                .to_string();
        let fallback_ct = "application/json".to_string();
        loop {
            // The short idle window detects unexpected failover after the first
            // response. Initial database migration and HTTP-client construction
            // happen after this fixture starts and need a separate startup bound.
            let accept_timeout = if idx == 0 {
                idle_timeout.max(Duration::from_secs(3))
            } else {
                idle_timeout
            };
            let Some((mut stream, captured)) = accept_http_request(&listener, accept_timeout)
            else {
                break;
            };
            let _ = tx.send(captured);

            let (status, body, content_type) = responses
                .get(idx)
                .map(|(status, body, ct)| (*status, body.as_str(), ct.as_str()))
                .unwrap_or((500, fallback_body.as_str(), fallback_ct.as_str()));
            let body_bytes = body.as_bytes().to_vec();
            let header = format!(
                "HTTP/1.1 {} OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                status,
                content_type,
                body_bytes.len()
            );
            stream
                .write_all(header.as_bytes())
                .expect("write upstream status");
            stream
                .write_all(&body_bytes)
                .expect("write upstream response body");
            let _ = stream.flush();
            idx = idx.saturating_add(1);
        }
    });

    (addr.to_string(), rx, join)
}

pub(super) struct TestServer {
    pub(super) addr: String,
    join: Option<thread::JoinHandle<()>>,
}

/// 函数 `check_health`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - addr: 参数 addr
///
/// # 返回
/// 返回函数执行结果
fn check_health(addr: &str) -> bool {
    let Ok(mut stream) = TcpStream::connect(addr) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
    let request = format!("GET /health HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n");
    if stream.write_all(request.as_bytes()).is_err() {
        return false;
    }
    let mut buf = String::new();
    if stream.read_to_string(&mut buf).is_err() {
        return false;
    }
    buf.starts_with("HTTP/1.1 200") || buf.starts_with("HTTP/1.0 200")
}

impl TestServer {
    /// 函数 `start`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - super: 参数 super
    ///
    /// # 返回
    /// 返回函数执行结果
    pub(super) fn start() -> Self {
        codexmanager_service::clear_shutdown_flag();
        for _ in 0..10 {
            let probe = bind_test_listener("probe");
            let port = probe.local_addr().expect("probe addr").port();
            drop(probe);

            let addr = format!("localhost:{port}");
            let addr_for_thread = addr.clone();
            let join = thread::spawn(move || {
                let _ = codexmanager_service::start_server(&addr_for_thread);
            });

            // 中文注释：前置代理与后端会串行启动；必须等 /health 成功，才能保证连到的是本测试服务而不是端口竞争者。
            for _ in 0..120 {
                if check_health(&addr) {
                    return Self {
                        addr,
                        join: Some(join),
                    };
                }
                if join.is_finished() {
                    break;
                }
                thread::sleep(Duration::from_millis(50));
            }
            let _ = join.join();
        }
        panic!("server start timeout");
    }
}

impl Drop for TestServer {
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
        codexmanager_service::request_shutdown(&self.addr);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
        codexmanager_service::clear_shutdown_flag();
    }
}
