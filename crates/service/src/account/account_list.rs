use codexmanager_core::{
    rpc::types::{AccountListResult, AccountSummary},
    storage::{
        Account, AccountMetadata, AccountQuotaCapacityOverride, AccountSubscription, Token,
        UsageSnapshotRecord,
    },
};
use std::collections::HashMap;

use crate::account_plan::resolve_effective_account_plan;
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
}

impl From<Account> for AccountSummaryParts {
    fn from(account: Account) -> Self {
        Self {
            id: account.id,
            label: account.label,
            group_name: account.group_name,
            sort: account.sort,
            status: account.status,
        }
    }
}

#[derive(Debug)]
struct AccountSummarySetup {
    preferred_account_id: Option<String>,
    status_reasons: HashMap<String, String>,
    tokens: HashMap<String, Token>,
    usage_snapshots: Vec<UsageSnapshotRecord>,
    metadata: HashMap<String, AccountMetadata>,
    subscriptions: HashMap<String, AccountSubscription>,
    model_slugs_by_account: HashMap<String, Vec<String>>,
    quota_overrides: HashMap<String, AccountQuotaCapacityOverride>,
}

impl From<&Account> for AccountSummaryParts {
    fn from(account: &Account) -> Self {
        Self {
            id: account.id.clone(),
            label: account.label.clone(),
            group_name: account.group_name.clone(),
            sort: account.sort,
            status: account.status.clone(),
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
        .list_accounts()
        .map_err(|err| format!("list accounts failed: {err}"))?;
    let total = accounts.len() as i64;
    let items = to_account_summaries(&storage, accounts)?;
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
pub(crate) fn build_account_summary_context(
    storage: &codexmanager_core::storage::Storage,
    accounts: &[Account],
) -> Result<AccountSummaryContext, String> {
    let account_ids = accounts
        .iter()
        .map(|account| account.id.clone())
        .collect::<Vec<_>>();
    let setup = load_account_summary_setup(storage, &account_ids)?;
    let items = build_account_summary_items(accounts.iter(), &setup);
    Ok(AccountSummaryContext {
        items,
        usage_snapshots: setup.usage_snapshots,
    })
}

fn to_account_summaries(
    storage: &codexmanager_core::storage::Storage,
    accounts: Vec<Account>,
) -> Result<Vec<AccountSummary>, String> {
    let account_ids = accounts
        .iter()
        .map(|account| account.id.clone())
        .collect::<Vec<_>>();
    let setup = load_account_summary_setup(storage, &account_ids)?;
    Ok(build_account_summary_items(accounts, &setup))
}

fn load_account_summary_setup(
    storage: &codexmanager_core::storage::Storage,
    account_ids: &[String],
) -> Result<AccountSummarySetup, String> {
    let preferred_account_id = storage
        .preferred_account_id()
        .map_err(|err| format!("load preferred account failed: {err}"))?;
    let status_reasons = storage
        .latest_account_status_reasons(account_ids)
        .map_err(|err| format!("load account status reasons failed: {err}"))?;
    let tokens = storage
        .list_tokens()
        .map_err(|err| format!("load account tokens failed: {err}"))?
        .into_iter()
        .map(|token| (token.account_id.clone(), token))
        .collect::<HashMap<String, Token>>();
    let usage_snapshots = storage
        .latest_usage_snapshots_by_account()
        .map_err(|err| format!("load account usage snapshots failed: {err}"))?;
    let metadata = storage
        .list_account_metadata()
        .map_err(|err| format!("load account metadata failed: {err}"))?
        .into_iter()
        .map(|item| (item.account_id.clone(), item))
        .collect::<HashMap<String, AccountMetadata>>();
    let subscriptions = storage
        .list_account_subscriptions()
        .map_err(|err| format!("load account subscriptions failed: {err}"))?
        .into_iter()
        .map(|item| (item.account_id.clone(), item))
        .collect::<HashMap<String, AccountSubscription>>();
    let source_assignments = storage
        .list_quota_source_model_assignments()
        .map_err(|err| format!("load quota source assignments failed: {err}"))?;
    let mut model_slugs_by_account: HashMap<String, Vec<String>> = HashMap::new();
    for assignment in source_assignments {
        if assignment.source_kind == "openai_account" {
            model_slugs_by_account
                .entry(assignment.source_id)
                .or_default()
                .push(assignment.model_slug);
        }
    }
    let quota_overrides = storage
        .list_account_quota_capacity_overrides()
        .map_err(|err| format!("load account quota capacity overrides failed: {err}"))?
        .into_iter()
        .map(|item| (item.account_id.clone(), item))
        .collect::<HashMap<String, AccountQuotaCapacityOverride>>();
    Ok(AccountSummarySetup {
        preferred_account_id,
        status_reasons,
        tokens,
        usage_snapshots,
        metadata,
        subscriptions,
        model_slugs_by_account,
        quota_overrides,
    })
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
    tokens: &HashMap<String, Token>,
    usages: &HashMap<String, &UsageSnapshotRecord>,
    metadata: &HashMap<String, AccountMetadata>,
    subscriptions: &HashMap<String, AccountSubscription>,
    model_slugs_by_account: &HashMap<String, Vec<String>>,
    quota_overrides: &HashMap<String, AccountQuotaCapacityOverride>,
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
    to_account_summary_with_reason(
        AccountSummaryParts {
            id: account_id,
            label,
            group_name,
            sort,
            status,
        },
        preferred,
        status_reason,
        has_token,
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
