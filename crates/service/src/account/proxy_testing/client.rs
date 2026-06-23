use reqwest::Proxy;
use std::time::Duration;
use url::Url;

use super::errors::{invalid_proxy_url_error, proxy_client_build_error, ProxyTestError};

pub(crate) const PROXY_TEST_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
pub(crate) const PROXY_TEST_READ_TIMEOUT: Duration = Duration::from_secs(30);
pub(crate) const PROXY_TEST_TOTAL_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProxyTestRedirectPolicy {
    None,
    Limited(usize),
}

impl ProxyTestRedirectPolicy {
    fn into_reqwest_policy(self) -> reqwest::redirect::Policy {
        match self {
            Self::None => reqwest::redirect::Policy::none(),
            Self::Limited(limit) => reqwest::redirect::Policy::limited(limit),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ProxyTestClientContext {
    pub parsed_proxy_url: Url,
    #[allow(dead_code)]
    pub proxy_url_redacted: String,
}

pub(crate) fn build_proxy_test_client(
    proxy_url: &str,
    redirect_policy: ProxyTestRedirectPolicy,
    accept_invalid_certs: bool,
) -> Result<(reqwest::Client, ProxyTestClientContext), ProxyTestError> {
    let (proxy, context) = build_proxy_components(proxy_url)?;
    let client = reqwest::Client::builder()
        .no_proxy()
        .proxy(proxy)
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
        .connect_timeout(PROXY_TEST_CONNECT_TIMEOUT)
        .read_timeout(PROXY_TEST_READ_TIMEOUT)
        .timeout(PROXY_TEST_TOTAL_TIMEOUT)
        .pool_max_idle_per_host(10)
        .redirect(redirect_policy.into_reqwest_policy())
        .danger_accept_invalid_certs(accept_invalid_certs)
        .build()
        .map_err(|err| proxy_client_build_error(proxy_url, err))?;
    Ok((client, context))
}

pub(crate) fn build_blocking_proxy_test_client(
    proxy_url: &str,
    redirect_policy: ProxyTestRedirectPolicy,
    accept_invalid_certs: bool,
) -> Result<(reqwest::blocking::Client, ProxyTestClientContext), ProxyTestError> {
    let (proxy, context) = build_proxy_components(proxy_url)?;
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .proxy(proxy)
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
        .connect_timeout(PROXY_TEST_CONNECT_TIMEOUT)
        .timeout(PROXY_TEST_TOTAL_TIMEOUT)
        .pool_max_idle_per_host(10)
        .redirect(redirect_policy.into_reqwest_policy())
        .danger_accept_invalid_certs(accept_invalid_certs)
        .build()
        .map_err(|err| proxy_client_build_error(proxy_url, err))?;
    Ok((client, context))
}

fn build_proxy_components(
    proxy_url: &str,
) -> Result<(Proxy, ProxyTestClientContext), ProxyTestError> {
    let trimmed_proxy_url = proxy_url.trim();
    let parsed_proxy_url =
        Url::parse(trimmed_proxy_url).map_err(|err| invalid_proxy_url_error(proxy_url, err))?;
    let proxy =
        Proxy::all(trimmed_proxy_url).map_err(|err| invalid_proxy_url_error(proxy_url, err))?;
    Ok((
        proxy,
        ProxyTestClientContext {
            parsed_proxy_url,
            proxy_url_redacted: crate::account_proxy::redact_proxy_url_for_log(proxy_url),
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::{build_blocking_proxy_test_client, ProxyTestRedirectPolicy};
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;
    use std::thread;

    #[test]
    fn blocking_builder_uses_explicit_proxy_and_ignores_env_proxy() {
        let _env_lock = crate::test_env_guard();
        let fake_env_proxy_port = reserve_free_port();
        let (proxy_url, rx, handle) = start_fake_http_proxy();
        let _http_proxy = EnvVarGuard::set(
            "HTTP_PROXY",
            format!("http://127.0.0.1:{fake_env_proxy_port}"),
        );
        let _https_proxy = EnvVarGuard::set(
            "HTTPS_PROXY",
            format!("http://127.0.0.1:{fake_env_proxy_port}"),
        );
        let _all_proxy = EnvVarGuard::set(
            "ALL_PROXY",
            format!("http://127.0.0.1:{fake_env_proxy_port}"),
        );

        let (client, context) = build_blocking_proxy_test_client(
            proxy_url.as_str(),
            ProxyTestRedirectPolicy::None,
            false,
        )
        .expect("build blocking proxy test client");

        let response = client
            .get("http://example.test/probe?token=secret")
            .send()
            .expect("proxy request succeeds");

        let request = rx.recv().expect("proxy recorded request");
        handle.join().expect("join fake proxy");

        assert_eq!(response.status(), reqwest::StatusCode::NO_CONTENT);
        assert!(request.starts_with("GET http://example.test/probe?token=secret HTTP/1.1"));
        assert_eq!(context.proxy_url_redacted, proxy_url);
    }

    #[test]
    fn blocking_builder_preserves_socks5h_scheme() {
        let (_, context) = build_blocking_proxy_test_client(
            "socks5h://user:pass@example.com:1080",
            ProxyTestRedirectPolicy::None,
            false,
        )
        .expect("build socks5h proxy client");

        assert_eq!(context.parsed_proxy_url.scheme(), "socks5h");
        assert_eq!(context.proxy_url_redacted, "socks5h://example.com:1080");
    }

    #[test]
    fn async_builder_preserves_socks5h_scheme() {
        let (_, context) = super::build_proxy_test_client(
            "socks5h://user:pass@example.com:1080",
            ProxyTestRedirectPolicy::None,
            false,
        )
        .expect("build async socks5h proxy client");

        assert_eq!(context.parsed_proxy_url.scheme(), "socks5h");
        assert_eq!(context.proxy_url_redacted, "socks5h://example.com:1080");
    }

    fn start_fake_http_proxy() -> (String, mpsc::Receiver<String>, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake proxy");
        let addr = listener.local_addr().expect("fake proxy addr");
        let proxy_url = format!("http://{addr}");
        let (tx, rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept proxy connection");
            let mut buffer = vec![0_u8; 8192];
            let size = stream.read(&mut buffer).expect("read proxy request");
            let request = String::from_utf8_lossy(&buffer[..size]).to_string();
            tx.send(request).expect("send proxy request");
            stream
                .write_all(
                    b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .expect("write proxy response");
        });
        (proxy_url, rx, handle)
    }

    fn reserve_free_port() -> u16 {
        TcpListener::bind("127.0.0.1:0")
            .expect("bind free port")
            .local_addr()
            .expect("free port addr")
            .port()
    }

    struct EnvVarGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: String) -> Self {
            let previous = std::env::var(key).ok();
            std::env::set_var(key, value);
            Self { key, previous }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }
}
