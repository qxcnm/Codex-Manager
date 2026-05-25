use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use codexmanager_core::storage::{Account, Storage, Token};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use crate::app_storage::resolve_db_path_with_legacy_migration;

const SWITCH_WARNING: &str = "已有 Codex / IDE 会话可能需要重开后才会使用新账号";

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs() as i64)
        .unwrap_or(0)
}

fn now_millis() -> i128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_millis() as i128)
        .unwrap_or(0)
}

fn civil_from_unix_days(days_since_epoch: u64) -> (i128, u32, u32) {
    let z = days_since_epoch as i128 + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    let year = year + if month <= 2 { 1 } else { 0 };
    (year, month as u32, day as u32)
}

fn utc_now_rfc3339_millis() -> String {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let total_seconds = duration.as_secs();
    let millis = duration.subsec_millis();
    let days = total_seconds / 86_400;
    let seconds_of_day = total_seconds % 86_400;
    let hour = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    let second = seconds_of_day % 60;
    let (year, month, day) = civil_from_unix_days(days);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}Z")
}

fn sha256_hex(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn user_home_dir() -> Result<PathBuf, String> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
        .ok_or_else(|| "无法定位用户主目录".to_string())
}

fn live_auth_path() -> Result<PathBuf, String> {
    Ok(user_home_dir()?.join(".codex").join("auth.json"))
}

fn codex_dir() -> Result<PathBuf, String> {
    Ok(user_home_dir()?.join(".codex"))
}

fn atomic_write_text(path: &Path, content: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("create dir failed ({}): {err}", parent.display()))?;
    }
    let tmp = path.with_file_name(format!(
        ".auth.json.tmp.{}.{}",
        std::process::id(),
        now_millis()
    ));
    fs::write(&tmp, content)
        .map_err(|err| format!("write temp auth failed ({}): {err}", tmp.display()))?;
    match fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(rename_err) => {
            fs::copy(&tmp, path).map_err(|copy_err| {
                format!(
                    "replace auth failed (rename: {rename_err}; copy {} -> {}: {copy_err})",
                    tmp.display(),
                    path.display()
                )
            })?;
            let _ = fs::remove_file(&tmp);
            Ok(())
        }
    }
}

fn backup_live_auth_if_exists(path: &Path) -> Result<Option<PathBuf>, String> {
    if !path.exists() {
        return Ok(None);
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("create auth dir failed ({}): {err}", parent.display()))?;
    }
    let backup = path.with_file_name(format!("auth.json.bak.{}", now_millis()));
    fs::copy(path, &backup).map_err(|err| {
        format!(
            "backup live auth failed ({} -> {}): {err}",
            path.display(),
            backup.display()
        )
    })?;
    Ok(Some(backup))
}

fn sorted_json_value(value: &Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().map(sorted_json_value).collect()),
        Value::Object(map) => {
            let mut entries = map.iter().collect::<Vec<_>>();
            entries.sort_by(|left, right| left.0.cmp(right.0));
            let mut out = Map::new();
            for (key, item) in entries {
                out.insert(key.clone(), sorted_json_value(item));
            }
            Value::Object(out)
        }
        _ => value.clone(),
    }
}

fn canonical_json_string(value: &Value) -> String {
    serde_json::to_string(&sorted_json_value(value)).unwrap_or_else(|_| value.to_string())
}

fn fingerprint_from_auth_content(content: &str) -> String {
    match serde_json::from_str::<Value>(content) {
        Ok(value) => sha256_hex(&canonical_json_string(&value)),
        Err(_) => sha256_hex(content),
    }
}

fn normalized_key_for_volatile_check(key: &str) -> String {
    key.replace('_', "").to_ascii_lowercase()
}

fn is_volatile_auth_key(key: &str) -> bool {
    matches!(
        normalized_key_for_volatile_check(key).as_str(),
        "accesstoken"
            | "refreshtoken"
            | "idtoken"
            | "expiresat"
            | "expiresin"
            | "lastrefresh"
            | "tokentype"
            | "devicetoken"
            | "sessiontoken"
            | "authorization"
    )
}

fn strip_volatile_auth_fields(value: &Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().map(strip_volatile_auth_fields).collect()),
        Value::Object(map) => {
            let mut out = Map::new();
            for (key, item) in map {
                if is_volatile_auth_key(key) {
                    continue;
                }
                out.insert(key.clone(), strip_volatile_auth_fields(item));
            }
            Value::Object(out)
        }
        _ => value.clone(),
    }
}

fn stable_fingerprint_from_auth_content(content: &str) -> Option<String> {
    let value = serde_json::from_str::<Value>(content).ok()?;
    let root = value.as_object()?;
    if let Some(account_id) = root
        .get("tokens")
        .and_then(Value::as_object)
        .and_then(|tokens| tokens.get("account_id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let stable = json!({
            "auth_mode": root.get("auth_mode").cloned().unwrap_or(Value::Null),
            "account_id": account_id
        });
        return Some(sha256_hex(&canonical_json_string(&stable)));
    }
    if let Some(api_key) = root
        .get("OPENAI_API_KEY")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let stable = json!({
            "auth_mode": root.get("auth_mode").cloned().unwrap_or(Value::Null),
            "OPENAI_API_KEY": api_key
        });
        return Some(sha256_hex(&canonical_json_string(&stable)));
    }
    Some(sha256_hex(&canonical_json_string(
        &strip_volatile_auth_fields(&value),
    )))
}

fn normalize_auth_content_for_runtime(content: &str, live_content: Option<&str>) -> String {
    let target = match serde_json::from_str::<Value>(content) {
        Ok(Value::Object(map)) => map,
        _ => return content.to_string(),
    };
    let live = live_content
        .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
        .and_then(|value| match value {
            Value::Object(map) => Some(map),
            _ => None,
        });
    let live_meta = live.as_ref().and_then(|map| map.get("meta").cloned());

    let mut merged = Map::new();
    if let Some(live_map) = live {
        for (key, value) in live_map {
            if key == "tokens" || key == "meta" {
                continue;
            }
            merged.insert(key, value);
        }
    }
    for (key, value) in &target {
        if key == "tokens" || key == "meta" {
            continue;
        }
        merged.insert(key.clone(), value.clone());
    }
    if let Some(tokens) = target.get("tokens") {
        merged.insert("tokens".to_string(), tokens.clone());
    }
    if let Some(meta) = target.get("meta") {
        merged.insert("meta".to_string(), meta.clone());
    } else if let Some(meta) = live_meta {
        merged.insert("meta".to_string(), meta);
    }
    if !merged.contains_key("OPENAI_API_KEY") {
        merged.insert("OPENAI_API_KEY".to_string(), Value::Null);
    }
    let refresh_missing = merged
        .get("last_refresh")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default()
        .is_empty();
    if refresh_missing {
        merged.insert(
            "last_refresh".to_string(),
            Value::String(utc_now_rfc3339_millis()),
        );
    }

    serde_json::to_string_pretty(&Value::Object(merged)).unwrap_or_else(|_| content.to_string())
}

fn build_account_pool_auth_content(account: &Account, token: &Token) -> Result<String, String> {
    let payload = json!({
        "tokens": {
            "access_token": token.access_token.clone(),
            "id_token": token.id_token.clone(),
            "refresh_token": token.refresh_token.clone(),
            "account_id": account.id.clone()
        },
        "meta": {
            "label": account.label.clone(),
            "issuer": account.issuer.clone(),
            "workspace_id": account.workspace_id.clone(),
            "chatgpt_account_id": account.chatgpt_account_id.clone(),
            "exported_at": now_secs()
        }
    });
    serde_json::to_string_pretty(&payload)
        .map_err(|err| format!("serialize account auth failed: {err}"))
}

fn token_string<'a>(tokens: &'a Map<String, Value>, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| tokens.get(*key).and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn auth_value_matches_account_token(value: &Value, account_id: &str, token: &Token) -> bool {
    let Some(tokens) = value.get("tokens").and_then(Value::as_object) else {
        return false;
    };
    if token_string(tokens, &["account_id", "accountId"]).is_some_and(|value| value == account_id)
    {
        return true;
    }
    let token_matches = |keys: &[&str], expected: &str| {
        let expected = expected.trim();
        !expected.is_empty() && token_string(tokens, keys).is_some_and(|value| value == expected)
    };
    token_matches(&["refresh_token", "refreshToken"], &token.refresh_token)
        || token_matches(&["access_token", "accessToken"], &token.access_token)
        || token_matches(&["id_token", "idToken"], &token.id_token)
}

fn open_account_pool_storage(app: &tauri::AppHandle) -> Result<Storage, String> {
    let db_path = resolve_db_path_with_legacy_migration(app)?;
    Storage::open(db_path).map_err(|err| format!("open account storage failed: {err}"))
}

fn active_account_pool_id(
    storage: &Storage,
    live_raw: Option<&str>,
) -> Result<Option<String>, String> {
    let Some(live_raw) = live_raw else {
        return Ok(None);
    };
    let live_value = serde_json::from_str::<Value>(live_raw).ok();
    let live_fingerprint = Some(fingerprint_from_auth_content(live_raw));
    let live_stable_fingerprint = stable_fingerprint_from_auth_content(live_raw);
    let accounts = storage
        .list_accounts()
        .map_err(|err| format!("list accounts failed: {err}"))?;
    for account in accounts {
        let Some(token) = storage
            .find_token_by_account_id(&account.id)
            .map_err(|err| format!("load account token failed: {err}"))?
        else {
            continue;
        };
        if live_value
            .as_ref()
            .is_some_and(|value| auth_value_matches_account_token(value, &account.id, &token))
        {
            return Ok(Some(account.id));
        }
        let auth_content = build_account_pool_auth_content(&account, &token)?;
        let full_matches = live_fingerprint
            .as_ref()
            .is_some_and(|fingerprint| *fingerprint == fingerprint_from_auth_content(&auth_content));
        let stable_matches = live_stable_fingerprint.as_ref().is_some_and(|fingerprint| {
            stable_fingerprint_from_auth_content(&auth_content).as_ref() == Some(fingerprint)
        });
        if full_matches || stable_matches {
            return Ok(Some(account.id));
        }
    }
    Ok(None)
}

fn account_pool_local_status_value(app: &tauri::AppHandle) -> Result<Value, String> {
    let live_path = live_auth_path()?;
    let live_auth_present = live_path.exists();
    let live_raw = if live_auth_present {
        fs::read_to_string(&live_path).ok()
    } else {
        None
    };
    let storage = open_account_pool_storage(app)?;
    let active_account_id = active_account_pool_id(&storage, live_raw.as_deref())?;
    Ok(json!({
        "activeAccountId": active_account_id,
        "liveAuthPresent": live_auth_present,
        "liveAuthPath": live_path.to_string_lossy().to_string()
    }))
}

#[tauri::command]
pub async fn codex_local_account_pool_status(app: tauri::AppHandle) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || account_pool_local_status_value(&app))
        .await
        .map_err(|err| format!("codex_local_account_pool_status task failed: {err}"))?
}

#[tauri::command]
pub async fn codex_local_account_pool_switch(
    app: tauri::AppHandle,
    account_id: String,
) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let account_id = account_id.trim().to_string();
        if account_id.is_empty() {
            return Err("缺少账号 ID".to_string());
        }
        let storage = open_account_pool_storage(&app)?;
        let account = storage
            .find_account_by_id(&account_id)
            .map_err(|err| format!("load account failed: {err}"))?
            .ok_or_else(|| "未找到账号池账号".to_string())?;
        let token = storage
            .find_token_by_account_id(&account_id)
            .map_err(|err| format!("load account token failed: {err}"))?
            .ok_or_else(|| "该账号缺少 AT/RT，无法切换为本地账号".to_string())?;
        let raw = build_account_pool_auth_content(&account, &token)?;
        let live_path = live_auth_path()?;
        let live_raw = fs::read_to_string(&live_path).ok();
        let backup_path = backup_live_auth_if_exists(&live_path)?;
        let normalized = normalize_auth_content_for_runtime(&raw, live_raw.as_deref());
        let dir = codex_dir()?;
        fs::create_dir_all(&dir)
            .map_err(|err| format!("create codex dir failed ({}): {err}", dir.display()))?;
        atomic_write_text(&live_path, &normalized)?;
        let status = account_pool_local_status_value(&app)?;
        let active_account_id = status
            .get("activeAccountId")
            .and_then(Value::as_str)
            .map(str::to_string);
        Ok(json!({
            "success": true,
            "activeAccountId": active_account_id,
            "backupPath": backup_path.map(|path| path.to_string_lossy().to_string()),
            "warning": SWITCH_WARNING,
            "status": status
        }))
    })
    .await
    .map_err(|err| format!("codex_local_account_pool_switch task failed: {err}"))?
}

#[tauri::command]
pub async fn local_account_delete(
    app: tauri::AppHandle,
    account_id: String,
) -> Result<serde_json::Value, String> {
    let db_path = resolve_db_path_with_legacy_migration(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        let mut storage = Storage::open(db_path).map_err(|e| e.to_string())?;
        storage
            .delete_account(&account_id)
            .map_err(|e| e.to_string())?;
        Ok(serde_json::json!({ "ok": true }))
    })
    .await
    .map_err(|err| format!("local_account_delete task failed: {err}"))?
}
