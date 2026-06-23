use url::Url;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProxyTestErrorCode {
    InvalidProxyUrl,
    ProxyClientBuildFailed,
    ProxyConnectTimeout,
    ProxyReadTimeout,
    ProxyTotalTimeout,
    ProxyDnsFailed,
    ProxyConnectFailed,
    ProxyTunnelFailed,
    ProxyTlsFailed,
    ProxyRedirectFailed,
    ProxyRequestFailed,
    UploadEndpointNotConfigured,
}

impl ProxyTestErrorCode {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidProxyUrl => "invalid_proxy_url",
            Self::ProxyClientBuildFailed => "proxy_client_build_failed",
            Self::ProxyConnectTimeout => "proxy_connect_timeout",
            Self::ProxyReadTimeout => "proxy_read_timeout",
            Self::ProxyTotalTimeout => "proxy_total_timeout",
            Self::ProxyDnsFailed => "proxy_dns_failed",
            Self::ProxyConnectFailed => "proxy_connect_failed",
            Self::ProxyTunnelFailed => "proxy_tunnel_failed",
            Self::ProxyTlsFailed => "proxy_tls_failed",
            Self::ProxyRedirectFailed => "proxy_redirect_failed",
            Self::ProxyRequestFailed => "proxy_request_failed",
            Self::UploadEndpointNotConfigured => "upload_endpoint_not_configured",
        }
    }
}

pub(crate) fn proxy_test_result_status(err: &ProxyTestError) -> &'static str {
    match err.code {
        ProxyTestErrorCode::InvalidProxyUrl => "invalid_url",
        ProxyTestErrorCode::ProxyConnectTimeout
        | ProxyTestErrorCode::ProxyReadTimeout
        | ProxyTestErrorCode::ProxyTotalTimeout => "timeout",
        _ => "failed",
    }
}

pub(crate) fn proxy_test_result_error_code(err: &ProxyTestError) -> &'static str {
    match err.code {
        ProxyTestErrorCode::InvalidProxyUrl => "invalid_proxy_url",
        ProxyTestErrorCode::ProxyConnectTimeout => "connect_timeout",
        ProxyTestErrorCode::ProxyReadTimeout | ProxyTestErrorCode::ProxyTotalTimeout => {
            "request_timeout"
        }
        ProxyTestErrorCode::ProxyDnsFailed => "dns_failed",
        ProxyTestErrorCode::ProxyConnectFailed => "proxy_connect_failed",
        ProxyTestErrorCode::ProxyTunnelFailed => {
            if err.message.to_ascii_lowercase().contains("407") {
                "proxy_auth_failed"
            } else {
                "proxy_connect_failed"
            }
        }
        ProxyTestErrorCode::ProxyTlsFailed => "tls_failed",
        ProxyTestErrorCode::ProxyRedirectFailed => "redirect_detected",
        ProxyTestErrorCode::ProxyClientBuildFailed | ProxyTestErrorCode::ProxyRequestFailed => {
            "unknown"
        }
        ProxyTestErrorCode::UploadEndpointNotConfigured => "upload_endpoint_not_configured",
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProxyTestError {
    pub code: ProxyTestErrorCode,
    pub message: String,
}

impl std::fmt::Display for ProxyTestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message.as_str())
    }
}

impl std::error::Error for ProxyTestError {}

pub(crate) fn invalid_proxy_url_error(
    proxy_url: &str,
    err: impl std::fmt::Display,
) -> ProxyTestError {
    ProxyTestError {
        code: ProxyTestErrorCode::InvalidProxyUrl,
        message: format!(
            "invalid proxyUrl for {}: {err}. Check the local HTTP/SOCKS proxy URL.",
            crate::account_proxy::redact_proxy_url_for_log(proxy_url)
        ),
    }
}

pub(crate) fn upload_endpoint_not_configured_error() -> ProxyTestError {
    ProxyTestError {
        code: ProxyTestErrorCode::UploadEndpointNotConfigured,
        message: "Upload endpoint is not configured. Specify CODEXMANAGER_PROXY_TEST_UPLOAD_URL in environment or settings.".to_string(),
    }
}

pub(crate) fn proxy_client_build_error(proxy_url: &str, err: reqwest::Error) -> ProxyTestError {
    let sanitized = sanitize_reqwest_error(proxy_url, err);
    ProxyTestError {
        code: ProxyTestErrorCode::ProxyClientBuildFailed,
        message: format!(
            "build proxy test client via {} failed: {sanitized}",
            crate::account_proxy::redact_proxy_url_for_log(proxy_url)
        ),
    }
}

pub(crate) fn map_proxy_test_reqwest_error(
    action: &str,
    proxy_url: &str,
    err: reqwest::Error,
) -> ProxyTestError {
    let code = classify_reqwest_error(&err);
    let sanitized = sanitize_reqwest_error(proxy_url, err);
    ProxyTestError {
        code,
        message: format!(
            "{action} via {} failed: {sanitized}",
            crate::account_proxy::redact_proxy_url_for_log(proxy_url)
        ),
    }
}

fn classify_reqwest_error(err: &reqwest::Error) -> ProxyTestErrorCode {
    let message = err.to_string().to_ascii_lowercase();

    if err.is_redirect() {
        return ProxyTestErrorCode::ProxyRedirectFailed;
    }
    if err.is_timeout() {
        if err.is_connect() || looks_like_connect_timeout(&message) {
            return ProxyTestErrorCode::ProxyConnectTimeout;
        }
        if looks_like_read_timeout(&message) {
            return ProxyTestErrorCode::ProxyReadTimeout;
        }
        return ProxyTestErrorCode::ProxyTotalTimeout;
    }
    if looks_like_dns_failure(&message) {
        return ProxyTestErrorCode::ProxyDnsFailed;
    }
    if looks_like_tunnel_failure(&message) {
        return ProxyTestErrorCode::ProxyTunnelFailed;
    }
    if looks_like_tls_failure(&message) {
        return ProxyTestErrorCode::ProxyTlsFailed;
    }
    if err.is_connect() {
        return ProxyTestErrorCode::ProxyConnectFailed;
    }
    if err.is_builder() {
        return ProxyTestErrorCode::ProxyClientBuildFailed;
    }
    ProxyTestErrorCode::ProxyRequestFailed
}

fn looks_like_connect_timeout(message: &str) -> bool {
    message.contains("connect timeout")
        || message.contains("connection timed out")
        || message.contains("timed out trying to connect")
}

fn looks_like_read_timeout(message: &str) -> bool {
    message.contains("read timeout")
        || message.contains("timed out while waiting")
        || message.contains("operation timed out")
}

fn looks_like_dns_failure(message: &str) -> bool {
    message.contains("dns")
        || message.contains("failed to lookup address information")
        || message.contains("failed to lookup")
        || message.contains("name or service not known")
        || message.contains("no such host is known")
        || message.contains("temporary failure in name resolution")
}

fn looks_like_tunnel_failure(message: &str) -> bool {
    message.contains("unsuccessful tunnel")
        || message.contains("proxy connect")
        || message.contains("tunnel")
}

fn looks_like_tls_failure(message: &str) -> bool {
    message.contains("tls")
        || message.contains("certificate")
        || message.contains("handshake")
        || message.contains("invalid peer certificate")
}

fn sanitize_reqwest_error(proxy_url: &str, err: reqwest::Error) -> String {
    let redacted_proxy_url = crate::account_proxy::redact_proxy_url_for_log(proxy_url);
    let err = redact_sensitive_error_url(err);
    let mut message = err.to_string();
    let trimmed_proxy_url = proxy_url.trim();

    if !trimmed_proxy_url.is_empty() {
        message = message.replace(trimmed_proxy_url, redacted_proxy_url.as_str());
    }

    if let Ok(parsed_proxy_url) = Url::parse(trimmed_proxy_url) {
        let username = parsed_proxy_url.username();
        if !username.is_empty() {
            let credentials = match parsed_proxy_url.password() {
                Some(password) => format!("{username}:{password}@"),
                None => format!("{username}@"),
            };
            message = message.replace(credentials.as_str(), "");
        }
    }

    message
}

fn redact_sensitive_error_url(mut err: reqwest::Error) -> reqwest::Error {
    if let Some(url) = err.url_mut() {
        redact_sensitive_url_parts(url);
    }
    err
}

fn redact_sensitive_url_parts(url: &mut Url) {
    let _ = url.set_username("");
    let _ = url.set_password(None);
    url.set_query(None);
    url.set_fragment(None);
}

#[cfg(test)]
mod tests {
    use super::{map_proxy_test_reqwest_error, ProxyTestErrorCode};
    use std::net::TcpListener;
    use std::thread;
    use std::time::Duration;
    use tiny_http::{Header, Response, Server, StatusCode};

    fn run_redirect_test_once() -> Result<(), String> {
        let server = Server::http("127.0.0.1:0").map_err(|e| e.to_string())?;
        let server_addr = format!("http://{}", server.server_addr());
        let redirect_url = format!("{server_addr}/loop?token=secret");
        let handle = thread::spawn(move || {
            for _ in 0..3 {
                let request = match server.recv_timeout(Duration::from_secs(10)) {
                    Ok(Some(request)) => request,
                    Ok(None) => break,
                    Err(_) => break,
                };
                let response = Response::empty(StatusCode(302)).with_header(
                    Header::from_bytes("Location", redirect_url.as_bytes())
                        .expect("location header"),
                );
                request.respond(response).expect("respond redirect");
            }
        });

        let client = reqwest::blocking::Client::builder()
            .redirect(reqwest::redirect::Policy::limited(0))
            .build()
            .map_err(|e| e.to_string())?;
        let err = match client
            .get(format!("{server_addr}/loop?token=secret"))
            .send()
        {
            Ok(_) => return Err("expected redirect error, got ok".to_string()),
            Err(e) => e,
        };

        let mapped = map_proxy_test_reqwest_error(
            "proxy latency test",
            "socks5h://user:pass@example.com:1080",
            err,
        );

        let _ = handle.join();

        if mapped.code != ProxyTestErrorCode::ProxyRedirectFailed {
            return Err(format!("expected ProxyRedirectFailed, got {:?}", mapped.code));
        }
        if mapped.message.contains("user:pass") {
            return Err("message contains proxy credentials".to_string());
        }
        if mapped.message.contains("token=secret") {
            return Err("message contains query tokens".to_string());
        }
        if !mapped.message.contains("socks5h://example.com:1080") {
            return Err("message does not contain redacted proxy URL".to_string());
        }
        if !mapped.message.contains("/loop") {
            return Err("message does not contain request path".to_string());
        }

        Ok(())
    }

    #[test]
    fn redirect_error_mapping_redacts_proxy_credentials_and_query_tokens() {
        let mut last_err = String::new();
        for _ in 0..5 {
            match run_redirect_test_once() {
                Ok(_) => return,
                Err(err) => {
                    last_err = err;
                    thread::sleep(Duration::from_millis(100));
                }
            }
        }
        panic!("redirect test failed after 5 retries. Last error: {last_err}");
    }

    #[test]
    fn connect_refused_maps_to_proxy_connect_failed() {
        let free_port = reserve_free_port();
        let client = reqwest::blocking::Client::builder()
            .build()
            .expect("build client");
        let err = client
            .get(format!("http://127.0.0.1:{free_port}/health"))
            .send()
            .expect_err("connect refused should fail");

        let mapped = map_proxy_test_reqwest_error(
            "proxy latency test",
            "http://user:pass@127.0.0.1:8080",
            err,
        );

        assert_eq!(mapped.code, ProxyTestErrorCode::ProxyConnectFailed);
        assert!(!mapped.message.contains("user:pass"));
        assert!(mapped.message.contains("http://127.0.0.1:8080"));
    }

    fn reserve_free_port() -> u16 {
        TcpListener::bind("127.0.0.1:0")
            .expect("bind free port")
            .local_addr()
            .expect("free port addr")
            .port()
    }
}
