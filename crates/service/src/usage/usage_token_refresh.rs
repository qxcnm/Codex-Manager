use codexmanager_core::auth::{extract_client_id_claim, extract_token_exp, DEFAULT_CLIENT_ID};
use codexmanager_core::storage::{now_ts, Storage, Token};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use crate::auth_tokens::obtain_api_key;
use crate::usage_http::{
    log_account_data_route, refresh_access_token, refresh_access_token_with_explicit_proxy,
    refresh_token_auth_error_reason_from_message, RefreshTokenAuthErrorReason,
};

pub(crate) const DEFAULT_TOKEN_REFRESH_AHEAD_SECS: i64 = 3600;
pub(crate) const ENV_TOKEN_REFRESH_AHEAD_SECS: &str = "CODEXMANAGER_TOKEN_REFRESH_AHEAD_SECS";

static TOKEN_REFRESH_LOCKS: OnceLock<Mutex<HashMap<String, Arc<Mutex<()>>>>> = OnceLock::new();

/// 函数 `refresh_and_persist_access_token`
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
pub(crate) fn refresh_and_persist_access_token(
    storage: &Storage,
    token: &mut Token,
    issuer: &str,
    client_id: &str,
    refresh_ahead_secs: i64,
) -> Result<(), String> {
    let original_access_token = token.access_token.clone();
    let original_refresh_token = token.refresh_token.clone();
    let refresh_lock = token_refresh_lock_for_account(&token.account_id);
    let _refresh_guard = refresh_lock
        .lock()
        .map_err(|_| "token refresh lock poisoned".to_string())?;

    if let Some(latest) = storage
        .find_token_by_account_id(&token.account_id)
        .map_err(|err| err.to_string())?
    {
        if latest.access_token != original_access_token
            || latest.refresh_token != original_refresh_token
        {
            *token = latest;
            return Ok(());
        }
        *token = latest;
    }

    let refresh_client_id = token_refresh_client_id(token, client_id);
    let proxy_mode = crate::account_proxy::resolve_account_proxy_mode(token.account_id.as_str());
    log_account_data_route(
        "token_refresh",
        token.account_id.as_str(),
        &proxy_mode,
        "refresh_token",
        true,
    );
    let refreshed = match &proxy_mode {
        crate::account_proxy::AccountProxyMode::Disabled => {
            refresh_access_token(issuer, &refresh_client_id, &token.refresh_token)
        }
        crate::account_proxy::AccountProxyMode::Explicit { proxy_url } => {
            refresh_access_token_with_explicit_proxy(
                issuer,
                &refresh_client_id,
                &token.refresh_token,
                proxy_url,
            )
        }
        crate::account_proxy::AccountProxyMode::Invalid { error, .. } => Err(error.clone()),
    };
    let refreshed = match refreshed {
        Ok(refreshed) => refreshed,
        Err(err) => {
            if recover_refresh_race_from_latest_token(
                storage,
                token,
                &original_refresh_token,
                err.as_str(),
            )? {
                return Ok(());
            }
            return Err(err);
        }
    };
    token.access_token = refreshed.access_token;

    if let Some(refresh_token) = refreshed.refresh_token {
        token.refresh_token = refresh_token;
    }

    if let Some(id_token) = refreshed.id_token {
        token.id_token = id_token.clone();
        let exchange_client_id = token_refresh_client_id(token, refresh_client_id.as_str());
        if let Ok(api_key) = obtain_api_key(issuer, &exchange_client_id, &id_token) {
            token.api_key_access_token = Some(api_key);
        }
    }

    token.last_refresh = now_ts();
    storage.insert_token(token).map_err(|err| err.to_string())?;
    let access_exp = extract_token_exp(&token.access_token);
    let next_refresh_at = next_refresh_at_from_token(token, refresh_ahead_secs);
    let _ = storage.update_token_refresh_schedule(&token.account_id, access_exp, next_refresh_at);
    Ok(())
}

pub(crate) fn token_refresh_ahead_secs() -> i64 {
    std::env::var(ENV_TOKEN_REFRESH_AHEAD_SECS)
        .ok()
        .and_then(|value| value.trim().parse::<i64>().ok())
        .filter(|secs| *secs > 0)
        .unwrap_or(DEFAULT_TOKEN_REFRESH_AHEAD_SECS)
}

pub(crate) fn token_refresh_client_id(token: &Token, fallback_client_id: &str) -> String {
    extract_client_id_claim(&token.access_token)
        .or_else(|| extract_client_id_claim(&token.id_token))
        .or_else(|| {
            let fallback = fallback_client_id.trim();
            (!fallback.is_empty()).then(|| fallback.to_string())
        })
        .unwrap_or_else(|| DEFAULT_CLIENT_ID.to_string())
}

fn token_refresh_lock_for_account(account_id: &str) -> Arc<Mutex<()>> {
    let locks = TOKEN_REFRESH_LOCKS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut locks = locks
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    locks
        .entry(account_id.to_string())
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone()
}

fn next_refresh_at_from_token(token: &Token, ahead_secs: i64) -> Option<i64> {
    let access_refresh_at =
        extract_token_exp(&token.access_token).map(|exp| exp.saturating_sub(ahead_secs));
    let refresh_refresh_at =
        extract_token_exp(&token.refresh_token).map(|exp| exp.saturating_sub(ahead_secs));

    match (access_refresh_at, refresh_refresh_at) {
        (Some(access_at), Some(refresh_at)) => Some(access_at.min(refresh_at)),
        (Some(access_at), None) => Some(access_at),
        (None, Some(refresh_at)) => Some(refresh_at),
        (None, None) => None,
    }
}

fn recover_refresh_race_from_latest_token(
    storage: &Storage,
    token: &mut Token,
    original_refresh_token: &str,
    err: &str,
) -> Result<bool, String> {
    if !is_refresh_race_recoverable_error(err) {
        return Ok(false);
    }

    let Some(latest) = storage
        .find_token_by_account_id(&token.account_id)
        .map_err(|err| err.to_string())?
    else {
        return Ok(false);
    };

    if latest.refresh_token.trim().is_empty() || latest.refresh_token == original_refresh_token {
        return Ok(false);
    }

    *token = latest;
    Ok(true)
}

fn is_refresh_race_recoverable_error(err: &str) -> bool {
    matches!(
        refresh_token_auth_error_reason_from_message(err),
        Some(RefreshTokenAuthErrorReason::InvalidGrant | RefreshTokenAuthErrorReason::Reused)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;
    use codexmanager_core::storage::Account;
    use std::ffi::OsString;
    use std::thread;
    use std::time::Duration;
    use tiny_http::{Header, Response, Server, StatusCode as TinyStatusCode};

    fn jwt_with_json(payload_json: &str) -> String {
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(payload_json);
        format!("eyJhbGciOiJIUzI1NiJ9.{payload}.sig")
    }

    struct EnvVarRestore {
        key: &'static str,
        original: Option<OsString>,
    }

    impl EnvVarRestore {
        fn set(key: &'static str, value: &str) -> Self {
            let original = std::env::var_os(key);
            std::env::set_var(key, value);
            Self { key, original }
        }

        fn remove(key: &'static str) -> Self {
            let original = std::env::var_os(key);
            std::env::remove_var(key);
            Self { key, original }
        }
    }

    impl Drop for EnvVarRestore {
        fn drop(&mut self) {
            match self.original.as_ref() {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }

    fn token_with_refresh(account_id: &str, refresh_token: &str) -> Token {
        Token {
            account_id: account_id.to_string(),
            id_token: "id-token".to_string(),
            access_token: "access-token".to_string(),
            refresh_token: refresh_token.to_string(),
            api_key_access_token: None,
            last_refresh: now_ts(),
        }
    }

    fn insert_account(storage: &Storage, account_id: &str) {
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
            .expect("insert account");
    }

    fn start_region_blocked_refresh_server() -> (
        String,
        std::sync::mpsc::Receiver<String>,
        thread::JoinHandle<()>,
    ) {
        let server = Server::http("127.0.0.1:0").expect("start refresh mock server");
        let url = format!("http://{}/oauth/token", server.server_addr());
        let (tx, rx) = std::sync::mpsc::channel();
        let handle = thread::spawn(move || {
            let mut request = server
                .recv_timeout(Duration::from_secs(5))
                .expect("refresh mock timeout")
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
        (url, rx, handle)
    }

    #[test]
    fn recover_refresh_race_uses_latest_token_when_refresh_token_changed() {
        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");
        insert_account(&storage, "acc-race");
        let mut token = token_with_refresh("acc-race", "refresh-old");
        storage.insert_token(&token).expect("insert old token");
        storage
            .insert_token(&token_with_refresh("acc-race", "refresh-new"))
            .expect("insert new token");

        let recovered = recover_refresh_race_from_latest_token(
            &storage,
            &mut token,
            "refresh-old",
            "refresh token failed with status 400 Bad Request: invalid_grant",
        )
        .expect("recover");

        assert!(recovered);
        assert_eq!(token.refresh_token, "refresh-new");
    }

    #[test]
    fn recover_refresh_race_keeps_error_when_refresh_token_unchanged() {
        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");
        insert_account(&storage, "acc-no-race");
        let mut token = token_with_refresh("acc-no-race", "refresh-old");
        storage.insert_token(&token).expect("insert token");

        let recovered = recover_refresh_race_from_latest_token(
            &storage,
            &mut token,
            "refresh-old",
            "refresh token failed with status 400 Bad Request: invalid_grant",
        )
        .expect("recover");

        assert!(!recovered);
        assert_eq!(token.refresh_token, "refresh-old");
    }

    #[test]
    fn refresh_and_persist_region_blocked_mock_suspends_account() {
        let _guard = crate::test_env_guard();
        let _ = crate::usage_http::usage_http_client();
        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");
        insert_account(&storage, "acc-region-blocked");
        let mut token = token_with_refresh("acc-region-blocked", "refresh-old");
        storage.insert_token(&token).expect("insert token");
        let (url, rx, handle) = start_region_blocked_refresh_server();
        let _restore = EnvVarRestore::set("CODEX_REFRESH_TOKEN_URL_OVERRIDE", &url);

        let err = refresh_and_persist_access_token(
            &storage,
            &mut token,
            "https://auth.openai.com",
            "client-id",
            token_refresh_ahead_secs(),
        )
        .expect_err("region blocked refresh should fail");
        let body = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("receive refresh request body");
        handle.join().expect("join refresh mock server");

        assert!(body.contains("refresh_token=refresh-old"));
        assert!(err.contains("auth_error=unsupported_country_region_territory"));
        assert!(
            crate::account_status::mark_account_unavailable_for_auth_error(
                &storage,
                "acc-region-blocked",
                &err,
            )
        );
        let account = storage
            .find_account_by_id("acc-region-blocked")
            .expect("find account")
            .expect("account exists");
        assert_eq!(account.status, "unavailable");
        let reasons = storage
            .latest_account_status_reasons(&["acc-region-blocked".to_string()])
            .expect("load reasons");
        assert_eq!(
            reasons.get("acc-region-blocked").map(String::as_str),
            Some(crate::account_status::REFRESH_TOKEN_REGION_BLOCKED_REASON)
        );
        let stored = storage
            .find_token_by_account_id("acc-region-blocked")
            .expect("load token")
            .expect("token exists");
        assert_eq!(stored.refresh_token, "refresh-old");
    }

    #[test]
    fn token_refresh_ahead_secs_defaults_to_one_hour() {
        let _guard = crate::test_env_guard();
        let _restore = EnvVarRestore::remove(ENV_TOKEN_REFRESH_AHEAD_SECS);

        assert_eq!(token_refresh_ahead_secs(), DEFAULT_TOKEN_REFRESH_AHEAD_SECS);
    }

    #[test]
    fn token_refresh_ahead_secs_reads_positive_env() {
        let _guard = crate::test_env_guard();
        let _restore = EnvVarRestore::set(ENV_TOKEN_REFRESH_AHEAD_SECS, "1800");

        assert_eq!(token_refresh_ahead_secs(), 1800);
    }

    #[test]
    fn token_refresh_ahead_secs_ignores_invalid_env() {
        let _guard = crate::test_env_guard();
        let _restore = EnvVarRestore::set(ENV_TOKEN_REFRESH_AHEAD_SECS, "0");

        assert_eq!(token_refresh_ahead_secs(), DEFAULT_TOKEN_REFRESH_AHEAD_SECS);
    }

    #[test]
    fn token_refresh_client_id_prefers_access_token_claim() {
        let token = Token {
            account_id: "acc-client-id".to_string(),
            id_token: jwt_with_json(r#"{"sub":"user-1","client_id":"client-from-id"}"#),
            access_token: jwt_with_json(r#"{"sub":"user-1","client_id":"client-from-access"}"#),
            refresh_token: "refresh-token".to_string(),
            api_key_access_token: None,
            last_refresh: now_ts(),
        };

        assert_eq!(
            token_refresh_client_id(&token, "client-from-env"),
            "client-from-access"
        );
    }

    #[test]
    fn token_refresh_client_id_falls_back_to_id_token_then_env() {
        let token = Token {
            account_id: "acc-client-id-fallback".to_string(),
            id_token: jwt_with_json(r#"{"sub":"user-1","client_id":"client-from-id"}"#),
            access_token: jwt_with_json(r#"{"sub":"user-1"}"#),
            refresh_token: "refresh-token".to_string(),
            api_key_access_token: None,
            last_refresh: now_ts(),
        };
        assert_eq!(
            token_refresh_client_id(&token, "client-from-env"),
            "client-from-id"
        );

        let token_without_claim = Token {
            id_token: jwt_with_json(r#"{"sub":"user-1"}"#),
            ..token
        };
        assert_eq!(
            token_refresh_client_id(&token_without_claim, "client-from-env"),
            "client-from-env"
        );
    }
}
