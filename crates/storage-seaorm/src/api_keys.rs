//! SeaORM repository for gateway API keys.
//!
//! The generated entity is intentionally private.  Callers only exchange the
//! database independent [`ApiKeyRecord`] value type.

use codexmanager_core::storage::ApiKey;
use sea_orm::entity::prelude::*;
use sea_orm::sea_query::OnConflict;
use sea_orm::{ColumnTrait, EntityTrait, QueryOrder, Set};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiKeyRecord {
    pub id: String,
    pub name: Option<String>,
    pub model_slug: Option<String>,
    pub reasoning_effort: Option<String>,
    pub service_tier: Option<String>,
    pub rotation_strategy: String,
    pub aggregate_api_id: Option<String>,
    pub account_plan_filter: Option<String>,
    pub account_group_filter: Option<String>,
    pub client_type: String,
    pub protocol_type: String,
    pub auth_scheme: String,
    pub upstream_base_url: Option<String>,
    pub static_headers_json: Option<String>,
    pub key_hash: String,
    pub status: String,
    pub created_at: i64,
    pub last_used_at: Option<i64>,
}

impl From<ApiKey> for ApiKeyRecord {
    fn from(key: ApiKey) -> Self {
        Self {
            id: key.id,
            name: key.name,
            model_slug: key.model_slug,
            reasoning_effort: key.reasoning_effort,
            service_tier: key.service_tier,
            rotation_strategy: key.rotation_strategy,
            aggregate_api_id: key.aggregate_api_id,
            account_plan_filter: key.account_plan_filter,
            account_group_filter: None,
            client_type: key.client_type,
            protocol_type: key.protocol_type,
            auth_scheme: key.auth_scheme,
            upstream_base_url: key.upstream_base_url,
            static_headers_json: key.static_headers_json,
            key_hash: key.key_hash,
            status: key.status,
            created_at: key.created_at,
            last_used_at: key.last_used_at,
        }
    }
}

#[derive(Clone, Debug, DeriveEntityModel)]
#[sea_orm(table_name = "api_keys")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    id: String,
    name: Option<String>,
    model_slug: Option<String>,
    reasoning_effort: Option<String>,
    service_tier: Option<String>,
    rotation_strategy: String,
    aggregate_api_id: Option<String>,
    #[sea_orm(column_type = "Text", nullable)]
    account_plan_filter: Option<String>,
    #[sea_orm(column_type = "Text", nullable)]
    account_group_filter: Option<String>,
    client_type: String,
    protocol_type: String,
    auth_scheme: String,
    #[sea_orm(column_type = "Text", nullable)]
    upstream_base_url: Option<String>,
    #[sea_orm(column_type = "Text", nullable)]
    static_headers_json: Option<String>,
    key_hash: String,
    status: String,
    created_at: i64,
    last_used_at: Option<i64>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

pub struct ApiKeysRepository;

impl ApiKeysRepository {
    pub async fn delete(db: &impl ConnectionTrait, id: &str) -> Result<bool, DbErr> {
        Entity::delete_by_id(id)
            .exec(db)
            .await
            .map(|result| result.rows_affected > 0)
    }

    pub async fn list(db: &impl ConnectionTrait) -> Result<Vec<ApiKeyRecord>, DbErr> {
        Entity::find()
            .order_by_desc(Column::CreatedAt)
            .order_by_asc(Column::Id)
            .all(db)
            .await
            .map(|rows| rows.into_iter().map(Into::into).collect())
    }

    pub async fn get(db: &impl ConnectionTrait, id: &str) -> Result<Option<ApiKeyRecord>, DbErr> {
        Entity::find_by_id(id)
            .one(db)
            .await
            .map(|row| row.map(Into::into))
    }

    pub async fn find_by_hash(
        db: &impl ConnectionTrait,
        key_hash: &str,
    ) -> Result<Option<ApiKeyRecord>, DbErr> {
        Entity::find()
            .filter(Column::KeyHash.eq(key_hash))
            .one(db)
            .await
            .map(|row| row.map(Into::into))
    }

    pub async fn upsert(db: &impl ConnectionTrait, key: ApiKeyRecord) -> Result<(), DbErr> {
        let model = ActiveModel {
            id: Set(key.id),
            name: Set(key.name),
            model_slug: Set(key.model_slug),
            reasoning_effort: Set(key.reasoning_effort),
            service_tier: Set(key.service_tier),
            rotation_strategy: Set(key.rotation_strategy),
            aggregate_api_id: Set(key.aggregate_api_id),
            account_plan_filter: Set(key.account_plan_filter),
            account_group_filter: Set(key.account_group_filter),
            client_type: Set(key.client_type),
            protocol_type: Set(key.protocol_type),
            auth_scheme: Set(key.auth_scheme),
            upstream_base_url: Set(key.upstream_base_url),
            static_headers_json: Set(key.static_headers_json),
            key_hash: Set(key.key_hash),
            status: Set(key.status),
            created_at: Set(key.created_at),
            last_used_at: Set(key.last_used_at),
        };
        Entity::insert(model)
            .on_conflict(
                OnConflict::column(Column::Id)
                    .update_columns([
                        Column::Name,
                        Column::ModelSlug,
                        Column::ReasoningEffort,
                        Column::ServiceTier,
                        Column::RotationStrategy,
                        Column::AggregateApiId,
                        Column::AccountPlanFilter,
                        Column::AccountGroupFilter,
                        Column::ClientType,
                        Column::ProtocolType,
                        Column::AuthScheme,
                        Column::UpstreamBaseUrl,
                        Column::StaticHeadersJson,
                        Column::KeyHash,
                        Column::Status,
                        Column::CreatedAt,
                        Column::LastUsedAt,
                    ])
                    .to_owned(),
            )
            .exec(db)
            .await
            .map(|_| ())
    }

    pub async fn update_status(
        db: &impl ConnectionTrait,
        id: &str,
        status: &str,
    ) -> Result<bool, DbErr> {
        let result = Entity::update_many()
            .col_expr(Column::Status, Expr::value(status))
            .filter(Column::Id.eq(id))
            .exec(db)
            .await?;
        Ok(result.rows_affected > 0)
    }

    pub async fn touch_last_used(
        db: &impl ConnectionTrait,
        id: &str,
        last_used_at: i64,
    ) -> Result<bool, DbErr> {
        let result = Entity::update_many()
            .col_expr(Column::LastUsedAt, Expr::value(last_used_at.max(0)))
            .filter(Column::Id.eq(id))
            .exec(db)
            .await?;
        Ok(result.rows_affected > 0)
    }
}

impl From<Model> for ApiKeyRecord {
    fn from(model: Model) -> Self {
        Self {
            id: model.id,
            name: model.name,
            model_slug: model.model_slug,
            reasoning_effort: model.reasoning_effort,
            service_tier: model.service_tier,
            rotation_strategy: model.rotation_strategy,
            aggregate_api_id: model.aggregate_api_id,
            account_plan_filter: model.account_plan_filter,
            account_group_filter: model.account_group_filter,
            client_type: model.client_type,
            protocol_type: model.protocol_type,
            auth_scheme: model.auth_scheme,
            upstream_base_url: model.upstream_base_url,
            static_headers_json: model.static_headers_json,
            key_hash: model.key_hash,
            status: model.status,
            created_at: model.created_at,
            last_used_at: model.last_used_at,
        }
    }
}

impl From<ApiKeyRecord> for ApiKey {
    fn from(record: ApiKeyRecord) -> Self {
        Self {
            id: record.id,
            name: record.name,
            model_slug: record.model_slug,
            reasoning_effort: record.reasoning_effort,
            service_tier: record.service_tier,
            rotation_strategy: record.rotation_strategy,
            aggregate_api_id: record.aggregate_api_id,
            account_plan_filter: record.account_plan_filter,
            aggregate_api_url: None,
            client_type: record.client_type,
            protocol_type: record.protocol_type,
            auth_scheme: record.auth_scheme,
            upstream_base_url: record.upstream_base_url,
            static_headers_json: record.static_headers_json,
            key_hash: record.key_hash,
            status: record.status,
            created_at: record.created_at,
            last_used_at: record.last_used_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{ConnectionTrait, Database, Statement};

    async fn db() -> DatabaseConnection {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        db.execute(Statement::from_string(
            db.get_database_backend(),
            "CREATE TABLE api_keys (id VARCHAR(255) PRIMARY KEY, name VARCHAR(255), model_slug VARCHAR(255), reasoning_effort VARCHAR(64), service_tier VARCHAR(64), rotation_strategy VARCHAR(64) NOT NULL, aggregate_api_id VARCHAR(255), account_plan_filter TEXT, account_group_filter TEXT, client_type VARCHAR(64) NOT NULL, protocol_type VARCHAR(64) NOT NULL, auth_scheme VARCHAR(64) NOT NULL, upstream_base_url TEXT, static_headers_json TEXT, key_hash VARCHAR(255) NOT NULL, status VARCHAR(64) NOT NULL, created_at BIGINT NOT NULL, last_used_at BIGINT)"
        )).await.unwrap();
        db
    }

    fn key(id: &str, hash: &str) -> ApiKeyRecord {
        ApiKeyRecord {
            id: id.into(),
            name: Some("Gateway".into()),
            model_slug: Some("gpt-5".into()),
            reasoning_effort: None,
            service_tier: None,
            rotation_strategy: "account_rotation".into(),
            aggregate_api_id: None,
            account_plan_filter: None,
            account_group_filter: None,
            client_type: "codex".into(),
            protocol_type: "openai_compat".into(),
            auth_scheme: "authorization_bearer".into(),
            upstream_base_url: None,
            static_headers_json: None,
            key_hash: hash.into(),
            status: "active".into(),
            created_at: 1,
            last_used_at: None,
        }
    }

    #[tokio::test]
    async fn api_key_repository_round_trip_and_mutations() {
        let db = db().await;
        ApiKeysRepository::upsert(&db, key("k1", "h1"))
            .await
            .unwrap();
        assert_eq!(
            ApiKeysRepository::find_by_hash(&db, "h1")
                .await
                .unwrap()
                .unwrap()
                .id,
            "k1"
        );
        assert!(ApiKeysRepository::update_status(&db, "k1", "disabled")
            .await
            .unwrap());
        assert!(ApiKeysRepository::touch_last_used(&db, "k1", -4)
            .await
            .unwrap());
        let got = ApiKeysRepository::get(&db, "k1").await.unwrap().unwrap();
        assert_eq!(got.status, "disabled");
        assert_eq!(got.last_used_at, Some(0));
        assert_eq!(ApiKeysRepository::list(&db).await.unwrap().len(), 1);
    }
}
