use codexmanager_core::{
    rpc::types::{AccountListParams, AccountListResult, AccountSummary},
    storage::{
        Account, AccountMetadata, AccountQuotaCapacityOverride, AccountSubscription, Token,
        UsageSnapshotRecord,
    },
};
use std::collections::HashMap;

use crate::account_plan::resolve_effective_account_plan;
use crate::storage_helpers::open_storage;

const DEFAULT_ACCOUNT_PAGE_SIZE: i64 = 5;
const MAX_ACCOUNT_PAGE_SIZE: i64 = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AccountFilter {
    All,
    Active,
    Low,
}

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
pub(crate) fn read_accounts(
    params: AccountListParams,
    pagination_requested: bool,
) -> Result<AccountListResult, String> {
    // 中文注释：账号页需要后端分页，但仪表盘/日志等全局功能仍依赖全量账号列表；
    // 因此这里兼容“无分页参数时返回全量，有分页参数时返回当前页”两种模式。
    let params = params.normalized();
    let storage = open_storage().ok_or_else(|| "open storage failed".to_string())?;
    let query = normalize_optional_text(params.query);
    let group_filter = normalize_optional_text(params.group_filter);
    let filter = normalize_filter(params.filter);

    if filter == AccountFilter::All {
        if pagination_requested {
            let page_size = normalize_page_size(params.page_size);
            let total = storage
                .account_count_filtered(query.as_deref(), group_filter.as_deref())
                .map_err(|err| format!("count accounts failed: {err}"))?;
            let page = clamp_page(params.page, total, page_size);
            let offset = (page - 1) * page_size;
            let accounts = storage
                .list_accounts_paginated(
                    query.as_deref(),
                    group_filter.as_deref(),
                    offset,
                    page_size,
                )
                .map_err(|err| format!("list accounts failed: {err}"))?;
            let items = to_account_summaries(&storage, accounts)?;
            return Ok(AccountListResult {
                items,
                total,
                page,
                page_size,
            });
        }

        let accounts = storage
            .list_accounts_filtered(query.as_deref(), group_filter.as_deref())
            .map_err(|err| format!("list accounts failed: {err}"))?;
        let total = accounts.len() as i64;
        let items = to_account_summaries(&storage, accounts)?;
        return Ok(AccountListResult {
            items,
            total,
            page: 1,
            page_size: if total > 0 {
                total
            } else {
                DEFAULT_ACCOUNT_PAGE_SIZE
            },
        });
    }

    if pagination_requested {
        let total =
            filtered_account_count(&storage, filter, query.as_deref(), group_filter.as_deref())?;
        let page_size = normalize_page_size(params.page_size);
        let page = clamp_page(params.page, total, page_size);
        let offset = (page - 1) * page_size;
        let paged = filtered_accounts(
            &storage,
            filter,
            query.as_deref(),
            group_filter.as_deref(),
            Some((offset, page_size)),
        )?;
        let items = to_account_summaries(&storage, paged)?;
        return Ok(AccountListResult {
            items,
            total,
            page,
            page_size,
        });
    }

    let accounts = filtered_accounts(
        &storage,
        filter,
        query.as_deref(),
        group_filter.as_deref(),
        None,
    )?;
    let total = accounts.len() as i64;
    let items = to_account_summaries(&storage, accounts)?;

    Ok(AccountListResult {
        items,
        total,
        page: 1,
        page_size: if total > 0 {
            total
        } else {
            DEFAULT_ACCOUNT_PAGE_SIZE
        },
    })
}

/// 函数 `normalize_optional_text`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - value: 参数 value
///
/// # 返回
/// 返回函数执行结果
fn normalize_optional_text(value: Option<String>) -> Option<String> {
    let trimmed = value.unwrap_or_default().trim().to_string();
    if trimmed.is_empty() || trimmed == "all" {
        return None;
    }
    Some(trimmed)
}

/// 函数 `normalize_filter`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - value: 参数 value
///
/// # 返回
/// 返回函数执行结果
fn normalize_filter(value: Option<String>) -> AccountFilter {
    match value
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "active" => AccountFilter::Active,
        "low" => AccountFilter::Low,
        _ => AccountFilter::All,
    }
}

/// 函数 `normalize_page_size`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - value: 参数 value
///
/// # 返回
/// 返回函数执行结果
fn normalize_page_size(value: i64) -> i64 {
    value.clamp(1, MAX_ACCOUNT_PAGE_SIZE)
}

/// 函数 `clamp_page`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - page: 参数 page
/// - total: 参数 total
/// - page_size: 参数 page_size
///
/// # 返回
/// 返回函数执行结果
fn clamp_page(page: i64, total: i64, page_size: i64) -> i64 {
    let normalized_page = page.max(1);
    let total_pages = if total <= 0 {
        1
    } else {
        ((total + page_size - 1) / page_size).max(1)
    };
    normalized_page.min(total_pages)
}

/// 函数 `filtered_account_count`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - storage: 参数 storage
/// - filter: 参数 filter
/// - query: 参数 query
/// - group_filter: 参数 group_filter
///
/// # 返回
/// 返回函数执行结果
fn filtered_account_count(
    storage: &codexmanager_core::storage::Storage,
    filter: AccountFilter,
    query: Option<&str>,
    group_filter: Option<&str>,
) -> Result<i64, String> {
    match filter {
        AccountFilter::All => storage
            .account_count_filtered(query, group_filter)
            .map_err(|err| format!("count accounts failed: {err}")),
        AccountFilter::Active => storage
            .account_count_active_available(query, group_filter)
            .map_err(|err| format!("count active accounts failed: {err}")),
        AccountFilter::Low => storage
            .account_count_low_quota(query, group_filter)
            .map_err(|err| format!("count low quota accounts failed: {err}")),
    }
}

/// 函数 `filtered_accounts`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - storage: 参数 storage
/// - filter: 参数 filter
/// - query: 参数 query
/// - group_filter: 参数 group_filter
/// - pagination: 参数 pagination
///
/// # 返回
/// 返回函数执行结果
fn filtered_accounts(
    storage: &codexmanager_core::storage::Storage,
    filter: AccountFilter,
    query: Option<&str>,
    group_filter: Option<&str>,
    pagination: Option<(i64, i64)>,
) -> Result<Vec<Account>, String> {
    match filter {
        AccountFilter::All => match pagination {
            Some((offset, limit)) => storage
                .list_accounts_paginated(query, group_filter, offset, limit)
                .map_err(|err| format!("list accounts failed: {err}")),
            None => storage
                .list_accounts_filtered(query, group_filter)
                .map_err(|err| format!("list accounts failed: {err}")),
        },
        AccountFilter::Active => storage
            .list_accounts_active_available(query, group_filter, pagination)
            .map_err(|err| format!("list active accounts failed: {err}")),
        AccountFilter::Low => storage
            .list_accounts_low_quota(query, group_filter, pagination)
            .map_err(|err| format!("list low quota accounts failed: {err}")),
    }
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
