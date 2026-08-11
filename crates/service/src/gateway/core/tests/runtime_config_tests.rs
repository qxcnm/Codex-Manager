use super::*;

struct EnvGuard {
    key: &'static str,
    original: Option<std::ffi::OsString>,
}

impl EnvGuard {
    /// 函数 `set`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - key: 参数 key
    /// - value: 参数 value
    ///
    /// # 返回
    /// 返回函数执行结果
    fn set(key: &'static str, value: &str) -> Self {
        let original = std::env::var_os(key);
        std::env::set_var(key, value);
        Self { key, original }
    }

    /// 函数 `clear`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - key: 参数 key
    ///
    /// # 返回
    /// 返回函数执行结果
    fn clear(key: &'static str) -> Self {
        let original = std::env::var_os(key);
        std::env::remove_var(key);
        Self { key, original }
    }
}

impl Drop for EnvGuard {
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
        if let Some(value) = &self.original {
            std::env::set_var(self.key, value);
        } else {
            std::env::remove_var(self.key);
        }
    }
}

/// 函数 `reload_from_env_updates_timeout_and_proxy`
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
fn reload_from_env_updates_timeout_and_proxy() {
    let _guard = crate::test_env_guard();
    let _timeout_guard = EnvGuard::set(ENV_UPSTREAM_TOTAL_TIMEOUT_MS, "777");
    let _stream_timeout_guard = EnvGuard::set(ENV_UPSTREAM_STREAM_TIMEOUT_MS, "888");
    let _inflight_guard = EnvGuard::set(ENV_ACCOUNT_MAX_INFLIGHT, "4");
    let _strict_allowlist_guard = EnvGuard::set(ENV_STRICT_REQUEST_PARAM_ALLOWLIST, "0");
    let _request_compression_guard = EnvGuard::set(ENV_ENABLE_REQUEST_COMPRESSION, "0");
    let _image_enabled_guard = EnvGuard::set(ENV_CODEX_IMAGE_GENERATION_ENABLED, "0");
    let _image_auto_inject_guard = EnvGuard::set(ENV_CODEX_IMAGE_GENERATION_AUTO_INJECT_TOOL, "1");
    let _image_main_model_guard = EnvGuard::set(ENV_CODEX_IMAGE_MAIN_MODEL, "gpt-5.4");
    let _image_tool_model_guard = EnvGuard::set(ENV_CODEX_IMAGE_TOOL_MODEL, "gpt-image-2");
    let _client_id_guard = EnvGuard::set(ENV_TOKEN_EXCHANGE_CLIENT_ID, "client-id-123");
    let _issuer_guard = EnvGuard::set(ENV_TOKEN_EXCHANGE_ISSUER, "https://issuer.example");
    let _proxy_guard = EnvGuard::set(ENV_UPSTREAM_PROXY_URL, "socks5://127.0.0.1:7890");

    reload_from_env();

    assert_eq!(upstream_total_timeout(), Some(Duration::from_millis(777)));
    assert_eq!(upstream_stream_timeout(), Some(Duration::from_millis(888)));
    assert_eq!(account_max_inflight_limit(), 4);
    assert!(!strict_request_param_allowlist_enabled());
    assert!(!request_compression_enabled());
    assert!(!codex_image_generation_enabled());
    assert!(codex_image_generation_auto_inject_tool_enabled());
    assert_eq!(current_codex_image_main_model(), "gpt-5.4");
    assert_eq!(current_codex_image_tool_model(), "gpt-image-2");
    assert_eq!(token_exchange_client_id(), "client-id-123");
    assert_eq!(
        token_exchange_default_issuer(),
        "https://issuer.example".to_string()
    );
    assert_eq!(
        upstream_proxy_url().as_deref(),
        Some("socks5h://127.0.0.1:7890")
    );
}

/// 函数 `reload_from_env_defaults_keep_conservative_account_limit`
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
fn reload_from_env_defaults_keep_conservative_account_limit() {
    let _guard = crate::test_env_guard();
    let _account_guard = EnvGuard::clear(ENV_ACCOUNT_MAX_INFLIGHT);
    let _strict_guard = EnvGuard::clear(ENV_STRICT_REQUEST_PARAM_ALLOWLIST);
    let _gate_guard = EnvGuard::clear(ENV_REQUEST_GATE_WAIT_TIMEOUT_MS);
    let _front_proxy_guard = EnvGuard::clear(ENV_FRONT_PROXY_MAX_BODY_BYTES);
    let _stream_guard = EnvGuard::clear(ENV_UPSTREAM_STREAM_TIMEOUT_MS);
    let _request_compression_guard = EnvGuard::clear(ENV_ENABLE_REQUEST_COMPRESSION);
    let _image_enabled_guard = EnvGuard::clear(ENV_CODEX_IMAGE_GENERATION_ENABLED);
    let _image_auto_inject_guard = EnvGuard::clear(ENV_CODEX_IMAGE_GENERATION_AUTO_INJECT_TOOL);
    let _image_main_model_guard = EnvGuard::clear(ENV_CODEX_IMAGE_MAIN_MODEL);
    let _image_tool_model_guard = EnvGuard::clear(ENV_CODEX_IMAGE_TOOL_MODEL);

    reload_from_env();

    assert_eq!(account_max_inflight_limit(), 1);
    assert!(!strict_request_param_allowlist_enabled());
    assert_eq!(request_gate_wait_timeout(), None);
    assert_eq!(front_proxy_max_body_bytes(), 0);
    assert_eq!(
        upstream_stream_timeout(),
        Some(Duration::from_millis(300_000))
    );
    assert!(request_compression_enabled());
    assert!(codex_image_generation_enabled());
    assert!(!codex_image_generation_auto_inject_tool_enabled());
    assert_eq!(current_codex_image_main_model(), "gpt-5.4-mini");
    assert_eq!(current_codex_image_tool_model(), "gpt-image-2");
}

/// 函数 `parse_proxy_list_env_limits_to_five_entries`
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
fn parse_proxy_list_env_limits_to_five_entries() {
    let _guard = crate::test_env_guard();
    let _guard = EnvGuard::set(
        ENV_PROXY_LIST,
        "http://p1:8080,http://p2:8080;http://p3:8080\nhttp://p4:8080\rhttp://p5:8080,http://p6:8080",
    );
    let parsed = parse_proxy_list_env();
    assert_eq!(parsed.len(), MAX_UPSTREAM_PROXY_POOL_SIZE);
    assert_eq!(parsed.first().map(String::as_str), Some("http://p1:8080"));
    assert_eq!(parsed.last().map(String::as_str), Some("http://p5:8080"));
}

/// 函数 `parse_proxy_list_env_normalizes_socks_entries`
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
fn parse_proxy_list_env_normalizes_socks_entries() {
    let _guard = crate::test_env_guard();
    let _guard = EnvGuard::set(
        ENV_PROXY_LIST,
        "socks5://127.0.0.1:7890,socks://127.0.0.1:7891,https://socks5://127.0.0.1:7892",
    );

    let parsed = parse_proxy_list_env();

    assert_eq!(parsed.len(), 3);
    assert_eq!(parsed[0], "socks5h://127.0.0.1:7890");
    assert_eq!(parsed[1], "socks5h://127.0.0.1:7891");
    assert_eq!(parsed[2], "socks5h://127.0.0.1:7892");
}

#[test]
fn parse_proxy_list_env_is_order_independent_and_deduplicated() {
    let _guard = crate::test_env_guard();
    let first = EnvGuard::set(
        ENV_PROXY_LIST,
        "http://pool-c:8080,http://pool-a:8080,http://pool-b:8080,http://pool-a:8080",
    );
    let first_parsed = parse_proxy_list_env();
    drop(first);
    let _second = EnvGuard::set(
        ENV_PROXY_LIST,
        "http://pool-b:8080,http://pool-c:8080,http://pool-a:8080",
    );
    let second_parsed = parse_proxy_list_env();

    assert_eq!(first_parsed, second_parsed);
    assert_eq!(first_parsed.len(), 3);
}

/// 函数 `stable_proxy_index_is_deterministic`
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
fn stable_proxy_index_is_deterministic() {
    let _guard = crate::test_env_guard();
    let idx1 = stable_proxy_index("account-42", 5);
    let idx2 = stable_proxy_index("account-42", 5);
    assert_eq!(idx1, idx2);
    assert!(idx1.expect("index") < 5);
}

#[test]
fn upstream_client_pool_builds_matching_blocking_and_async_clients() {
    let _guard = crate::test_env_guard();
    let _global_proxy_guard = EnvGuard::clear(ENV_UPSTREAM_PROXY_URL);
    let _proxy_list_guard = EnvGuard::set(
        ENV_PROXY_LIST,
        "http://pool-a.example:8080,http://pool-b.example:8080",
    );

    reload_from_env();

    let pool = crate::lock_utils::read_recover(upstream_client_pool_lock(), "test_pool");
    assert_eq!(pool.proxies.len(), 2);
    assert_eq!(pool.retry_clients.len(), pool.proxies.len());
    assert_eq!(pool.async_retry_clients.len(), pool.proxies.len());

    let account_id = "account-42";
    assert!(pool.retry_client_for_account(account_id).is_some());
    assert!(pool.async_retry_client_for_account(account_id).is_some());
    assert_eq!(
        pool.proxy_for_account(account_id),
        Some(pool.proxies[stable_proxy_index(account_id, pool.proxies.len()).unwrap()].as_str())
    );
}

#[test]
fn upstream_client_reuses_cached_default_client() {
    let _guard = crate::test_env_guard();
    let _proxy_guard = EnvGuard::clear(ENV_UPSTREAM_PROXY_URL);
    let _proxy_list_guard = EnvGuard::clear(ENV_PROXY_LIST);

    reload_from_env();
    reset_upstream_client_build_count_for_test();

    let first = upstream_client();
    let after_first = upstream_client_build_count_for_test();
    let second = upstream_client();

    assert_eq!(upstream_client_build_count_for_test(), after_first);
    drop(first);
    drop(second);

    let fresh = fresh_upstream_client_for_account("account-42");
    let another_fresh = fresh_upstream_client_for_account("account-42");
    assert_eq!(upstream_client_build_count_for_test(), after_first);
    drop(fresh);
    drop(another_fresh);
}

#[test]
fn async_upstream_client_for_account_reuses_cached_default_client() {
    let _guard = crate::test_env_guard();
    let _proxy_guard = EnvGuard::clear(ENV_UPSTREAM_PROXY_URL);
    let _proxy_list_guard = EnvGuard::clear(ENV_PROXY_LIST);

    reload_from_env();
    reset_async_upstream_client_build_count_for_test();

    let first = async_upstream_client_for_account("account-42");
    let after_first = async_upstream_client_build_count_for_test();
    let second = async_upstream_client_for_account("account-42");

    assert_eq!(async_upstream_client_build_count_for_test(), after_first);
    drop(first);
    drop(second);

    let fresh = fresh_async_upstream_client_for_account("account-42");
    let another_fresh = fresh_async_upstream_client_for_account("account-42");
    assert_eq!(async_upstream_client_build_count_for_test(), after_first);
    drop(fresh);
    drop(another_fresh);
}

#[test]
fn upstream_client_for_account_reuses_cached_proxy_pool_retry_client() {
    let _guard = crate::test_env_guard();
    let _global_proxy_guard = EnvGuard::clear(ENV_UPSTREAM_PROXY_URL);
    let _proxy_list_guard = EnvGuard::set(
        ENV_PROXY_LIST,
        "http://pool-a.example:8080,http://pool-b.example:8080",
    );

    reload_from_env();
    reset_upstream_client_build_count_for_test();

    let first = upstream_client_for_account("account-42");
    let after_first = upstream_client_build_count_for_test();
    let second = upstream_client_for_account("account-42");

    assert_eq!(upstream_client_build_count_for_test(), after_first);
    drop(first);
    drop(second);

    let fresh = fresh_upstream_client_for_account("account-42");
    let another_fresh = fresh_upstream_client_for_account("account-42");
    assert_eq!(upstream_client_build_count_for_test(), after_first);
    drop(fresh);
    drop(another_fresh);
}

#[test]
fn account_candidate_client_prepare_reuses_cached_account_clients() {
    let _guard = crate::test_env_guard();
    let _global_proxy_guard = EnvGuard::clear(ENV_UPSTREAM_PROXY_URL);
    let _proxy_list_guard = EnvGuard::clear(ENV_PROXY_LIST);

    reload_from_env();
    let _ = upstream_total_timeout();
    reset_upstream_client_build_count_for_test();
    reset_async_upstream_client_build_count_for_test();

    prepare_upstream_client_for_account("account-42").expect("prepare account client");
    assert_eq!(upstream_client_build_count_for_test(), 1);
    assert_eq!(async_upstream_client_build_count_for_test(), 1);

    prepare_upstream_client_for_account("account-42").expect("prepare account client again");
    let blocking = upstream_client_for_account("account-42");
    let async_client = async_upstream_client_for_account("account-42");
    assert_eq!(upstream_client_build_count_for_test(), 1);
    assert_eq!(async_upstream_client_build_count_for_test(), 1);
    drop(blocking);
    drop(async_client);

    prepare_upstream_client_for_account("account-43").expect("prepare second account client");
    assert_eq!(upstream_client_build_count_for_test(), 2);
    assert_eq!(async_upstream_client_build_count_for_test(), 2);
}

#[test]
fn aggregate_candidate_client_cache_keys_include_id_and_url() {
    let _guard = crate::test_env_guard();
    let _global_proxy_guard = EnvGuard::clear(ENV_UPSTREAM_PROXY_URL);
    let _proxy_list_guard = EnvGuard::clear(ENV_PROXY_LIST);

    reload_from_env();
    let _ = upstream_total_timeout();
    reset_upstream_client_build_count_for_test();

    prepare_upstream_client_for_aggregate_api_candidate("agg-a", "https://agg.example/v1")
        .expect("prepare aggregate client");
    assert_eq!(upstream_client_build_count_for_test(), 1);

    prepare_upstream_client_for_aggregate_api_candidate("agg-a", "https://agg.example/v1")
        .expect("prepare same aggregate client");
    let client = upstream_client_for_aggregate_api_candidate("agg-a", "https://agg.example/v1");
    assert_eq!(upstream_client_build_count_for_test(), 1);
    drop(client);

    prepare_upstream_client_for_aggregate_api_candidate("agg-a", "https://other.example/v1")
        .expect("prepare aggregate client with changed url");
    assert_eq!(upstream_client_build_count_for_test(), 2);

    prepare_upstream_client_for_aggregate_api_candidate("agg-b", "https://agg.example/v1")
        .expect("prepare aggregate client with changed id");
    assert_eq!(upstream_client_build_count_for_test(), 3);
}

#[test]
fn aggregate_candidate_client_key_includes_proxy_profile() {
    let _guard = crate::test_env_guard();
    let _global_proxy_guard = EnvGuard::clear(ENV_UPSTREAM_PROXY_URL);
    let _proxy_list_guard = EnvGuard::clear(ENV_PROXY_LIST);

    reload_from_env();
    let direct_key =
        aggregate_candidate_client_key("agg-a", "https://agg.example/v1").expect("direct key");

    set_upstream_proxy_url(Some("http://127.0.0.1:7890")).expect("set proxy");
    let proxied_key =
        aggregate_candidate_client_key("agg-a", "https://agg.example/v1").expect("proxied key");

    assert_ne!(direct_key, proxied_key);
}

#[test]
fn aggregate_candidate_client_key_bypasses_proxy_for_configured_host() {
    let _guard = crate::test_env_guard();
    let _global_proxy_guard = EnvGuard::set(ENV_UPSTREAM_PROXY_URL, "http://127.0.0.1:7890");
    let _bypass_guard = EnvGuard::set(ENV_UPSTREAM_PROXY_BYPASS_HOSTS, "api.example.test");
    let _proxy_list_guard = EnvGuard::clear(ENV_PROXY_LIST);

    reload_from_env();

    let bypass_key =
        aggregate_candidate_client_key("agg-a", "https://api.example.test/v1").expect("key");
    let proxied_key =
        aggregate_candidate_client_key("agg-a", "https://api.openai.com/v1").expect("key");

    assert_eq!(bypass_key.proxy_profile.as_deref(), None);
    assert_eq!(
        proxied_key.proxy_profile.as_deref(),
        Some("http://127.0.0.1:7890")
    );
}

#[test]
fn async_upstream_client_for_account_reuses_cached_proxy_pool_client() {
    let _guard = crate::test_env_guard();
    let _global_proxy_guard = EnvGuard::clear(ENV_UPSTREAM_PROXY_URL);
    let _proxy_list_guard = EnvGuard::set(
        ENV_PROXY_LIST,
        "http://pool-a.example:8080,http://pool-b.example:8080",
    );

    reload_from_env();
    reset_async_upstream_client_build_count_for_test();

    let first = async_upstream_client_for_account("account-42");
    let after_first = async_upstream_client_build_count_for_test();
    let second = async_upstream_client_for_account("account-42");

    assert_eq!(async_upstream_client_build_count_for_test(), after_first);
    drop(first);
    drop(second);

    let fresh = fresh_async_upstream_client_for_account("account-42");
    let another_fresh = fresh_async_upstream_client_for_account("account-42");
    assert_eq!(async_upstream_client_build_count_for_test(), after_first);
    drop(fresh);
    drop(another_fresh);
}

#[test]
fn aggregate_api_proxy_bypass_is_empty_without_configured_hosts() {
    let _guard = crate::test_env_guard();
    let _bypass_guard = EnvGuard::clear(ENV_UPSTREAM_PROXY_BYPASS_HOSTS);

    reload_from_env();

    assert!(!aggregate_api_should_bypass_upstream_proxy(
        "https://api.minimax.io"
    ));
    assert!(!aggregate_api_should_bypass_upstream_proxy(
        "https://api.minimax.io/v1/models"
    ));
    assert!(!aggregate_api_should_bypass_upstream_proxy(
        "https://chat.minimax.io/v1/responses"
    ));

    assert!(!aggregate_api_should_bypass_upstream_proxy(
        "https://api.openai.com/v1/models"
    ));
    assert!(!aggregate_api_should_bypass_upstream_proxy(
        "https://notminimax.io/v1/models"
    ));
    assert!(!aggregate_api_should_bypass_upstream_proxy("not a url"));
}

#[test]
fn aggregate_api_proxy_bypass_uses_configured_hosts() {
    let _guard = crate::test_env_guard();
    let _bypass_guard = EnvGuard::set(
        ENV_UPSTREAM_PROXY_BYPASS_HOSTS,
        "api.example.test, *.direct.example, https://service.local:8443/path",
    );

    reload_from_env();

    assert!(aggregate_api_should_bypass_upstream_proxy(
        "https://api.example.test/v1/models"
    ));
    assert!(aggregate_api_should_bypass_upstream_proxy(
        "https://chat.direct.example/v1/responses"
    ));
    assert!(aggregate_api_should_bypass_upstream_proxy(
        "https://service.local:9443/v1"
    ));
    assert!(!aggregate_api_should_bypass_upstream_proxy(
        "https://not-api.example.test/v1/models"
    ));
    assert!(!aggregate_api_should_bypass_upstream_proxy(
        "https://direct.example/v1/responses"
    ));
}

#[test]
fn aggregate_api_client_uses_global_proxy_when_no_bypass_host_is_configured() {
    let _guard = crate::test_env_guard();
    let _proxy_guard = EnvGuard::set(ENV_UPSTREAM_PROXY_URL, "http://127.0.0.1:7890");
    let _bypass_guard = EnvGuard::clear(ENV_UPSTREAM_PROXY_BYPASS_HOSTS);
    let _proxy_list_guard = EnvGuard::clear(ENV_PROXY_LIST);

    reload_from_env();
    reset_direct_upstream_client_use_count_for_test();

    let first = upstream_client_for_aggregate_url("https://api.minimax.io");
    let second = upstream_client_for_aggregate_url("https://api.minimax.io/v1/models");

    assert_eq!(direct_upstream_client_use_count_for_test(), 0);
    drop(first);
    drop(second);

    let non_minimax = upstream_client_for_aggregate_url("https://api.openai.com/v1/models");
    assert_eq!(direct_upstream_client_use_count_for_test(), 0);
    drop(non_minimax);
}

#[test]
fn aggregate_api_client_uses_direct_client_for_configured_bypass_host() {
    let _guard = crate::test_env_guard();
    let _proxy_guard = EnvGuard::set(ENV_UPSTREAM_PROXY_URL, "http://127.0.0.1:7890");
    let _bypass_guard = EnvGuard::set(ENV_UPSTREAM_PROXY_BYPASS_HOSTS, "api.example.test");
    let _proxy_list_guard = EnvGuard::clear(ENV_PROXY_LIST);

    reload_from_env();
    reset_direct_upstream_client_use_count_for_test();

    let direct = upstream_client_for_aggregate_url("https://api.example.test/v1/models");
    let after_direct = direct_upstream_client_use_count_for_test();
    let proxied = upstream_client_for_aggregate_url("https://api.openai.com/v1/models");

    assert_eq!(after_direct, 1);
    assert_eq!(direct_upstream_client_use_count_for_test(), after_direct);
    drop(direct);
    drop(proxied);
}

#[test]
fn set_upstream_proxy_bypass_hosts_normalizes_and_updates_env() {
    let _guard = crate::test_env_guard();
    let _bypass_guard = EnvGuard::clear(ENV_UPSTREAM_PROXY_BYPASS_HOSTS);

    let applied = set_upstream_proxy_bypass_hosts(Some(
        " https://API.Example.Test:8443/v1\n*.Direct.Example, api.example.test ",
    ));

    assert_eq!(applied, "api.example.test\n*.direct.example");
    assert_eq!(
        std::env::var(ENV_UPSTREAM_PROXY_BYPASS_HOSTS)
            .ok()
            .as_deref(),
        Some("api.example.test\n*.direct.example")
    );
    assert_eq!(upstream_proxy_bypass_hosts(), applied);
}

/// 函数 `set_upstream_proxy_url_updates_env_and_cache`
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
fn set_upstream_proxy_url_updates_env_and_cache() {
    let _guard = crate::test_env_guard();
    let _guard = EnvGuard::set(ENV_UPSTREAM_PROXY_URL, "");

    let applied = set_upstream_proxy_url(Some("http://127.0.0.1:7890")).expect("set proxy");
    assert_eq!(applied.as_deref(), Some("http://127.0.0.1:7890"));
    assert_eq!(
        std::env::var(ENV_UPSTREAM_PROXY_URL).ok().as_deref(),
        Some("http://127.0.0.1:7890")
    );
    assert_eq!(
        upstream_proxy_url().as_deref(),
        Some("http://127.0.0.1:7890")
    );

    let cleared = set_upstream_proxy_url(None).expect("clear proxy");
    assert!(cleared.is_none());
    assert_eq!(std::env::var(ENV_UPSTREAM_PROXY_URL).ok(), None);
    assert_eq!(upstream_proxy_url(), None);
}

/// 函数 `set_upstream_proxy_url_normalizes_socks_scheme`
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
fn set_upstream_proxy_url_normalizes_socks_scheme() {
    let _guard = crate::test_env_guard();
    let _guard = EnvGuard::set(ENV_UPSTREAM_PROXY_URL, "");

    let applied =
        set_upstream_proxy_url(Some("https://socks5://127.0.0.1:7890")).expect("set proxy");

    assert_eq!(applied.as_deref(), Some("socks5h://127.0.0.1:7890"));
    assert_eq!(
        std::env::var(ENV_UPSTREAM_PROXY_URL).ok().as_deref(),
        Some("socks5h://127.0.0.1:7890")
    );
}

/// 函数 `set_upstream_stream_timeout_ms_updates_env_and_cache`
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
fn set_upstream_stream_timeout_ms_updates_env_and_cache() {
    let _guard = crate::test_env_guard();
    let _guard = EnvGuard::set(ENV_UPSTREAM_STREAM_TIMEOUT_MS, "1800000");

    let applied = set_upstream_stream_timeout_ms(432100);

    assert_eq!(applied, 432100);
    assert_eq!(current_upstream_stream_timeout_ms(), 432100);
    assert_eq!(
        upstream_stream_timeout(),
        Some(Duration::from_millis(432100))
    );
    assert_eq!(
        std::env::var(ENV_UPSTREAM_STREAM_TIMEOUT_MS)
            .ok()
            .as_deref(),
        Some("432100")
    );
}

#[test]
fn set_upstream_total_timeout_ms_updates_env_and_cache() {
    let _guard = crate::test_env_guard();
    let _guard = EnvGuard::set(ENV_UPSTREAM_TOTAL_TIMEOUT_MS, "0");

    let applied = set_upstream_total_timeout_ms(120000);

    assert_eq!(applied, 120000);
    assert_eq!(current_upstream_total_timeout_ms(), 120000);
    assert_eq!(
        upstream_total_timeout(),
        Some(Duration::from_millis(120000))
    );
    assert_eq!(
        std::env::var(ENV_UPSTREAM_TOTAL_TIMEOUT_MS).ok().as_deref(),
        Some("120000")
    );
}

/// 函数 `normalize_model_slug_maps_legacy_gpt_5_4_pro_to_gpt_5_4`
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
fn normalize_model_slug_maps_legacy_gpt_5_4_pro_to_gpt_5_4() {
    let _guard = crate::test_env_guard();

    let actual = normalize_model_slug("gpt-5.4-pro").expect("normalize model");

    assert_eq!(actual, "gpt-5.4");
}

/// 函数 `normalize_model_slug_accepts_auto`
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
fn normalize_model_slug_accepts_auto() {
    let _guard = crate::test_env_guard();

    let actual = normalize_model_slug("auto").expect("normalize model");

    assert_eq!(actual, "auto");
}

#[test]
fn set_model_forward_rules_updates_env_cache_and_matching() {
    let _guard = crate::test_env_guard();
    let _rules_guard = EnvGuard::clear(ENV_MODEL_FORWARD_RULES);

    let applied = set_model_forward_rules("spark*=gpt-5.4-mini\nclaude-sonnet-4*=gpt-5.4")
        .expect("set model forward rules");

    assert_eq!(applied, "spark*=gpt-5.4-mini\nclaude-sonnet-4*=gpt-5.4");
    assert_eq!(current_model_forward_rules(), applied);
    assert_eq!(
        std::env::var(ENV_MODEL_FORWARD_RULES).ok().as_deref(),
        Some(applied.as_str())
    );
    assert_eq!(
        resolve_forwarded_model("spark"),
        Some("gpt-5.4-mini".to_string())
    );
    assert_eq!(
        resolve_forwarded_model("claude-sonnet-4-20250514"),
        Some("gpt-5.4".to_string())
    );
    assert_eq!(resolve_forwarded_model("gpt-5.4"), None);
}

#[test]
fn set_model_forward_rules_preserves_case_while_matching_case_insensitively() {
    let _guard = crate::test_env_guard();
    let _rules_guard = EnvGuard::clear(ENV_MODEL_FORWARD_RULES);

    let applied = set_model_forward_rules("Spark*=GPT-5.4-mini\nClaude-Sonnet-4*=Gemini-2.5-Pro")
        .expect("set mixed-case model forward rules");

    assert_eq!(
        applied,
        "Spark*=GPT-5.4-mini\nClaude-Sonnet-4*=Gemini-2.5-Pro"
    );
    assert_eq!(current_model_forward_rules(), applied);
    assert_eq!(
        std::env::var(ENV_MODEL_FORWARD_RULES).ok().as_deref(),
        Some(applied.as_str())
    );
    assert_eq!(
        resolve_forwarded_model("spark-lite"),
        Some("GPT-5.4-mini".to_string())
    );
    assert_eq!(
        resolve_forwarded_model("claude-sonnet-4-20250514"),
        Some("Gemini-2.5-Pro".to_string())
    );
}

#[test]
fn builtin_model_forward_rule_does_not_remap_codex_long_tail_slug() {
    let _guard = crate::test_env_guard();
    let _rules_guard = EnvGuard::clear(ENV_MODEL_FORWARD_RULES);

    assert_eq!(resolve_forwarded_model("gpt-5.3-codex-spark"), None);
}

#[test]
fn explicit_model_forward_rule_can_remap_codex_long_tail_slug() {
    let _guard = crate::test_env_guard();
    let _rules_guard = EnvGuard::clear(ENV_MODEL_FORWARD_RULES);

    let applied = set_model_forward_rules("gpt-5.3-codex-spark*=gpt-5.4-mini")
        .expect("set explicit spark rule");

    assert_eq!(
        resolve_forwarded_model("gpt-5.3-codex-spark"),
        Some("gpt-5.4-mini".to_string())
    );
    assert_eq!(current_model_forward_rules(), applied);
}

#[test]
fn set_model_forward_rules_rejects_invalid_target_auto() {
    let _guard = crate::test_env_guard();

    let err = set_model_forward_rules("spark*=auto").expect_err("auto target should be rejected");

    assert!(err.contains("target model cannot be auto"));
}

#[test]
fn set_compact_model_forward_rules_keeps_legacy_storage_only() {
    let _guard = crate::test_env_guard();
    let _rules_guard = EnvGuard::clear(ENV_COMPACT_MODEL_FORWARD_RULES);

    let applied = set_compact_model_forward_rules("gpt-5.4=gpt-5.4-openai-compact")
        .expect("set compact model forward rules");

    assert_eq!(applied, "gpt-5.4=gpt-5.4-openai-compact");
    assert_eq!(current_compact_model_forward_rules(), applied);
    assert_eq!(
        std::env::var(ENV_COMPACT_MODEL_FORWARD_RULES)
            .ok()
            .as_deref(),
        Some(applied.as_str())
    );
    assert_eq!(resolve_forwarded_model("gpt-5.4"), None);
}

#[test]
fn compact_api_path_reads_chat_completions_override_from_env() {
    let _guard = crate::test_env_guard();
    let _compact_api_path = EnvGuard::set(ENV_COMPACT_API_PATH, "/v1/chat/completions");

    reload_from_env();

    assert_eq!(current_compact_api_path(), "/v1/chat/completions");
    assert!(compact_api_path_uses_chat_completions());
}

/// 函数 `set_originator_updates_env_and_dynamic_user_agent`
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
fn set_originator_updates_env_and_dynamic_user_agent() {
    let _guard = crate::test_env_guard();
    let _guard = EnvGuard::set(ENV_ORIGINATOR, "codex_cli_rs");

    let applied = set_originator("codex_cli_rs_windows").expect("set originator");

    assert_eq!(applied, "codex_cli_rs_windows");
    assert_eq!(current_originator(), "codex_cli_rs_windows");
    assert_eq!(current_wire_originator(), "codex_cli_rs_windows");
    assert_eq!(
        std::env::var(ENV_ORIGINATOR).ok().as_deref(),
        Some("codex_cli_rs_windows")
    );
    let expected_prefix = format!(
        "codex_cli_rs_windows/{}",
        current_codex_user_agent_version()
    );
    assert!(current_codex_user_agent().contains(expected_prefix.as_str()));
    assert!(current_codex_user_agent().contains("(Ubuntu 22.4.0; x86_64) xterm-256color"));
}

/// 函数 `set_codex_user_agent_version_updates_env_and_user_agent`
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
fn set_codex_user_agent_version_updates_env_and_user_agent() {
    let _guard = crate::test_env_guard();

    let applied = set_codex_user_agent_version("0.102.1").expect("set codex user agent version");

    assert_eq!(applied, "0.102.1");
    assert_eq!(current_codex_user_agent_version(), "0.102.1");
    assert!(current_codex_user_agent().contains("codex_cli_rs/0.102.1"));
}

/// 函数 `set_residency_requirement_updates_env_and_cache`
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
fn set_residency_requirement_updates_env_and_cache() {
    let _guard = crate::test_env_guard();
    let _guard = EnvGuard::clear(ENV_RESIDENCY_REQUIREMENT);

    let applied = set_residency_requirement(Some("us")).expect("set residency requirement");
    assert_eq!(applied.as_deref(), Some("us"));
    assert_eq!(current_residency_requirement().as_deref(), Some("us"));
    assert_eq!(
        std::env::var(ENV_RESIDENCY_REQUIREMENT).ok().as_deref(),
        Some("us")
    );

    let cleared = set_residency_requirement(None).expect("clear residency requirement");
    assert!(cleared.is_none());
    assert_eq!(current_residency_requirement(), None);
    assert_eq!(std::env::var(ENV_RESIDENCY_REQUIREMENT).ok(), None);
}

/// 函数 `set_request_compression_enabled_updates_env_and_cache`
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
fn set_request_compression_enabled_updates_env_and_cache() {
    let _guard = crate::test_env_guard();
    let _guard = EnvGuard::set(ENV_ENABLE_REQUEST_COMPRESSION, "1");

    let applied = set_request_compression_enabled(false);

    assert!(!applied);
    assert!(!request_compression_enabled());
    assert_eq!(
        std::env::var(ENV_ENABLE_REQUEST_COMPRESSION)
            .ok()
            .as_deref(),
        Some("0")
    );

    let reapplied = set_request_compression_enabled(true);
    assert!(reapplied);
    assert!(request_compression_enabled());
    assert_eq!(
        std::env::var(ENV_ENABLE_REQUEST_COMPRESSION)
            .ok()
            .as_deref(),
        Some("1")
    );
}

#[test]
fn candidate_client_cache_evicts_only_the_least_recently_used_entry() {
    let cache = HashMap::from([
        ("oldest".to_string(), 3_u64),
        ("recent".to_string(), 12_u64),
        ("middle".to_string(), 7_u64),
    ]);

    assert_eq!(
        least_recently_used_key(&cache, |last_used| *last_used).as_deref(),
        Some("oldest")
    );
    assert_eq!(cache.len(), 3, "selection must not clear the whole cache");
}
