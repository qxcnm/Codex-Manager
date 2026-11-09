//! Independently authorized Excel/BPS routing. Codex credentials are never replaced.
use std::{
    collections::{BTreeSet, HashMap},
    sync::{Mutex, OnceLock},
    time::Duration,
};

use base64::Engine;
use codexmanager_core::{
    auth::{extract_chatgpt_account_id, extract_chatgpt_user_id},
    storage::{now_ts, Account, AccountBpsSettings, Event, Token},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::remote_storage::AccountStorage;
use crate::{auth_tokens::run_auth_storage, storage_helpers::open_storage};

const BPS_CLIENT_ID: &str = "app_fnr0pYvVwwFDocDumLG3H2Bp";
const AUTH_BASE: &str = "https://auth.openai.com";
const BPS_REDIRECT: &str = "https://auth.openai.com/basispoints/deviceauth/callback";
const LOGIN_TTL: i64 = 900;
const MAX_SESSIONS: usize = 64;
const DEVICEAUTH_DISABLED_MESSAGE: &str = "BPS 设备代码登录未启用（deviceauth_disabled）。请登录对应的 ChatGPT 个人账号，在“设置 → 安全”中启用“设备代码登录（Device code login）”，然后重新发起 BPS 授权。";

pub(crate) struct BpsCredentials {
    pub access_token: String,
    pub account_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BpsUpdate {
    account_ids: Vec<String>,
    enabled: bool,
}

#[derive(Serialize)]
pub(crate) struct BpsUpdateResult {
    updated: usize,
}

pub(crate) fn update(params: BpsUpdate) -> Result<BpsUpdateResult, String> {
    if params.account_ids.is_empty() || params.account_ids.iter().any(|id| id.trim().is_empty()) {
        return Err("missing accountIds".into());
    }
    let ids = params
        .account_ids
        .iter()
        .map(|id| id.trim().to_string())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let db = open_storage().ok_or("storage unavailable")?;
    let storage = AccountStorage::new(&db);
    let updated = storage
        .set_account_bps_enabled(&ids, params.enabled)
        .map_err(|e| e.to_string())?;
    for id in ids {
        let _ = storage.insert_event(&Event {
            account_id: Some(id),
            event_type: "account_bps_update".into(),
            message: format!("enabled={}", params.enabled),
            created_at: now_ts(),
        });
    }
    Ok(BpsUpdateResult { updated })
}

pub(crate) fn enabled_for_account(account_id: &str) -> Result<bool, String> {
    let db = open_storage().ok_or("storage unavailable")?;
    AccountStorage::new(&db)
        .find_account_bps_settings(account_id)
        .map(|s| s.is_some_and(|v| v.enabled))
        .map_err(|e| e.to_string())
}

pub(crate) fn any_enabled() -> Result<bool, String> {
    let db = open_storage().ok_or("storage unavailable")?;
    AccountStorage::new(&db)
        .any_account_bps_enabled()
        .map_err(|e| e.to_string())
}

pub(crate) async fn credentials_for_account(
    account_id: &str,
) -> Result<Option<BpsCredentials>, String> {
    let id = account_id.to_owned();
    run_auth_storage(move || {
        let db = open_storage().ok_or("storage unavailable")?;
        let storage = AccountStorage::new(&db);
        let Some(settings) = storage
            .find_account_bps_settings(&id)
            .map_err(|e| e.to_string())?
            .filter(|s| s.enabled)
        else {
            return Ok(None);
        };
        let account = storage
            .find_account_by_id(&id)
            .map_err(|e| e.to_string())?
            .ok_or("account not found")?;
        if !settings.authorized_at(now_ts())
            || expected_account_id(&account) != settings.upstream_account_id.as_deref()
        {
            return Err(
                "BPS authorization is missing or expired; authorize this account again".into(),
            );
        }
        Ok(Some(BpsCredentials {
            access_token: settings.access_token.unwrap_or_default(),
            account_id: settings.upstream_account_id.unwrap_or_default(),
        }))
    })
    .await
}

fn expected_account_id(account: &Account) -> Option<&str> {
    account
        .chatgpt_account_id
        .as_deref()
        .filter(|v| !v.is_empty())
        .or(account.workspace_id.as_deref().filter(|v| !v.is_empty()))
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BpsLoginStart {
    login_id: String,
    user_code: String,
    verification_uri: String,
    interval: u64,
    expires_at: i64,
}

#[derive(Clone, Serialize)]
pub(crate) struct BpsLoginStatus {
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}
struct LoginSession {
    account_id: String,
    expires_at: i64,
    result: BpsLoginStatus,
    prompt: Option<BpsLoginStart>,
}
struct StartingSessionGuard(Option<String>);
impl Drop for StartingSessionGuard {
    fn drop(&mut self) {
        if let Some(id) = self.0.take() {
            if let Ok(mut sessions) = sessions().lock() {
                sessions.remove(&id);
            }
        }
    }
}
fn sessions() -> &'static Mutex<HashMap<String, LoginSession>> {
    static SESSIONS: OnceLock<Mutex<HashMap<String, LoginSession>>> = OnceLock::new();
    SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn login_status(account_id: &str, login_id: &str) -> Result<BpsLoginStatus, String> {
    let mut sessions = sessions()
        .lock()
        .map_err(|_| "BPS login state unavailable")?;
    let session = sessions
        .get_mut(login_id)
        .filter(|s| s.account_id == account_id)
        .ok_or("BPS login session not found; start authorization again")?;
    if session.expires_at <= now_ts() && session.result.status == "pending" {
        session.result = failed("BPS authorization timed out; start authorization again");
    }
    Ok(session.result.clone())
}

fn failed(error: &str) -> BpsLoginStatus {
    BpsLoginStatus {
        status: "failed".into(),
        error: Some(error.into()),
    }
}

pub(crate) async fn login_start(account_id: &str) -> Result<BpsLoginStart, String> {
    let id = account_id.trim().to_owned();
    if id.is_empty() {
        return Err("missing accountId".into());
    }
    {
        let sessions = sessions()
            .lock()
            .map_err(|_| "BPS login state unavailable")?;
        if let Some(prompt) = sessions
            .values()
            .find(|s| s.account_id == id && s.expires_at > now_ts() && s.result.status == "pending")
            .and_then(|s| s.prompt.clone())
        {
            return Ok(prompt);
        }
    }
    let account_id_for_storage = id.clone();
    let proxy = run_auth_storage(move || {
        let db = open_storage().ok_or("storage unavailable")?;
        let storage = AccountStorage::new(&db);
        let account = storage
            .find_account_by_id(&account_id_for_storage)
            .map_err(|e| e.to_string())?
            .ok_or("account not found")?;
        if expected_account_id(&account).is_none() {
            return Err("This account has no ChatGPT identity for BPS authorization".into());
        }
        crate::gateway::account_test_proxy_url_for_account(&account_id_for_storage)
            .map_err(|_| "BPS authorization proxy is unavailable".into())
    })
    .await?;
    let mut builder = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(30));
    if let Some(proxy) = proxy {
        builder = builder
            .proxy(reqwest::Proxy::all(proxy).map_err(|_| "BPS authorization proxy is invalid")?);
    }
    let client = builder
        .build()
        .map_err(|_| "BPS authorization client unavailable")?;
    let login_id = codexmanager_core::auth::generate_state();
    {
        let mut sessions = sessions()
            .lock()
            .map_err(|_| "BPS login state unavailable")?;
        sessions.retain(|_, s| s.expires_at + 300 > now_ts());
        if sessions.len() >= MAX_SESSIONS {
            return Err("Too many BPS authorization sessions; try again later".into());
        }
        if let Some(session) = sessions
            .values()
            .find(|s| s.account_id == id && s.expires_at > now_ts() && s.result.status == "pending")
        {
            return session
                .prompt
                .clone()
                .ok_or_else(|| "BPS authorization is starting; try again shortly".into());
        }
        sessions.insert(
            login_id.clone(),
            LoginSession {
                account_id: id.clone(),
                expires_at: now_ts() + LOGIN_TTL,
                result: BpsLoginStatus {
                    status: "pending".into(),
                    error: None,
                },
                prompt: None,
            },
        );
    }
    let mut starting = StartingSessionGuard(Some(login_id.clone()));
    let prompt = request_device_code(&client, AUTH_BASE).await?;
    let expires_at = now_ts() + LOGIN_TTL;
    let result = BpsLoginStart {
        login_id: login_id.clone(),
        user_code: prompt.user_code.clone(),
        verification_uri: format!(
            "{AUTH_BASE}/api/accounts/deviceauth/authorize?client_id={BPS_CLIENT_ID}"
        ),
        interval: prompt.interval,
        expires_at,
    };
    if let Ok(mut sessions) = sessions().lock() {
        if let Some(session) = sessions.get_mut(&login_id) {
            session.expires_at = expires_at;
            session.prompt = Some(result.clone());
        }
    }
    starting.0 = None;
    tokio::spawn(async move {
        let result = tokio::time::timeout(
            Duration::from_secs(LOGIN_TTL as u64),
            complete_login(client, AUTH_BASE, id.clone(), prompt, expires_at),
        )
        .await;
        let status = match result {
            Ok(Ok(())) => BpsLoginStatus {
                status: "completed".into(),
                error: None,
            },
            Ok(Err(error)) => failed(&error),
            Err(_) => failed("BPS authorization timed out; start authorization again"),
        };
        if let Ok(mut sessions) = sessions().lock() {
            if let Some(session) = sessions.get_mut(&login_id) {
                session.result = status;
            }
        }
    });
    Ok(result)
}

struct DeviceCode {
    device_auth_id: String,
    user_code: String,
    interval: u64,
}

async fn json_response(mut response: reqwest::Response) -> Result<Value, String> {
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "BPS authorization response interrupted")?
    {
        if bytes.len() + chunk.len() > 1024 * 1024 {
            return Err("BPS authorization response is too large".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|_| "BPS authorization response is invalid".into())
}

fn authorization_error_code(value: &Value) -> &str {
    [
        value.get("error_code"),
        value.pointer("/error/error_code"),
        value.pointer("/error/code"),
        value.get("code"),
        value.get("error"),
    ]
    .into_iter()
    .flatten()
    .filter_map(Value::as_str)
    .find(|code| !code.is_empty())
    .unwrap_or("")
}

async fn authorization_response(
    response: reqwest::Response,
) -> Result<(reqwest::StatusCode, Value), String> {
    let status = response.status();
    let value = match json_response(response).await {
        Ok(value) => value,
        Err(error) if status.is_success() => return Err(error),
        Err(_) => Value::Null,
    };
    // Only a recognized code selects this local message. Never expose the
    // upstream description/body, which can contain account or token details.
    if authorization_error_code(&value) == "deviceauth_disabled" {
        return Err(DEVICEAUTH_DISABLED_MESSAGE.into());
    }
    Ok((status, value))
}

async fn request_device_code(client: &reqwest::Client, base: &str) -> Result<DeviceCode, String> {
    let response = client
        .post(format!("{base}/api/accounts/deviceauth/usercode"))
        .json(&json!({"client_id": BPS_CLIENT_ID}))
        .send()
        .await
        .map_err(|_| "BPS authorization connection failed")?;
    let (status, value) = authorization_response(response).await?;
    if !status.is_success() {
        return Err(format!(
            "BPS authorization request rejected (HTTP {})",
            status.as_u16()
        ));
    }
    let device_auth_id = required_string(&value, "device_auth_id")?;
    let user_code = value
        .get("user_code")
        .or_else(|| value.get("usercode"))
        .and_then(Value::as_str)
        .filter(|v| !v.is_empty())
        .ok_or("BPS authorization code is missing")?
        .to_string();
    let interval = match value.get("interval") {
        None | Some(Value::Null) => 5,
        Some(value) => value
            .as_u64()
            .or_else(|| value.as_str()?.parse().ok())
            .filter(|v| (1..=300).contains(v))
            .ok_or("BPS authorization polling interval is invalid")?,
    };
    Ok(DeviceCode {
        device_auth_id,
        user_code,
        interval,
    })
}

fn required_string(value: &Value, key: &str) -> Result<String, String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|v| !v.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| format!("BPS authorization response missing {key}"))
}

async fn complete_login(
    client: reqwest::Client,
    base: &str,
    account_id: String,
    prompt: DeviceCode,
    login_expires_at: i64,
) -> Result<(), String> {
    let tokens = poll_and_exchange(&client, base, prompt).await?;
    run_auth_storage(move || {
        if now_ts() >= login_expires_at {
            return Err("BPS authorization timed out; start authorization again".into());
        }
        let db = open_storage().ok_or("storage unavailable")?;
        let storage = AccountStorage::new(&db);
        let account = storage
            .find_account_by_id(&account_id)
            .map_err(|e| e.to_string())?
            .ok_or("account not found")?;
        let original_token = storage
            .find_token_by_account_id(&account_id)
            .map_err(|e| e.to_string())?;
        let settings = validate_credentials(&account, original_token.as_ref(), &tokens, now_ts())?;
        storage
            .save_account_bps_credentials(&settings)
            .map_err(|e| e.to_string())?;
        let _ = storage.insert_event(&Event {
            account_id: Some(account_id),
            event_type: "account_bps_authorized".into(),
            message: "Independent BPS authorization saved; generation not verified".into(),
            created_at: now_ts(),
        });
        Ok(())
    })
    .await
}

async fn poll_and_exchange(
    client: &reqwest::Client,
    base: &str,
    prompt: DeviceCode,
) -> Result<Value, String> {
    let mut interval = prompt.interval;
    let grant = loop {
        tokio::time::sleep(Duration::from_secs(interval)).await;
        let response = client.post(format!("{base}/api/accounts/deviceauth/token")).json(&json!({"client_id": BPS_CLIENT_ID, "device_auth_id": prompt.device_auth_id, "user_code": prompt.user_code})).send().await.map_err(|_| "BPS authorization polling failed")?;
        let (status, value) = authorization_response(response).await?;
        if status.is_success() {
            break value;
        }
        let code = authorization_error_code(&value);
        if code == "slow_down" {
            interval = interval.saturating_add(5);
            continue;
        }
        if matches!(
            code,
            "authorization_pending" | "deviceauth_authorization_pending"
        ) {
            continue;
        }
        return Err(format!(
            "BPS authorization was rejected (HTTP {}); start authorization again",
            status.as_u16()
        ));
    };
    let code = required_string(&grant, "authorization_code")?;
    let verifier = required_string(&grant, "code_verifier")?;
    let challenge = required_string(&grant, "code_challenge")?;
    validate_pkce(&verifier, &challenge)?;
    let response = client
        .post(format!("{base}/oauth/token?unified=true"))
        .form(&[
            ("grant_type", "authorization_code"),
            ("client_id", BPS_CLIENT_ID),
            ("code", &code),
            ("code_verifier", &verifier),
            ("redirect_uri", BPS_REDIRECT),
        ])
        .send()
        .await
        .map_err(|_| "BPS token exchange connection failed")?;
    let (status, tokens) = authorization_response(response).await?;
    if !status.is_success() {
        return Err(format!(
            "BPS token exchange rejected (HTTP {})",
            status.as_u16()
        ));
    }
    if !tokens
        .get("token_type")
        .and_then(Value::as_str)
        .is_some_and(|v| v.eq_ignore_ascii_case("Bearer"))
    {
        return Err("BPS authorization returned an unsupported token type".into());
    }
    Ok(tokens)
}

fn validate_pkce(verifier: &str, challenge: &str) -> Result<(), String> {
    if !(43..=128).contains(&verifier.len())
        || !verifier
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-._~".contains(&b))
        || base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(Sha256::digest(verifier.as_bytes()))
            != challenge
    {
        return Err("BPS authorization PKCE verification failed".into());
    }
    Ok(())
}

fn validate_credentials(
    account: &Account,
    original: Option<&Token>,
    tokens: &Value,
    now: i64,
) -> Result<AccountBpsSettings, String> {
    if !tokens
        .get("token_type")
        .and_then(Value::as_str)
        .is_some_and(|v| v.eq_ignore_ascii_case("Bearer"))
    {
        return Err("BPS authorization returned an unsupported token type".into());
    }
    let access_token = required_string(tokens, "access_token")?;
    let payload = access_token
        .split('.')
        .nth(1)
        .and_then(|v| {
            base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(v)
                .ok()
        })
        .and_then(|v| serde_json::from_slice::<Value>(&v).ok())
        .ok_or("BPS authorization returned an unverifiable account identity")?;
    if payload.get("client_id").and_then(Value::as_str) != Some(BPS_CLIENT_ID) {
        return Err("BPS authorization returned credentials for a different application".into());
    }
    let upstream_id = extract_chatgpt_account_id(&access_token)
        .ok_or("BPS authorization returned no ChatGPT account identity")?;
    if expected_account_id(account) != Some(upstream_id.as_str()) {
        return Err("BPS authorization belongs to a different ChatGPT account; sign in to the selected account".into());
    }
    if let Some(expected_user) = original.and_then(|t| {
        extract_chatgpt_user_id(&t.access_token).or_else(|| extract_chatgpt_user_id(&t.id_token))
    }) {
        if extract_chatgpt_user_id(&access_token).as_deref() != Some(expected_user.as_str()) {
            return Err("BPS authorization belongs to a different ChatGPT user".into());
        }
    }
    let jwt_expiry = payload
        .get("exp")
        .and_then(Value::as_i64)
        .ok_or("BPS authorization expiry is missing")?;
    let expires_at = tokens
        .get("expires_in")
        .and_then(Value::as_i64)
        .filter(|seconds| *seconds > 0)
        .and_then(|seconds| now.checked_add(seconds))
        .map(|expiry| expiry.min(jwt_expiry))
        .unwrap_or(jwt_expiry);
    if expires_at <= now {
        return Err("BPS authorization has expired; authorize again".into());
    }
    Ok(AccountBpsSettings {
        account_id: account.id.clone(),
        enabled: false,
        access_token: Some(access_token),
        refresh_token: tokens
            .get("refresh_token")
            .and_then(Value::as_str)
            .map(str::to_owned),
        upstream_account_id: Some(upstream_id),
        expires_at: Some(expires_at),
        updated_at: now,
    })
}

#[cfg(test)]
#[path = "bps_tests.rs"]
mod tests;
