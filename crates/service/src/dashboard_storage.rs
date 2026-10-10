//! Dashboard persistence boundary; never merge local and remote accounting.
use crate::storage_helpers::{seaorm_block_on, seaorm_enabled};
use codexmanager_core::storage::*;
use codexmanager_storage_seaorm::{
    ApiKeyDetailsRepository, BillingRepository, UsageAnalyticsRepository, UsersRepository,
};

pub(crate) struct DashboardStorage<'a>(&'a Storage);
fn error(message: String) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::other(message)))
}
impl<'a> DashboardStorage<'a> {
    pub(crate) fn new(storage: &'a Storage) -> Self {
        Self(storage)
    }
    pub(crate) fn summarize_request_token_stats_daily(
        &self,
        start: i64,
        end: i64,
        bucket: i64,
    ) -> rusqlite::Result<Vec<DailyTokenUsageRollup>> {
        if !seaorm_enabled() {
            return self
                .0
                .summarize_request_token_stats_daily(start, end, bucket);
        }
        seaorm_block_on(move |s| async move {
            UsageAnalyticsRepository::daily(s.connection(), start, end, bucket, None)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn summarize_request_token_stats_daily_for_user(
        &self,
        user: &str,
        start: i64,
        end: i64,
        bucket: i64,
    ) -> rusqlite::Result<Vec<DailyTokenUsageRollup>> {
        if !seaorm_enabled() {
            return self
                .0
                .summarize_request_token_stats_daily_for_user(user, start, end, bucket);
        }
        let user = user.to_owned();
        seaorm_block_on(move |s| async move {
            UsageAnalyticsRepository::daily(s.connection(), start, end, bucket, Some(&user))
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn summarize_request_token_stats_by_model_timeline(
        &self,
        start: i64,
        end: i64,
        bucket: i64,
    ) -> rusqlite::Result<Vec<ModelTokenUsageRollup>> {
        if !seaorm_enabled() {
            return self
                .0
                .summarize_request_token_stats_by_model_timeline(start, end, bucket);
        }
        seaorm_block_on(move |s| async move {
            UsageAnalyticsRepository::models(s.connection(), start, end, bucket)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn summarize_request_token_stats_by_user_between_limited(
        &self,
        start: i64,
        end: i64,
        limit: Option<usize>,
    ) -> rusqlite::Result<Vec<UserTokenUsageRollup>> {
        if !seaorm_enabled() {
            return self
                .0
                .summarize_request_token_stats_by_user_between_limited(start, end, limit);
        }
        seaorm_block_on(move |s| async move {
            UsageAnalyticsRepository::users(s.connection(), start, end, limit)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn summarize_request_token_stats_by_sources_between_limited(
        &self,
        kinds: &[&str],
        start: i64,
        end: i64,
        limit: Option<usize>,
    ) -> rusqlite::Result<Vec<SourceTokenUsageRollup>> {
        if !seaorm_enabled() {
            return self
                .0
                .summarize_request_token_stats_by_sources_between_limited(
                    kinds, start, end, limit,
                );
        }
        let kinds = kinds.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        seaorm_block_on(move |s| async move {
            UsageAnalyticsRepository::sources(s.connection(), &kinds, start, end, limit)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn list_api_key_ids_for_user(&self, user: &str) -> rusqlite::Result<Vec<String>> {
        if !seaorm_enabled() {
            return self.0.list_api_key_ids_for_user(user);
        }
        let user = user.to_owned();
        seaorm_block_on(move |s| async move {
            UsersRepository::list_owners(s.connection())
                .await
                .map(|owners| {
                    owners
                        .into_iter()
                        .filter(|owner| {
                            owner.owner_kind == "user"
                                && owner.owner_user_id.as_deref() == Some(user.as_str())
                        })
                        .map(|owner| owner.key_id)
                        .collect()
                })
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn find_wallet_by_owner(
        &self,
        kind: &str,
        id: &str,
    ) -> rusqlite::Result<Option<AppWallet>> {
        if !seaorm_enabled() {
            return self.0.find_wallet_by_owner(kind, id);
        }
        let kind = kind.to_owned();
        let id = id.to_owned();
        seaorm_block_on(move |s| async move {
            BillingRepository::wallet_by_owner(s.connection(), &kind, &id)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn list_dashboard_app_user_summaries_for_ids(
        &self,
        ids: &[String],
    ) -> rusqlite::Result<Vec<DashboardAppUserSummary>> {
        if !seaorm_enabled() {
            return self.0.list_dashboard_app_user_summaries_for_ids(ids);
        }
        let ids = ids.to_vec();
        seaorm_block_on(move |s| async move {
            let mut users = Vec::new();
            for id in ids {
                if let Some(user) = UsersRepository::get(s.connection(), &id)
                    .await
                    .map_err(|e| e.to_string())?
                {
                    let wallet = BillingRepository::wallet_by_owner(s.connection(), "user", &id)
                        .await
                        .map_err(|e| e.to_string())?;
                    users.push(DashboardAppUserSummary {
                        id: user.id,
                        username: user.username,
                        display_name: user.display_name,
                        role: user.role,
                        status: user.status,
                        wallet_available_credit_micros: wallet.map(|w| {
                            w.balance_credit_micros
                                .saturating_sub(w.frozen_credit_micros)
                        }),
                    });
                }
            }
            Ok(users)
        })
        .map_err(error)
    }
    pub(crate) fn load_member_dashboard_usage_breakdown_snapshot(
        &self,
        ids: &[String],
        start: i64,
        end: i64,
        days: i64,
        limit: usize,
    ) -> rusqlite::Result<MemberDashboardUsageBreakdownSnapshot> {
        if !seaorm_enabled() {
            return self
                .0
                .load_member_dashboard_usage_breakdown_snapshot(ids, start, end, days, limit);
        }
        if ids.is_empty() {
            return Ok(MemberDashboardUsageBreakdownSnapshot::default());
        }
        let ids = ids.to_vec();
        let trend_start = start.saturating_sub(
            days.max(1)
                .saturating_sub(1)
                .saturating_mul(end.saturating_sub(start).max(1)),
        );
        seaorm_block_on(move |s| async move {
            let today = ApiKeyDetailsRepository::usage_by_key_model(
                s.connection(),
                Some(start),
                Some(end),
                Some(&ids),
            )
            .await
            .map_err(|e| e.to_string())?;
            let total = ApiKeyDetailsRepository::usage_by_key(s.connection(), None, None)
                .await
                .map_err(|e| e.to_string())?
                .into_iter()
                .filter(|row| ids.contains(&row.key_id))
                .collect();
            let models = ApiKeyDetailsRepository::usage_by_key_model(
                s.connection(),
                Some(trend_start),
                Some(end),
                Some(&ids),
            )
            .await
            .map_err(|e| e.to_string())?;
            let mut grouped = std::collections::HashMap::<String, TokenUsageSummary>::new();
            for row in models {
                let sum = grouped
                    .entry(row.model.clone())
                    .or_insert_with(|| TokenUsageSummary {
                        model: row.model.clone(),
                        ..Default::default()
                    });
                sum.input_tokens = sum.input_tokens.saturating_add(row.input_tokens);
                sum.cached_input_tokens = sum
                    .cached_input_tokens
                    .saturating_add(row.cached_input_tokens);
                sum.output_tokens = sum.output_tokens.saturating_add(row.output_tokens);
                sum.reasoning_output_tokens = sum
                    .reasoning_output_tokens
                    .saturating_add(row.reasoning_output_tokens);
                sum.total_tokens = sum.total_tokens.saturating_add(row.total_tokens);
                sum.estimated_cost_usd += row.estimated_cost_usd;
            }
            let mut top = grouped.into_values().collect::<Vec<_>>();
            top.sort_by(|a, b| {
                b.total_tokens
                    .cmp(&a.total_tokens)
                    .then_with(|| a.model.cmp(&b.model))
            });
            top.truncate(limit);
            Ok(MemberDashboardUsageBreakdownSnapshot {
                today_key_model_usage: today,
                total_key_usage: total,
                top_model_usage: top,
            })
        })
        .map_err(error)
    }
}
