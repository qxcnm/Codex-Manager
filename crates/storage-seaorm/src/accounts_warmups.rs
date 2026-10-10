//! Reset reminders preserve the SQLite cycle/claim semantics across databases.
use crate::{
    account_details::warmups as w, AccountTokensRepository, AccountsRepository,
    UsageSnapshotsRepository, UsersRepository,
};
use codexmanager_core::storage::{AccountResetWarmupTarget, UsageSnapshotRecord};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseConnection, DbErr, EntityTrait,
    IntoActiveModel, QueryFilter, Set, TransactionTrait,
};

async fn eligible(
    db: &impl ConnectionTrait,
    row: &w::Model,
    now: i64,
) -> Result<Option<AccountResetWarmupTarget>, DbErr> {
    let Some(reset) = row
        .pending_reset_at
        .filter(|r| row.enabled && *r > row.consumed_reset_at.unwrap_or(0) && *r <= now)
    else {
        return Ok(None);
    };
    let Some(account) = AccountsRepository::get(db, &row.account_id).await? else {
        return Ok(None);
    };
    if matches!(
        account.status.trim().to_ascii_lowercase().as_str(),
        "inactive" | "disabled" | "unavailable" | "banned"
    ) {
        return Ok(None);
    }
    if AccountTokensRepository::get(db, &row.account_id)
        .await?
        .is_none_or(|t| t.access_token.trim().is_empty())
    {
        return Ok(None);
    }
    let Some(latest) = UsageSnapshotsRepository::latest_for_account(db, &row.account_id).await?
    else {
        return Ok(None);
    };
    let snap = latest.snapshot;
    let mut due = reset;
    for (minutes, used, at) in [
        (snap.window_minutes, snap.used_percent, snap.resets_at),
        (
            snap.secondary_window_minutes,
            snap.secondary_used_percent,
            snap.secondary_resets_at,
        ),
    ] {
        if minutes.is_some_and(|m| m > 300) && used.is_some_and(|u| u >= 100.0) {
            let Some(at) = at.filter(|t| *t > 0) else {
                return Ok(None);
            };
            due = due.max(at);
        }
    }
    let Some(due_at) = due.checked_add(5).filter(|d| *d <= now) else {
        return Ok(None);
    };
    Ok(Some(AccountResetWarmupTarget {
        account_id: row.account_id.clone(),
        reset_at: reset,
        due_at,
    }))
}

impl AccountsRepository {
    pub async fn list_account_reset_warmup_targets(
        db: &impl ConnectionTrait,
        now: i64,
        limit: usize,
    ) -> Result<Vec<AccountResetWarmupTarget>, DbErr> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let rows = w::Entity::find()
            .filter(w::Column::Enabled.eq(true))
            .filter(w::Column::PendingResetAt.lte(now))
            .all(db)
            .await?;
        let mut targets = Vec::new();
        for row in rows {
            if let Some(target) = eligible(db, &row, now).await? {
                targets.push(target)
            }
        }
        targets.sort_by(|a, b| (a.due_at, &a.account_id).cmp(&(b.due_at, &b.account_id)));
        targets.truncate(limit);
        Ok(targets)
    }
    pub async fn claim_account_reset_warmup(
        db: &DatabaseConnection,
        account_id: &str,
        reset_at: i64,
        now: i64,
    ) -> Result<bool, DbErr> {
        let tx = db.begin().await?;
        UsersRepository::lock(&tx, "accounts").await?;
        let Some(row) = w::Entity::find_by_id(account_id).one(&tx).await? else {
            return Ok(false);
        };
        if row.pending_reset_at != Some(reset_at) || eligible(&tx, &row, now).await?.is_none() {
            return Ok(false);
        }
        let mut active = row.into_active_model();
        active.pending_reset_at = Set(None);
        active.consumed_reset_at = Set(Some(reset_at));
        active.last_claimed_at = Set(Some(now));
        active.update(&tx).await?;
        tx.commit().await?;
        Ok(true)
    }
    pub async fn set_account_reset_warmup_enabled(
        db: &DatabaseConnection,
        ids: &[String],
        enabled: bool,
    ) -> Result<usize, DbErr> {
        if ids.is_empty() || ids.iter().any(|id| id.is_empty() || id.trim() != id) {
            return Err(DbErr::Custom(
                "accountIds must contain non-empty account IDs".into(),
            ));
        }
        let ids = ids.iter().collect::<std::collections::BTreeSet<_>>();
        let tx = db.begin().await?;
        UsersRepository::lock(&tx, "accounts").await?;
        for id in &ids {
            if AccountsRepository::get(&tx, id).await?.is_none() {
                return Err(DbErr::RecordNotFound("account not found".into()));
            }
        }
        for id in &ids {
            w::Entity::insert(w::ActiveModel {
                account_id: Set((*id).clone()),
                enabled: Set(enabled),
                pending_reset_at: Set(None),
                consumed_reset_at: Set(None),
                last_claimed_at: Set(None),
            })
            .on_conflict(
                sea_orm::sea_query::OnConflict::column(w::Column::AccountId)
                    .update_column(w::Column::Enabled)
                    .to_owned(),
            )
            .exec(&tx)
            .await?;
        }
        tx.commit().await?;
        Ok(ids.len())
    }
    pub async fn list_account_reset_warmup_settings_for_accounts(
        db: &impl ConnectionTrait,
        ids: &[String],
    ) -> Result<Vec<(String, bool)>, DbErr> {
        let ids = ids
            .iter()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect::<std::collections::BTreeSet<_>>();
        let mut values = Vec::new();
        for id in ids {
            if AccountsRepository::get(db, id).await?.is_some() {
                let enabled = w::Entity::find_by_id(id)
                    .one(db)
                    .await?
                    .map(|r| r.enabled)
                    .unwrap_or(true);
                values.push((id.into(), enabled));
            }
        }
        Ok(values)
    }
}

/// Caller holds the accounts domain lock and snapshot transaction.
pub(crate) async fn observe_usage_snapshot(
    db: &impl ConnectionTrait,
    snap: &UsageSnapshotRecord,
) -> Result<(), DbErr> {
    let windows = [
        (snap.window_minutes, snap.used_percent, snap.resets_at),
        (
            snap.secondary_window_minutes,
            snap.secondary_used_percent,
            snap.secondary_resets_at,
        ),
    ]
    .into_iter()
    .filter_map(|(minutes, used, reset)| {
        if minutes == Some(300) {
            reset.filter(|r| *r > 0).map(|r| (used, r))
        } else {
            None
        }
    })
    .collect::<Vec<_>>();
    if windows.is_empty() {
        return Ok(());
    }
    if UsageSnapshotsRepository::latest_for_account(db, &snap.account_id)
        .await?
        .is_some_and(|s| s.snapshot.captured_at > snap.captured_at)
    {
        return Ok(());
    }
    let existing = w::Entity::find_by_id(&snap.account_id).one(db).await?;
    let mut row = existing.clone().unwrap_or(w::Model {
        account_id: snap.account_id.clone(),
        enabled: true,
        pending_reset_at: None,
        consumed_reset_at: None,
        last_claimed_at: None,
    });
    if let Some(reset) = windows
        .iter()
        .filter(|(u, _)| u.is_some_and(|u| u.is_finite() && u > 0.0 && u < 100.0))
        .map(|(_, r)| *r)
        .max()
    {
        if let Some(pending) = row
            .pending_reset_at
            .filter(|p| *p < reset && *p <= snap.captured_at)
        {
            row.consumed_reset_at = Some(row.consumed_reset_at.unwrap_or(0).max(pending));
            row.pending_reset_at = None;
        }
    }
    if let Some(reset) = windows
        .iter()
        .filter(|(u, _)| u.is_some_and(|u| u.is_finite() && u >= 100.0))
        .map(|(_, r)| *r)
        .max()
        .filter(|r| *r > row.consumed_reset_at.unwrap_or(0))
    {
        if AccountsRepository::get(db, &snap.account_id)
            .await?
            .is_some()
        {
            row.pending_reset_at = Some(row.pending_reset_at.unwrap_or(0).max(reset));
        }
    }
    if existing.is_some() || row.pending_reset_at.is_some() {
        w::Entity::insert(w::ActiveModel {
            account_id: Set(row.account_id),
            enabled: Set(row.enabled),
            pending_reset_at: Set(row.pending_reset_at),
            consumed_reset_at: Set(row.consumed_reset_at),
            last_claimed_at: Set(row.last_claimed_at),
        })
        .on_conflict(
            sea_orm::sea_query::OnConflict::column(w::Column::AccountId)
                .update_columns([w::Column::PendingResetAt, w::Column::ConsumedResetAt])
                .to_owned(),
        )
        .exec(db)
        .await?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "accounts_warmups_tests.rs"]
mod tests;
