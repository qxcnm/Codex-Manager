//! Account quota snapshots. Percentages remain floating point observations;
//! monetary charging belongs to the integer billing repositories.

use codexmanager_core::storage::UsageSnapshotRecord;
use sea_orm::entity::prelude::*;
use sea_orm::{ConnectionTrait, QueryOrder, QuerySelect, Set};

#[derive(Debug, Clone)]
pub struct UsageSnapshotRow {
    pub id: i64,
    pub snapshot: UsageSnapshotRecord,
}

#[derive(Clone, Debug, DeriveEntityModel)]
#[sea_orm(table_name = "usage_snapshots")]
pub struct Model {
    #[sea_orm(primary_key)]
    id: i64,
    account_id: String,
    used_percent: Option<f64>,
    window_minutes: Option<i64>,
    resets_at: Option<i64>,
    secondary_used_percent: Option<f64>,
    secondary_window_minutes: Option<i64>,
    secondary_resets_at: Option<i64>,
    #[sea_orm(column_type = "Text", nullable)]
    credits_json: Option<String>,
    captured_at: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}
impl ActiveModelBehavior for ActiveModel {}

pub struct UsageSnapshotsRepository;

impl UsageSnapshotsRepository {
    pub async fn insert(
        db: &impl ConnectionTrait,
        snapshot: &UsageSnapshotRecord,
    ) -> Result<i64, DbErr> {
        let model = ActiveModel {
            account_id: Set(snapshot.account_id.clone()),
            used_percent: Set(snapshot.used_percent),
            window_minutes: Set(snapshot.window_minutes),
            resets_at: Set(snapshot.resets_at),
            secondary_used_percent: Set(snapshot.secondary_used_percent),
            secondary_window_minutes: Set(snapshot.secondary_window_minutes),
            secondary_resets_at: Set(snapshot.secondary_resets_at),
            credits_json: Set(snapshot.credits_json.clone()),
            captured_at: Set(snapshot.captured_at),
            ..Default::default()
        };
        Ok(Entity::insert(model).exec(db).await?.last_insert_id)
    }

    pub async fn latest_for_account(
        db: &impl ConnectionTrait,
        account_id: &str,
    ) -> Result<Option<UsageSnapshotRow>, DbErr> {
        Entity::find()
            .filter(Column::AccountId.eq(account_id))
            .order_by_desc(Column::CapturedAt)
            .order_by_desc(Column::Id)
            .one(db)
            .await
            .map(|row| row.map(Into::into))
    }

    /// Bound account-history reads without changing the ordering used by the
    /// existing SQLite adapter. A zero limit explicitly means no rows.
    pub async fn list_for_account(
        db: &impl ConnectionTrait,
        account_id: &str,
        limit: u64,
    ) -> Result<Vec<UsageSnapshotRow>, DbErr> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        Entity::find()
            .filter(Column::AccountId.eq(account_id))
            .order_by_desc(Column::CapturedAt)
            .order_by_desc(Column::Id)
            .limit(limit.min(1_000))
            .all(db)
            .await
            .map(|rows| rows.into_iter().map(Into::into).collect())
    }

    pub async fn delete(db: &impl ConnectionTrait, id: i64) -> Result<bool, DbErr> {
        Ok(Entity::delete_by_id(id).exec(db).await?.rows_affected > 0)
    }
}

impl From<Model> for UsageSnapshotRow {
    fn from(model: Model) -> Self {
        Self {
            id: model.id,
            snapshot: UsageSnapshotRecord {
                account_id: model.account_id,
                used_percent: model.used_percent,
                window_minutes: model.window_minutes,
                resets_at: model.resets_at,
                secondary_used_percent: model.secondary_used_percent,
                secondary_window_minutes: model.secondary_window_minutes,
                secondary_resets_at: model.secondary_resets_at,
                credits_json: model.credits_json,
                captured_at: model.captured_at,
            },
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use sea_orm::{Database, Schema, TransactionTrait};

    pub(crate) async fn exercise(db: &sea_orm::DatabaseConnection) {
        let account_id = format!("snapshot-fixture-{}", std::process::id());
        let snapshot = UsageSnapshotRecord {
            account_id: account_id.clone(),
            used_percent: None,
            window_minutes: Some(300),
            resets_at: Some(2_147_483_650),
            secondary_used_percent: Some(100.0),
            secondary_window_minutes: Some(10_080),
            secondary_resets_at: None,
            credits_json: Some(format!(
                "{{\"label\":\"额度\",\"data\":\"{}\"}}",
                "x".repeat(512)
            )),
            captured_at: 2_147_483_648,
        };
        let first = UsageSnapshotsRepository::insert(db, &snapshot)
            .await
            .unwrap();
        let mut newer = snapshot.clone();
        newer.used_percent = Some(12.5);
        let second = UsageSnapshotsRepository::insert(db, &newer).await.unwrap();
        assert!(second > first);
        let latest = UsageSnapshotsRepository::latest_for_account(db, &account_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(latest.id, second);
        assert_eq!(latest.snapshot.used_percent, Some(12.5));
        assert_eq!(latest.snapshot.credits_json, snapshot.credits_json);
        assert_eq!(latest.snapshot.resets_at, snapshot.resets_at);
        assert_eq!(latest.snapshot.secondary_resets_at, None);
        assert!(
            UsageSnapshotsRepository::list_for_account(db, &account_id, 0)
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            UsageSnapshotsRepository::list_for_account(db, &account_id, 1)
                .await
                .unwrap()
                .len(),
            1
        );
        let tx = db.begin().await.unwrap();
        let pending = UsageSnapshotsRepository::insert(&tx, &snapshot)
            .await
            .unwrap();
        tx.rollback().await.unwrap();
        let rows = UsageSnapshotsRepository::list_for_account(db, &account_id, 10)
            .await
            .unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|row| row.id != pending));
        assert!(UsageSnapshotsRepository::delete(db, first).await.unwrap());
        assert!(UsageSnapshotsRepository::delete(db, second).await.unwrap());
        assert!(
            UsageSnapshotsRepository::latest_for_account(db, &account_id)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn snapshot_ordering_nulls_large_timestamps_and_rollback() {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        let backend = db.get_database_backend();
        let table = Schema::new(backend).create_table_from_entity(Entity);
        db.execute(backend.build(&table)).await.unwrap();
        exercise(&db).await;
    }
}
impl UsageSnapshotsRepository {
    pub async fn latest(db: &impl ConnectionTrait) -> Result<Option<UsageSnapshotRecord>, DbErr> {
        Ok(Entity::find()
            .order_by_desc(Column::CapturedAt)
            .order_by_desc(Column::Id)
            .one(db)
            .await?
            .map(|v| UsageSnapshotRow::from(v).snapshot))
    }
    pub async fn latest_for_accounts(
        db: &impl ConnectionTrait,
        ids: &[String],
    ) -> Result<Vec<UsageSnapshotRecord>, DbErr> {
        let mut out = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for id in ids {
            if seen.insert(id) {
                if let Some(row) = Self::latest_for_account(db, id).await? {
                    out.push(row.snapshot)
                }
            }
        }
        out.sort_by(|a, b| {
            b.captured_at
                .cmp(&a.captured_at)
                .then_with(|| a.account_id.cmp(&b.account_id))
        });
        Ok(out)
    }
    pub async fn latest_by_account(
        db: &impl ConnectionTrait,
        limit: Option<usize>,
    ) -> Result<Vec<UsageSnapshotRecord>, DbErr> {
        let ids = Entity::find()
            .select_only()
            .column(Column::AccountId)
            .distinct()
            .into_tuple::<String>()
            .all(db)
            .await?;
        let mut out = Self::latest_for_accounts(db, &ids).await?;
        if let Some(limit) = limit {
            out.truncate(limit);
        }
        Ok(out)
    }
    pub async fn begin_snapshot_write(
        db: &sea_orm::DatabaseConnection,
        id: &str,
    ) -> Result<(sea_orm::DatabaseTransaction, Option<UsageSnapshotRecord>), DbErr> {
        use sea_orm::TransactionTrait;
        let tx = db.begin().await?;
        crate::UsersRepository::lock(&tx, "accounts").await?;
        let previous = Self::latest_for_account(&tx, id).await?.map(|v| v.snapshot);
        Ok((tx, previous))
    }
    pub async fn finish_snapshot_write(
        tx: sea_orm::DatabaseTransaction,
        snapshot: &UsageSnapshotRecord,
        retain: usize,
    ) -> Result<usize, DbErr> {
        Self::insert(&tx, snapshot).await?;
        crate::accounts_warmups::observe_usage_snapshot(&tx, snapshot).await?;
        let pruned = Self::prune_for_account(&tx, &snapshot.account_id, retain).await?;
        tx.commit().await?;
        Ok(pruned)
    }
    pub async fn insert_and_prune(
        db: &sea_orm::DatabaseConnection,
        snapshot: &UsageSnapshotRecord,
        retain: usize,
    ) -> Result<(UsageSnapshotRecord, usize), DbErr> {
        let (tx, previous) = Self::begin_snapshot_write(db, &snapshot.account_id).await?;
        let mut resolved = snapshot.clone();
        if let Some(previous) = previous {
            resolved.captured_at = resolved.captured_at.max(previous.captured_at);
        }
        let pruned = Self::finish_snapshot_write(tx, &resolved, retain).await?;
        Ok((resolved, pruned))
    }
    pub async fn prune_for_account(
        db: &impl ConnectionTrait,
        id: &str,
        retain: usize,
    ) -> Result<usize, DbErr> {
        if retain == 0 {
            return Ok(0);
        }
        let keep = Entity::find()
            .select_only()
            .column(Column::Id)
            .filter(Column::AccountId.eq(id))
            .order_by_desc(Column::CapturedAt)
            .order_by_desc(Column::Id)
            .limit(retain as u64)
            .into_tuple::<i64>()
            .all(db)
            .await?;
        Ok(Entity::delete_many()
            .filter(Column::AccountId.eq(id))
            .filter(Column::Id.is_not_in(keep))
            .exec(db)
            .await?
            .rows_affected as usize)
    }
    pub async fn count_for_account(db: &impl ConnectionTrait, id: &str) -> Result<i64, DbErr> {
        use sea_orm::PaginatorTrait;
        Ok(Entity::find()
            .filter(Column::AccountId.eq(id))
            .count(db)
            .await? as i64)
    }
}
