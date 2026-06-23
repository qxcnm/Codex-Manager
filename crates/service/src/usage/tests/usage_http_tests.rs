use super::{
    build_usage_request_headers, summarize_usage_error_response, usage_http_client,
    CHATGPT_ACCOUNT_ID_HEADER_NAME,
};
use codexmanager_core::storage::{now_ts, Account, Storage};
use reqwest::header::{HeaderMap, HeaderValue};
use reqwest::Client;
use reqwest::StatusCode;
use std::path::PathBuf;
use std::sync::MutexGuard;
use std::thread;
use std::time::Duration;
use tiny_http::{Header, Response, Server, StatusCode as TinyStatusCode};

struct RecordedSubscriptionRequest {
    path: String,
    authorization: Option<String>,
    chatgpt_account_id: Option<String>,
    originator: Option<String>,
    residency: Option<String>,
    origin: Option<String>,
    referer: Option<String>,
    accept: Option<String>,
}

struct EnvVarRestore {
    key: &'static str,
    value: Option<String>,
}

impl EnvVarRestore {
    fn set(key: &'static str, value: &str) -> Self {
        let restore = Self {
            key,
            value: std::env::var(key).ok(),
        };
        std::env::set_var(key, value);
        restore
    }

    fn remove(key: &'static str) -> Self {
        let restore = Self {
            key,
            value: std::env::var(key).ok(),
        };
        std::env::remove_var(key);
        restore
    }
}

impl Drop for EnvVarRestore {
    fn drop(&mut self) {
        match self.value.as_deref() {
            Some(value) => std::env::set_var(self.key, value),
            None => std::env::remove_var(self.key),
        }
    }
}

struct TestDbGuard {
    previous_db_path: Option<String>,
    db_path: PathBuf,
}

impl TestDbGuard {
    fn new(label: &str) -> Self {
        let db_path = std::env::temp_dir().join(format!(
            "codexmanager-usage-http-{label}-{}-{}.sqlite",
            std::process::id(),
            codexmanager_core::storage::now_ts()
        ));
        let previous_db_path = std::env::var("CODEXMANAGER_DB_PATH").ok();
        std::env::set_var("CODEXMANAGER_DB_PATH", &db_path);
        let storage = Storage::open(&db_path).expect("open test storage");
        storage.init().expect("init test storage");
        Self {
            previous_db_path,
            db_path,
        }
    }
}

impl Drop for TestDbGuard {
    fn drop(&mut self) {
        match self.previous_db_path.as_deref() {
            Some(value) => std::env::set_var("CODEXMANAGER_DB_PATH", value),
            None => std::env::remove_var("CODEXMANAGER_DB_PATH"),
        }
        let _ = std::fs::remove_file(&self.db_path);
    }
}

fn seed_account_proxy(db_path: &PathBuf, account_id: &str, enabled: bool, proxy_url: Option<&str>) {
    let storage = Storage::open(db_path).expect("reopen test storage");
    let now = now_ts();
    storage
        .insert_account(&Account {
            id: account_id.to_string(),
            label: account_id.to_string(),
            issuer: "issuer".to_string(),
            chatgpt_account_id: None,
            workspace_id: None,
            group_name: None,
            sort: 0,
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
        })
        .expect("seed account");
    storage
        .upsert_account_proxy_settings(
            account_id,
            enabled,
            Some("custom"),
            None,
            proxy_url,
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
        .expect("seed account proxy");
    crate::gateway::invalidate_account_proxy_cache(account_id);
}

fn spawn_recording_http_proxy(
    response_body: &'static str,
    content_type: &'static str,
) -> (
    String,
    std::sync::mpsc::Receiver<String>,
    thread::JoinHandle<()>,
) {
    use std::io::{Read, Write};
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock HTTP proxy");
    let proxy_addr = listener.local_addr().expect("mock HTTP proxy addr");
    let (request_tx, request_rx) = std::sync::mpsc::channel();
    let handle = thread::spawn(move || {
        let (mut client, _) = listener.accept().expect("accept proxy client");
        client
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("set proxy read timeout");
        let mut request = Vec::new();
        let mut buf = [0_u8; 1024];
        while !request.windows(4).any(|window| window == b"\r\n\r\n") {
            let read = client.read(&mut buf).expect("read proxy request");
            if read == 0 {
                break;
            }
            request.extend_from_slice(&buf[..read]);
        }
        let request_text = String::from_utf8_lossy(request.as_slice()).to_string();
        request_tx.send(request_text).expect("send proxy request");
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response_body}",
            response_body.len()
        );
        client
            .write_all(response.as_bytes())
            .expect("write proxy response");
        client.flush().expect("flush proxy response");
    });
    (format!("http://{proxy_addr}"), request_rx, handle)
}

fn spawn_timeout_recording_http_proxy(
    response_body: &'static str,
    content_type: &'static str,
    accept_timeout: Duration,
) -> (
    String,
    std::sync::mpsc::Receiver<String>,
    thread::JoinHandle<()>,
) {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::time::Instant;

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind timeout HTTP proxy");
    listener
        .set_nonblocking(true)
        .expect("set timeout proxy nonblocking");
    let proxy_addr = listener.local_addr().expect("timeout HTTP proxy addr");
    let (request_tx, request_rx) = std::sync::mpsc::channel();
    let handle = thread::spawn(move || {
        let started_at = Instant::now();
        loop {
            match listener.accept() {
                Ok((mut client, _)) => {
                    client
                        .set_read_timeout(Some(Duration::from_secs(5)))
                        .expect("set timeout proxy read timeout");
                    let mut request = Vec::new();
                    let mut buf = [0_u8; 1024];
                    while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                        let read = client.read(&mut buf).expect("read timeout proxy request");
                        if read == 0 {
                            break;
                        }
                        request.extend_from_slice(&buf[..read]);
                    }
                    let request_text = String::from_utf8_lossy(request.as_slice()).to_string();
                    request_tx
                        .send(request_text)
                        .expect("send timeout proxy request");
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response_body}",
                        response_body.len()
                    );
                    client
                        .write_all(response.as_bytes())
                        .expect("write timeout proxy response");
                    client.flush().expect("flush timeout proxy response");
                    return;
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    if started_at.elapsed() >= accept_timeout {
                        return;
                    }
                    thread::sleep(Duration::from_millis(20));
                }
                Err(err) => panic!("accept timeout proxy client failed: {err}"),
            }
        }
    });
    (format!("http://{proxy_addr}"), request_rx, handle)
}

/// 函数 `usage_header_runtime_scope`
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
fn usage_header_runtime_scope() -> (MutexGuard<'static, ()>, UsageHeaderRuntimeRestore) {
    let guard = crate::test_env_guard();
    let restore = UsageHeaderRuntimeRestore::capture();
    let _ = crate::gateway::set_originator("codex_cli_rs");
    let _ = crate::gateway::set_residency_requirement(None);
    (guard, restore)
}

struct UsageHeaderRuntimeRestore {
    originator: String,
    residency_requirement: Option<String>,
}

impl UsageHeaderRuntimeRestore {
    /// 函数 `capture`
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
    fn capture() -> Self {
        Self {
            originator: crate::gateway::current_originator(),
            residency_requirement: crate::gateway::current_residency_requirement(),
        }
    }
}

impl Drop for UsageHeaderRuntimeRestore {
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
        let _ = crate::gateway::set_originator(&self.originator);
        let _ = crate::gateway::set_residency_requirement(self.residency_requirement.as_deref());
    }
}

/// 函数 `usage_http_client_is_cloneable`
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
fn usage_http_client_is_cloneable() {
    let first = usage_http_client();
    let second = usage_http_client();
    let first_ptr = &first as *const Client;
    let second_ptr = &second as *const Client;
    assert_ne!(first_ptr, second_ptr);
}

/// 函数 `refresh_token_status_error_omits_empty_body`
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
fn refresh_token_status_error_omits_empty_body() {
    assert_eq!(
        super::format_refresh_token_status_error(StatusCode::FORBIDDEN, "   "),
        "refresh token failed with status 403 Forbidden"
    );
}

/// 函数 `refresh_token_status_error_includes_body_snippet`
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
fn refresh_token_status_error_includes_body_snippet() {
    assert_eq!(
        super::format_refresh_token_status_error(
            StatusCode::BAD_REQUEST,
            "{\n  \"error\": \"invalid_grant\"\n}"
        ),
        "refresh token failed with status 400 Bad Request: invalid_grant"
    );
}

#[test]
fn refresh_token_status_error_maps_app_session_terminated_400_to_official_message() {
    assert_eq!(
        super::format_refresh_token_status_error(
            StatusCode::BAD_REQUEST,
            r#"{"code":"app_session_terminated","type":"invalid_request_error","message":"Your session has ended. Please log in again."}"#
        ),
        "refresh token failed with status 400 Bad Request: Your session has ended. Please log in again."
    );
}

#[test]
fn refresh_token_body_matches_codex_refresh_scope() {
    assert_eq!(
        super::build_refresh_token_body("client-id", "refresh-token"),
        "client_id=client-id&grant_type=refresh_token&refresh_token=refresh-token&scope=openid+profile+email"
    );
}

/// 函数 `refresh_token_status_error_maps_invalidated_401_to_official_message`
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
fn refresh_token_status_error_maps_invalidated_401_to_official_message() {
    assert_eq!(
        super::format_refresh_token_status_error(
            StatusCode::UNAUTHORIZED,
            "{\"error\":\"refresh_token_invalidated\"}"
        ),
        "refresh token failed with status 401 Unauthorized: Your access token could not be refreshed because your refresh token was revoked. Please log out and sign in again."
    );
}

/// 函数 `refresh_token_status_error_maps_unknown_401_to_official_message`
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
fn refresh_token_status_error_maps_unknown_401_to_official_message() {
    assert_eq!(
        super::format_refresh_token_status_error(
            StatusCode::UNAUTHORIZED,
            "{\"error\":\"something_else\"}"
        ),
        "refresh token failed with status 401 Unauthorized: Your access token could not be refreshed. Please log out and sign in again."
    );
}

/// 函数 `classify_refresh_token_auth_error_reason_maps_known_and_unknown_401`
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
fn classify_refresh_token_auth_error_reason_maps_known_and_unknown_401() {
    assert_eq!(
        super::classify_refresh_token_auth_error_reason(
            StatusCode::UNAUTHORIZED,
            "{\"error\":\"refresh_token_invalidated\"}"
        ),
        Some(super::RefreshTokenAuthErrorReason::Invalidated)
    );
    assert_eq!(
        super::classify_refresh_token_auth_error_reason(
            StatusCode::UNAUTHORIZED,
            "{\"error\":\"something_else\"}"
        ),
        Some(super::RefreshTokenAuthErrorReason::Unknown401)
    );
    assert_eq!(
        super::classify_refresh_token_auth_error_reason(
            StatusCode::FORBIDDEN,
            "{\"error\":\"refresh_token_invalidated\"}"
        ),
        None
    );
}

/// 函数 `refresh_token_status_error_ignores_headers_for_401_reason_when_body_lacks_code`
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
fn refresh_token_status_error_ignores_headers_for_401_reason_when_body_lacks_code() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-error-json",
        HeaderValue::from_static("{\"identity_error_code\":\"refresh_token_invalidated\"}"),
    );
    headers.insert(
        "x-openai-authorization-error",
        HeaderValue::from_static("refresh_token_expired"),
    );

    assert_eq!(
        super::format_refresh_token_status_error_with_headers(
            StatusCode::UNAUTHORIZED,
            Some(&headers),
            "<html><title>Just a moment...</title></html>"
        ),
        "refresh token failed with status 401 Unauthorized: Your access token could not be refreshed. Please log out and sign in again."
    );
}

/// 函数 `refresh_token_status_error_stabilizes_html_and_debug_headers_for_non_401`
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
fn refresh_token_status_error_stabilizes_html_and_debug_headers_for_non_401() {
    let mut headers = HeaderMap::new();
    headers.insert("x-request-id", HeaderValue::from_static("req_refresh_123"));
    headers.insert("cf-ray", HeaderValue::from_static("cf_refresh_123"));
    headers.insert(
        "x-openai-authorization-error",
        HeaderValue::from_static("missing_authorization_header"),
    );
    headers.insert(
        "x-error-json",
        HeaderValue::from_static("{\"identity_error_code\":\"token_expired\"}"),
    );

    let message = super::format_refresh_token_status_error_with_headers(
        StatusCode::FORBIDDEN,
        Some(&headers),
        "<html><head><title>Just a moment...</title></head><body>challenge</body></html>",
    );

    assert!(message.contains("refresh token failed with status 403 Forbidden"));
    assert!(message.contains("Cloudflare 安全验证页"));
    assert!(message.contains("kind=cloudflare_challenge"));
    assert!(message.contains("request_id=req_refresh_123"));
    assert!(message.contains("cf_ray=cf_refresh_123"));
    assert!(message.contains("auth_error=missing_authorization_header"));
    assert!(message.contains("identity_error_code=token_expired"));
}

/// 函数 `refresh_token_status_error_uses_header_only_debug_suffix_for_empty_body`
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
fn refresh_token_status_error_uses_header_only_debug_suffix_for_empty_body() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-request-id",
        HeaderValue::from_static("req_refresh_empty"),
    );
    headers.insert("cf-ray", HeaderValue::from_static("cf_refresh_empty"));

    let message = super::format_refresh_token_status_error_with_headers(
        StatusCode::BAD_GATEWAY,
        Some(&headers),
        "",
    );

    assert!(message.contains("refresh token failed with status 502 Bad Gateway"));
    assert!(message.contains("kind=cloudflare_edge"));
    assert!(message.contains("request_id=req_refresh_empty"));
    assert!(message.contains("cf_ray=cf_refresh_empty"));
}

#[test]
fn refresh_token_status_error_detects_region_blocked_header_marker() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-openai-authorization-error",
        HeaderValue::from_static("unsupported_country_region_territory"),
    );
    headers.insert("cf-ray", HeaderValue::from_static("cf_refresh_region"));

    let message = super::format_refresh_token_status_error_with_headers(
        StatusCode::FORBIDDEN,
        Some(&headers),
        "",
    );

    assert!(message.contains("refresh token failed with status 403 Forbidden"));
    assert!(message.contains("kind=cloudflare_blocked"));
    assert!(message.contains("auth_error=unsupported_country_region_territory"));
    assert!(super::is_region_blocked_error_message(&message));
    assert!(super::is_refresh_token_region_blocked_error_message(
        &message
    ));
}

#[test]
fn refresh_token_status_error_plain_forbidden_is_not_region_blocked() {
    let message = super::format_refresh_token_status_error(StatusCode::FORBIDDEN, "");

    assert_eq!(message, "refresh token failed with status 403 Forbidden");
    assert!(!super::is_region_blocked_error_message(&message));
    assert!(!super::is_refresh_token_region_blocked_error_message(
        &message
    ));
}

/// 函数 `refresh_token_auth_error_reason_from_message_tracks_canonical_messages`
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
fn refresh_token_auth_error_reason_from_message_tracks_canonical_messages() {
    let invalidated = super::format_refresh_token_status_error(
        StatusCode::UNAUTHORIZED,
        "{\"error\":\"refresh_token_invalidated\"}",
    );
    assert_eq!(
        super::refresh_token_auth_error_reason_from_message(&invalidated),
        Some(super::RefreshTokenAuthErrorReason::Invalidated)
    );

    let unknown = super::format_refresh_token_status_error(
        StatusCode::UNAUTHORIZED,
        "{\"error\":\"something_else\"}",
    );
    assert_eq!(
        super::refresh_token_auth_error_reason_from_message(&unknown),
        Some(super::RefreshTokenAuthErrorReason::Unknown401)
    );

    let invalid_grant = super::format_refresh_token_status_error(
        StatusCode::BAD_REQUEST,
        "{\"error\":\"invalid_grant\"}",
    );
    assert_eq!(
        super::refresh_token_auth_error_reason_from_message(&invalid_grant),
        Some(super::RefreshTokenAuthErrorReason::InvalidGrant)
    );

    let app_session_terminated = super::format_refresh_token_status_error(
        StatusCode::BAD_REQUEST,
        r#"{"code":"app_session_terminated","type":"invalid_request_error","message":"Your session has ended. Please log in again."}"#,
    );
    assert_eq!(
        super::refresh_token_auth_error_reason_from_message(&app_session_terminated),
        Some(super::RefreshTokenAuthErrorReason::AppSessionTerminated)
    );

    let legacy_app_session_terminated =
        "refresh token failed with status 400 Bad Request: code=app_session_terminated type=invalid_request_error Your session has ended. Please log in again.";
    assert_eq!(
        super::refresh_token_auth_error_reason_from_message(legacy_app_session_terminated),
        Some(super::RefreshTokenAuthErrorReason::AppSessionTerminated)
    );
}

/// 函数 `usage_http_default_headers_follow_gateway_runtime_profile`
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
fn usage_http_default_headers_follow_gateway_runtime_profile() {
    let (_guard, _restore) = usage_header_runtime_scope();
    crate::gateway::set_originator("codex_cli_rs_usage").expect("set gateway originator");
    crate::gateway::set_residency_requirement(Some("us"))
        .expect("set gateway residency requirement");

    let headers = super::build_usage_http_default_headers();

    assert_eq!(
        headers
            .get("originator")
            .and_then(|value| value.to_str().ok()),
        Some("codex_cli_rs_usage")
    );
    assert_eq!(
        headers
            .get("x-openai-internal-codex-residency")
            .and_then(|value| value.to_str().ok()),
        Some("us")
    );
}

/// 函数 `usage_request_headers_use_official_chatgpt_account_header_name`
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
fn usage_request_headers_use_official_chatgpt_account_header_name() {
    let headers = build_usage_request_headers(Some("workspace_123"));

    assert_eq!(
        headers
            .get(CHATGPT_ACCOUNT_ID_HEADER_NAME)
            .and_then(|value| value.to_str().ok()),
        Some("workspace_123")
    );
    assert_eq!(headers.len(), 1);
}

/// 函数 `subscription_request_uses_only_authorization_without_custom_usage_headers`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-17
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn subscription_request_uses_only_authorization_without_custom_usage_headers() {
    let (_guard, _restore) = usage_header_runtime_scope();
    crate::gateway::set_originator("codex_cli_rs_usage").expect("set gateway originator");
    crate::gateway::set_residency_requirement(Some("us"))
        .expect("set gateway residency requirement");

    let server = Server::http("127.0.0.1:0").expect("start mock subscription server");
    let addr = format!("http://{}", server.server_addr());
    let (tx, rx) = std::sync::mpsc::channel();
    let handle = thread::spawn(move || {
        let request = server
            .recv_timeout(Duration::from_secs(5))
            .expect("subscription server timeout")
            .expect("receive subscription request");
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
        let originator = request
            .headers()
            .iter()
            .find(|header| header.field.equiv("originator"))
            .map(|header| header.value.as_str().to_string());
        let residency = request
            .headers()
            .iter()
            .find(|header| header.field.equiv("x-openai-internal-codex-residency"))
            .map(|header| header.value.as_str().to_string());
        let origin = request
            .headers()
            .iter()
            .find(|header| header.field.equiv("Origin"))
            .map(|header| header.value.as_str().to_string());
        let referer = request
            .headers()
            .iter()
            .find(|header| header.field.equiv("Referer"))
            .map(|header| header.value.as_str().to_string());
        let accept = request
            .headers()
            .iter()
            .find(|header| header.field.equiv("Accept"))
            .map(|header| header.value.as_str().to_string());
        tx.send(RecordedSubscriptionRequest {
            path: path.clone(),
            authorization,
            chatgpt_account_id,
            originator,
            residency,
            origin,
            referer,
            accept,
        })
        .expect("send subscription request");
        let response_body = match path.as_str() {
            "/accounts/check/v4-2023-04-27" => {
                r#"{"accounts":{"32673762-4fd7-4cef-8d9e-fa96aec5b5c4":{"account":{"plan_type":"pro","is_default":true},"entitlement":{"subscription_plan":"plus","expires_at":"2026-05-06T03:31:29Z","next_renewal_at":"2026-04-20T03:31:29Z","has_active_subscription":true}}}}"#
            }
            other => panic!("unexpected subscription path: {other}"),
        };
        let response = Response::from_string(response_body)
            .with_status_code(TinyStatusCode(200))
            .with_header(
                Header::from_bytes("Content-Type", "application/json")
                    .expect("content-type header"),
            );
        request
            .respond(response)
            .expect("respond subscription request");
    });

    let snapshot = super::fetch_account_subscription(
        &addr,
        "token_123",
        "32673762-4fd7-4cef-8d9e-fa96aec5b5c4",
        Some("workspace_123"),
    )
    .expect("fetch subscription");

    let recorded = rx
        .recv_timeout(Duration::from_secs(5))
        .expect("receive recorded request");
    handle.join().expect("join subscription server");

    assert!(snapshot.has_subscription);
    assert_eq!(snapshot.account_plan_type.as_deref(), Some("pro"));
    assert_eq!(snapshot.plan_type.as_deref(), Some("plus"));
    assert_eq!(snapshot.expires_at, Some(1_778_038_289));
    assert_eq!(snapshot.renews_at, Some(1_776_655_889));
    assert_eq!(recorded.path, "/accounts/check/v4-2023-04-27");
    assert_eq!(recorded.authorization.as_deref(), Some("Bearer token_123"));
    assert_eq!(recorded.chatgpt_account_id, None);
    assert_eq!(recorded.originator, None);
    assert_eq!(recorded.residency, None);
    assert_eq!(recorded.origin.as_deref(), Some("https://chatgpt.com"));
    assert_eq!(recorded.referer.as_deref(), Some("https://chatgpt.com/"));
    assert_eq!(recorded.accept.as_deref(), Some("application/json"));
}

/// 函数 `refresh_token_url_uses_official_default_for_openai_issuer`
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
fn refresh_token_url_uses_official_default_for_openai_issuer() {
    let _lock = crate::test_env_guard();
    std::env::remove_var("CODEX_REFRESH_TOKEN_URL_OVERRIDE");

    assert_eq!(
        super::resolve_refresh_token_url("https://auth.openai.com"),
        "https://auth.openai.com/oauth/token"
    );
    assert_eq!(
        super::resolve_refresh_token_url("https://auth.openai.com/"),
        "https://auth.openai.com/oauth/token"
    );
}

/// 函数 `refresh_token_url_preserves_custom_issuer_and_override`
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
fn refresh_token_url_preserves_custom_issuer_and_override() {
    let _lock = crate::test_env_guard();
    let previous = std::env::var("CODEX_REFRESH_TOKEN_URL_OVERRIDE").ok();

    std::env::remove_var("CODEX_REFRESH_TOKEN_URL_OVERRIDE");
    assert_eq!(
        super::resolve_refresh_token_url("https://auth.example.com"),
        "https://auth.example.com/oauth/token"
    );

    std::env::set_var(
        "CODEX_REFRESH_TOKEN_URL_OVERRIDE",
        "https://override.example.com/custom/token",
    );
    assert_eq!(
        super::resolve_refresh_token_url("https://auth.example.com"),
        "https://override.example.com/custom/token"
    );

    match previous {
        Some(value) => std::env::set_var("CODEX_REFRESH_TOKEN_URL_OVERRIDE", value),
        None => std::env::remove_var("CODEX_REFRESH_TOKEN_URL_OVERRIDE"),
    }
}

#[test]
fn refresh_access_token_mock_region_blocked_response_surfaces_marker() {
    let _lock = crate::test_env_guard();
    let previous = std::env::var("CODEX_REFRESH_TOKEN_URL_OVERRIDE").ok();
    let _ = super::usage_http_client();
    let server = Server::http("127.0.0.1:0").expect("start mock refresh server");
    let url = format!("http://{}/oauth/token", server.server_addr());
    let (tx, rx) = std::sync::mpsc::channel();
    let handle = thread::spawn(move || {
        let mut request = server
            .recv_timeout(Duration::from_secs(5))
            .expect("refresh server timeout")
            .expect("receive refresh request");
        let mut body = String::new();
        request
            .as_reader()
            .read_to_string(&mut body)
            .expect("read refresh request body");
        tx.send(body).expect("send refresh request body");
        let response = Response::from_string("")
            .with_status_code(TinyStatusCode(403))
            .with_header(
                Header::from_bytes(
                    "x-openai-authorization-error",
                    "unsupported_country_region_territory",
                )
                .expect("auth error header"),
            )
            .with_header(Header::from_bytes("cf-ray", "ray-hkg").expect("cf-ray header"));
        request.respond(response).expect("respond refresh request");
    });
    std::env::set_var("CODEX_REFRESH_TOKEN_URL_OVERRIDE", &url);

    let err =
        match super::refresh_access_token("https://auth.openai.com", "client-id", "refresh-old") {
            Ok(_) => panic!("region blocked refresh should fail"),
            Err(err) => err,
        };
    let body = rx
        .recv_timeout(Duration::from_secs(5))
        .expect("receive refresh request body");
    handle.join().expect("join refresh server");
    match previous {
        Some(value) => std::env::set_var("CODEX_REFRESH_TOKEN_URL_OVERRIDE", value),
        None => std::env::remove_var("CODEX_REFRESH_TOKEN_URL_OVERRIDE"),
    }

    assert!(body.contains("grant_type=refresh_token"));
    assert!(body.contains("refresh_token=refresh-old"));
    assert!(err.contains("refresh token failed with status 403 Forbidden"));
    assert!(err.contains("auth_error=unsupported_country_region_territory"));
    assert!(super::is_refresh_token_region_blocked_error_message(&err));
}

#[test]
fn fetch_usage_snapshot_with_explicit_proxy_uses_explicit_proxy_before_global_proxy() {
    let _guard = crate::test_env_guard();
    let _global_proxy = EnvVarRestore::set("CODEXMANAGER_UPSTREAM_PROXY_URL", "http://127.0.0.1:1");
    super::reload_usage_http_client_from_env();
    let (proxy_url, request_rx, proxy_handle) = spawn_recording_http_proxy(
        r#"{"gpt4":{"usedPercent":12.5,"windowMinutes":180}}"#,
        "application/json",
    );

    let snapshot = super::fetch_usage_snapshot_with_explicit_proxy(
        "http://chatgpt.test",
        "token_123",
        Some("workspace_123"),
        proxy_url.as_str(),
    )
    .expect("fetch usage snapshot");
    let request = request_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("capture usage proxy request");
    proxy_handle.join().expect("join usage proxy");
    let request = request.to_ascii_lowercase();

    assert!(request.starts_with("get http://chatgpt.test/"));
    assert!(request.contains("authorization: bearer token_123"));
    assert!(request.contains("chatgpt-account-id: workspace_123"));
    assert_eq!(snapshot["gpt4"]["usedPercent"], 12.5);
}

#[test]
fn fetch_account_subscription_with_explicit_proxy_uses_explicit_proxy_before_global_proxy() {
    let _guard = crate::test_env_guard();
    let _global_proxy = EnvVarRestore::set("CODEXMANAGER_UPSTREAM_PROXY_URL", "http://127.0.0.1:1");
    super::reload_usage_http_client_from_env();
    let (proxy_url, request_rx, proxy_handle) = spawn_recording_http_proxy(
        r#"{"accounts":{"acct-chatgpt":{"account":{"plan_type":"pro","is_default":true},"entitlement":{"subscription_plan":"plus","has_active_subscription":true}}}}"#,
        "application/json",
    );

    let snapshot = super::fetch_account_subscription_with_explicit_proxy(
        "http://chatgpt.test",
        "token_123",
        "acct-chatgpt",
        Some("workspace_123"),
        proxy_url.as_str(),
    )
    .expect("fetch subscription");
    let request = request_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("capture subscription proxy request");
    proxy_handle.join().expect("join subscription proxy");
    let request = request.to_ascii_lowercase();

    assert!(request.starts_with("get http://chatgpt.test/"));
    assert!(request.contains("authorization: bearer token_123"));
    assert!(request.contains("origin: https://chatgpt.com"));
    assert!(!request.contains("user-agent:"));
    assert!(snapshot.has_subscription);
    assert_eq!(snapshot.account_plan_type.as_deref(), Some("pro"));
}

#[test]
fn refresh_access_token_with_explicit_proxy_fails_for_invalid_proxy_url() {
    let _guard = crate::test_env_guard();
    let _override_restore = EnvVarRestore::remove("CODEX_REFRESH_TOKEN_URL_OVERRIDE");

    let err = match super::refresh_access_token_with_explicit_proxy(
        "https://auth.openai.com",
        "client-id",
        "refresh-token",
        "http://",
    ) {
        Ok(_) => panic!("invalid explicit proxy should fail closed"),
        Err(err) => err,
    };

    assert!(err.contains("explicit account proxy URL is invalid and fail-closed"));
}

#[test]
fn refresh_access_token_with_explicit_proxy_fails_closed_for_empty_proxy_url() {
    let _guard = crate::test_env_guard();

    let err = match super::refresh_access_token_with_explicit_proxy(
        "https://auth.openai.com",
        "client-id",
        "refresh-token",
        "   ",
    ) {
        Ok(_) => panic!("empty explicit proxy should fail closed"),
        Err(err) => err,
    };

    assert!(err.contains("explicit account proxy URL is required and fail-closed"));
}

#[test]
fn legacy_subscription_request_ignores_proxy_pool_when_account_proxy_is_disabled() {
    let _guard = crate::test_env_guard();
    let db = TestDbGuard::new("subscription-disabled-proxy-pool");
    seed_account_proxy(
        &db.db_path,
        "acc-disabled-subscription",
        false,
        Some("http://127.0.0.1:7891"),
    );
    let server = Server::http("127.0.0.1:0").expect("start legacy subscription server");
    let addr = format!("http://{}", server.server_addr());
    let (proxy_url, proxy_rx, proxy_handle) = spawn_timeout_recording_http_proxy(
        r#"{"accounts":{"acct-chatgpt":{"account":{"plan_type":"pro","is_default":true},"entitlement":{"subscription_plan":"plus","has_active_subscription":true}}}}"#,
        "application/json",
        Duration::from_millis(400),
    );
    let _global_proxy = EnvVarRestore::set("CODEXMANAGER_UPSTREAM_PROXY_URL", "");
    let _pool_proxy = EnvVarRestore::set("CODEXMANAGER_PROXY_LIST", proxy_url.as_str());
    super::reload_usage_http_client_from_env();

    let (tx, rx) = std::sync::mpsc::channel();
    let handle = thread::spawn(move || {
        let request = server
            .recv_timeout(Duration::from_secs(5))
            .expect("subscription server timeout")
            .expect("receive legacy subscription request");
        tx.send(request.url().to_string())
            .expect("send legacy subscription path");
        let response = Response::from_string(
            r#"{"accounts":{"acct-chatgpt":{"account":{"plan_type":"pro","is_default":true},"entitlement":{"subscription_plan":"plus","has_active_subscription":true}}}}"#,
        )
        .with_status_code(TinyStatusCode(200))
        .with_header(
            Header::from_bytes("Content-Type", "application/json")
                .expect("content-type header"),
        );
        request
            .respond(response)
            .expect("respond legacy subscription");
    });

    let snapshot = super::fetch_account_subscription(
        &addr,
        "token_123",
        "acct-chatgpt",
        Some("workspace_123"),
    )
    .expect("fetch legacy subscription");

    assert!(snapshot.has_subscription);
    assert_eq!(
        rx.recv_timeout(Duration::from_secs(5))
            .expect("receive legacy subscription path"),
        "/accounts/check/v4-2023-04-27"
    );
    assert!(proxy_rx.recv_timeout(Duration::from_millis(300)).is_err());
    handle.join().expect("join legacy subscription server");
    proxy_handle.join().expect("join unused proxy");
}

#[test]
fn legacy_usage_request_ignores_proxy_pool_when_account_proxy_is_disabled() {
    let _guard = crate::test_env_guard();
    let db = TestDbGuard::new("usage-disabled-proxy-pool");
    seed_account_proxy(
        &db.db_path,
        "acc-disabled-usage",
        false,
        Some("http://127.0.0.1:7891"),
    );
    let server = Server::http("127.0.0.1:0").expect("start legacy usage server");
    let addr = format!("http://{}", server.server_addr());
    let (proxy_url, proxy_rx, proxy_handle) = spawn_timeout_recording_http_proxy(
        r#"{"gpt4":{"usedPercent":99.0,"windowMinutes":180}}"#,
        "application/json",
        Duration::from_millis(400),
    );
    let _global_proxy = EnvVarRestore::set("CODEXMANAGER_UPSTREAM_PROXY_URL", "");
    let _pool_proxy = EnvVarRestore::set("CODEXMANAGER_PROXY_LIST", proxy_url.as_str());
    super::reload_usage_http_client_from_env();

    let (tx, rx) = std::sync::mpsc::channel();
    let handle = thread::spawn(move || {
        let request = server
            .recv_timeout(Duration::from_secs(5))
            .expect("usage server timeout")
            .expect("receive legacy usage request");
        tx.send(request.url().to_string())
            .expect("send legacy usage path");
        let response =
            Response::from_string(r#"{"gpt4":{"usedPercent":99.0,"windowMinutes":180}}"#)
                .with_status_code(TinyStatusCode(200))
                .with_header(
                    Header::from_bytes("Content-Type", "application/json")
                        .expect("content-type header"),
                );
        request.respond(response).expect("respond legacy usage");
    });

    let snapshot = super::fetch_usage_snapshot(&addr, "token_123", Some("workspace_123"))
        .expect("fetch legacy usage");

    assert_eq!(snapshot["gpt4"]["usedPercent"], 99.0);
    assert_eq!(
        rx.recv_timeout(Duration::from_secs(5))
            .expect("receive legacy usage path"),
        "/api/codex/usage"
    );
    assert!(proxy_rx.recv_timeout(Duration::from_millis(300)).is_err());
    handle.join().expect("join legacy usage server");
    proxy_handle.join().expect("join unused proxy");
}

/// 函数 `summarize_usage_error_response_stabilizes_html_and_debug_headers`
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
fn summarize_usage_error_response_stabilizes_html_and_debug_headers() {
    let mut headers = HeaderMap::new();
    headers.insert("x-request-id", HeaderValue::from_static("req_usage_123"));
    headers.insert("cf-ray", HeaderValue::from_static("cf_usage_123"));
    headers.insert(
        "x-openai-authorization-error",
        HeaderValue::from_static("missing_authorization_header"),
    );
    headers.insert(
        "x-error-json",
        HeaderValue::from_static("eyJlcnJvciI6eyJjb2RlIjoidG9rZW5fZXhwaXJlZCJ9fQ=="),
    );

    let summary = summarize_usage_error_response(
        StatusCode::FORBIDDEN,
        &headers,
        "<html><head><title>Just a moment...</title></head><body>challenge</body></html>",
        true,
    );

    assert!(summary.contains("usage endpoint failed: status=403 Forbidden"));
    assert!(summary.contains("Cloudflare 安全验证页"));
    assert!(summary.contains("request id: req_usage_123"));
    assert!(summary.contains("cf-ray: cf_usage_123"));
    assert!(summary.contains("auth error: missing_authorization_header"));
    assert!(summary.contains("identity error code: token_expired"));
}

/// 函数 `summarize_usage_error_response_accepts_raw_error_json_header`
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
fn summarize_usage_error_response_accepts_raw_error_json_header() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-request-id",
        HeaderValue::from_static("req_usage_raw_123"),
    );
    headers.insert(
        "x-error-json",
        HeaderValue::from_static("{\"details\":{\"identity_error_code\":\"proxy_auth_required\"}}"),
    );

    let summary = summarize_usage_error_response(
        StatusCode::BAD_GATEWAY,
        &headers,
        "<html><head><title>502 Bad Gateway</title></head></html>",
        false,
    );

    assert!(summary.contains("request id: req_usage_raw_123"));
    assert!(summary.contains("identity error code: proxy_auth_required"));
    assert!(summary.contains("<html><head><title>502 Bad Gateway</title></head></html>"));
}
