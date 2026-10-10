use super::remote_storage::AccountStorage;
use crate::storage_helpers::{seaorm_block_on, seaorm_enabled};
use codexmanager_core::storage::*;
use codexmanager_storage_seaorm::UsageSnapshotsRepository;
use std::ops::Deref;
fn error(message: String) -> rusqlite::Error {
    rusqlite::Error::SqliteFailure((), Some(message))
}
// Keep the full dual-backend adapter surface available for SeaORM runtime selection.
#[allow(dead_code)]
impl AccountStorage<'_> {
    pub(crate) fn latest_usage_snapshot(&self) -> rusqlite::Result<Option<UsageSnapshotRecord>> {
        if !seaorm_enabled() {
            return self.deref().latest_usage_snapshot();
        }
        seaorm_block_on(move |s| async move {
            UsageSnapshotsRepository::latest(s.connection())
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn latest_usage_snapshot_for_account(
        &self,
        id: &str,
    ) -> rusqlite::Result<Option<UsageSnapshotRecord>> {
        if !seaorm_enabled() {
            return self.deref().latest_usage_snapshot_for_account(id);
        }
        let id = id.to_owned();
        seaorm_block_on(move |s| async move {
            UsageSnapshotsRepository::latest_for_account(s.connection(), &id)
                .await
                .map(|v| v.map(|v| v.snapshot))
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn latest_usage_snapshots_for_accounts(
        &self,
        ids: &[String],
    ) -> rusqlite::Result<Vec<UsageSnapshotRecord>> {
        if !seaorm_enabled() {
            return self.deref().latest_usage_snapshots_for_accounts(ids);
        }
        let ids = ids.to_vec();
        seaorm_block_on(move |s| async move {
            UsageSnapshotsRepository::latest_for_accounts(s.connection(), &ids)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn latest_usage_snapshots_by_account(
        &self,
    ) -> rusqlite::Result<Vec<UsageSnapshotRecord>> {
        if !seaorm_enabled() {
            return self.deref().latest_usage_snapshots_by_account();
        }
        seaorm_block_on(move |s| async move {
            UsageSnapshotsRepository::latest_by_account(s.connection(), None)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn latest_usage_snapshots_by_account_limited(
        &self,
        limit: Option<usize>,
    ) -> rusqlite::Result<Vec<UsageSnapshotRecord>> {
        if !seaorm_enabled() {
            return self
                .deref()
                .latest_usage_snapshots_by_account_limited(limit);
        }
        seaorm_block_on(move |s| async move {
            UsageSnapshotsRepository::latest_by_account(s.connection(), limit)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn insert_usage_snapshot(&self, snap: &UsageSnapshotRecord) -> rusqlite::Result<()> {
        if !seaorm_enabled() {
            return self.deref().insert_usage_snapshot(snap);
        }
        let snap = snap.clone();
        seaorm_block_on(move |s| async move {
            UsageSnapshotsRepository::insert_and_prune(s.connection(), &snap, 0)
                .await
                .map(|_| ())
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn insert_usage_snapshot_and_prune(
        &self,
        snap: &UsageSnapshotRecord,
        retain: usize,
    ) -> rusqlite::Result<usize> {
        if !seaorm_enabled() {
            return self.deref().insert_usage_snapshot_and_prune(snap, retain);
        }
        let snap = snap.clone();
        seaorm_block_on(move |s| async move {
            UsageSnapshotsRepository::insert_and_prune(s.connection(), &snap, retain)
                .await
                .map(|(_, n)| n)
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn prune_usage_snapshots_for_account(
        &self,
        id: &str,
        retain: usize,
    ) -> rusqlite::Result<usize> {
        if !seaorm_enabled() {
            return self.deref().prune_usage_snapshots_for_account(id, retain);
        }
        let id = id.to_owned();
        seaorm_block_on(move |s| async move {
            UsageSnapshotsRepository::prune_for_account(s.connection(), &id, retain)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn usage_snapshot_count_for_account(&self, id: &str) -> rusqlite::Result<i64> {
        if !seaorm_enabled() {
            return self.deref().usage_snapshot_count_for_account(id);
        }
        let id = id.to_owned();
        seaorm_block_on(move |s| async move {
            UsageSnapshotsRepository::count_for_account(s.connection(), &id)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn insert_usage_snapshot_and_prune_with_previous<F>(
        &self,
        snap: &UsageSnapshotRecord,
        retain: usize,
        resolve: F,
    ) -> rusqlite::Result<(UsageSnapshotRecord, usize)>
    where
        F: FnOnce(Option<&UsageSnapshotRecord>, &mut Option<String>) -> rusqlite::Result<()>,
    {
        if !seaorm_enabled() {
            return self
                .deref()
                .insert_usage_snapshot_and_prune_with_previous(snap, retain, resolve);
        }
        if super::remote_storage::current_usage_transaction().is_some() {
            return Err(error("nested usage transaction is not supported".into()));
        }
        let id = snap.account_id.clone();
        let (tx, previous) = seaorm_block_on(move |s| async move {
            UsageSnapshotsRepository::begin_snapshot_write(s.connection(), &id)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)?;
        let mut resolved = snap.clone();
        if let Some(previous) = &previous {
            resolved.captured_at = resolved.captured_at.max(previous.captured_at);
        }
        let tx = std::sync::Arc::new(tx);
        let result = super::remote_storage::with_usage_transaction(tx.clone(), || {
            resolve(previous.as_ref(), &mut resolved.credits_json)
        });
        let tx = std::sync::Arc::try_unwrap(tx)
            .map_err(|_| error("usage transaction still borrowed".into()))?;
        if let Err(err) = result {
            let _ = crate::storage_helpers::seaorm_transaction_block_on(async move {
                tx.rollback().await.map_err(|e| e.to_string())
            });
            return Err(err);
        }
        let persisted = resolved.clone();
        let pruned = crate::storage_helpers::seaorm_transaction_block_on(async move {
            UsageSnapshotsRepository::finish_snapshot_write(tx, &persisted, retain)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)?;
        Ok((resolved, pruned))
    }
    pub(crate) fn latest_usage_snapshot_with_extra_rate_limits_for_account(
        &self,
        id: &str,
    ) -> rusqlite::Result<Option<UsageSnapshotRecord>> {
        if !seaorm_enabled() {
            return self
                .deref()
                .latest_usage_snapshot_with_extra_rate_limits_for_account(id);
        }
        let id = id.to_owned();
        seaorm_block_on(move |s| async move {
            let rows = UsageSnapshotsRepository::list_for_account(s.connection(), &id, u64::MAX)
                .await
                .map_err(|e| e.to_string())?;
            Ok(rows.into_iter().map(|r| r.snapshot).find(|r| {
                r.credits_json
                    .as_deref()
                    .is_some_and(|v| v.contains("extra_rate_limits"))
            }))
        })
        .map_err(error)
    }
    pub(crate) fn insert_request_log(&self, log: &RequestLog) -> rusqlite::Result<i64> {
        crate::requestlog::seaorm::insert_log(self.deref(), log)
    }
}
impl AccountStorage<'_> {
    pub(crate) fn latest_usage_snapshot_summary_rows(
        &self,
    ) -> rusqlite::Result<Vec<UsageSnapshotSummaryRow>> {
        Ok(self
            .latest_usage_snapshots_by_account()?
            .into_iter()
            .map(|s| UsageSnapshotSummaryRow {
                account_id: s.account_id,
                used_percent: s.used_percent,
                window_minutes: s.window_minutes,
                secondary_used_percent: s.secondary_used_percent,
                secondary_window_minutes: s.secondary_window_minutes,
                credits_json: s.credits_json,
            })
            .collect())
    }
    pub(crate) fn latest_usage_quota_source_rows_for_accounts(
        &self,
        ids: &[String],
    ) -> rusqlite::Result<Vec<UsageSnapshotQuotaSourceRow>> {
        Ok(self
            .latest_usage_snapshots_for_accounts(ids)?
            .into_iter()
            .map(|s| UsageSnapshotQuotaSourceRow {
                account_id: s.account_id,
                used_percent: s.used_percent,
                secondary_used_percent: s.secondary_used_percent,
                captured_at: s.captured_at,
            })
            .collect())
    }
}

impl AccountStorage<'_> {
    pub(crate) fn summarize_request_token_stats_between(
        &self,
        start: i64,
        end: i64,
    ) -> rusqlite::Result<RequestLogTodaySummary> {
        if !seaorm_enabled() {
            return self
                .deref()
                .summarize_request_token_stats_between(start, end);
        }
        seaorm_block_on(move |s| async move {
            codexmanager_storage_seaorm::ApiKeyDetailsRepository::today_summary(
                s.connection(),
                None,
                start,
                end,
            )
            .await
            .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn summarize_request_token_stats_by_model(
        &self,
        start: Option<i64>,
        end: Option<i64>,
    ) -> rusqlite::Result<Vec<TokenUsageSummary>> {
        if !seaorm_enabled() {
            return self
                .deref()
                .summarize_request_token_stats_by_model(start, end);
        }
        seaorm_block_on(move |s| async move {
            codexmanager_storage_seaorm::ApiKeyDetailsRepository::usage_by_model(
                s.connection(),
                start,
                end,
            )
            .await
            .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
}
