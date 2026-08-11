use codexmanager_core::{
    auth::extract_token_exp,
    rpc::types::{AccountListResult, AccountSummary},
    storage::{
        Account, AccountListSummaryRow, AccountMetadata, AccountQuotaCapacityOverride,
        AccountSubscription, AccountSummaryStorageSnapshot, AccountSummaryStorageSnapshotOptions,
        AccountTokenPlan, AdapterCredentialProbeState, UsageSnapshotRecord,
    },
};
use std::collections::HashMap;

use crate::account_plan::resolve_effective_account_plan;
use crate::account_status::derive_credential_state;
use crate::storage_helpers::open_storage;

const DEFAULT_ACCOUNT_PAGE_SIZE: i64 = 5;

#[derive(Debug)]
pub(crate) struct AccountSummaryContext {
    pub items: Vec<AccountSummary>,
    pub usage_snapshots: Vec<UsageSnapshotRecord>,
}

#[derive(Debug)]
struct AccountSummaryParts {
    id: String,
    label: String,
    group_name: Option<String>,
    sort: i64,
    status: String,
    created_at: i64,
    updated_at: i64,
}

impl From<Account> for AccountSummaryParts {
    fn from(account: Account) -> Self {
        Self {
            id: account.id,
            label: account.label,
            group_name: account.group_name,
            sort: account.sort,
            status: account.status,
            created_at: account.created_at,
            updated_at: account.updated_at,
        }
    }
}

impl From<AccountListSummaryRow> for AccountSummaryParts {
    fn from(account: AccountListSummaryRow) -> Self {
        Self {
            id: account.id,
            label: account.label,
            group_name: account.group_name,
            sort: account.sort,
            status: account.status,
            created_at: account.created_at,
            updated_at: account.updated_at,
        }
    }
}

#[derive(Debug)]
struct AccountSummarySetup {
    preferred_account_id: Option<String>,
    status_reasons: HashMap<String, String>,
    tokens: HashMap<String, AccountTokenPlan>,
    usage_snapshots: Vec<UsageSnapshotRecord>,
    metadata: HashMap<String, AccountMetadata>,
    subscriptions: HashMap<String, AccountSubscription>,
    model_slugs_by_account: HashMap<String, Vec<String>>,
    quota_overrides: HashMap<String, AccountQuotaCapacityOverride>,
    agent_identities: HashMap<String, codexmanager_core::storage::CodexAgentIdentityRecord>,
    gateway_probe_states: HashMap<String, AdapterCredentialProbeState>,
}

impl From<&Account> for AccountSummaryParts {
    fn from(account: &Account) -> Self {
        Self {
            id: account.id.clone(),
            label: account.label.clone(),
            group_name: account.group_name.clone(),
            sort: account.sort,
            status: account.status.clone(),
            created_at: account.created_at,
            updated_at: account.updated_at,
        }
    }
}

/// 函数 `read_accounts`
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
pub(crate) fn read_accounts() -> Result<AccountListResult, String> {
    let storage = open_storage().ok_or_else(|| "open storage failed".to_string())?;
    let db_path = std::env::var("CODEXMANAGER_DB_PATH").unwrap_or_else(|_| "<unset>".to_string());
    let accounts = storage
        .list_account_summary_rows()
        .map_err(|err| format!("list accounts failed: {err}"))?;
    let total = accounts.len() as i64;
    let context = build_account_summary_context_from_rows(&storage, accounts)?;
    let items = context.items;
    let page_size = if total > 0 {
        total
    } else {
        DEFAULT_ACCOUNT_PAGE_SIZE
    };

    log::info!(
        "account/list read: db_path={} total={} item_count={}",
        db_path,
        total,
        items.len()
    );

    Ok(AccountListResult {
        items,
        total,
        page: 1,
        page_size,
    })
}

/// 函数 `to_account_summary_with_reason`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - acc: 参数 acc
/// - status_reason: 参数 status_reason
/// - plan_type: 参数 plan_type
/// - plan_type_raw: 参数 plan_type_raw
/// - note: 参数 note
/// - tags: 参数 tags
///
/// # 返回
/// 返回函数执行结果
fn to_account_summary_with_reason(
    parts: AccountSummaryParts,
    preferred: bool,
    status_reason: Option<String>,
    has_token: bool,
    auth_mode: String,
    agent_identity_status: Option<String>,
    has_agent_identity_task: bool,
    credential_state: String,
    credential_action: String,
    access_token_expires_at: Option<i64>,
    gateway_probe_state: Option<&AdapterCredentialProbeState>,
    plan_type: Option<String>,
    plan_type_raw: Option<String>,
    has_subscription: Option<bool>,
    subscription_plan: Option<String>,
    subscription_expires_at: Option<i64>,
    subscription_renews_at: Option<i64>,
    note: Option<String>,
    tags: Option<String>,
    model_slugs: Vec<String>,
    quota_capacity_primary_window_tokens: Option<i64>,
    quota_capacity_secondary_window_tokens: Option<i64>,
) -> AccountSummary {
    AccountSummary {
        id: parts.id,
        label: parts.label,
        group_name: parts.group_name,
        preferred,
        sort: parts.sort,
        status: parts.status,
        status_reason,
        has_token,
        auth_mode,
        agent_identity_status,
        has_agent_identity_task,
        credential_state,
        credential_action,
        access_token_expires_at,
        gateway_probe_status: gateway_probe_state.map(|state| state.status.clone()),
        gateway_probe_reason: gateway_probe_state.and_then(|state| state.error_code.clone()),
        gateway_probe_checked_at: gateway_probe_state.map(|state| state.checked_at),
        gateway_probe_retry_after: gateway_probe_state.and_then(|state| state.retry_after),
        plan_type,
        plan_type_raw,
        has_subscription,
        subscription_plan,
        subscription_expires_at,
        subscription_renews_at,
        note,
        tags,
        model_slugs,
        quota_capacity_primary_window_tokens,
        quota_capacity_secondary_window_tokens,
        created_at: parts.created_at,
        updated_at: parts.updated_at,
    }
}

/// 函数 `to_account_summaries`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - storage: 参数 storage
/// - accounts: 参数 accounts
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn build_account_summary_context_from_rows(
    storage: &codexmanager_core::storage::Storage,
    accounts: Vec<AccountListSummaryRow>,
) -> Result<AccountSummaryContext, String> {
    build_account_summary_context_from_rows_with_options(
        storage,
        accounts,
        AccountSummaryStorageSnapshotOptions::default(),
    )
}

pub(crate) fn build_account_summary_context_from_rows_with_options(
    storage: &codexmanager_core::storage::Storage,
    accounts: Vec<AccountListSummaryRow>,
    options: AccountSummaryStorageSnapshotOptions,
) -> Result<AccountSummaryContext, String> {
    build_account_summary_context_for_items(storage, accounts, options)
}

fn build_account_summary_context_for_items<A>(
    storage: &codexmanager_core::storage::Storage,
    accounts: Vec<A>,
    options: AccountSummaryStorageSnapshotOptions,
) -> Result<AccountSummaryContext, String>
where
    A: Into<AccountSummaryParts> + AsAccountId,
{
    if accounts.is_empty() {
        return Ok(AccountSummaryContext {
            items: Vec::new(),
            usage_snapshots: Vec::new(),
        });
    }
    let account_ids = accounts
        .iter()
        .map(|account| account.account_id().to_string())
        .collect::<Vec<_>>();
    let setup = load_account_summary_setup(storage, &account_ids, options)?;
    let items = build_account_summary_items(accounts, &setup);
    Ok(AccountSummaryContext {
        items,
        usage_snapshots: setup.usage_snapshots,
    })
}

trait AsAccountId {
    fn account_id(&self) -> &str;
}

impl AsAccountId for Account {
    fn account_id(&self) -> &str {
        self.id.as_str()
    }
}

impl AsAccountId for AccountListSummaryRow {
    fn account_id(&self) -> &str {
        self.id.as_str()
    }
}

fn load_account_summary_setup(
    storage: &codexmanager_core::storage::Storage,
    account_ids: &[String],
    options: AccountSummaryStorageSnapshotOptions,
) -> Result<AccountSummarySetup, String> {
    let snapshot = storage
        .load_account_summary_storage_snapshot_with_options(account_ids, options)
        .map_err(|err| format!("load account summary snapshot failed: {err}"))?;
    let mut setup = account_summary_setup_from_snapshot(snapshot);
    setup.agent_identities = storage
        .list_codex_agent_identities()
        .map_err(|error| format!("load agent identities failed: {error}"))?
        .into_iter()
        .filter(|item| account_ids.iter().any(|id| id == &item.account_id))
        .map(|item| (item.account_id.clone(), item))
        .collect();
    setup.gateway_probe_states = storage
        .list_adapter_credential_probe_states("codex", account_ids)
        .map_err(|error| format!("load gateway probe states failed: {error}"))?
        .into_iter()
        .map(|state| (state.credential_id.clone(), state))
        .collect();
    Ok(setup)
}

fn account_summary_setup_from_snapshot(
    snapshot: AccountSummaryStorageSnapshot,
) -> AccountSummarySetup {
    let tokens = snapshot
        .tokens
        .into_iter()
        .map(|token| (token.account_id.clone(), token))
        .collect::<HashMap<String, AccountTokenPlan>>();
    let metadata = snapshot
        .metadata
        .into_iter()
        .map(|item| (item.account_id.clone(), item))
        .collect::<HashMap<String, AccountMetadata>>();
    let subscriptions = snapshot
        .subscriptions
        .into_iter()
        .map(|item| (item.account_id.clone(), item))
        .collect::<HashMap<String, AccountSubscription>>();
    let mut model_slugs_by_account: HashMap<String, Vec<String>> = HashMap::new();
    for assignment in snapshot.model_assignments {
        model_slugs_by_account
            .entry(assignment.source_id)
            .or_default()
            .push(assignment.model_slug);
    }
    let quota_overrides = snapshot
        .quota_overrides
        .into_iter()
        .map(|item| (item.account_id.clone(), item))
        .collect::<HashMap<String, AccountQuotaCapacityOverride>>();
    AccountSummarySetup {
        preferred_account_id: snapshot.preferred_account_id,
        status_reasons: snapshot.status_reasons,
        tokens,
        usage_snapshots: snapshot.usage_snapshots,
        metadata,
        subscriptions,
        model_slugs_by_account,
        quota_overrides,
        agent_identities: HashMap::new(),
        gateway_probe_states: HashMap::new(),
    }
}

fn build_account_summary_items<I, A>(
    accounts: I,
    setup: &AccountSummarySetup,
) -> Vec<AccountSummary>
where
    I: IntoIterator<Item = A>,
    A: Into<AccountSummaryParts>,
{
    let usages = setup
        .usage_snapshots
        .iter()
        .map(|snapshot| (snapshot.account_id.clone(), snapshot))
        .collect::<HashMap<String, &UsageSnapshotRecord>>();
    accounts
        .into_iter()
        .map(|account| {
            map_account_summary(
                account,
                setup.preferred_account_id.as_deref(),
                &setup.status_reasons,
                &setup.tokens,
                &usages,
                &setup.metadata,
                &setup.subscriptions,
                &setup.model_slugs_by_account,
                &setup.quota_overrides,
                &setup.agent_identities,
                &setup.gateway_probe_states,
            )
        })
        .collect()
}

/// 函数 `map_account_summary`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - account: 参数 account
/// - status_reasons: 参数 status_reasons
/// - tokens: 参数 tokens
/// - usages: 参数 usages
/// - metadata: 参数 metadata
///
/// # 返回
/// 返回函数执行结果
fn map_account_summary<A>(
    account: A,
    preferred_account_id: Option<&str>,
    status_reasons: &HashMap<String, String>,
    tokens: &HashMap<String, AccountTokenPlan>,
    usages: &HashMap<String, &UsageSnapshotRecord>,
    metadata: &HashMap<String, AccountMetadata>,
    subscriptions: &HashMap<String, AccountSubscription>,
    model_slugs_by_account: &HashMap<String, Vec<String>>,
    quota_overrides: &HashMap<String, AccountQuotaCapacityOverride>,
    agent_identities: &HashMap<String, codexmanager_core::storage::CodexAgentIdentityRecord>,
    gateway_probe_states: &HashMap<String, AdapterCredentialProbeState>,
) -> AccountSummary
where
    A: Into<AccountSummaryParts>,
{
    let account = account.into();
    let AccountSummaryParts {
        id: account_id,
        label,
        group_name,
        sort,
        status,
        created_at,
        updated_at,
    } = account;
    let status_reason = status_reasons.get(&account_id).cloned();
    let preferred = preferred_account_id.is_some_and(|id| id == account_id);
    let subscription = subscriptions.get(&account_id);
    let plan = resolve_effective_account_plan(
        tokens.get(&account_id),
        usages.get(&account_id).copied(),
        subscription,
    );
    let has_token = tokens.contains_key(&account_id);
    let access_token_expires_at = tokens
        .get(&account_id)
        .and_then(|token| extract_token_exp(&token.access_token));
    let credential = derive_credential_state(
        &status,
        status_reason.as_deref(),
        has_token,
        access_token_expires_at,
        codexmanager_core::storage::now_ts(),
    );
    let account_metadata = metadata.get(&account_id);
    let model_slugs = model_slugs_by_account
        .get(&account_id)
        .cloned()
        .unwrap_or_default();
    let quota_override = quota_overrides.get(&account_id);
    let (fallback_plan_type, plan_type_raw) = match plan {
        Some(value) => (Some(value.normalized), value.raw),
        None => (None, None),
    };
    let subscription_plan = subscription.and_then(|value| value.plan_type.clone());
    let plan_type = fallback_plan_type;
    let agent_identity = agent_identities.get(&account_id);
    let gateway_probe_state = gateway_probe_states.get(&account_id);
    to_account_summary_with_reason(
        AccountSummaryParts {
            id: account_id,
            label,
            group_name,
            sort,
            status,
            created_at,
            updated_at,
        },
        preferred,
        status_reason,
        has_token,
        if agent_identity.is_some() {
            "agentIdentity".to_string()
        } else {
            "oauth".to_string()
        },
        agent_identity.map(|identity| identity.status.clone()),
        agent_identity
            .and_then(|identity| identity.task_id.as_deref())
            .is_some_and(|task| !task.trim().is_empty()),
        credential.state.to_string(),
        credential.action.to_string(),
        access_token_expires_at,
        gateway_probe_state,
        plan_type,
        plan_type_raw,
        subscription.map(|value| value.has_subscription),
        subscription_plan,
        subscription.and_then(|value| value.expires_at),
        subscription.and_then(|value| value.renews_at),
        account_metadata.and_then(|value| value.note.clone()),
        account_metadata.and_then(|value| value.tags.clone()),
        model_slugs,
        quota_override.and_then(|value| value.primary_window_tokens),
        quota_override.and_then(|value| value.secondary_window_tokens),
    )
}

#[cfg(test)]
mod tests {
    use super::build_account_summary_context_for_items;
    use codexmanager_core::storage::AccountSummaryStorageSnapshotOptions;
    use codexmanager_core::storage::{Account, AdapterCredentialProbeState, Storage, Token};

    #[test]
    fn account_summary_exposes_gateway_admission_evidence() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        let account = Account {
            id: "account-1".to_string(),
            label: "account-1@example.com".to_string(),
            issuer: "openai".to_string(),
            chatgpt_account_id: None,
            workspace_id: None,
            group_name: None,
            sort: 0,
            status: "active".to_string(),
            created_at: 100,
            updated_at: 100,
        };
        storage.insert_account(&account).expect("insert account");
        storage
            .insert_token(&Token {
                account_id: account.id.clone(),
                id_token: String::new(),
                access_token: "access".to_string(),
                refresh_token: "refresh".to_string(),
                api_key_access_token: None,
                last_refresh: 100,
            })
            .expect("insert token");
        storage
            .upsert_adapter_credential_probe_state(&AdapterCredentialProbeState {
                pool_id: "codex".to_string(),
                credential_id: account.id.clone(),
                status: "available".to_string(),
                error_code: Some("codex_responses_verified".to_string()),
                checked_at: 120,
                retry_after: None,
            })
            .expect("insert probe state");

        let context = build_account_summary_context_for_items(
            &storage,
            vec![account],
            AccountSummaryStorageSnapshotOptions::default(),
        )
        .expect("build summaries");
        assert_eq!(
            context.items[0].gateway_probe_status.as_deref(),
            Some("available")
        );
        assert_eq!(
            context.items[0].gateway_probe_reason.as_deref(),
            Some("codex_responses_verified")
        );
        assert_eq!(context.items[0].gateway_probe_checked_at, Some(120));
    }
}
