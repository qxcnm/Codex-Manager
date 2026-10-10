//! Repository for per-request token accounting records.

use codexmanager_core::storage::RequestTokenStat;
use sea_orm::entity::prelude::*;
use sea_orm::sea_query::OnConflict;
use sea_orm::{ColumnTrait, EntityTrait, QueryOrder, Set};

#[derive(Debug, Clone, PartialEq)]
pub struct RequestTokenStatRecord {
    pub request_log_id: i64,
    pub key_id: Option<String>,
    pub account_id: Option<String>,
    pub model: Option<String>,
    pub actual_source_kind: Option<String>,
    pub actual_source_id: Option<String>,
    pub input_tokens: Option<i64>,
    pub cached_input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub reasoning_output_tokens: Option<i64>,
    pub estimated_cost_usd: Option<f64>,
    pub usage_included: bool,
    pub created_at: i64,
}

#[derive(Clone, Debug, DeriveEntityModel)]
#[sea_orm(table_name = "request_token_stats")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    request_log_id: i64,
    // Preserve the desktop SQLite row identity when importing historical data.
    // New adapter writes use request_log_id as their identity instead.
    id: Option<i64>,
    key_id: Option<String>,
    account_id: Option<String>,
    model: Option<String>,
    actual_source_kind: Option<String>,
    actual_source_id: Option<String>,
    input_tokens: Option<i64>,
    cached_input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    total_tokens: Option<i64>,
    reasoning_output_tokens: Option<i64>,
    estimated_cost_usd: Option<f64>,
    usage_included: bool,
    created_at: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}
impl ActiveModelBehavior for ActiveModel {}

pub struct RequestTokenStatsRepository;

impl RequestTokenStatsRepository {
    pub async fn delete(db: &impl ConnectionTrait, request_log_id: i64) -> Result<bool, DbErr> {
        Entity::delete_by_id(request_log_id)
            .exec(db)
            .await
            .map(|result| result.rows_affected > 0)
    }

    pub async fn get(
        db: &impl ConnectionTrait,
        request_log_id: i64,
    ) -> Result<Option<RequestTokenStatRecord>, DbErr> {
        Entity::find()
            .filter(Column::RequestLogId.eq(request_log_id))
            .one(db)
            .await
            .map(|row| row.map(Into::into))
    }

    pub async fn list(db: &impl ConnectionTrait) -> Result<Vec<RequestTokenStatRecord>, DbErr> {
        Entity::find()
            .order_by_desc(Column::CreatedAt)
            .all(db)
            .await
            .map(|rows| rows.into_iter().map(Into::into).collect())
    }

    pub async fn upsert(
        db: &impl ConnectionTrait,
        stat: RequestTokenStatRecord,
    ) -> Result<(), DbErr> {
        let model = ActiveModel {
            request_log_id: Set(stat.request_log_id),
            id: Set(None),
            key_id: Set(stat.key_id),
            account_id: Set(stat.account_id),
            model: Set(stat.model),
            actual_source_kind: Set(stat.actual_source_kind),
            actual_source_id: Set(stat.actual_source_id),
            input_tokens: Set(stat.input_tokens),
            cached_input_tokens: Set(stat.cached_input_tokens),
            output_tokens: Set(stat.output_tokens),
            total_tokens: Set(stat.total_tokens),
            reasoning_output_tokens: Set(stat.reasoning_output_tokens),
            estimated_cost_usd: Set(stat.estimated_cost_usd),
            usage_included: Set(stat.usage_included),
            created_at: Set(stat.created_at),
        };
        Entity::insert(model)
            .on_conflict(
                OnConflict::column(Column::RequestLogId)
                    .update_columns([
                        Column::KeyId,
                        Column::AccountId,
                        Column::Model,
                        Column::ActualSourceKind,
                        Column::ActualSourceId,
                        Column::InputTokens,
                        Column::CachedInputTokens,
                        Column::OutputTokens,
                        Column::TotalTokens,
                        Column::ReasoningOutputTokens,
                        Column::EstimatedCostUsd,
                        Column::UsageIncluded,
                        Column::CreatedAt,
                    ])
                    .to_owned(),
            )
            .exec(db)
            .await
            .map(|_| ())
    }
}

impl From<Model> for RequestTokenStatRecord {
    fn from(m: Model) -> Self {
        Self {
            request_log_id: m.request_log_id,
            key_id: m.key_id,
            account_id: m.account_id,
            model: m.model,
            actual_source_kind: m.actual_source_kind,
            actual_source_id: m.actual_source_id,
            input_tokens: m.input_tokens,
            cached_input_tokens: m.cached_input_tokens,
            output_tokens: m.output_tokens,
            total_tokens: m.total_tokens,
            reasoning_output_tokens: m.reasoning_output_tokens,
            estimated_cost_usd: m.estimated_cost_usd,
            usage_included: m.usage_included,
            created_at: m.created_at,
        }
    }
}

impl From<RequestTokenStatRecord> for RequestTokenStat {
    fn from(s: RequestTokenStatRecord) -> Self {
        Self {
            request_log_id: s.request_log_id,
            key_id: s.key_id,
            account_id: s.account_id,
            model: s.model,
            actual_source_kind: s.actual_source_kind,
            actual_source_id: s.actual_source_id,
            input_tokens: s.input_tokens,
            cached_input_tokens: s.cached_input_tokens,
            output_tokens: s.output_tokens,
            total_tokens: s.total_tokens,
            reasoning_output_tokens: s.reasoning_output_tokens,
            estimated_cost_usd: s.estimated_cost_usd,
            created_at: s.created_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{ConnectionTrait, Database, Statement};

    #[tokio::test]
    async fn upsert_replaces_stat_by_request_log() {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        db.execute(Statement::from_string(db.get_database_backend(), "CREATE TABLE request_token_stats (request_log_id BIGINT PRIMARY KEY, id BIGINT NULL, key_id TEXT, account_id TEXT, model TEXT, actual_source_kind TEXT, actual_source_id TEXT, input_tokens BIGINT, cached_input_tokens BIGINT, output_tokens BIGINT, total_tokens BIGINT, reasoning_output_tokens BIGINT, estimated_cost_usd DOUBLE, usage_included BOOLEAN NOT NULL DEFAULT 1, created_at BIGINT NOT NULL)")).await.unwrap();
        let stat = RequestTokenStatRecord {
            request_log_id: 9,
            key_id: Some("k".into()),
            account_id: None,
            model: Some("gpt-5".into()),
            actual_source_kind: None,
            actual_source_id: None,
            input_tokens: Some(3),
            cached_input_tokens: None,
            output_tokens: Some(2),
            total_tokens: Some(5),
            reasoning_output_tokens: None,
            estimated_cost_usd: Some(0.01),
            usage_included: true,
            created_at: 1,
        };
        RequestTokenStatsRepository::upsert(&db, stat.clone())
            .await
            .unwrap();
        db.execute(Statement::from_string(
            db.get_database_backend(),
            "UPDATE request_token_stats SET id = 123 WHERE request_log_id = 9",
        ))
        .await
        .unwrap();
        let mut changed = stat;
        changed.total_tokens = Some(7);
        changed.usage_included = false;
        RequestTokenStatsRepository::upsert(&db, changed)
            .await
            .unwrap();
        let got = RequestTokenStatsRepository::get(&db, 9)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(got.total_tokens, Some(7));
        assert!(!got.usage_included);
        assert_eq!(
            Entity::find_by_id(9).one(&db).await.unwrap().unwrap().id,
            Some(123)
        );
        assert_eq!(
            RequestTokenStatsRepository::list(&db).await.unwrap().len(),
            1
        );
    }
}
