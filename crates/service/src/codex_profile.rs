use chrono::TimeZone;
use codexmanager_core::auth::DEFAULT_CLIENT_ID;
use codexmanager_core::storage::{now_ts, Account, ApiKey, Token};
use rusqlite::{backup::Backup, params, Connection};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};
use toml_edit::{value as toml_value, DocumentMut, Item, Table};

const APP_SETTING_CODEX_HOME_KEY: &str = "codex_profile.codex_home";
const APP_SETTING_STATE_KEY: &str = "codex_profile.state";
const APP_SETTING_BACKUPS_KEY: &str = "codex_profile.backups";
const MARKER_FILE: &str = ".codexmanager_profile.json";
const MANAGED_PROFILE_ROOT_DIR: &str = "codex-profiles";
const INTERNAL_MARKER_FILE: &str = "profile.json";
const INTERNAL_HISTORY_BACKUP_DIR: &str = "history-backups";
const HISTORY_BACKUP_MANIFEST_FILE: &str = "backup.json";
const MAX_HISTORY_BACKUPS_PER_PROFILE: usize = 3;
const MAX_HISTORY_BACKUP_AGE_DAYS: u64 = 7;
const MIN_HISTORY_BACKUPS_PER_PROFILE: usize = 1;
const AUTH_FILE: &str = "auth.json";
const CONFIG_FILE: &str = "config.toml";
const PROVIDER_ID: &str = "cm";
const DEFAULT_HISTORY_PROVIDER_ID: &str = "openai";
const HISTORY_BACKUP_DIR: &str = ".codexmanager_history_backups";
const STATE_DB_FILE: &str = "state_5.sqlite";
const SESSION_INDEX_FILE: &str = "session_index.jsonl";
const SESSION_DIRS: [&str; 2] = ["sessions", "archived_sessions"];
const DEFAULT_GATEWAY_BASE_URL: &str = "http://localhost:48760/v1";
const ENV_CODEX_HOME: &str = "CODEX_HOME";
const ENV_HOME: &str = "HOME";
const ENV_USERPROFILE: &str = "USERPROFILE";
const ENV_HOMEDRIVE: &str = "HOMEDRIVE";
const ENV_HOMEPATH: &str = "HOMEPATH";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CodexProfileMode {
    Missing,
    Unmanaged,
    DirectAccount,
    Gateway,
    ManagedUnknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodexProfileStatus {
    pub codex_home: String,
    pub auth_path: String,
    pub config_path: String,
    pub managed_storage_root: String,
    pub marker_path: String,
    pub history_backup_root: String,
    pub history_backup_count: usize,
    pub history_backup_bytes: u64,
    pub history_retention: CodexProfileHistoryRetention,
    pub mode: CodexProfileMode,
    pub selected_account_id: Option<String>,
    pub selected_api_key_id: Option<String>,
    pub gateway_base_url: Option<String>,
    pub provider_id: String,
    pub has_backup: bool,
    pub last_applied_at: Option<i64>,
    pub profile_writable: bool,
    pub error: Option<String>,
    pub warnings: Vec<String>,
    pub history_repair: Option<CodexProfileHistoryRepairSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodexProfileHistoryRetention {
    pub max_history_backups_per_profile: usize,
    pub max_history_backup_age_days: u64,
    pub min_history_backups_per_profile: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodexProfileAccountCandidate {
    pub id: String,
    pub label: String,
    pub group_name: Option<String>,
    pub status: String,
    pub chatgpt_account_id: Option<String>,
    pub workspace_id: Option<String>,
    pub issuer: String,
    pub last_refresh: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodexProfileApiKeyCandidate {
    pub id: String,
    pub name: Option<String>,
    pub status: String,
    pub model_slug: Option<String>,
    pub reasoning_effort: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodexProfileCandidates {
    pub accounts: Vec<CodexProfileAccountCandidate>,
    pub api_keys: Vec<CodexProfileApiKeyCandidate>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodexProfileHistoryRepairSummary {
    pub codex_home: String,
    pub target_provider: String,
    pub changed_rollout_file_count: usize,
    pub updated_sqlite_row_count: usize,
    pub added_session_index_entry_count: usize,
    pub backup_dir: Option<String>,
    pub warnings: Vec<String>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodexProfilePruneHistoryBackupsResult {
    pub codex_home: String,
    pub history_backup_root: String,
    pub before_count: usize,
    pub after_count: usize,
    pub removed_count: usize,
    pub before_bytes: u64,
    pub after_bytes: u64,
    pub removed_bytes: u64,
    pub retention: CodexProfileHistoryRetention,
    pub warnings: Vec<String>,
}

struct RolloutUpdate {
    path: PathBuf,
    content: String,
}

struct SqliteRepairPlan {
    rows_to_update: usize,
    missing_session_index_entries: Vec<serde_json::Value>,
}

struct ManagedProfilePaths {
    root: PathBuf,
    marker_path: PathBuf,
    history_backup_root: PathBuf,
    legacy_marker_path: PathBuf,
    legacy_history_backup_root: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HistoryBackupManifest {
    version: u32,
    codex_home: String,
    created_at: i64,
    target_provider: String,
    changed_rollout_file_count: usize,
    updated_sqlite_row_count: usize,
    added_session_index_entry_count: usize,
    files: Vec<String>,
}

struct HistoryBackupEntry {
    path: PathBuf,
    modified: SystemTime,
    bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManagedState {
    profile_dir: String,
    mode: CodexProfileMode,
    account_id: Option<String>,
    api_key_id: Option<String>,
    gateway_base_url: Option<String>,
    provider_id: String,
    updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BackupEntry {
    profile_dir: String,
    auth_json: Option<String>,
    config_toml: Option<String>,
    created_at: i64,
    updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MarkerFile {
    writer: String,
    mode: CodexProfileMode,
    account_id: Option<String>,
    api_key_id: Option<String>,
    gateway_base_url: Option<String>,
    provider_id: String,
    updated_at: i64,
}

pub(crate) fn get_status(codex_home: Option<&str>) -> Result<CodexProfileStatus, String> {
    let profile_dir = resolve_profile_dir(codex_home)?;
    status_for_profile(&profile_dir)
}

pub(crate) fn set_config(codex_home: Option<&str>) -> Result<CodexProfileStatus, String> {
    let profile_dir = resolve_profile_dir(codex_home)?;
    ensure_profile_dir_valid(&profile_dir)?;
    crate::app_settings::save_persisted_app_setting(
        APP_SETTING_CODEX_HOME_KEY,
        Some(&profile_dir.to_string_lossy()),
    )?;
    status_for_profile(&profile_dir)
}

pub(crate) fn list_candidates() -> Result<CodexProfileCandidates, String> {
    let storage = open_storage()?;
    let tokens = storage
        .list_tokens()
        .map_err(|err| format!("list tokens failed: {err}"))?
        .into_iter()
        .map(|token| (token.account_id.clone(), token))
        .collect::<HashMap<_, _>>();
    let mut accounts = storage
        .list_accounts()
        .map_err(|err| format!("list accounts failed: {err}"))?
        .into_iter()
        .filter_map(|account| account_candidate(&account, tokens.get(&account.id)))
        .collect::<Vec<_>>();
    accounts.sort_by(|left, right| {
        left.label
            .to_ascii_lowercase()
            .cmp(&right.label.to_ascii_lowercase())
            .then_with(|| left.id.cmp(&right.id))
    });

    let mut api_keys = storage
        .list_api_keys()
        .map_err(|err| format!("list api keys failed: {err}"))?
        .into_iter()
        .filter_map(api_key_candidate)
        .collect::<Vec<_>>();
    api_keys.sort_by(|left, right| {
        left.name
            .as_deref()
            .unwrap_or("")
            .to_ascii_lowercase()
            .cmp(&right.name.as_deref().unwrap_or("").to_ascii_lowercase())
            .then_with(|| left.id.cmp(&right.id))
    });

    Ok(CodexProfileCandidates { accounts, api_keys })
}

pub(crate) fn apply_direct_account(
    account_id: Option<&str>,
    codex_home: Option<&str>,
) -> Result<CodexProfileStatus, String> {
    let account_id = normalize_required(account_id, "missing accountId")?;
    let profile_dir = resolve_profile_dir(codex_home)?;
    ensure_profile_dir_valid(&profile_dir)?;
    let _ = ensure_managed_profile_migrated(&profile_dir);

    let storage = open_storage()?;
    let account = storage
        .find_account_by_id(account_id)
        .map_err(|err| format!("read account failed: {err}"))?
        .ok_or_else(|| "account not found".to_string())?;
    if account.status.trim() != "active" {
        return Err("account is not active".to_string());
    }
    let mut token = storage
        .find_token_by_account_id(account_id)
        .map_err(|err| format!("read token failed: {err}"))?
        .ok_or_else(|| "account token not found".to_string())?;
    ensure_usable_token(&token)?;

    let issuer = account.issuer.trim();
    if issuer.is_empty() {
        return Err("account issuer is empty".to_string());
    }
    crate::usage_token_refresh::refresh_and_persist_access_token(
        &storage,
        &mut token,
        issuer,
        DEFAULT_CLIENT_ID,
        crate::usage_token_refresh::token_refresh_ahead_secs(),
    )?;
    ensure_usable_token(&token)?;

    ensure_backup(&profile_dir)?;
    let auth_json = build_direct_auth_json(&account, &token)?;
    let config_toml = patch_config_for_direct(read_optional(&profile_dir.join(CONFIG_FILE))?)?;
    write_profile_files(
        &profile_dir,
        &auth_json,
        &config_toml,
        ManagedState {
            profile_dir: profile_key(&profile_dir),
            mode: CodexProfileMode::DirectAccount,
            account_id: Some(account.id.clone()),
            api_key_id: None,
            gateway_base_url: None,
            provider_id: PROVIDER_ID.to_string(),
            updated_at: now_ts(),
        },
    )?;
    persist_codex_home(&profile_dir)?;
    status_for_profile_with_history_repair(&profile_dir, DEFAULT_HISTORY_PROVIDER_ID)
}

pub(crate) fn apply_gateway(
    api_key_id: Option<&str>,
    codex_home: Option<&str>,
    base_url: Option<&str>,
) -> Result<CodexProfileStatus, String> {
    let api_key_id = normalize_required(api_key_id, "missing apiKeyId")?;
    let profile_dir = resolve_profile_dir(codex_home)?;
    ensure_profile_dir_valid(&profile_dir)?;
    let _ = ensure_managed_profile_migrated(&profile_dir);
    let gateway_base_url = normalize_gateway_base_url(base_url);

    let storage = open_storage()?;
    let api_key = storage
        .find_api_key_by_id(api_key_id)
        .map_err(|err| format!("read api key failed: {err}"))?
        .ok_or_else(|| "api key not found".to_string())?;
    if !is_active_api_key(&api_key) {
        return Err("api key is disabled".to_string());
    }
    let secret = storage
        .find_api_key_secret_by_id(api_key_id)
        .map_err(|err| format!("read api key secret failed: {err}"))?
        .ok_or_else(|| "api key secret not found".to_string())?;
    if secret.trim().is_empty() {
        return Err("api key secret is empty".to_string());
    }

    ensure_backup(&profile_dir)?;
    let auth_json = build_gateway_auth_json(&secret)?;
    let config_toml = patch_config_for_gateway(
        read_optional(&profile_dir.join(CONFIG_FILE))?,
        &gateway_base_url,
    )?;
    write_profile_files(
        &profile_dir,
        &auth_json,
        &config_toml,
        ManagedState {
            profile_dir: profile_key(&profile_dir),
            mode: CodexProfileMode::Gateway,
            account_id: None,
            api_key_id: Some(api_key.id),
            gateway_base_url: Some(gateway_base_url),
            provider_id: PROVIDER_ID.to_string(),
            updated_at: now_ts(),
        },
    )?;
    persist_codex_home(&profile_dir)?;
    status_for_profile_with_history_repair(&profile_dir, PROVIDER_ID)
}

pub(crate) fn restore(codex_home: Option<&str>) -> Result<CodexProfileStatus, String> {
    let profile_dir = resolve_profile_dir(codex_home)?;
    let key = profile_key(&profile_dir);
    let mut backups = load_backups();
    let backup = backups
        .remove(&key)
        .ok_or_else(|| "backup not found for this Codex profile".to_string())?;

    fs::create_dir_all(&profile_dir).map_err(|err| {
        format!(
            "create profile dir failed ({}): {err}",
            profile_dir.display()
        )
    })?;
    restore_optional_file(&profile_dir.join(AUTH_FILE), backup.auth_json.as_deref())?;
    restore_optional_file(
        &profile_dir.join(CONFIG_FILE),
        backup.config_toml.as_deref(),
    )?;
    let paths = managed_profile_paths(&profile_dir)?;
    remove_file_if_exists(&paths.marker_path)?;
    remove_file_if_exists(&paths.legacy_marker_path)?;
    save_backups(&backups)?;
    if load_state().is_some_and(|state| state.profile_dir == key) {
        crate::app_settings::save_persisted_app_setting(APP_SETTING_STATE_KEY, None)?;
    }
    status_for_profile(&profile_dir)
}

pub(crate) fn repair_history(
    codex_home: Option<&str>,
) -> Result<CodexProfileHistoryRepairSummary, String> {
    let profile_dir = resolve_profile_dir(codex_home)?;
    ensure_profile_dir_valid(&profile_dir)?;
    let migration_warnings = ensure_managed_profile_migrated(&profile_dir);
    let target_provider = target_history_provider_for_profile(&profile_dir)?;
    let mut summary = repair_history_for_provider(&profile_dir, &target_provider);
    summary.warnings.extend(migration_warnings);
    Ok(summary)
}

pub(crate) fn prune_history_backups(
    codex_home: Option<&str>,
) -> Result<CodexProfilePruneHistoryBackupsResult, String> {
    let profile_dir = resolve_profile_dir(codex_home)?;
    ensure_profile_dir_valid(&profile_dir)?;
    let mut warnings = ensure_managed_profile_migrated(&profile_dir);
    let mut result = prune_history_backups_for_profile(&profile_dir)?;
    result.warnings.append(&mut warnings);
    Ok(result)
}

fn open_storage() -> Result<crate::storage_helpers::StorageHandle, String> {
    crate::storage_helpers::open_storage().ok_or_else(|| "open storage failed".to_string())
}

fn account_candidate(
    account: &Account,
    token: Option<&Token>,
) -> Option<CodexProfileAccountCandidate> {
    let token = token?;
    if account.status.trim() != "active" || !token_is_usable(token) {
        return None;
    }
    Some(CodexProfileAccountCandidate {
        id: account.id.clone(),
        label: if account.label.trim().is_empty() {
            token.account_id.clone()
        } else {
            account.label.clone()
        },
        group_name: account.group_name.clone(),
        status: account.status.clone(),
        chatgpt_account_id: account.chatgpt_account_id.clone(),
        workspace_id: account.workspace_id.clone(),
        issuer: account.issuer.clone(),
        last_refresh: token.last_refresh,
    })
}

fn api_key_candidate(api_key: ApiKey) -> Option<CodexProfileApiKeyCandidate> {
    if !is_active_api_key(&api_key) {
        return None;
    }
    Some(CodexProfileApiKeyCandidate {
        id: api_key.id,
        name: api_key.name,
        status: api_key.status,
        model_slug: api_key.model_slug,
        reasoning_effort: api_key.reasoning_effort,
    })
}

fn is_active_api_key(api_key: &ApiKey) -> bool {
    !api_key.status.trim().eq_ignore_ascii_case("disabled")
}

fn token_is_usable(token: &Token) -> bool {
    !token.access_token.trim().is_empty() && !token.refresh_token.trim().is_empty()
}

fn ensure_usable_token(token: &Token) -> Result<(), String> {
    if token_is_usable(token) {
        Ok(())
    } else {
        Err("account token is missing access_token or refresh_token".to_string())
    }
}

fn normalize_required<'a>(value: Option<&'a str>, message: &str) -> Result<&'a str, String> {
    value
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .ok_or_else(|| message.to_string())
}

fn normalize_gateway_base_url(base_url: Option<&str>) -> String {
    let value = base_url
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .unwrap_or(DEFAULT_GATEWAY_BASE_URL)
        .trim_end_matches('/')
        .to_string();
    if value.ends_with("/v1") {
        value
    } else {
        format!("{value}/v1")
    }
}

fn persist_codex_home(profile_dir: &Path) -> Result<(), String> {
    crate::app_settings::save_persisted_app_setting(
        APP_SETTING_CODEX_HOME_KEY,
        Some(&profile_dir.to_string_lossy()),
    )
}

fn default_history_retention() -> CodexProfileHistoryRetention {
    CodexProfileHistoryRetention {
        max_history_backups_per_profile: MAX_HISTORY_BACKUPS_PER_PROFILE,
        max_history_backup_age_days: MAX_HISTORY_BACKUP_AGE_DAYS,
        min_history_backups_per_profile: MIN_HISTORY_BACKUPS_PER_PROFILE,
    }
}

fn managed_profile_paths(profile_dir: &Path) -> Result<ManagedProfilePaths, String> {
    let root = managed_profile_root(profile_dir)?;
    Ok(ManagedProfilePaths {
        marker_path: root.join(INTERNAL_MARKER_FILE),
        history_backup_root: root.join(INTERNAL_HISTORY_BACKUP_DIR),
        legacy_marker_path: profile_dir.join(MARKER_FILE),
        legacy_history_backup_root: profile_dir.join(HISTORY_BACKUP_DIR),
        root,
    })
}

fn managed_profile_root(profile_dir: &Path) -> Result<PathBuf, String> {
    let hash = profile_hash(profile_dir);
    Ok(managed_profile_base_dir()
        .join(MANAGED_PROFILE_ROOT_DIR)
        .join(hash))
}

fn managed_profile_base_dir() -> PathBuf {
    #[cfg(test)]
    {
        if let Ok(raw) = std::env::var("CODEXMANAGER_TEST_DB_DIR") {
            let trimmed = raw.trim();
            if !trimmed.is_empty() {
                return PathBuf::from(trimmed);
            }
        }
        return std::env::temp_dir().join("codexmanager-service-managed-tests");
    }

    #[cfg(not(test))]
    crate::process_env::db_dir()
}

fn profile_hash(profile_dir: &Path) -> String {
    let mut hasher = Sha256::new();
    hasher.update(profile_key(profile_dir).as_bytes());
    format!("{:x}", hasher.finalize())
}

fn ensure_managed_profile_migrated(profile_dir: &Path) -> Vec<String> {
    let mut warnings = Vec::new();
    let paths = match managed_profile_paths(profile_dir) {
        Ok(paths) => paths,
        Err(err) => {
            warnings.push(err);
            return warnings;
        }
    };

    if paths.legacy_marker_path.exists() {
        match migrate_legacy_marker(&paths) {
            Ok(()) => {}
            Err(err) => warnings.push(err),
        }
    }

    if paths.legacy_history_backup_root.exists() {
        match migrate_legacy_history_backups(&paths) {
            Ok(()) => match prune_history_backups_for_profile(profile_dir) {
                Ok(_) => {}
                Err(err) => warnings.push(err),
            },
            Err(err) => warnings.push(err),
        }
    }

    warnings
}

fn migrate_legacy_marker(paths: &ManagedProfilePaths) -> Result<(), String> {
    if !paths.marker_path.exists() {
        let content = fs::read_to_string(&paths.legacy_marker_path).map_err(|err| {
            format!(
                "read legacy CodexManager marker failed ({}): {err}",
                paths.legacy_marker_path.display()
            )
        })?;
        fs::create_dir_all(&paths.root).map_err(|err| {
            format!(
                "create managed profile root failed ({}): {err}",
                paths.root.display()
            )
        })?;
        write_atomic(&paths.marker_path, &content)?;
    }
    remove_file_if_exists(&paths.legacy_marker_path)
}

fn migrate_legacy_history_backups(paths: &ManagedProfilePaths) -> Result<(), String> {
    fs::create_dir_all(&paths.history_backup_root).map_err(|err| {
        format!(
            "create managed history backup root failed ({}): {err}",
            paths.history_backup_root.display()
        )
    })?;
    let entries = fs::read_dir(&paths.legacy_history_backup_root).map_err(|err| {
        format!(
            "read legacy Codex history backups failed ({}): {err}",
            paths.legacy_history_backup_root.display()
        )
    })?;

    for entry in entries {
        let entry = entry.map_err(|err| {
            format!(
                "read legacy Codex history backup entry failed ({}): {err}",
                paths.legacy_history_backup_root.display()
            )
        })?;
        let source = entry.path();
        let file_name = source
            .file_name()
            .ok_or_else(|| format!("invalid legacy backup path: {}", source.display()))?;
        let destination = unique_destination(&paths.history_backup_root.join(file_name));
        move_path_recursive(&source, &destination)?;
    }

    remove_dir_if_empty_or_missing(&paths.legacy_history_backup_root)
}

fn status_for_profile(profile_dir: &Path) -> Result<CodexProfileStatus, String> {
    let auth_path = profile_dir.join(AUTH_FILE);
    let config_path = profile_dir.join(CONFIG_FILE);
    let paths = managed_profile_paths(profile_dir)?;
    let warnings = ensure_managed_profile_migrated(profile_dir);
    let stats = history_backup_stats(&paths.history_backup_root);
    let key = profile_key(profile_dir);
    let backup_map = load_backups();
    let marker = read_marker(&paths.marker_path).ok();
    let persisted = load_state().filter(|state| state.profile_dir == key);
    let detected_mode = detect_mode(&auth_path, &config_path, marker.as_ref());
    let state = marker
        .map(|marker| ManagedState {
            profile_dir: key.clone(),
            mode: marker.mode,
            account_id: marker.account_id,
            api_key_id: marker.api_key_id,
            gateway_base_url: marker.gateway_base_url,
            provider_id: marker.provider_id,
            updated_at: marker.updated_at,
        })
        .or(persisted);

    Ok(CodexProfileStatus {
        codex_home: profile_dir.to_string_lossy().to_string(),
        auth_path: auth_path.to_string_lossy().to_string(),
        config_path: config_path.to_string_lossy().to_string(),
        managed_storage_root: paths.root.to_string_lossy().to_string(),
        marker_path: paths.marker_path.to_string_lossy().to_string(),
        history_backup_root: paths.history_backup_root.to_string_lossy().to_string(),
        history_backup_count: stats.0,
        history_backup_bytes: stats.1,
        history_retention: default_history_retention(),
        mode: state
            .as_ref()
            .map(|state| state.mode.clone())
            .unwrap_or(detected_mode),
        selected_account_id: state.as_ref().and_then(|state| state.account_id.clone()),
        selected_api_key_id: state.as_ref().and_then(|state| state.api_key_id.clone()),
        gateway_base_url: state
            .as_ref()
            .and_then(|state| state.gateway_base_url.clone()),
        provider_id: PROVIDER_ID.to_string(),
        has_backup: backup_map.contains_key(&key),
        last_applied_at: state.as_ref().map(|state| state.updated_at),
        profile_writable: profile_writable(profile_dir),
        error: None,
        warnings,
        history_repair: None,
    })
}

fn status_for_profile_with_history_repair(
    profile_dir: &Path,
    target_provider: &str,
) -> Result<CodexProfileStatus, String> {
    let history_repair = repair_history_for_provider(profile_dir, target_provider);
    let mut status = status_for_profile(profile_dir)?;
    status.history_repair = Some(history_repair);
    Ok(status)
}

fn target_history_provider_for_profile(profile_dir: &Path) -> Result<String, String> {
    let status = status_for_profile(profile_dir)?;
    match status.mode {
        CodexProfileMode::Gateway => Ok(PROVIDER_ID.to_string()),
        CodexProfileMode::DirectAccount => Ok(DEFAULT_HISTORY_PROVIDER_ID.to_string()),
        CodexProfileMode::Missing
        | CodexProfileMode::Unmanaged
        | CodexProfileMode::ManagedUnknown => read_optional(&profile_dir.join(CONFIG_FILE))
            .and_then(|content| {
                content.as_deref().map(parse_config).transpose().map(|doc| {
                    doc.and_then(|doc| {
                        doc.get("model_provider")
                            .and_then(Item::as_str)
                            .map(str::trim)
                            .filter(|provider| !provider.is_empty())
                            .map(str::to_string)
                    })
                    .unwrap_or_else(|| DEFAULT_HISTORY_PROVIDER_ID.to_string())
                })
            }),
    }
}

fn repair_history_for_provider(
    profile_dir: &Path,
    target_provider: &str,
) -> CodexProfileHistoryRepairSummary {
    let mut warnings = ensure_managed_profile_migrated(profile_dir);
    let mut changed_rollout_file_count = 0usize;
    let mut updated_sqlite_row_count = 0usize;
    let mut added_session_index_entry_count = 0usize;

    if !profile_dir.exists() {
        warnings.push(format!(
            "Codex profile directory does not exist: {}",
            profile_dir.display()
        ));
        return history_repair_summary(profile_dir, target_provider, 0, 0, 0, None, warnings);
    }

    let rollout_updates = match collect_rollout_updates(profile_dir, target_provider) {
        Ok(value) => value,
        Err(err) => {
            warnings.push(err);
            Vec::new()
        }
    };
    let sqlite_plan = match inspect_sqlite_repair_plan(profile_dir, target_provider) {
        Ok(value) => value,
        Err(err) => {
            warnings.push(err);
            SqliteRepairPlan {
                rows_to_update: 0,
                missing_session_index_entries: Vec::new(),
            }
        }
    };

    let needs_backup = !rollout_updates.is_empty()
        || sqlite_plan.rows_to_update > 0
        || !sqlite_plan.missing_session_index_entries.is_empty();
    let backup_dir = if needs_backup {
        match create_history_backup_dir(profile_dir) {
            Ok(path) => Some(path),
            Err(err) => {
                warnings.push(err);
                None
            }
        }
    } else {
        None
    };

    if needs_backup && backup_dir.is_none() {
        warnings.push(
            "history repair skipped because backup directory could not be created".to_string(),
        );
        return history_repair_summary(profile_dir, target_provider, 0, 0, 0, None, warnings);
    }

    if let Some(backup_dir) = backup_dir.as_ref() {
        for update in rollout_updates {
            match backup_existing_file(profile_dir, &update.path, backup_dir)
                .and_then(|_| write_atomic(&update.path, &update.content))
            {
                Ok(()) => changed_rollout_file_count += 1,
                Err(err) => warnings.push(err),
            }
        }

        if sqlite_plan.rows_to_update > 0 {
            match backup_sqlite_snapshot(profile_dir, backup_dir)
                .and_then(|_| update_sqlite_provider(profile_dir, target_provider))
            {
                Ok(updated) => updated_sqlite_row_count = updated,
                Err(err) => warnings.push(err),
            }
        }

        if !sqlite_plan.missing_session_index_entries.is_empty() {
            match backup_session_index_file(profile_dir, backup_dir).and_then(|_| {
                append_session_index_entries(
                    profile_dir,
                    &sqlite_plan.missing_session_index_entries,
                )
            }) {
                Ok(added) => added_session_index_entry_count = added,
                Err(err) => warnings.push(err),
            }
        }

        if let Err(err) = write_history_backup_manifest(
            profile_dir,
            backup_dir,
            target_provider,
            changed_rollout_file_count,
            updated_sqlite_row_count,
            added_session_index_entry_count,
        ) {
            warnings.push(err);
        }

        if let Err(err) = prune_history_backups_for_profile(profile_dir) {
            warnings.push(err);
        }
    }

    history_repair_summary(
        profile_dir,
        target_provider,
        changed_rollout_file_count,
        updated_sqlite_row_count,
        added_session_index_entry_count,
        backup_dir.map(|path| path.to_string_lossy().to_string()),
        warnings,
    )
}

fn history_repair_summary(
    profile_dir: &Path,
    target_provider: &str,
    changed_rollout_file_count: usize,
    updated_sqlite_row_count: usize,
    added_session_index_entry_count: usize,
    backup_dir: Option<String>,
    warnings: Vec<String>,
) -> CodexProfileHistoryRepairSummary {
    let total_changes =
        changed_rollout_file_count + updated_sqlite_row_count + added_session_index_entry_count;
    let message = if total_changes == 0 && warnings.is_empty() {
        "Codex history visibility is already aligned with the current provider".to_string()
    } else if warnings.is_empty() {
        format!(
            "Codex history visibility repaired: rollout files {}, sqlite rows {}, session index entries {}",
            changed_rollout_file_count, updated_sqlite_row_count, added_session_index_entry_count
        )
    } else {
        format!(
            "Codex history repair completed with warnings: rollout files {}, sqlite rows {}, session index entries {}, warnings {}",
            changed_rollout_file_count,
            updated_sqlite_row_count,
            added_session_index_entry_count,
            warnings.len()
        )
    };

    CodexProfileHistoryRepairSummary {
        codex_home: profile_dir.to_string_lossy().to_string(),
        target_provider: target_provider.to_string(),
        changed_rollout_file_count,
        updated_sqlite_row_count,
        added_session_index_entry_count,
        backup_dir,
        warnings,
        message,
    }
}

fn collect_rollout_updates(
    profile_dir: &Path,
    target_provider: &str,
) -> Result<Vec<RolloutUpdate>, String> {
    let mut files = Vec::new();
    for dir_name in SESSION_DIRS {
        collect_jsonl_files(&profile_dir.join(dir_name), &mut files)?;
    }

    let mut updates = Vec::new();
    for path in files {
        let content = fs::read_to_string(&path)
            .map_err(|err| format!("read rollout failed ({}): {err}", path.display()))?;
        if let Some(next_content) = patch_rollout_provider(&content, target_provider)? {
            updates.push(RolloutUpdate {
                path,
                content: next_content,
            });
        }
    }
    Ok(updates)
}

fn collect_jsonl_files(dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => {
            return Err(format!(
                "read session dir failed ({}): {err}",
                dir.display()
            ))
        }
    };

    for entry in entries {
        let entry = entry
            .map_err(|err| format!("read session dir entry failed ({}): {err}", dir.display()))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|err| format!("read file type failed ({}): {err}", path.display()))?;
        if file_type.is_dir() {
            collect_jsonl_files(&path, files)?;
        } else if file_type.is_file()
            && path
                .extension()
                .and_then(|item| item.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("jsonl"))
        {
            files.push(path);
        }
    }
    Ok(())
}

fn patch_rollout_provider(content: &str, target_provider: &str) -> Result<Option<String>, String> {
    let mut changed = false;
    let mut output = String::with_capacity(content.len());
    for segment in content.split_inclusive('\n') {
        let (line, suffix) = segment
            .strip_suffix('\n')
            .map(|line| (line, "\n"))
            .unwrap_or((segment, ""));
        if let Some(patched) = patch_rollout_line_provider(line, target_provider)? {
            output.push_str(&patched);
            output.push_str(suffix);
            changed = true;
        } else {
            output.push_str(segment);
        }
    }
    if content.is_empty() {
        return Ok(None);
    }
    Ok(changed.then_some(output))
}

fn patch_rollout_line_provider(
    line: &str,
    target_provider: &str,
) -> Result<Option<String>, String> {
    let json_text = line.trim_end_matches('\r');
    if json_text.trim().is_empty() {
        return Ok(None);
    }
    let mut value = match serde_json::from_str::<serde_json::Value>(json_text) {
        Ok(value) => value,
        Err(_) => return Ok(None),
    };
    if value
        .get("type")
        .and_then(|item| item.as_str())
        .is_none_or(|item| item != "session_meta")
    {
        return Ok(None);
    }
    let Some(payload) = value
        .get_mut("payload")
        .and_then(|item| item.as_object_mut())
    else {
        return Ok(None);
    };
    if payload
        .get("model_provider")
        .and_then(|item| item.as_str())
        .is_some_and(|provider| provider == target_provider)
    {
        return Ok(None);
    }
    payload.insert(
        "model_provider".to_string(),
        serde_json::Value::String(target_provider.to_string()),
    );
    serde_json::to_string(&value)
        .map(|patched| Some(format!("{patched}{}", &line[json_text.len()..])))
        .map_err(|err| format!("serialize rollout session_meta failed: {err}"))
}

fn inspect_sqlite_repair_plan(
    profile_dir: &Path,
    target_provider: &str,
) -> Result<SqliteRepairPlan, String> {
    let db_path = profile_dir.join(STATE_DB_FILE);
    if !db_path.exists() {
        return Ok(SqliteRepairPlan {
            rows_to_update: 0,
            missing_session_index_entries: Vec::new(),
        });
    }
    let conn = open_codex_state_db(&db_path)?;
    let columns = read_threads_columns(&conn)?;
    if !columns.contains("id") {
        return Ok(SqliteRepairPlan {
            rows_to_update: 0,
            missing_session_index_entries: Vec::new(),
        });
    }

    let rows_to_update = if columns.contains("model_provider") {
        conn.query_row(
            "SELECT COUNT(*) FROM threads WHERE COALESCE(model_provider, '') <> ?1",
            params![target_provider],
            |row| row.get::<_, i64>(0),
        )
        .map(|value| value.max(0) as usize)
        .map_err(|err| format!("count Codex history sqlite rows failed: {err}"))?
    } else {
        0
    };
    let missing_session_index_entries =
        missing_session_index_entries_from_sqlite(profile_dir, &conn, &columns)?;
    Ok(SqliteRepairPlan {
        rows_to_update,
        missing_session_index_entries,
    })
}

fn open_codex_state_db(db_path: &Path) -> Result<Connection, String> {
    let conn = Connection::open(db_path).map_err(|err| {
        format!(
            "open Codex state sqlite failed ({}): {err}",
            db_path.display()
        )
    })?;
    conn.busy_timeout(Duration::from_millis(250))
        .map_err(|err| format!("set Codex state sqlite busy timeout failed: {err}"))?;
    Ok(conn)
}

fn read_threads_columns(conn: &Connection) -> Result<HashSet<String>, String> {
    let mut stmt = conn
        .prepare("PRAGMA table_info(threads)")
        .map_err(|err| format!("inspect Codex threads table failed: {err}"))?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|err| format!("read Codex threads columns failed: {err}"))?;
    let mut columns = HashSet::new();
    for row in rows {
        columns.insert(row.map_err(|err| format!("read Codex threads column failed: {err}"))?);
    }
    Ok(columns)
}

fn missing_session_index_entries_from_sqlite(
    profile_dir: &Path,
    conn: &Connection,
    columns: &HashSet<String>,
) -> Result<Vec<serde_json::Value>, String> {
    if !columns.contains("id") {
        return Ok(Vec::new());
    }
    let existing = read_session_index_ids(profile_dir)?;
    let title_expr = if columns.contains("title") {
        "title"
    } else if columns.contains("first_user_message") {
        "first_user_message"
    } else {
        "id"
    };
    let updated_expr = if columns.contains("updated_at_ms") && columns.contains("updated_at") {
        "COALESCE(updated_at_ms, updated_at * 1000)"
    } else if columns.contains("updated_at_ms") {
        "updated_at_ms"
    } else if columns.contains("updated_at") {
        "updated_at * 1000"
    } else {
        "0"
    };
    let sql = format!("SELECT id, {title_expr}, {updated_expr} FROM threads ORDER BY id");
    let mut stmt = conn
        .prepare(&sql)
        .map_err(|err| format!("read Codex sqlite threads failed: {err}"))?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<i64>>(2)?,
            ))
        })
        .map_err(|err| format!("read Codex sqlite thread rows failed: {err}"))?;

    let mut entries = Vec::new();
    for row in rows {
        let (id, title, updated_at_ms) =
            row.map_err(|err| format!("read Codex sqlite thread row failed: {err}"))?;
        if existing.contains(&id) {
            continue;
        }
        entries.push(serde_json::json!({
            "id": id,
            "thread_name": title.filter(|item| !item.trim().is_empty()).unwrap_or_else(|| "Untitled".to_string()),
            "updated_at": format_session_index_timestamp(updated_at_ms.unwrap_or(0)),
        }));
    }
    Ok(entries)
}

fn read_session_index_ids(profile_dir: &Path) -> Result<HashSet<String>, String> {
    let path = profile_dir.join(SESSION_INDEX_FILE);
    let content = match fs::read_to_string(&path) {
        Ok(content) => content,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(HashSet::new()),
        Err(err) => {
            return Err(format!(
                "read Codex session index failed ({}): {err}",
                path.display()
            ))
        }
    };
    let mut ids = HashSet::new();
    for line in content.lines() {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(line) {
            if let Some(id) = value.get("id").and_then(|item| item.as_str()) {
                ids.insert(id.to_string());
            }
        }
    }
    Ok(ids)
}

fn format_session_index_timestamp(updated_at_ms: i64) -> String {
    chrono::Utc
        .timestamp_millis_opt(updated_at_ms)
        .single()
        .unwrap_or_else(chrono::Utc::now)
        .to_rfc3339_opts(chrono::SecondsFormat::Nanos, true)
}

fn update_sqlite_provider(profile_dir: &Path, target_provider: &str) -> Result<usize, String> {
    let db_path = profile_dir.join(STATE_DB_FILE);
    let conn = open_codex_state_db(&db_path)?;
    let columns = read_threads_columns(&conn)?;
    if !columns.contains("model_provider") {
        return Ok(0);
    }
    conn.execute(
        "UPDATE threads SET model_provider = ?1 WHERE COALESCE(model_provider, '') <> ?1",
        params![target_provider],
    )
    .map_err(|err| {
        format!(
            "update Codex history sqlite provider failed ({}): {err}",
            db_path.display()
        )
    })
}

fn append_session_index_entries(
    profile_dir: &Path,
    entries: &[serde_json::Value],
) -> Result<usize, String> {
    if entries.is_empty() {
        return Ok(0);
    }
    let path = profile_dir.join(SESSION_INDEX_FILE);
    let mut content = read_optional(&path)?.unwrap_or_default();
    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }
    let mut added = 0usize;
    for entry in entries {
        let line = serde_json::to_string(entry)
            .map_err(|err| format!("serialize Codex session index entry failed: {err}"))?;
        content.push_str(&line);
        content.push('\n');
        added += 1;
    }
    write_atomic(&path, &content)?;
    Ok(added)
}

fn create_history_backup_dir(profile_dir: &Path) -> Result<PathBuf, String> {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let paths = managed_profile_paths(profile_dir)?;
    let path = paths
        .history_backup_root
        .join(format!("{}-{}", now_ts(), unique));
    fs::create_dir_all(&path).map_err(|err| {
        format!(
            "create Codex history backup dir failed ({}): {err}",
            path.display()
        )
    })?;
    Ok(path)
}

fn backup_existing_file(profile_dir: &Path, path: &Path, backup_dir: &Path) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    let relative = path.strip_prefix(profile_dir).unwrap_or(path);
    let backup_path = backup_dir.join(relative);
    if let Some(parent) = backup_path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            format!(
                "create Codex history backup parent failed ({}): {err}",
                parent.display()
            )
        })?;
    }
    fs::copy(path, &backup_path).map(|_| ()).map_err(|err| {
        format!(
            "backup Codex history file failed ({} -> {}): {err}",
            path.display(),
            backup_path.display()
        )
    })
}

fn backup_sqlite_snapshot(profile_dir: &Path, backup_dir: &Path) -> Result<(), String> {
    let db_path = profile_dir.join(STATE_DB_FILE);
    if !db_path.exists() {
        return Ok(());
    }
    let backup_path = backup_dir.join(STATE_DB_FILE);
    if let Some(parent) = backup_path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            format!(
                "create Codex sqlite backup parent failed ({}): {err}",
                parent.display()
            )
        })?;
    }
    let source = open_codex_state_db(&db_path)?;
    let mut target = Connection::open(&backup_path).map_err(|err| {
        format!(
            "open Codex sqlite backup failed ({}): {err}",
            backup_path.display()
        )
    })?;
    let backup = Backup::new(&source, &mut target)
        .map_err(|err| format!("create Codex sqlite online backup failed: {err}"))?;
    backup
        .run_to_completion(64, Duration::from_millis(25), None)
        .map_err(|err| {
            format!(
                "backup Codex sqlite snapshot failed ({} -> {}): {err}",
                db_path.display(),
                backup_path.display()
            )
        })
}

fn backup_session_index_file(profile_dir: &Path, backup_dir: &Path) -> Result<(), String> {
    backup_existing_file(
        profile_dir,
        &profile_dir.join(SESSION_INDEX_FILE),
        backup_dir,
    )
}

fn write_history_backup_manifest(
    profile_dir: &Path,
    backup_dir: &Path,
    target_provider: &str,
    changed_rollout_file_count: usize,
    updated_sqlite_row_count: usize,
    added_session_index_entry_count: usize,
) -> Result<(), String> {
    let files = collect_relative_files(backup_dir, backup_dir)?
        .into_iter()
        .filter(|path| path != HISTORY_BACKUP_MANIFEST_FILE)
        .collect::<Vec<_>>();
    let manifest = HistoryBackupManifest {
        version: 1,
        codex_home: profile_dir.to_string_lossy().to_string(),
        created_at: now_ts(),
        target_provider: target_provider.to_string(),
        changed_rollout_file_count,
        updated_sqlite_row_count,
        added_session_index_entry_count,
        files,
    };
    let raw = serde_json::to_string_pretty(&manifest)
        .map_err(|err| format!("serialize Codex history backup manifest failed: {err}"))?;
    write_atomic(&backup_dir.join(HISTORY_BACKUP_MANIFEST_FILE), &raw)
}

fn prune_history_backups_for_profile(
    profile_dir: &Path,
) -> Result<CodexProfilePruneHistoryBackupsResult, String> {
    let paths = managed_profile_paths(profile_dir)?;
    let root = paths.history_backup_root;
    let (before_count, before_bytes) = history_backup_stats(&root);
    let retention = default_history_retention();
    let mut warnings = Vec::new();
    let mut entries = list_history_backup_entries(&root)?;
    entries.sort_by(|left, right| {
        right
            .modified
            .cmp(&left.modified)
            .then_with(|| right.path.cmp(&left.path))
    });

    let cutoff = SystemTime::now()
        .checked_sub(Duration::from_secs(
            retention
                .max_history_backup_age_days
                .saturating_mul(24 * 60 * 60),
        ))
        .unwrap_or(UNIX_EPOCH);

    for (index, entry) in entries.iter().enumerate() {
        let keep_for_minimum = index < retention.min_history_backups_per_profile;
        let keep_for_policy =
            index < retention.max_history_backups_per_profile && entry.modified >= cutoff;
        if keep_for_minimum || keep_for_policy {
            continue;
        }
        if let Err(err) = fs::remove_dir_all(&entry.path) {
            warnings.push(format!(
                "remove Codex history backup failed ({}): {err}",
                entry.path.display()
            ));
        }
    }

    let (after_count, after_bytes) = history_backup_stats(&root);
    Ok(CodexProfilePruneHistoryBackupsResult {
        codex_home: profile_dir.to_string_lossy().to_string(),
        history_backup_root: root.to_string_lossy().to_string(),
        before_count,
        after_count,
        removed_count: before_count.saturating_sub(after_count),
        before_bytes,
        after_bytes,
        removed_bytes: before_bytes.saturating_sub(after_bytes),
        retention,
        warnings,
    })
}

fn history_backup_stats(root: &Path) -> (usize, u64) {
    let entries = match list_history_backup_entries(root) {
        Ok(entries) => entries,
        Err(_) => return (0, 0),
    };
    let count = entries.len();
    let bytes = entries
        .into_iter()
        .fold(0u64, |total, entry| total.saturating_add(entry.bytes));
    (count, bytes)
}

fn list_history_backup_entries(root: &Path) -> Result<Vec<HistoryBackupEntry>, String> {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => {
            return Err(format!(
                "read Codex history backup root failed ({}): {err}",
                root.display()
            ))
        }
    };

    let mut backups = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|err| {
            format!(
                "read Codex history backup entry failed ({}): {err}",
                root.display()
            )
        })?;
        let path = entry.path();
        let metadata = entry.metadata().map_err(|err| {
            format!(
                "read Codex history backup metadata failed ({}): {err}",
                path.display()
            )
        })?;
        if !metadata.is_dir() {
            continue;
        }
        backups.push(HistoryBackupEntry {
            bytes: dir_size(&path),
            modified: metadata.modified().unwrap_or(UNIX_EPOCH),
            path,
        });
    }
    Ok(backups)
}

fn dir_size(path: &Path) -> u64 {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(_) => return 0,
    };
    if metadata.is_file() {
        return metadata.len();
    }
    if !metadata.is_dir() {
        return 0;
    }
    match fs::read_dir(path) {
        Ok(entries) => entries.filter_map(Result::ok).fold(0u64, |total, entry| {
            total.saturating_add(dir_size(&entry.path()))
        }),
        Err(_) => 0,
    }
}

fn collect_relative_files(root: &Path, dir: &Path) -> Result<Vec<String>, String> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => {
            return Err(format!(
                "read backup files failed ({}): {err}",
                dir.display()
            ))
        }
    };
    let mut files = Vec::new();
    for entry in entries {
        let entry = entry
            .map_err(|err| format!("read backup file entry failed ({}): {err}", dir.display()))?;
        let path = entry.path();
        let metadata = entry.metadata().map_err(|err| {
            format!(
                "read backup file metadata failed ({}): {err}",
                path.display()
            )
        })?;
        if metadata.is_dir() {
            files.extend(collect_relative_files(root, &path)?);
        } else if metadata.is_file() {
            files.push(
                path.strip_prefix(root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .replace('\\', "/"),
            );
        }
    }
    files.sort();
    Ok(files)
}

fn unique_destination(path: &Path) -> PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }
    for index in 1..1000 {
        let candidate = PathBuf::from(format!("{}-legacy-{index}", path.to_string_lossy()));
        if !candidate.exists() {
            return candidate;
        }
    }
    PathBuf::from(format!(
        "{}-legacy-{}",
        path.to_string_lossy(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ))
}

fn move_path_recursive(source: &Path, destination: &Path) -> Result<(), String> {
    match fs::rename(source, destination) {
        Ok(()) => return Ok(()),
        Err(rename_err) => {
            copy_path_recursive(source, destination).map_err(|copy_err| {
                format!(
                    "move legacy Codex history backup failed ({} -> {}): rename error: {rename_err}; copy fallback error: {copy_err}",
                    source.display(),
                    destination.display()
                )
            })?;
            remove_path_recursive(source).map_err(|remove_err| {
                format!(
                    "remove migrated legacy Codex history backup failed ({}): {remove_err}",
                    source.display()
                )
            })
        }
    }
}

fn copy_path_recursive(source: &Path, destination: &Path) -> Result<(), String> {
    let metadata = fs::metadata(source)
        .map_err(|err| format!("read path metadata failed ({}): {err}", source.display()))?;
    if metadata.is_file() {
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).map_err(|err| {
                format!("create copy parent failed ({}): {err}", parent.display())
            })?;
        }
        fs::copy(source, destination).map(|_| ()).map_err(|err| {
            format!(
                "copy file failed ({} -> {}): {err}",
                source.display(),
                destination.display()
            )
        })
    } else if metadata.is_dir() {
        fs::create_dir_all(destination)
            .map_err(|err| format!("create copy dir failed ({}): {err}", destination.display()))?;
        let entries = fs::read_dir(source)
            .map_err(|err| format!("read copy dir failed ({}): {err}", source.display()))?;
        for entry in entries {
            let entry = entry.map_err(|err| {
                format!("read copy dir entry failed ({}): {err}", source.display())
            })?;
            copy_path_recursive(&entry.path(), &destination.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        Ok(())
    }
}

fn remove_path_recursive(path: &Path) -> Result<(), String> {
    let metadata = fs::metadata(path).map_err(|err| {
        format!(
            "read remove path metadata failed ({}): {err}",
            path.display()
        )
    })?;
    if metadata.is_dir() {
        fs::remove_dir_all(path)
            .map_err(|err| format!("remove dir failed ({}): {err}", path.display()))
    } else {
        fs::remove_file(path)
            .map_err(|err| format!("remove file failed ({}): {err}", path.display()))
    }
}

fn remove_dir_if_empty_or_missing(path: &Path) -> Result<(), String> {
    match fs::remove_dir(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::DirectoryNotEmpty => Ok(()),
        Err(err) => Err(format!("remove dir failed ({}): {err}", path.display())),
    }
}

fn detect_mode(
    auth_path: &Path,
    config_path: &Path,
    marker: Option<&MarkerFile>,
) -> CodexProfileMode {
    if let Some(marker) = marker {
        return marker.mode.clone();
    }
    let auth = read_optional(auth_path).ok().flatten();
    let config = read_optional(config_path).ok().flatten();
    if auth.is_none() && config.is_none() {
        return CodexProfileMode::Missing;
    }
    if auth.as_deref().is_some_and(auth_json_is_gateway)
        || config
            .as_deref()
            .is_some_and(|content| config_uses_managed_provider(content).unwrap_or(false))
    {
        return CodexProfileMode::Gateway;
    }
    if auth.as_deref().is_some_and(auth_json_has_tokens) {
        return CodexProfileMode::DirectAccount;
    }
    CodexProfileMode::Unmanaged
}

fn profile_writable(profile_dir: &Path) -> bool {
    if profile_dir.exists() {
        return profile_dir.is_dir()
            && fs::metadata(profile_dir)
                .map(|metadata| !metadata.permissions().readonly())
                .unwrap_or(false);
    }
    profile_dir
        .parent()
        .filter(|parent| parent.exists())
        .and_then(|parent| fs::metadata(parent).ok())
        .map(|metadata| !metadata.permissions().readonly())
        .unwrap_or(false)
}

fn auth_json_is_gateway(content: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(content)
        .ok()
        .and_then(|value| {
            value
                .get("auth_mode")
                .and_then(|item| item.as_str())
                .map(|auth_mode| auth_mode.eq_ignore_ascii_case("apikey"))
        })
        .unwrap_or(false)
}

fn auth_json_has_tokens(content: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(content)
        .ok()
        .and_then(|value| value.get("tokens").cloned())
        .and_then(|tokens| tokens.as_object().cloned())
        .is_some_and(|tokens| {
            tokens.contains_key("access_token") || tokens.contains_key("accessToken")
        })
}

fn config_uses_managed_provider(content: &str) -> Result<bool, String> {
    let doc = parse_config(content)?;
    Ok(doc
        .get("model_provider")
        .and_then(Item::as_str)
        .is_some_and(|provider| provider == PROVIDER_ID))
}

fn ensure_profile_dir_valid(profile_dir: &Path) -> Result<(), String> {
    if profile_dir.exists() && !profile_dir.is_dir() {
        return Err(format!(
            "Codex profile path is not a directory: {}",
            profile_dir.display()
        ));
    }
    Ok(())
}

fn resolve_profile_dir(codex_home: Option<&str>) -> Result<PathBuf, String> {
    if let Some(input) = codex_home.map(str::trim).filter(|value| !value.is_empty()) {
        return Ok(expand_home_prefix(input));
    }
    if let Some(persisted) =
        crate::app_settings::get_persisted_app_setting(APP_SETTING_CODEX_HOME_KEY)
    {
        return Ok(expand_home_prefix(&persisted));
    }
    if let Ok(raw) = std::env::var(ENV_CODEX_HOME) {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            return Ok(expand_home_prefix(trimmed));
        }
    }
    default_profile_dir()
}

fn default_profile_dir() -> Result<PathBuf, String> {
    for key in [ENV_USERPROFILE, ENV_HOME] {
        if let Ok(raw) = std::env::var(key) {
            let trimmed = raw.trim();
            if !trimmed.is_empty() {
                return Ok(PathBuf::from(trimmed).join(".codex"));
            }
        }
    }
    let home_drive = std::env::var(ENV_HOMEDRIVE).unwrap_or_default();
    let home_path = std::env::var(ENV_HOMEPATH).unwrap_or_default();
    let combined = format!("{home_drive}{home_path}");
    if !combined.trim().is_empty() {
        return Ok(PathBuf::from(combined).join(".codex"));
    }
    Err("unable to resolve Codex profile directory".to_string())
}

fn expand_home_prefix(input: &str) -> PathBuf {
    if input == "~" || input.starts_with("~/") {
        if let Ok(home) = std::env::var(ENV_HOME) {
            let suffix = input.strip_prefix("~/").unwrap_or("");
            return PathBuf::from(home).join(suffix);
        }
    }
    PathBuf::from(input)
}

fn profile_key(profile_dir: &Path) -> String {
    profile_dir.to_string_lossy().to_string()
}

fn build_direct_auth_json(account: &Account, token: &Token) -> Result<String, String> {
    let account_id = account
        .chatgpt_account_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(account.id.as_str());
    serde_json::to_string_pretty(&serde_json::json!({
        "OPENAI_API_KEY": null,
        "tokens": {
            "id_token": token.id_token,
            "access_token": token.access_token,
            "refresh_token": token.refresh_token,
            "account_id": account_id,
        },
        "last_refresh": chrono::Utc::now()
            .format("%Y-%m-%dT%H:%M:%S%.6fZ")
            .to_string(),
    }))
    .map_err(|err| format!("serialize auth.json failed: {err}"))
}

fn build_gateway_auth_json(api_key: &str) -> Result<String, String> {
    serde_json::to_string_pretty(&serde_json::json!({
        "auth_mode": "apikey",
        "OPENAI_API_KEY": api_key,
    }))
    .map_err(|err| format!("serialize auth.json failed: {err}"))
}

fn patch_config_for_direct(content: Option<String>) -> Result<String, String> {
    let mut doc = parse_config(content.as_deref().unwrap_or(""))?;
    if doc
        .get("model_provider")
        .and_then(Item::as_str)
        .is_some_and(|provider| provider == PROVIDER_ID)
    {
        doc.as_table_mut().remove("model_provider");
    }
    if let Some(providers) = doc
        .as_table_mut()
        .get_mut("model_providers")
        .and_then(Item::as_table_mut)
    {
        providers.remove(PROVIDER_ID);
        if providers.is_empty() {
            doc.as_table_mut().remove("model_providers");
        }
    }
    Ok(doc.to_string())
}

fn patch_config_for_gateway(content: Option<String>, base_url: &str) -> Result<String, String> {
    let mut doc = parse_config(content.as_deref().unwrap_or(""))?;
    doc.as_table_mut()
        .insert("model_provider", toml_value(PROVIDER_ID));

    if doc.as_table().get("model_providers").is_none() {
        doc.as_table_mut()
            .insert("model_providers", Item::Table(Table::new()));
    }
    let providers = doc
        .as_table_mut()
        .get_mut("model_providers")
        .and_then(Item::as_table_mut)
        .ok_or_else(|| "config.toml model_providers is not a table".to_string())?;
    if providers
        .get(PROVIDER_ID)
        .and_then(Item::as_table)
        .is_none()
    {
        providers.insert(PROVIDER_ID, Item::Table(Table::new()));
    }
    let provider = providers
        .get_mut(PROVIDER_ID)
        .and_then(Item::as_table_mut)
        .ok_or_else(|| "config.toml model_providers.cm is not a table".to_string())?;
    provider.clear();
    provider.insert("name", toml_value("CodexManager"));
    provider.insert("base_url", toml_value(base_url));
    provider.insert("wire_api", toml_value("responses"));
    Ok(doc.to_string())
}

fn parse_config(content: &str) -> Result<DocumentMut, String> {
    content
        .parse::<DocumentMut>()
        .map_err(|err| format!("parse config.toml failed: {err}"))
}

fn write_profile_files(
    profile_dir: &Path,
    auth_json: &str,
    config_toml: &str,
    state: ManagedState,
) -> Result<(), String> {
    fs::create_dir_all(profile_dir).map_err(|err| {
        format!(
            "create profile dir failed ({}): {err}",
            profile_dir.display()
        )
    })?;
    write_atomic(&profile_dir.join(AUTH_FILE), auth_json)?;
    write_atomic(&profile_dir.join(CONFIG_FILE), config_toml)?;
    let paths = managed_profile_paths(profile_dir)?;
    let marker = MarkerFile {
        writer: "codexmanager".to_string(),
        mode: state.mode.clone(),
        account_id: state.account_id.clone(),
        api_key_id: state.api_key_id.clone(),
        gateway_base_url: state.gateway_base_url.clone(),
        provider_id: state.provider_id.clone(),
        updated_at: state.updated_at,
    };
    let marker_json = serde_json::to_string_pretty(&marker)
        .map_err(|err| format!("serialize marker failed: {err}"))?;
    write_atomic(&paths.marker_path, &marker_json)?;
    let _ = remove_file_if_exists(&paths.legacy_marker_path);
    save_state(&state)?;
    Ok(())
}

fn ensure_backup(profile_dir: &Path) -> Result<(), String> {
    let key = profile_key(profile_dir);
    let mut backups = load_backups();
    if backups.contains_key(&key) {
        return Ok(());
    }
    let now = now_ts();
    backups.insert(
        key.clone(),
        BackupEntry {
            profile_dir: key,
            auth_json: read_optional(&profile_dir.join(AUTH_FILE))?,
            config_toml: read_optional(&profile_dir.join(CONFIG_FILE))?,
            created_at: now,
            updated_at: now,
        },
    );
    save_backups(&backups)
}

fn load_state() -> Option<ManagedState> {
    crate::app_settings::get_persisted_app_setting(APP_SETTING_STATE_KEY)
        .and_then(|value| serde_json::from_str(&value).ok())
}

fn save_state(state: &ManagedState) -> Result<(), String> {
    let value =
        serde_json::to_string(state).map_err(|err| format!("serialize state failed: {err}"))?;
    crate::app_settings::save_persisted_app_setting(APP_SETTING_STATE_KEY, Some(&value))
}

fn load_backups() -> HashMap<String, BackupEntry> {
    crate::app_settings::get_persisted_app_setting(APP_SETTING_BACKUPS_KEY)
        .and_then(|value| serde_json::from_str(&value).ok())
        .unwrap_or_default()
}

fn save_backups(backups: &HashMap<String, BackupEntry>) -> Result<(), String> {
    let value =
        serde_json::to_string(backups).map_err(|err| format!("serialize backups failed: {err}"))?;
    crate::app_settings::save_persisted_app_setting(APP_SETTING_BACKUPS_KEY, Some(&value))
}

fn read_marker(path: &Path) -> Result<MarkerFile, String> {
    let content = fs::read_to_string(path)
        .map_err(|err| format!("read marker failed ({}): {err}", path.display()))?;
    serde_json::from_str(&content).map_err(|err| format!("parse marker failed: {err}"))
}

fn read_optional(path: &Path) -> Result<Option<String>, String> {
    match fs::read_to_string(path) {
        Ok(value) => Ok(Some(value)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(format!("read file failed ({}): {err}", path.display())),
    }
}

fn restore_optional_file(path: &Path, content: Option<&str>) -> Result<(), String> {
    match content {
        Some(content) => write_atomic(path, content),
        None => remove_file_if_exists(path),
    }
}

fn remove_file_if_exists(path: &Path) -> Result<(), String> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(format!("remove file failed ({}): {err}", path.display())),
    }
}

fn write_atomic(path: &Path, content: &str) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("unable to resolve parent for {}", path.display()))?;
    fs::create_dir_all(parent)
        .map_err(|err| format!("create parent dir failed ({}): {err}", parent.display()))?;
    let temp_path = temp_file_path(parent, path);
    fs::write(&temp_path, content)
        .map_err(|err| format!("write temp file failed ({}): {err}", temp_path.display()))?;
    match fs::rename(&temp_path, path) {
        Ok(()) => Ok(()),
        Err(err) => {
            let _ = fs::remove_file(&temp_path);
            Err(format!("replace file failed ({}): {err}", path.display()))
        }
    }
}

fn temp_file_path(parent: &Path, target: &Path) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let file_name = target
        .file_name()
        .and_then(|item| item.to_str())
        .unwrap_or("file");
    parent.join(format!(
        ".{file_name}.tmp.{}.{}",
        std::process::id(),
        unique
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn temp_profile(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        std::env::temp_dir().join(format!("codexmanager-{name}-{unique}"))
    }

    fn cleanup_profile(dir: &Path) {
        if let Ok(root) = managed_profile_root(dir) {
            let _ = fs::remove_dir_all(root);
        }
        let _ = fs::remove_dir_all(dir);
    }

    fn write_test_rollout(dir: &Path, thread_id: &str, provider: &str) -> (PathBuf, String) {
        let rollout_dir = dir.join("sessions").join("2026").join("06").join("06");
        fs::create_dir_all(&rollout_dir).expect("mkdir rollout");
        let path = rollout_dir.join(format!("rollout-2026-06-06T00-00-00-{thread_id}.jsonl"));
        let event_line = r#"{"timestamp":"2026-06-06T00:00:01Z","type":"event_msg","payload":{"type":"user_message","message":"keep me"}}"#.to_string();
        let content = format!(
            "{{\"timestamp\":\"2026-06-06T00:00:00Z\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"{thread_id}\",\"model_provider\":\"{provider}\",\"cwd\":\"/tmp\"}}}}\n{event_line}\n"
        );
        fs::write(&path, content).expect("write rollout");
        (path, event_line)
    }

    fn create_state_db(dir: &Path, thread_id: &str, provider: &str) {
        let conn = Connection::open(dir.join(STATE_DB_FILE)).expect("open sqlite");
        conn.execute(
            "CREATE TABLE threads (
                id TEXT PRIMARY KEY,
                model_provider TEXT,
                title TEXT,
                updated_at INTEGER,
                updated_at_ms INTEGER
            )",
            [],
        )
        .expect("create threads");
        conn.execute(
            "INSERT INTO threads (id, model_provider, title, updated_at, updated_at_ms)
             VALUES (?1, ?2, 'Thread title', 1770000000, 1770000000000)",
            params![thread_id, provider],
        )
        .expect("insert thread");
    }

    fn sqlite_provider(dir: &Path, thread_id: &str) -> String {
        let conn = Connection::open(dir.join(STATE_DB_FILE)).expect("open sqlite");
        conn.query_row(
            "SELECT model_provider FROM threads WHERE id = ?1",
            params![thread_id],
            |row| row.get::<_, String>(0),
        )
        .expect("read provider")
    }

    #[test]
    fn direct_config_removes_only_managed_provider() {
        let input = r#"
model_provider = "cm"
model = "gpt-5.4"

[model_providers.cm]
name = "CodexManager"
base_url = "http://localhost:48760/v1"
wire_api = "responses"

[model_providers.other]
name = "Other"
base_url = "https://example.test/v1"
"#;

        let output = patch_config_for_direct(Some(input.to_string())).expect("patch direct");

        assert!(!output.contains("model_provider = \"cm\""));
        assert!(!output.contains("[model_providers.cm]"));
        assert!(output.contains("[model_providers.other]"));
        assert!(output.contains("model = \"gpt-5.4\""));
    }

    #[test]
    fn gateway_config_sets_managed_provider_and_preserves_other_values() {
        let input = r#"
model = "gpt-5.4"

[model_providers.other]
name = "Other"
"#;

        let output = patch_config_for_gateway(Some(input.to_string()), "http://127.0.0.1:48770/v1")
            .expect("patch gateway");

        assert!(output.contains("model_provider = \"cm\""));
        assert!(output.contains("[model_providers.cm]"));
        assert!(output.contains("base_url = \"http://127.0.0.1:48770/v1\""));
        assert!(output.contains("wire_api = \"responses\""));
        assert!(output.contains("[model_providers.other]"));
    }

    #[test]
    fn invalid_toml_is_rejected() {
        assert!(patch_config_for_gateway(Some("bad = [".to_string()), "http://x/v1").is_err());
    }

    #[test]
    fn restore_optional_file_removes_files_that_were_missing() {
        let dir = temp_profile("restore-missing");
        fs::create_dir_all(&dir).expect("mkdir");
        let path = dir.join("auth.json");
        fs::write(&path, "{}").expect("write");

        restore_optional_file(&path, None).expect("restore missing");

        assert!(!path.exists());
        cleanup_profile(&dir);
    }

    #[test]
    fn auth_json_shapes_match_codex_modes() {
        let now = now_ts();
        let account = Account {
            id: "acc-1".to_string(),
            label: "Account".to_string(),
            issuer: "https://auth.openai.com".to_string(),
            chatgpt_account_id: Some("chatgpt-1".to_string()),
            workspace_id: None,
            group_name: None,
            sort: 0,
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
        };
        let token = Token {
            account_id: "acc-1".to_string(),
            id_token: "id-token".to_string(),
            access_token: "access-token".to_string(),
            refresh_token: "refresh-token".to_string(),
            api_key_access_token: None,
            last_refresh: now,
        };

        let direct = build_direct_auth_json(&account, &token).expect("direct auth");
        let gateway = build_gateway_auth_json("cm-key").expect("gateway auth");

        assert!(auth_json_has_tokens(&direct));
        assert!(!auth_json_is_gateway(&direct));
        assert!(auth_json_is_gateway(&gateway));
    }

    #[test]
    fn write_profile_files_uses_internal_marker() {
        let dir = temp_profile("internal-marker");
        let state = ManagedState {
            profile_dir: profile_key(&dir),
            mode: CodexProfileMode::Gateway,
            account_id: None,
            api_key_id: Some("key-1".to_string()),
            gateway_base_url: Some("http://localhost:48760/v1".to_string()),
            provider_id: PROVIDER_ID.to_string(),
            updated_at: now_ts(),
        };

        write_profile_files(&dir, "{}", "", state).expect("write profile");

        let paths = managed_profile_paths(&dir).expect("paths");
        assert!(paths.marker_path.exists());
        assert!(!dir.join(MARKER_FILE).exists());
        let status = status_for_profile(&dir).expect("status");
        assert!(matches!(status.mode, CodexProfileMode::Gateway));
        assert_eq!(
            status.marker_path,
            paths.marker_path.to_string_lossy().to_string()
        );
        cleanup_profile(&dir);
    }

    #[test]
    fn legacy_marker_migrates_to_internal_marker() {
        let dir = temp_profile("legacy-marker");
        fs::create_dir_all(&dir).expect("mkdir profile");
        let marker = MarkerFile {
            writer: "codexmanager".to_string(),
            mode: CodexProfileMode::DirectAccount,
            account_id: Some("acc-1".to_string()),
            api_key_id: None,
            gateway_base_url: None,
            provider_id: PROVIDER_ID.to_string(),
            updated_at: now_ts(),
        };
        fs::write(
            dir.join(MARKER_FILE),
            serde_json::to_string_pretty(&marker).expect("marker json"),
        )
        .expect("write legacy marker");

        let status = status_for_profile(&dir).expect("status");

        let paths = managed_profile_paths(&dir).expect("paths");
        assert!(paths.marker_path.exists());
        assert!(!paths.legacy_marker_path.exists());
        assert!(matches!(status.mode, CodexProfileMode::DirectAccount));
        cleanup_profile(&dir);
    }

    #[test]
    fn legacy_history_backups_migrate_and_are_pruned() {
        let dir = temp_profile("legacy-history-backups");
        let legacy_root = dir.join(HISTORY_BACKUP_DIR);
        fs::create_dir_all(&legacy_root).expect("mkdir legacy root");
        for index in 0..5 {
            let backup_dir = legacy_root.join(format!("backup-{index}"));
            fs::create_dir_all(&backup_dir).expect("mkdir legacy backup");
            fs::write(backup_dir.join("file.txt"), format!("backup-{index}"))
                .expect("write legacy backup");
        }

        let status = status_for_profile(&dir).expect("status");

        let paths = managed_profile_paths(&dir).expect("paths");
        assert!(!paths.legacy_history_backup_root.exists());
        assert!(paths.history_backup_root.exists());
        assert_eq!(status.history_backup_count, MAX_HISTORY_BACKUPS_PER_PROFILE);
        cleanup_profile(&dir);
    }

    #[test]
    fn history_repair_aligns_direct_and_gateway_providers() {
        let dir = temp_profile("history-provider");
        fs::create_dir_all(&dir).expect("mkdir profile");
        let thread_id = "thread-provider";
        let (rollout_path, event_line) = write_test_rollout(&dir, thread_id, PROVIDER_ID);
        create_state_db(&dir, thread_id, PROVIDER_ID);
        fs::write(
            dir.join(SESSION_INDEX_FILE),
            format!(
                "{{\"id\":\"{thread_id}\",\"thread_name\":\"Thread title\",\"updated_at\":\"2026-06-06T00:00:00Z\"}}\n"
            ),
        )
        .expect("write session index");

        let direct = repair_history_for_provider(&dir, DEFAULT_HISTORY_PROVIDER_ID);

        assert!(direct.warnings.is_empty(), "{:?}", direct.warnings);
        assert_eq!(direct.changed_rollout_file_count, 1);
        assert_eq!(direct.updated_sqlite_row_count, 1);
        assert_eq!(
            sqlite_provider(&dir, thread_id),
            DEFAULT_HISTORY_PROVIDER_ID
        );
        let direct_rollout = fs::read_to_string(&rollout_path).expect("read direct rollout");
        assert!(direct_rollout.contains("\"model_provider\":\"openai\""));
        assert!(direct_rollout.contains(&event_line));
        assert!(!dir.join(HISTORY_BACKUP_DIR).exists());
        let direct_backup = direct.backup_dir.as_ref().expect("direct backup dir");
        assert!(direct_backup.contains(MANAGED_PROFILE_ROOT_DIR));
        let direct_backup_path = PathBuf::from(direct_backup);
        assert!(direct_backup_path.join(STATE_DB_FILE).exists());
        assert!(!direct_backup_path
            .join(format!("{STATE_DB_FILE}-wal"))
            .exists());
        assert!(!direct_backup_path
            .join(format!("{STATE_DB_FILE}-shm"))
            .exists());
        assert!(direct_backup_path
            .join(HISTORY_BACKUP_MANIFEST_FILE)
            .exists());

        let gateway = repair_history_for_provider(&dir, PROVIDER_ID);

        assert!(gateway.warnings.is_empty(), "{:?}", gateway.warnings);
        assert_eq!(gateway.changed_rollout_file_count, 1);
        assert_eq!(gateway.updated_sqlite_row_count, 1);
        assert_eq!(sqlite_provider(&dir, thread_id), PROVIDER_ID);
        let gateway_rollout = fs::read_to_string(&rollout_path).expect("read gateway rollout");
        assert!(gateway_rollout.contains("\"model_provider\":\"cm\""));
        assert!(gateway_rollout.contains(&event_line));
        cleanup_profile(&dir);
    }

    #[test]
    fn history_repair_appends_missing_session_index_once() {
        let dir = temp_profile("history-index");
        fs::create_dir_all(&dir).expect("mkdir profile");
        let thread_id = "thread-index";
        create_state_db(&dir, thread_id, DEFAULT_HISTORY_PROVIDER_ID);

        let first = repair_history_for_provider(&dir, DEFAULT_HISTORY_PROVIDER_ID);
        let second = repair_history_for_provider(&dir, DEFAULT_HISTORY_PROVIDER_ID);

        assert!(first.warnings.is_empty(), "{:?}", first.warnings);
        assert_eq!(first.added_session_index_entry_count, 1);
        assert!(second.warnings.is_empty(), "{:?}", second.warnings);
        assert_eq!(second.added_session_index_entry_count, 0);
        let index = fs::read_to_string(dir.join(SESSION_INDEX_FILE)).expect("read index");
        assert_eq!(index.lines().count(), 1);
        assert!(index.contains(thread_id));
        cleanup_profile(&dir);
    }

    #[test]
    fn history_repair_handles_sqlite_with_only_updated_at_ms() {
        let dir = temp_profile("history-index-updated-ms-only");
        fs::create_dir_all(&dir).expect("mkdir profile");
        let thread_id = "thread-index-ms";
        let conn = Connection::open(dir.join(STATE_DB_FILE)).expect("open sqlite");
        conn.execute(
            "CREATE TABLE threads (
                id TEXT PRIMARY KEY,
                model_provider TEXT,
                title TEXT,
                updated_at_ms INTEGER
            )",
            [],
        )
        .expect("create threads");
        conn.execute(
            "INSERT INTO threads (id, model_provider, title, updated_at_ms)
             VALUES (?1, ?2, 'Thread title', 1770000000000)",
            params![thread_id, DEFAULT_HISTORY_PROVIDER_ID],
        )
        .expect("insert thread");
        drop(conn);

        let summary = repair_history_for_provider(&dir, DEFAULT_HISTORY_PROVIDER_ID);

        assert!(summary.warnings.is_empty(), "{:?}", summary.warnings);
        assert_eq!(summary.added_session_index_entry_count, 1);
        let index = fs::read_to_string(dir.join(SESSION_INDEX_FILE)).expect("read index");
        assert!(index.contains(thread_id));
        assert!(index.contains("2026"));
        cleanup_profile(&dir);
    }

    #[test]
    fn history_repair_reports_sqlite_lock_as_warning() {
        let dir = temp_profile("history-locked");
        fs::create_dir_all(&dir).expect("mkdir profile");
        let thread_id = "thread-locked";
        create_state_db(&dir, thread_id, PROVIDER_ID);
        fs::write(
            dir.join(SESSION_INDEX_FILE),
            format!(
                "{{\"id\":\"{thread_id}\",\"thread_name\":\"Thread title\",\"updated_at\":\"2026-06-06T00:00:00Z\"}}\n"
            ),
        )
        .expect("write session index");
        let lock_conn = Connection::open(dir.join(STATE_DB_FILE)).expect("open lock sqlite");
        lock_conn
            .execute("BEGIN IMMEDIATE", [])
            .expect("begin immediate");

        let summary = repair_history_for_provider(&dir, DEFAULT_HISTORY_PROVIDER_ID);

        assert_eq!(summary.updated_sqlite_row_count, 0);
        assert!(
            summary
                .warnings
                .iter()
                .any(|warning| warning.contains("update Codex history sqlite provider failed")),
            "{:?}",
            summary.warnings
        );
        drop(lock_conn);
        cleanup_profile(&dir);
    }
}
