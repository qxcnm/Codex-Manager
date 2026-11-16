use super::*;
use axum::{extract::State, http::Uri, Json, Router};
use std::{ffi::OsString, net::TcpListener, sync::Arc};

#[derive(Default)]
struct ProxyEnvironment(Vec<(&'static str, Option<OsString>)>);

impl ProxyEnvironment {
    fn set(&mut self, name: &'static str, value: Option<&str>) {
        self.0.push((name, std::env::var_os(name)));
        match value {
            Some(value) => std::env::set_var(name, value),
            None => std::env::remove_var(name),
        }
    }

    fn configure(proxy: &str, no_proxy: &str) -> Self {
        let mut env = Self::default();
        for name in [
            "HTTP_PROXY",
            "http_proxy",
            "HTTPS_PROXY",
            "https_proxy",
            "ALL_PROXY",
            "all_proxy",
        ] {
            env.set(name, Some(proxy));
        }
        env.set("NO_PROXY", Some(no_proxy));
        env.set("no_proxy", Some(no_proxy));
        env.set("REQUEST_METHOD", None);
        env
    }
}

impl Drop for ProxyEnvironment {
    fn drop(&mut self) {
        // Restore in reverse order because Windows aliases upper/lowercase names.
        while let Some((name, value)) = self.0.pop() {
            match value {
                Some(value) => std::env::set_var(name, value),
                None => std::env::remove_var(name),
            }
        }
    }
}

struct MockAuth {
    url: String,
    requests: Arc<Mutex<Vec<String>>>,
    task: tokio::task::JoinHandle<()>,
}

impl MockAuth {
    async fn start() -> Self {
        async fn respond(State(requests): State<Arc<Mutex<Vec<String>>>>, uri: Uri) -> Json<Value> {
            requests.lock().unwrap().push(uri.to_string());
            Json(match uri.path() {
                "/api/accounts/deviceauth/usercode" => {
                    json!({"device_auth_id":"fixture-device","user_code":"CODE","interval":1})
                }
                "/api/accounts/deviceauth/token" => {
                    let verifier = "v".repeat(43);
                    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD
                        .encode(Sha256::digest(verifier.as_bytes()));
                    json!({"authorization_code":"fixture-code","code_verifier":verifier,"code_challenge":challenge})
                }
                "/oauth/token" => json!({"access_token":"fixture-token","token_type":"Bearer"}),
                path => panic!("unexpected authorization path: {path}"),
            })
        }

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let app = Router::new().fallback(respond).with_state(requests.clone());
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Self {
            url,
            requests,
            task,
        }
    }
}

impl Drop for MockAuth {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn authorize(client: &reqwest::Client, base: &str) {
    let code = request_device_code(client, base).await.unwrap();
    let tokens = poll_and_exchange(client, base, code).await.unwrap();
    assert_eq!(tokens["access_token"], "fixture-token");
}

fn expected_proxy_requests() -> Vec<String> {
    [
        "/api/accounts/deviceauth/usercode",
        "/api/accounts/deviceauth/token",
        "/oauth/token?unified=true",
    ]
    .map(|path| format!("http://bps-auth.invalid{path}"))
    .into()
}

#[tokio::test]
async fn bps_authorization_inherits_system_proxy_for_entire_device_flow() {
    if crate::loopback_proxy_test::run_proxy_control_in_isolated_process_async().await {
        return;
    }
    let _lock = crate::test_env_guard();
    let proxy = MockAuth::start().await;
    let _env = ProxyEnvironment::configure(&proxy.url, "unrelated.invalid");
    let client = authorization_http_client(None).unwrap();

    authorize(&client, "http://bps-auth.invalid").await;

    assert_eq!(*proxy.requests.lock().unwrap(), expected_proxy_requests());
}

#[tokio::test]
async fn bps_explicit_proxy_overrides_system_proxy_and_environment_bypass() {
    if crate::loopback_proxy_test::run_proxy_control_in_isolated_process_async().await {
        return;
    }
    let _lock = crate::test_env_guard();
    let system_proxy = TcpListener::bind("127.0.0.1:0").unwrap();
    system_proxy.set_nonblocking(true).unwrap();
    let _env = ProxyEnvironment::configure(
        &format!("http://{}", system_proxy.local_addr().unwrap()),
        "*",
    );
    let explicit = MockAuth::start().await;
    let client = authorization_http_client(Some(&explicit.url)).unwrap();

    authorize(&client, "http://bps-auth.invalid").await;

    assert_eq!(
        *explicit.requests.lock().unwrap(),
        expected_proxy_requests()
    );
    assert!(
        matches!(system_proxy.accept(), Err(err) if err.kind() == std::io::ErrorKind::WouldBlock)
    );
}

#[test]
fn bps_invalid_explicit_proxy_returns_sanitized_error() {
    assert_eq!(
        authorization_http_client(Some("http://[")).err().as_deref(),
        Some("BPS authorization proxy is invalid")
    );
}

#[tokio::test]
async fn bps_inherited_proxy_respects_no_proxy_for_direct_requests() {
    if crate::loopback_proxy_test::run_proxy_control_in_isolated_process_async().await {
        return;
    }
    let _lock = crate::test_env_guard();
    let system_proxy = TcpListener::bind("127.0.0.1:0").unwrap();
    system_proxy.set_nonblocking(true).unwrap();
    let _env = ProxyEnvironment::configure(
        &format!("http://{}", system_proxy.local_addr().unwrap()),
        "127.0.0.1",
    );
    let direct = MockAuth::start().await;
    let client = authorization_http_client(None).unwrap();

    authorize(&client, &direct.url).await;

    assert_eq!(
        *direct.requests.lock().unwrap(),
        [
            "/api/accounts/deviceauth/usercode",
            "/api/accounts/deviceauth/token",
            "/oauth/token?unified=true",
        ]
    );
    assert!(
        matches!(system_proxy.accept(), Err(err) if err.kind() == std::io::ErrorKind::WouldBlock)
    );
}
