use codexmanager_core::rpc::types::JsonRpcRequest;
use codexmanager_core::storage::Storage;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;

mod support;
use support::{test_env_guard, EnvGuard};

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
fn post_rpc(addr: &str, body: &str) -> String {
    let mut stream = TcpStream::connect(addr).expect("connect server");
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(30)))
        .expect("read timeout");
    let token = codexmanager_service::rpc_auth_token().to_string();
    let request = format!(
        "POST /rpc HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\nContent-Type: application/json\r\nX-CodexManager-Rpc-Token: {token}\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    );
    stream.write_all(request.as_bytes()).expect("write");
    let mut buf = String::new();
    stream.read_to_string(&mut buf).expect("read");
    buf
}

/// 函数 `e2e_initialize_writes_event`
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
fn e2e_initialize_writes_event() {
    let _guard = test_env_guard();
    let mut dir = std::env::temp_dir();
    dir.push(format!("codexmanager-e2e-{}", std::process::id()));
    let _ = fs::create_dir_all(&dir);
    let db_path: PathBuf = dir.join("codexmanager.db");

    let _guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());

    let server = codexmanager_service::start_one_shot_server().expect("start server");
    let req = JsonRpcRequest {
        id: 1.into(),
        method: "initialize".to_string(),
        params: None,
        trace: None,
    };
    let json = serde_json::to_string(&req).expect("serialize");
    let buf = post_rpc(&server.addr, &json);
    assert!(!buf.trim().is_empty());

    let storage = Storage::open(&db_path).expect("open db");
    storage.init().expect("init schema");
    let count = storage.event_count().expect("count events");
    assert!(count >= 1);
}
