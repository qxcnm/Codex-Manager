use codexmanager_core::auth::{
    extract_chatgpt_account_id, extract_chatgpt_user_id, extract_workspace_id,
    parse_id_token_claims, IdTokenClaims, DEFAULT_ISSUER,
};
use codexmanager_core::storage::{
    now_ts, Account, AccountImportSnapshot, AccountImportTokenSubject, Storage, Token,
};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::time::Instant;

use crate::account_identity::{
    build_account_storage_id, build_fallback_subject_key, clean_value,
    pick_existing_account_id_by_identity, AccountIdentityView,
};
use crate::storage_helpers::{account_key, open_storage};

const MAX_ERROR_ITEMS: usize = 50;
const DEFAULT_IMPORT_BATCH_SIZE: usize = 200;
const IMPORT_BATCH_SIZE_ENV: &str = "CODEXMANAGER_ACCOUNT_IMPORT_BATCH_SIZE";
const ACCOUNT_SORT_STEP: i64 = 5;
const IMPORT_TOKEN_SUBJECT_PREFIX: &str = "import-token-";

#[derive(Debug, Serialize)]
pub(crate) struct AccountImportResult {
    total: usize,
    created: usize,
    updated: usize,
    failed: usize,
    errors: Vec<AccountImportError>,
    imported_account_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
struct AccountImportError {
    index: usize,
    message: String,
}

#[derive(Debug)]
struct ImportTokenPayload {
    access_token: String,
    id_token: String,
    refresh_token: String,
    account_id_hint: Option<String>,
    chatgpt_account_id_hint: Option<String>,
}

#[derive(Debug)]
struct ImportedAccount {
    account_id: String,
    created: bool,
}

#[derive(Debug, Default)]
struct ImportAccountMeta {
    label: Option<String>,
    issuer: Option<String>,
    group_name: Option<String>,
    note: Option<String>,
    tags: Option<String>,
    workspace_id: Option<String>,
    chatgpt_account_id: Option<String>,
}

#[derive(Default)]
struct ExistingAccountIndex {
    by_id: HashMap<String, AccountImportSnapshot>,
    by_subject_storage_id: HashMap<String, String>,
    by_subject_key: HashMap<String, String>,
    ambiguous_subject_keys: HashSet<String>,
    next_sort: i64,
}

impl ExistingAccountIndex {
    /// 函数 `build`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - storage: 参数 storage
    ///
    /// # 返回
    /// 返回函数执行结果
    fn build(storage: &Storage) -> Result<Self, String> {
        let accounts = storage
            .list_account_import_snapshots()
            .map_err(|e| e.to_string())?;
        let mut idx = ExistingAccountIndex::default();
        for account in accounts {
            idx.next_sort = idx
                .next_sort
                .max(account.sort.saturating_add(ACCOUNT_SORT_STEP));
            idx.by_id.insert(account.id.clone(), account);
        }
        for token in storage
            .list_account_import_token_subjects()
            .map_err(|e| e.to_string())?
        {
            if let Some(account) = idx.by_id.get(&token.account_id).cloned() {
                idx.index_token_subject(&account, &token);
            }
        }
        Ok(idx)
    }

    /// 函数 `find_existing_account_id`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - chatgpt_account_id: 参数 chatgpt_account_id
    /// - workspace_id: 参数 workspace_id
    /// - fallback_subject_key: 参数 fallback_subject_key
    /// - account_id_hint: 参数 account_id_hint
    ///
    /// # 返回
    /// 返回函数执行结果
    fn find_existing_account_id(
        &self,
        chatgpt_account_id: Option<&str>,
        workspace_id: Option<&str>,
        fallback_subject_key: Option<&str>,
        account_id_hint: Option<&str>,
    ) -> Option<String> {
        if let Some(found) =
            self.find_by_subject_identity(chatgpt_account_id, workspace_id, fallback_subject_key)
        {
            return Some(found);
        }
        if fallback_subject_key.is_some() {
            return None;
        }
        pick_existing_account_id_by_identity(
            self.by_id.values(),
            chatgpt_account_id,
            workspace_id,
            None,
            account_id_hint,
        )
    }

    /// 函数 `find_by_subject_identity`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - chatgpt_account_id: 参数 chatgpt_account_id
    /// - workspace_id: 参数 workspace_id
    /// - fallback_subject_key: 参数 fallback_subject_key
    ///
    /// # 返回
    /// 返回函数执行结果
    fn find_by_subject_identity(
        &self,
        chatgpt_account_id: Option<&str>,
        workspace_id: Option<&str>,
        fallback_subject_key: Option<&str>,
    ) -> Option<String> {
        let subject_key = fallback_subject_key
            .map(str::trim)
            .filter(|v| !v.is_empty())?;
        let scoped_id =
            build_account_storage_id(subject_key, chatgpt_account_id, workspace_id, None);
        if let Some(account) = self.by_id.get(&scoped_id) {
            if account_scope_matches(account, chatgpt_account_id, workspace_id) {
                return Some(scoped_id);
            }
            return None;
        }
        if let Some(account_id) = self.by_subject_storage_id.get(&scoped_id) {
            return Some(account_id.clone());
        }
        if chatgpt_account_id.is_some() || workspace_id.is_some() {
            return None;
        }
        if self.by_id.contains_key(subject_key) {
            return Some(subject_key.to_string());
        }
        if let Some(account_id) = self.by_subject_key.get(subject_key) {
            return Some(account_id.clone());
        }
        None
    }

    /// 函数 `upsert_index`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - account: 参数 account
    ///
    /// # 返回
    /// 无
    fn upsert_index(&mut self, account: &Account) {
        self.by_id.insert(
            account.id.clone(),
            account_import_snapshot_from_account(account),
        );
    }

    /// 函数 `index_token_subject`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - account: 参数 account
    /// - token: 参数 token
    ///
    /// # 返回
    /// 无
    fn index_token_subject(
        &mut self,
        account: &AccountImportSnapshot,
        token: &AccountImportTokenSubject,
    ) {
        let Some(subject_account_id) = extract_import_subject_account_id(
            None,
            &token.id_token,
            &token.access_token,
            &token.refresh_token,
        ) else {
            return;
        };
        let Some(subject_key) =
            build_fallback_subject_key(Some(subject_account_id.as_str()), None::<&str>)
        else {
            return;
        };
        let scoped_id = build_account_storage_id(
            subject_key.as_str(),
            account.chatgpt_account_id.as_deref(),
            account.workspace_id.as_deref(),
            None,
        );
        self.by_subject_storage_id
            .insert(scoped_id, account.id.clone());
        self.record_subject_key(subject_key, account.id.clone());
    }

    /// 函数 `record_subject_key`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - subject_key: 参数 subject_key
    /// - account_id: 参数 account_id
    ///
    /// # 返回
    /// 无
    fn record_subject_key(&mut self, subject_key: String, account_id: String) {
        if self.ambiguous_subject_keys.contains(&subject_key) {
            return;
        }
        match self.by_subject_key.get(&subject_key) {
            Some(existing) if existing == &account_id => {}
            Some(_) => {
                self.by_subject_key.remove(&subject_key);
                self.ambiguous_subject_keys.insert(subject_key);
            }
            None => {
                self.by_subject_key.insert(subject_key, account_id);
            }
        }
    }
}

impl AccountIdentityView for AccountImportSnapshot {
    fn account_id(&self) -> &str {
        self.id.as_str()
    }

    fn chatgpt_account_id(&self) -> Option<&str> {
        self.chatgpt_account_id.as_deref()
    }

    fn workspace_id(&self) -> Option<&str> {
        self.workspace_id.as_deref()
    }
}

fn account_scope_matches(
    account: &AccountImportSnapshot,
    chatgpt_account_id: Option<&str>,
    workspace_id: Option<&str>,
) -> bool {
    if let Some(chatgpt) = chatgpt_account_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if account.chatgpt_account_id.as_deref().map(str::trim) != Some(chatgpt) {
            return false;
        }
    }
    if let Some(workspace) = workspace_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if account.workspace_id.as_deref().map(str::trim) != Some(workspace) {
            return false;
        }
    }
    true
}

fn resolve_scoped_account_id_collision(
    existing: &HashMap<String, AccountImportSnapshot>,
    account_id: String,
    chatgpt_account_id: Option<&str>,
    workspace_id: Option<&str>,
    payload: &ImportTokenPayload,
) -> String {
    let Some(account) = existing.get(&account_id) else {
        return account_id;
    };
    if account_scope_matches(account, chatgpt_account_id, workspace_id) {
        return account_id;
    }

    let fingerprint_source = if payload.refresh_token.trim().is_empty() {
        payload.access_token.as_str()
    } else {
        payload.refresh_token.as_str()
    };
    let fingerprint = token_fingerprint(fingerprint_source);
    let mut candidate = account_key(&account_id, Some(&format!("conflict_fp_{fingerprint}")));
    let mut suffix = 2usize;
    while existing.contains_key(&candidate) {
        candidate = account_key(
            &account_id,
            Some(&format!("conflict_fp_{fingerprint}_{suffix}")),
        );
        suffix = suffix.saturating_add(1);
    }
    candidate
}

fn account_import_snapshot_from_account(account: &Account) -> AccountImportSnapshot {
    AccountImportSnapshot {
        id: account.id.clone(),
        label: account.label.clone(),
        issuer: account.issuer.clone(),
        chatgpt_account_id: account.chatgpt_account_id.clone(),
        workspace_id: account.workspace_id.clone(),
        sort: account.sort,
        created_at: account.created_at,
    }
}

/// 函数 `import_account_auth_json`
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
pub(crate) fn import_account_auth_json(
    contents: Vec<String>,
) -> Result<AccountImportResult, String> {
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let mut index = ExistingAccountIndex::build(&storage)?;
    let mut result = AccountImportResult {
        total: 0,
        created: 0,
        updated: 0,
        failed: 0,
        errors: Vec::new(),
        imported_account_ids: Vec::new(),
    };
    let mut progress = AccountImportProgress::new();
    let batch_size = import_batch_size();

    for content in contents {
        match parse_items_from_content(&content) {
            Ok(items) => {
                import_items_in_batches(
                    &storage,
                    &mut index,
                    &mut result,
                    &mut progress,
                    items,
                    batch_size,
                );
            }
            Err(err) => {
                record_import_error(&mut result, &mut progress, err);
            }
        }
    }

    progress.finish();
    Ok(result)
}

/// 函数 `import_batch_size`
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
fn import_batch_size() -> usize {
    std::env::var(IMPORT_BATCH_SIZE_ENV)
        .ok()
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_IMPORT_BATCH_SIZE)
}

/// 函数 `import_items_in_batches`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - storage: 参数 storage
/// - index: 参数 index
/// - result: 参数 result
/// - progress: 参数 progress
/// - items: 参数 items
/// - batch_size: 参数 batch_size
///
/// # 返回
/// 无
fn import_items_in_batches(
    storage: &Storage,
    index: &mut ExistingAccountIndex,
    result: &mut AccountImportResult,
    progress: &mut AccountImportProgress,
    items: Vec<Value>,
    batch_size: usize,
) {
    if items.is_empty() {
        return;
    }
    let total_batches = items.len().div_ceil(batch_size);
    for (batch_index, batch) in items.chunks(batch_size).enumerate() {
        progress.begin_batch(batch_index + 1, total_batches, batch.len());
        for item in batch {
            result.total += 1;
            let current_index = result.total;
            match import_single_item_with_account_id(storage, index, item, current_index) {
                Ok(imported) => {
                    if imported.created {
                        result.created += 1;
                    } else {
                        result.updated += 1;
                    }
                    if !result
                        .imported_account_ids
                        .iter()
                        .any(|id| id == &imported.account_id)
                    {
                        result
                            .imported_account_ids
                            .push(imported.account_id.clone());
                    }
                    progress.on_item_success(imported.created);
                    let _ = crate::usage_refresh::enqueue_usage_refresh_after_account_add(
                        &imported.account_id,
                    );
                }
                Err(err) => {
                    result.failed += 1;
                    progress.on_item_failure();
                    if result.errors.len() < MAX_ERROR_ITEMS {
                        result.errors.push(AccountImportError {
                            index: current_index,
                            message: err,
                        });
                    }
                }
            }
        }
        progress.finish_batch();
    }
}

/// 函数 `record_import_error`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - result: 参数 result
/// - progress: 参数 progress
/// - message: 参数 message
///
/// # 返回
/// 无
fn record_import_error(
    result: &mut AccountImportResult,
    progress: &mut AccountImportProgress,
    message: String,
) {
    result.total += 1;
    result.failed += 1;
    progress.on_item_failure();
    if result.errors.len() < MAX_ERROR_ITEMS {
        result.errors.push(AccountImportError {
            index: result.total,
            message,
        });
    }
}

#[derive(Debug)]
struct AccountImportProgress {
    started_at: Instant,
    processed: usize,
    created: usize,
    updated: usize,
    failed: usize,
    active_batch: Option<AccountImportBatchProgress>,
}

#[derive(Debug)]
struct AccountImportBatchProgress {
    index: usize,
    total: usize,
    size: usize,
    processed: usize,
    created: usize,
    updated: usize,
    failed: usize,
}

impl AccountImportProgress {
    /// 函数 `new`
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
    fn new() -> Self {
        Self {
            started_at: Instant::now(),
            processed: 0,
            created: 0,
            updated: 0,
            failed: 0,
            active_batch: None,
        }
    }

    /// 函数 `begin_batch`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - index: 参数 index
    /// - total: 参数 total
    /// - size: 参数 size
    ///
    /// # 返回
    /// 无
    fn begin_batch(&mut self, index: usize, total: usize, size: usize) {
        self.active_batch = Some(AccountImportBatchProgress {
            index,
            total,
            size,
            processed: 0,
            created: 0,
            updated: 0,
            failed: 0,
        });
    }

    /// 函数 `on_item_success`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - created: 参数 created
    ///
    /// # 返回
    /// 无
    fn on_item_success(&mut self, created: bool) {
        self.processed += 1;
        if created {
            self.created += 1;
        } else {
            self.updated += 1;
        }
        if let Some(batch) = self.active_batch.as_mut() {
            batch.processed += 1;
            if created {
                batch.created += 1;
            } else {
                batch.updated += 1;
            }
        }
    }

    /// 函数 `on_item_failure`
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
    fn on_item_failure(&mut self) {
        self.processed += 1;
        self.failed += 1;
        if let Some(batch) = self.active_batch.as_mut() {
            batch.processed += 1;
            batch.failed += 1;
        }
    }

    /// 函数 `finish_batch`
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
    fn finish_batch(&mut self) {
        if let Some(batch) = self.active_batch.take() {
            log::info!(
                "account import batch finished: {}/{} size={} processed={} created={} updated={} failed={} total_processed={} elapsed_ms={}",
                batch.index,
                batch.total,
                batch.size,
                batch.processed,
                batch.created,
                batch.updated,
                batch.failed,
                self.processed,
                self.started_at.elapsed().as_millis()
            );
        }
    }

    /// 函数 `finish`
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
    fn finish(&self) {
        log::info!(
            "account import finished: processed={} created={} updated={} failed={} elapsed_ms={}",
            self.processed,
            self.created,
            self.updated,
            self.failed,
            self.started_at.elapsed().as_millis()
        );
    }
}

/// 函数 `parse_items_from_content`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - content: 参数 content
///
/// # 返回
/// 返回函数执行结果
fn parse_items_from_content(content: &str) -> Result<Vec<Value>, String> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }

    if trimmed.starts_with('[') {
        let values: Vec<Value> =
            serde_json::from_str(trimmed).map_err(|err| format!("invalid JSON array: {err}"))?;
        return Ok(values);
    }

    let mut out = Vec::new();
    let stream = serde_json::Deserializer::from_str(trimmed).into_iter::<Value>();
    for value in stream {
        out.push(value.map_err(|err| format!("invalid JSON object stream: {err}"))?);
    }
    Ok(out)
}

/// 函数 `import_single_item`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - storage: 参数 storage
/// - index: 参数 index
/// - item: 参数 item
/// - sequence: 参数 sequence
///
/// # 返回
/// 返回函数执行结果
#[cfg(test)]
fn import_single_item(
    storage: &Storage,
    index: &mut ExistingAccountIndex,
    item: &Value,
    sequence: usize,
) -> Result<bool, String> {
    import_single_item_with_account_id(storage, index, item, sequence)
        .map(|imported| imported.created)
}

fn import_single_item_with_account_id(
    storage: &Storage,
    index: &mut ExistingAccountIndex,
    item: &Value,
    sequence: usize,
) -> Result<ImportedAccount, String> {
    let imported_identity = crate::account_agent_identity::parse_imported_agent_identity(item)?;
    let payload = match extract_token_payload(item) {
        Ok(payload) => payload,
        Err(_error) if imported_identity.is_some() => {
            let identity = imported_identity.as_ref().expect("identity checked");
            ImportTokenPayload {
                access_token: String::new(),
                id_token: String::new(),
                refresh_token: String::new(),
                account_id_hint: identity.account_id.clone(),
                chatgpt_account_id_hint: identity.account_id.clone(),
            }
        }
        Err(error) => return Err(error),
    };
    let meta = extract_account_meta(item);
    let claims = parse_id_token_claims(&payload.id_token).ok();
    let token_fingerprint = token_fingerprint(&payload.refresh_token);
    let subject_account_id = extract_import_subject_account_id(
        claims.as_ref(),
        &payload.id_token,
        &payload.access_token,
        &payload.refresh_token,
    )
    .or_else(|| {
        imported_identity
            .as_ref()
            .and_then(|identity| identity.chatgpt_user_id.clone())
    })
    .or_else(|| {
        imported_identity
            .as_ref()
            .and_then(|identity| identity.account_id.clone())
    });
    let chatgpt_account_id = clean_value(
        meta.chatgpt_account_id
            .clone()
            .or_else(|| payload.chatgpt_account_id_hint.clone())
            .or_else(|| {
                claims
                    .as_ref()
                    .and_then(|c| c.auth.as_ref()?.chatgpt_account_id.clone())
            })
            .or_else(|| extract_chatgpt_account_id(&payload.id_token))
            .or_else(|| extract_chatgpt_account_id(&payload.access_token)),
    );

    let workspace_id = clean_value(
        meta.workspace_id
            .clone()
            .or_else(|| claims.as_ref().and_then(|c| c.workspace_id.clone()))
            .or_else(|| extract_workspace_id(&payload.id_token))
            .or_else(|| extract_workspace_id(&payload.access_token))
            .or_else(|| payload.account_id_hint.clone())
            .or_else(|| chatgpt_account_id.clone()),
    );
    let fallback_subject_key =
        build_fallback_subject_key(subject_account_id.as_deref(), None::<&str>);
    let token_fingerprint_for_id = match subject_account_id.as_deref() {
        Some(subject) if subject.starts_with(IMPORT_TOKEN_SUBJECT_PREFIX) => None,
        _ => Some(token_fingerprint.as_str()),
    };
    let account_id = index
        .find_existing_account_id(
            chatgpt_account_id.as_deref(),
            workspace_id.as_deref(),
            fallback_subject_key.as_deref(),
            payload.account_id_hint.as_deref(),
        )
        .unwrap_or(resolve_logical_account_id(
            &payload,
            subject_account_id.as_deref(),
            chatgpt_account_id.as_deref(),
            workspace_id.as_deref(),
            token_fingerprint_for_id,
        )?);
    let account_id = resolve_scoped_account_id_collision(
        &index.by_id,
        account_id,
        chatgpt_account_id.as_deref(),
        workspace_id.as_deref(),
        &payload,
    );

    let label = imported_identity
        .as_ref()
        .and_then(|identity| identity.email.clone())
        .or_else(|| {
            meta.label.clone().or_else(|| {
                claims
                    .as_ref()
                    .and_then(|c| c.email.clone())
                    .filter(|v| !v.trim().is_empty())
            })
        })
        .or_else(|| {
            item.get("email")
                .and_then(Value::as_str)
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        })
        .unwrap_or_else(|| format!("导入账号{:04}", sequence));
    let default_issuer =
        std::env::var("CODEXMANAGER_ISSUER").unwrap_or_else(|_| DEFAULT_ISSUER.to_string());
    let issuer = meta
        .issuer
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(default_issuer);
    let group_name = meta
        .group_name
        .clone()
        .filter(|value| !value.trim().is_empty());
    let note = meta.note.clone().filter(|value| !value.trim().is_empty());
    let tags = meta.tags.clone().filter(|value| !value.trim().is_empty());

    let now = now_ts();
    let (account_id, account, created) =
        if let Some(existing) = index.by_id.get(&account_id).cloned() {
            let merged_chatgpt_account_id = chatgpt_account_id
                .clone()
                .or_else(|| clean_value(existing.chatgpt_account_id.clone()));
            let merged_workspace_id = workspace_id
                .clone()
                .or_else(|| clean_value(existing.workspace_id.clone()));
            let updated = Account {
                id: existing.id.clone(),
                label: if existing.label.trim().is_empty() {
                    label
                } else {
                    existing.label.clone()
                },
                issuer: if existing.issuer.trim().is_empty() {
                    issuer
                } else {
                    existing.issuer.clone()
                },
                chatgpt_account_id: merged_chatgpt_account_id,
                workspace_id: merged_workspace_id,
                group_name,
                sort: existing.sort,
                status: "active".to_string(),
                created_at: existing.created_at,
                updated_at: now,
            };
            (existing.id.clone(), updated, false)
        } else {
            let next_sort = index.next_sort;
            index.next_sort = index.next_sort.saturating_add(ACCOUNT_SORT_STEP);
            let created = Account {
                id: account_id.clone(),
                label,
                issuer,
                chatgpt_account_id: chatgpt_account_id.clone(),
                workspace_id,
                group_name,
                sort: next_sort,
                status: "active".to_string(),
                created_at: now,
                updated_at: now,
            };
            (account_id.clone(), created, true)
        };

    let token = Token {
        account_id: account_id.clone(),
        id_token: payload.id_token,
        access_token: payload.access_token,
        refresh_token: payload.refresh_token,
        api_key_access_token: None,
        last_refresh: now,
    };
    storage
        .upsert_imported_account_bundle(&account, note.as_deref(), tags.as_deref(), &token)
        .map_err(|e| e.to_string())?;
    // An import may replace the token of an existing account. Its previous
    // probe result must not authorize the new credential into a gateway batch.
    // Import/model discovery remains separate evidence. Batch refill performs
    // its own real Responses admission before customer traffic can use it.
    storage
        .mark_adapter_credential_unprobed("codex", &account_id, now)
        .map_err(|error| format!("reset imported account probe state failed: {error}"))?;
    if let Some(identity) = imported_identity.as_ref() {
        crate::account_agent_identity::save_imported_agent_identity(
            storage,
            &account_id,
            identity,
        )?;
    }
    index.upsert_index(&account);
    let account_snapshot = account_import_snapshot_from_account(&account);
    let token_subject = AccountImportTokenSubject {
        account_id: token.account_id.clone(),
        id_token: token.id_token.clone(),
        access_token: token.access_token.clone(),
        refresh_token: token.refresh_token.clone(),
    };
    index.index_token_subject(&account_snapshot, &token_subject);
    Ok(ImportedAccount {
        account_id,
        created,
    })
}

/// 函数 `extract_import_subject_account_id`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - claims: 参数 claims
/// - id_token: 参数 id_token
/// - access_token: 参数 access_token
/// - refresh_token: 参数 refresh_token
///
/// # 返回
/// 返回函数执行结果
fn extract_import_subject_account_id(
    claims: Option<&IdTokenClaims>,
    id_token: &str,
    access_token: &str,
    refresh_token: &str,
) -> Option<String> {
    clean_value(
        claims
            .and_then(|c| {
                c.auth.as_ref().and_then(|auth| {
                    auth.chatgpt_user_id
                        .clone()
                        .or_else(|| auth.user_id.clone())
                })
            })
            .or_else(|| {
                claims
                    .map(|c| c.sub.trim().to_string())
                    .filter(|v| !v.is_empty())
            })
            .or_else(|| extract_chatgpt_user_id(id_token))
            .or_else(|| extract_chatgpt_user_id(access_token))
            .or_else(|| {
                if refresh_token.trim().is_empty() {
                    None
                } else {
                    let token_fingerprint = token_fingerprint(refresh_token);
                    Some(format!("{IMPORT_TOKEN_SUBJECT_PREFIX}{token_fingerprint}"))
                }
            }),
    )
}

/// 函数 `extract_token_payload`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - item: 参数 item
///
/// # 返回
/// 返回函数执行结果
fn extract_token_payload(item: &Value) -> Result<ImportTokenPayload, String> {
    let tokens = item.get("tokens").unwrap_or(item);
    let access_token = required_string_any(
        &[
            (tokens, "access_token"),
            (tokens, "accessToken"),
            (item, "access_token"),
            (item, "accessToken"),
        ],
        "access_token/accessToken",
    )?;
    let id_token = optional_string_any(&[
        (tokens, "id_token"),
        (tokens, "idToken"),
        (item, "id_token"),
        (item, "idToken"),
    ])
    .unwrap_or_default();
    let refresh_token = optional_string_any(&[
        (tokens, "refresh_token"),
        (tokens, "refreshToken"),
        (item, "refresh_token"),
        (item, "refreshToken"),
    ])
    .unwrap_or_default();
    let account_id_hint = optional_string_any(&[
        (tokens, "account_id"),
        (tokens, "accountId"),
        (item, "account_id"),
        (item, "accountId"),
    ]);
    let chatgpt_account_id_hint = optional_string_any(&[
        (tokens, "chatgpt_account_id"),
        (tokens, "chatgptAccountId"),
        (item, "chatgpt_account_id"),
        (item, "chatgptAccountId"),
    ]);
    Ok(ImportTokenPayload {
        access_token,
        id_token,
        refresh_token,
        account_id_hint,
        chatgpt_account_id_hint,
    })
}

/// 函数 `resolve_logical_account_id`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - payload: 参数 payload
/// - subject_account_id: 参数 subject_account_id
/// - chatgpt_account_id: 参数 chatgpt_account_id
/// - workspace_id: 参数 workspace_id
/// - token_fingerprint: 参数 token_fingerprint
///
/// # 返回
/// 返回函数执行结果
fn resolve_logical_account_id(
    payload: &ImportTokenPayload,
    subject_account_id: Option<&str>,
    chatgpt_account_id: Option<&str>,
    workspace_id: Option<&str>,
    token_fingerprint: Option<&str>,
) -> Result<String, String> {
    let account_id_hint = payload
        .account_id_hint
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty());
    let hint_suffix = account_id_hint.and_then(|value| {
        value
            .split_once("::")
            .map(|(_, suffix)| suffix.trim())
            .filter(|suffix| !suffix.is_empty())
    });

    if let Some(sub) = subject_account_id.map(str::trim).filter(|v| !v.is_empty()) {
        let scoped_id = build_account_storage_id(sub, chatgpt_account_id, workspace_id, None);
        if scoped_id != sub {
            return Ok(scoped_id);
        }
        if let Some(v) = hint_suffix {
            return Ok(account_key(sub, Some(&format!("hint={v}"))));
        }
        if let Some(fp) = token_fingerprint.map(str::trim).filter(|v| !v.is_empty()) {
            return Ok(account_key(sub, Some(&format!("fp_{fp}"))));
        }
        return Ok(sub.to_string());
    }

    let chatgpt = chatgpt_account_id
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
        .or_else(|| extract_chatgpt_account_id(&payload.id_token))
        .or_else(|| extract_chatgpt_account_id(&payload.access_token));
    let workspace = workspace_id
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string);
    if let Some(chatgpt) = chatgpt.as_ref() {
        if let Some(workspace) = workspace.as_ref() {
            if chatgpt != workspace {
                return Ok(account_key(chatgpt, Some(workspace)));
            }
        }
        return Ok(chatgpt.to_string());
    }

    if let Some(value) = account_id_hint {
        return Ok(value.to_string());
    }

    if let Some(workspace) = workspace {
        return Ok(workspace);
    }

    Err("unable to resolve account id from tokens.account_id / id_token / access_token".to_string())
}

/// 函数 `token_fingerprint`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - refresh_token: 参数 refresh_token
///
/// # 返回
/// 返回函数执行结果
fn token_fingerprint(refresh_token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(refresh_token.as_bytes());
    let digest = hasher.finalize();
    let mut out = String::with_capacity(12);
    for b in digest.iter().take(6) {
        out.push_str(&format!("{:02x}", b));
    }
    out
}

/// 函数 `extract_account_meta`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - item: 参数 item
///
/// # 返回
/// 返回函数执行结果
fn extract_account_meta(item: &Value) -> ImportAccountMeta {
    let meta = item.get("meta").unwrap_or(item);
    ImportAccountMeta {
        label: optional_string_any(&[(meta, "label"), (item, "label")]),
        issuer: optional_string_any(&[(meta, "issuer"), (item, "issuer")]),
        group_name: optional_string_any(&[
            (meta, "group_name"),
            (meta, "groupName"),
            (item, "group_name"),
            (item, "groupName"),
        ]),
        note: optional_string_any(&[(meta, "note"), (item, "note")]),
        tags: optional_tags_any(&[(meta, "tags"), (item, "tags")]),
        workspace_id: optional_string_any(&[
            (meta, "workspace_id"),
            (meta, "workspaceId"),
            (item, "workspace_id"),
            (item, "workspaceId"),
        ]),
        chatgpt_account_id: optional_string_any(&[
            (meta, "chatgpt_account_id"),
            (meta, "chatgptAccountId"),
            (item, "chatgpt_account_id"),
            (item, "chatgptAccountId"),
        ]),
    }
}

/// 函数 `required_string`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - value: 参数 value
/// - key: 参数 key
///
/// # 返回
/// 返回函数执行结果
fn required_string(value: &Value, key: &str) -> Result<String, String> {
    let raw = value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("missing field: tokens.{key}"))?;
    let out = raw.trim();
    if out.is_empty() {
        return Err(format!("empty field: tokens.{key}"));
    }
    Ok(out.to_string())
}

/// 函数 `required_string_any`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - candidates: 参数 candidates
/// - label: 参数 label
///
/// # 返回
/// 返回函数执行结果
fn required_string_any(candidates: &[(&Value, &str)], label: &str) -> Result<String, String> {
    for (value, key) in candidates {
        if let Ok(found) = required_string(value, key) {
            return Ok(found);
        }
    }
    Err(format!("missing field: {label}"))
}

/// 函数 `optional_string`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - value: 参数 value
/// - key: 参数 key
///
/// # 返回
/// 返回函数执行结果
fn optional_string(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

/// 函数 `optional_string_any`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - candidates: 参数 candidates
///
/// # 返回
/// 返回函数执行结果
fn optional_string_any(candidates: &[(&Value, &str)]) -> Option<String> {
    for (value, key) in candidates {
        if let Some(found) = optional_string(value, key) {
            return Some(found);
        }
    }
    None
}

/// 函数 `optional_tags`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - value: 参数 value
/// - key: 参数 key
///
/// # 返回
/// 返回函数执行结果
fn optional_tags(value: &Value, key: &str) -> Option<String> {
    let value = value.get(key)?;
    if let Some(text) = value.as_str() {
        let normalized = text
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .collect::<Vec<_>>();
        if normalized.is_empty() {
            None
        } else {
            Some(normalized.join(","))
        }
    } else if let Some(items) = value.as_array() {
        let normalized = items
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .collect::<Vec<_>>();
        if normalized.is_empty() {
            None
        } else {
            Some(normalized.join(","))
        }
    } else {
        None
    }
}

/// 函数 `optional_tags_any`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - candidates: 参数 candidates
///
/// # 返回
/// 返回函数执行结果
fn optional_tags_any(candidates: &[(&Value, &str)]) -> Option<String> {
    for (value, key) in candidates {
        if let Some(found) = optional_tags(value, key) {
            return Some(found);
        }
    }
    None
}

#[cfg(test)]
#[path = "tests/account_import_tests.rs"]
mod tests;
