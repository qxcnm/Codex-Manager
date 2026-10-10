use codexmanager_core::rpc::types::JsonRpcRequest;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use super::address::{resolve_service_addr, resolve_socket_addrs};
use super::http::parse_http_body;

const RPC_CONNECT_TIMEOUT: Duration = Duration::from_millis(400);
const RPC_DEFAULT_IO_TIMEOUT: Duration = Duration::from_secs(10);
const RPC_BULK_USAGE_REFRESH_IO_TIMEOUT: Duration = Duration::from_secs(600);
const RPC_ACCOUNT_IMPORT_IO_TIMEOUT: Duration = Duration::from_secs(600);
const RPC_CODEX_SKILLS_MUTATION_IO_TIMEOUT: Duration = Duration::from_secs(600);
const RPC_CODEX_SKILLS_SEARCH_IO_TIMEOUT: Duration = Duration::from_secs(60);
const RPC_RESET_CREDIT_CONSUME_IO_TIMEOUT: Duration = Duration::from_secs(600);
const RPC_RESET_CREDIT_CONSUME_METHOD: &str = "account/usage/resetCredit/consume";

#[derive(Debug)]
enum RpcAttemptError {
    BeforeSend(String),
    MayHaveBeenSent(String),
}

impl RpcAttemptError {
    fn may_have_been_sent(&self) -> bool {
        matches!(self, Self::MayHaveBeenSent(_))
    }

    fn into_message(self) -> String {
        match self {
            Self::BeforeSend(message) | Self::MayHaveBeenSent(message) => message,
        }
    }
}

fn rpc_must_not_replay_after_send(method: &str) -> bool {
    method == RPC_RESET_CREDIT_CONSUME_METHOD
}

/// 函数 `rpc_io_timeout`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - method: 参数 method
/// - params: 参数 params
///
/// # 返回
/// 返回函数执行结果
fn rpc_io_timeout(method: &str, params: Option<&serde_json::Value>) -> Duration {
    if method == "account/import" {
        return RPC_ACCOUNT_IMPORT_IO_TIMEOUT;
    }

    // A redemption can include the provider request followed by usage and credit refreshes.
    // Keep the socket open for the whole transaction instead of treating normal provider
    // latency as a transport failure with an unknown redemption outcome.
    if method == RPC_RESET_CREDIT_CONSUME_METHOD {
        return RPC_RESET_CREDIT_CONSUME_IO_TIMEOUT;
    }

    // Skills repository, registry, and Marketplace mutations can include bounded network and
    // filesystem work. Keep the desktop RPC socket open for the complete transaction.
    if method.starts_with("codexSkills/marketplace")
        || matches!(
            method,
            "codexSkills/installZip"
                | "codexSkills/importDirectory"
                | "codexSkills/delete"
                | "codexSkills/repositoryAdd"
                | "codexSkills/repositoryRefresh"
                | "codexSkills/repositoryInstall"
                | "codexSkills/registryInstall"
        )
    {
        return RPC_CODEX_SKILLS_MUTATION_IO_TIMEOUT;
    }

    if method == "codexSkills/registrySearch" {
        return RPC_CODEX_SKILLS_SEARCH_IO_TIMEOUT;
    }

    if method == "account/usage/refresh"
        && params
            .and_then(|value| value.get("accountId"))
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .is_none()
    {
        return RPC_BULK_USAGE_REFRESH_IO_TIMEOUT;
    }

    RPC_DEFAULT_IO_TIMEOUT
}

/// 函数 `rpc_call_on_socket`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - method: 参数 method
/// - addr: 参数 addr
/// - sock: 参数 sock
/// - params: 参数 params
///
/// # 返回
/// 返回函数执行结果
fn rpc_call_on_socket(
    method: &str,
    addr: &str,
    sock: SocketAddr,
    params: Option<serde_json::Value>,
    io_timeout: Duration,
) -> Result<serde_json::Value, RpcAttemptError> {
    let req = JsonRpcRequest {
        id: 1.into(),
        method: method.to_string(),
        params,
        trace: None,
    };
    let json = serde_json::to_string(&req)
        .map_err(|error| RpcAttemptError::BeforeSend(error.to_string()))?;
    let rpc_token = codexmanager_service::rpc_auth_token();
    let http = format!(
        "POST /rpc HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\nX-CodexManager-Rpc-Token: {rpc_token}\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}",
        json.len(),
        json
    );

    let mut stream = TcpStream::connect_timeout(&sock, RPC_CONNECT_TIMEOUT).map_err(|e| {
        let msg = format!("Failed to connect to service at {addr}: {e}");
        log::warn!(
            "rpc connect failed ({} -> {} via {}): {}",
            method,
            addr,
            sock,
            e
        );
        RpcAttemptError::BeforeSend(msg)
    })?;
    let _ = stream.set_read_timeout(Some(io_timeout));
    let _ = stream.set_write_timeout(Some(io_timeout));

    stream.write_all(http.as_bytes()).map_err(|e| {
        let msg = e.to_string();
        log::warn!(
            "rpc write failed ({} -> {} via {}): {}",
            method,
            addr,
            sock,
            msg
        );
        RpcAttemptError::MayHaveBeenSent(msg)
    })?;

    let mut buf = String::new();
    stream.read_to_string(&mut buf).map_err(|e| {
        let msg = e.to_string();
        log::warn!(
            "rpc read failed ({} -> {} via {}): {}",
            method,
            addr,
            sock,
            msg
        );
        RpcAttemptError::MayHaveBeenSent(msg)
    })?;
    let body = parse_http_body(&buf).map_err(|msg| {
        log::warn!(
            "rpc parse failed ({} -> {} via {}): {}",
            method,
            addr,
            sock,
            msg
        );
        RpcAttemptError::MayHaveBeenSent(msg)
    })?;
    if body.trim().is_empty() {
        log::warn!("rpc empty response ({} -> {} via {})", method, addr, sock);
        return Err(RpcAttemptError::MayHaveBeenSent(
            "Empty response from service (service not ready, exited, or port occupied)".to_string(),
        ));
    }

    let v: serde_json::Value = serde_json::from_str(&body).map_err(|e| {
        let msg = format!("Unexpected RPC response (non-JSON body): {e}");
        log::warn!(
            "rpc json parse failed ({} -> {} via {}): {}",
            method,
            addr,
            sock,
            msg
        );
        RpcAttemptError::MayHaveBeenSent(msg)
    })?;
    if let Some(err) = v.get("error") {
        log::warn!("rpc error ({} -> {} via {}): {}", method, addr, sock, err);
    }
    Ok(v)
}

/// 函数 `rpc_call_with_sockets`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn rpc_call_with_sockets(
    method: &str,
    addr: &str,
    socket_addrs: &[SocketAddr],
    params: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    let io_timeout = rpc_io_timeout(method, params.as_ref());
    rpc_call_with_sockets_and_timeout(method, addr, socket_addrs, params, io_timeout)
}

fn rpc_call_with_sockets_and_timeout(
    method: &str,
    addr: &str,
    socket_addrs: &[SocketAddr],
    params: Option<serde_json::Value>,
    io_timeout: Duration,
) -> Result<serde_json::Value, String> {
    if socket_addrs.is_empty() {
        return Err(format!(
            "Invalid service address {addr}: no address resolved"
        ));
    }
    let mut last_err =
        "Empty response from service (service not ready, exited, or port occupied)".to_string();
    for attempt in 0..=1 {
        for sock in socket_addrs {
            match rpc_call_on_socket(method, addr, *sock, params.clone(), io_timeout) {
                Ok(v) => return Ok(v),
                Err(err) => {
                    let may_have_been_sent = err.may_have_been_sent();
                    let message = err.into_message();
                    if may_have_been_sent && rpc_must_not_replay_after_send(method) {
                        log::warn!(
                            "rpc replay suppressed after request send ({} -> {} via {}): {}",
                            method,
                            addr,
                            sock,
                            message
                        );
                        return Err(format!(
                            "Reset credit request may have completed, but its response was unavailable. The result is unknown and the request was not retried. Refresh usage and reset-credit status before trying again: {message}"
                        ));
                    }
                    last_err = message;
                }
            }
        }
        if attempt == 0 {
            std::thread::sleep(Duration::from_millis(120));
        }
    }
    Err(last_err)
}

/// 函数 `rpc_call`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn rpc_call(
    method: &str,
    addr: Option<String>,
    params: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    let addr = resolve_service_addr(addr)?;
    let socket_addrs = resolve_socket_addrs(&addr)?;
    rpc_call_with_sockets(method, &addr, &socket_addrs, params)
}

#[cfg(test)]
mod tests {
    use super::{
        rpc_call_with_sockets_and_timeout, rpc_io_timeout, RPC_BULK_USAGE_REFRESH_IO_TIMEOUT,
        RPC_CODEX_SKILLS_MUTATION_IO_TIMEOUT, RPC_CODEX_SKILLS_SEARCH_IO_TIMEOUT,
        RPC_DEFAULT_IO_TIMEOUT, RPC_RESET_CREDIT_CONSUME_IO_TIMEOUT,
        RPC_RESET_CREDIT_CONSUME_METHOD,
    };
    use std::io::{ErrorKind, Read, Write};
    use std::net::{SocketAddr, TcpListener};
    use std::time::Duration;

    fn assert_no_pending_connection(listener: &TcpListener) {
        listener
            .set_nonblocking(true)
            .expect("set listener nonblocking");
        match listener.accept() {
            Err(error) if error.kind() == ErrorKind::WouldBlock => {}
            Err(error) => panic!("unexpected accept error: {error}"),
            Ok(_) => panic!("non-replayable RPC connected to a fallback socket"),
        }
    }

    fn success_response(body: &str) -> String {
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        )
    }

    /// 函数 `bulk_usage_refresh_uses_extended_timeout`
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
    fn bulk_usage_refresh_uses_extended_timeout() {
        let timeout = rpc_io_timeout("account/usage/refresh", None);
        assert_eq!(timeout, RPC_BULK_USAGE_REFRESH_IO_TIMEOUT);
    }

    /// 函数 `single_usage_refresh_keeps_default_timeout`
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
    fn single_usage_refresh_keeps_default_timeout() {
        let timeout = rpc_io_timeout(
            "account/usage/refresh",
            Some(&serde_json::json!({ "accountId": "acc-1" })),
        );
        assert_eq!(timeout, RPC_DEFAULT_IO_TIMEOUT);
    }

    #[test]
    fn reset_credit_consume_uses_extended_timeout() {
        assert_eq!(
            rpc_io_timeout(RPC_RESET_CREDIT_CONSUME_METHOD, None),
            RPC_RESET_CREDIT_CONSUME_IO_TIMEOUT
        );
    }

    #[test]
    fn reset_credit_consume_does_not_replay_after_empty_response() {
        let empty_listener = TcpListener::bind("127.0.0.1:0").expect("bind empty listener");
        let fallback_listener = TcpListener::bind("127.0.0.1:0").expect("bind fallback listener");
        let empty_addr = empty_listener.local_addr().expect("empty listener addr");
        let fallback_addr = fallback_listener
            .local_addr()
            .expect("fallback listener addr");

        let empty_server = std::thread::spawn(move || {
            let (mut stream, _) = empty_listener.accept().expect("accept reset credit RPC");
            let mut request = [0_u8; 2048];
            let read = stream.read(&mut request).expect("read reset credit RPC");
            assert!(
                String::from_utf8_lossy(&request[..read]).contains(RPC_RESET_CREDIT_CONSUME_METHOD)
            );
            // Drop the connection without a response after receiving the request.
        });

        let error = rpc_call_with_sockets_and_timeout(
            RPC_RESET_CREDIT_CONSUME_METHOD,
            "localhost:48760",
            &[empty_addr, fallback_addr],
            Some(serde_json::json!({ "accountId": "fake-account" })),
            Duration::from_secs(1),
        )
        .expect_err("missing response must produce an unknown result");

        empty_server.join().expect("join empty response server");
        assert!(
            error.contains("result is unknown"),
            "unexpected error: {error}"
        );
        assert!(
            error.contains("was not retried"),
            "unexpected error: {error}"
        );
        assert_no_pending_connection(&fallback_listener);
    }

    #[test]
    fn reset_credit_consume_does_not_replay_after_response_timeout() {
        let slow_listener = TcpListener::bind("127.0.0.1:0").expect("bind slow listener");
        let fallback_listener = TcpListener::bind("127.0.0.1:0").expect("bind fallback listener");
        let slow_addr = slow_listener.local_addr().expect("slow listener addr");
        let fallback_addr = fallback_listener
            .local_addr()
            .expect("fallback listener addr");

        let slow_server = std::thread::spawn(move || {
            let (mut stream, _) = slow_listener.accept().expect("accept reset credit RPC");
            let mut request = [0_u8; 2048];
            let read = stream.read(&mut request).expect("read reset credit RPC");
            assert!(
                String::from_utf8_lossy(&request[..read]).contains(RPC_RESET_CREDIT_CONSUME_METHOD)
            );
            std::thread::sleep(Duration::from_millis(150));
            let response = success_response(r#"{"result":{"consumed":true}}"#);
            let _ = stream.write_all(response.as_bytes());
        });

        let error = rpc_call_with_sockets_and_timeout(
            RPC_RESET_CREDIT_CONSUME_METHOD,
            "localhost:48760",
            &[slow_addr, fallback_addr],
            Some(serde_json::json!({ "accountId": "fake-account" })),
            Duration::from_millis(30),
        )
        .expect_err("read timeout must produce an unknown result");

        slow_server.join().expect("join slow response server");
        assert!(
            error.contains("result is unknown"),
            "unexpected error: {error}"
        );
        assert!(
            error.contains("was not retried"),
            "unexpected error: {error}"
        );
        assert_no_pending_connection(&fallback_listener);
    }

    #[test]
    fn reset_credit_consume_can_fall_back_before_request_is_sent() {
        let good_listener = TcpListener::bind("127.0.0.1:0").expect("bind good listener");
        let good_addr = good_listener.local_addr().expect("good listener addr");
        let unavailable_addr = SocketAddr::from(([127, 0, 0, 1], 0));

        let good_server = std::thread::spawn(move || {
            let (mut stream, _) = good_listener.accept().expect("accept reset credit RPC");
            let mut request = [0_u8; 2048];
            let read = stream.read(&mut request).expect("read reset credit RPC");
            assert!(
                String::from_utf8_lossy(&request[..read]).contains(RPC_RESET_CREDIT_CONSUME_METHOD)
            );
            let response = success_response(r#"{"result":{"consumed":true}}"#);
            stream
                .write_all(response.as_bytes())
                .expect("write reset credit response");
        });

        let response = rpc_call_with_sockets_and_timeout(
            RPC_RESET_CREDIT_CONSUME_METHOD,
            "localhost:48760",
            &[unavailable_addr, good_addr],
            Some(serde_json::json!({ "accountId": "fake-account" })),
            Duration::from_secs(1),
        )
        .expect("connect failure before send may fall back");

        good_server.join().expect("join good response server");
        assert_eq!(response["result"]["consumed"], true);
    }

    #[test]
    fn codex_marketplace_rpcs_use_extended_timeout() {
        for method in [
            "codexSkills/marketplaceList",
            "codexSkills/marketplaceAdd",
            "codexSkills/marketplaceRefresh",
            "codexSkills/marketplacePluginInstall",
        ] {
            assert_eq!(
                rpc_io_timeout(method, None),
                RPC_CODEX_SKILLS_MUTATION_IO_TIMEOUT,
                "unexpected timeout for {method}"
            );
        }
    }

    #[test]
    fn codex_skill_file_mutations_use_extended_timeout() {
        for method in [
            "codexSkills/installZip",
            "codexSkills/importDirectory",
            "codexSkills/delete",
        ] {
            assert_eq!(
                rpc_io_timeout(method, None),
                RPC_CODEX_SKILLS_MUTATION_IO_TIMEOUT,
                "unexpected timeout for {method}"
            );
        }
        assert_eq!(
            rpc_io_timeout("codexSkills/list", None),
            RPC_DEFAULT_IO_TIMEOUT
        );
    }

    #[test]
    fn codex_skill_repository_and_registry_timeouts_match_operation_cost() {
        for method in [
            "codexSkills/repositoryAdd",
            "codexSkills/repositoryRefresh",
            "codexSkills/repositoryInstall",
            "codexSkills/registryInstall",
        ] {
            assert_eq!(
                rpc_io_timeout(method, None),
                RPC_CODEX_SKILLS_MUTATION_IO_TIMEOUT,
                "unexpected timeout for {method}"
            );
        }
        for method in ["codexSkills/repositoryList", "codexSkills/repositoryDelete"] {
            assert_eq!(
                rpc_io_timeout(method, None),
                RPC_DEFAULT_IO_TIMEOUT,
                "unexpected timeout for {method}"
            );
        }
        assert_eq!(
            rpc_io_timeout("codexSkills/registrySearch", None),
            RPC_CODEX_SKILLS_SEARCH_IO_TIMEOUT
        );
    }

    /// 函数 `unrelated_rpc_keeps_default_timeout`
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
    fn unrelated_rpc_keeps_default_timeout() {
        let timeout = rpc_io_timeout("account/list", None);
        assert_eq!(timeout, RPC_DEFAULT_IO_TIMEOUT);
    }
}
