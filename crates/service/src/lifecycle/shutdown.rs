use std::io::Write;
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

static SHUTDOWN_REQUESTED: AtomicBool = AtomicBool::new(false);

/// 函数 `shutdown_requested`
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
pub fn shutdown_requested() -> bool {
    SHUTDOWN_REQUESTED.load(Ordering::SeqCst)
}

/// 函数 `clear_shutdown_flag`
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
pub fn clear_shutdown_flag() {
    SHUTDOWN_REQUESTED.store(false, Ordering::SeqCst);
}

/// 函数 `request_shutdown`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - addr: 参数 addr
///
/// # 返回
/// 无
pub fn request_shutdown(addr: &str) {
    SHUTDOWN_REQUESTED.store(true, Ordering::SeqCst);
    let _ = send_shutdown_request(addr);
    let addr_trimmed = addr.trim();
    if addr_trimmed.len() > "localhost:".len()
        && addr_trimmed[..("localhost:".len())].eq_ignore_ascii_case("localhost:")
    {
        let port = &addr_trimmed["localhost:".len()..];
        let _ = send_shutdown_request(&format!("127.0.0.1:{port}"));
        let _ = send_shutdown_request(&format!("[::1]:{port}"));
    }
}

/// 函数 `send_shutdown_request`
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
fn send_shutdown_request(addr: &str) -> std::io::Result<()> {
    let addr = addr.trim();
    if addr.is_empty() {
        return Ok(());
    }
    let addr = addr.strip_prefix("http://").unwrap_or(addr);
    let addr = addr.strip_prefix("https://").unwrap_or(addr);
    let addr = addr.split('/').next().unwrap_or(addr);
    let mut stream = TcpStream::connect(addr)?;
    let _ = stream.set_write_timeout(Some(Duration::from_millis(200)));
    let _ = stream.set_read_timeout(Some(Duration::from_millis(200)));
    let token = axum::http::HeaderValue::from_str(crate::rpc_auth_token()).map_err(|_| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid RPC token header")
    })?;
    let token = token.to_str().map_err(|_| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid RPC token header")
    })?;
    let request = format!("GET /__shutdown HTTP/1.1\r\nHost: {addr}\r\nX-CodexManager-Rpc-Token: {token}\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes())?;
    Ok(())
}
