//! Aggregate provider configuration, secrets and supplier model storage.
use codexmanager_core::storage::*;
use sea_orm::{entity::prelude::*, DatabaseConnection, QueryOrder, Set, TransactionTrait};
use std::collections::{HashMap, HashSet};
pub(crate) mod providers {
    use sea_orm::entity::prelude::*;
    #[derive(Clone, Debug, DeriveEntityModel)]
    #[sea_orm(table_name = "aggregate_apis")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        #[sea_orm(column_type = "Text")]
        pub provider_type: String,
        #[sea_orm(column_type = "Text", nullable)]
        pub supplier_name: Option<String>,
        pub sort: i64,
        #[sea_orm(column_type = "Text")]
        pub url: String,
        #[sea_orm(column_type = "Text")]
        pub auth_type: String,
        #[sea_orm(column_type = "Text", nullable)]
        pub auth_params_json: Option<String>,
        #[sea_orm(column_type = "Text", nullable)]
        pub action: Option<String>,
        #[sea_orm(column_type = "Text", nullable)]
        pub model_override: Option<String>,
        #[sea_orm(column_type = "Text", nullable)]
        pub user_agent: Option<String>,
        #[sea_orm(column_type = "Text")]
        pub status: String,
        pub created_at: i64,
        pub updated_at: i64,
        pub last_test_at: Option<i64>,
        #[sea_orm(column_type = "Text", nullable)]
        pub last_test_status: Option<String>,
        #[sea_orm(column_type = "Text", nullable)]
        pub last_test_error: Option<String>,
        pub balance_query_enabled: bool,
        #[sea_orm(column_type = "Text", nullable)]
        pub balance_query_template: Option<String>,
        #[sea_orm(column_type = "Text", nullable)]
        pub balance_query_base_url: Option<String>,
        #[sea_orm(column_type = "Text", nullable)]
        pub balance_query_user_id: Option<String>,
        #[sea_orm(column_type = "Text", nullable)]
        pub balance_query_config_json: Option<String>,
        pub last_balance_at: Option<i64>,
        #[sea_orm(column_type = "Text", nullable)]
        pub last_balance_status: Option<String>,
        #[sea_orm(column_type = "Text", nullable)]
        pub last_balance_error: Option<String>,
        #[sea_orm(column_type = "Text", nullable)]
        pub last_balance_json: Option<String>,
    }
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}
    impl ActiveModelBehavior for ActiveModel {}
    impl From<Model> for codexmanager_core::storage::AggregateApi {
        fn from(v: Model) -> Self {
            Self {
                id: v.id,
                provider_type: v.provider_type,
                supplier_name: v.supplier_name,
                sort: v.sort,
                url: v.url,
                auth_type: v.auth_type,
                auth_params_json: v.auth_params_json,
                action: v.action,
                model_override: v.model_override,
                user_agent: v.user_agent,
                status: v.status,
                created_at: v.created_at,
                updated_at: v.updated_at,
                last_test_at: v.last_test_at,
                last_test_status: v.last_test_status,
                last_test_error: v.last_test_error,
                balance_query_enabled: v.balance_query_enabled,
                balance_query_template: v.balance_query_template,
                balance_query_base_url: v.balance_query_base_url,
                balance_query_user_id: v.balance_query_user_id,
                balance_query_config_json: v.balance_query_config_json,
                last_balance_at: v.last_balance_at,
                last_balance_status: v.last_balance_status,
                last_balance_error: v.last_balance_error,
                last_balance_json: v.last_balance_json,
            }
        }
    }
    impl From<codexmanager_core::storage::AggregateApi> for ActiveModel {
        fn from(v: codexmanager_core::storage::AggregateApi) -> Self {
            Self {
                id: sea_orm::Set(v.id),
                provider_type: sea_orm::Set(v.provider_type),
                supplier_name: sea_orm::Set(v.supplier_name),
                sort: sea_orm::Set(v.sort),
                url: sea_orm::Set(v.url),
                auth_type: sea_orm::Set(v.auth_type),
                auth_params_json: sea_orm::Set(v.auth_params_json),
                action: sea_orm::Set(v.action),
                model_override: sea_orm::Set(v.model_override),
                user_agent: sea_orm::Set(v.user_agent),
                status: sea_orm::Set(v.status),
                created_at: sea_orm::Set(v.created_at),
                updated_at: sea_orm::Set(v.updated_at),
                last_test_at: sea_orm::Set(v.last_test_at),
                last_test_status: sea_orm::Set(v.last_test_status),
                last_test_error: sea_orm::Set(v.last_test_error),
                balance_query_enabled: sea_orm::Set(v.balance_query_enabled),
                balance_query_template: sea_orm::Set(v.balance_query_template),
                balance_query_base_url: sea_orm::Set(v.balance_query_base_url),
                balance_query_user_id: sea_orm::Set(v.balance_query_user_id),
                balance_query_config_json: sea_orm::Set(v.balance_query_config_json),
                last_balance_at: sea_orm::Set(v.last_balance_at),
                last_balance_status: sea_orm::Set(v.last_balance_status),
                last_balance_error: sea_orm::Set(v.last_balance_error),
                last_balance_json: sea_orm::Set(v.last_balance_json),
            }
        }
    }
}
pub(crate) mod suppliers {
    use sea_orm::entity::prelude::*;
    #[derive(Clone, Debug, DeriveEntityModel)]
    #[sea_orm(table_name = "aggregate_api_supplier_models")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub supplier_key: String,
        #[sea_orm(primary_key, auto_increment = false)]
        pub provider_type: String,
        #[sea_orm(primary_key, auto_increment = false)]
        pub upstream_model: String,
        #[sea_orm(column_type = "Text", nullable)]
        pub display_name: Option<String>,
        #[sea_orm(column_type = "Text")]
        pub status: String,
        pub created_at: i64,
        pub updated_at: i64,
    }
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}
    impl ActiveModelBehavior for ActiveModel {}
    impl From<Model> for codexmanager_core::storage::AggregateApiSupplierModel {
        fn from(v: Model) -> Self {
            Self {
                supplier_key: v.supplier_key,
                provider_type: v.provider_type,
                upstream_model: v.upstream_model,
                display_name: v.display_name,
                status: v.status,
                created_at: v.created_at,
                updated_at: v.updated_at,
            }
        }
    }
    impl From<codexmanager_core::storage::AggregateApiSupplierModel> for ActiveModel {
        fn from(v: codexmanager_core::storage::AggregateApiSupplierModel) -> Self {
            Self {
                supplier_key: sea_orm::Set(v.supplier_key),
                provider_type: sea_orm::Set(v.provider_type),
                upstream_model: sea_orm::Set(v.upstream_model),
                display_name: sea_orm::Set(v.display_name),
                status: sea_orm::Set(v.status),
                created_at: sea_orm::Set(v.created_at),
                updated_at: sea_orm::Set(v.updated_at),
            }
        }
    }
}
pub(crate) mod secrets {
    use sea_orm::entity::prelude::*;
    #[derive(Clone, Debug, DeriveEntityModel)]
    #[sea_orm(table_name = "aggregate_api_secrets")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub aggregate_api_id: String,
        #[sea_orm(column_type = "Text")]
        pub secret_value: String,
        pub created_at: i64,
        pub updated_at: i64,
    }
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}
    impl ActiveModelBehavior for ActiveModel {}
}
pub(crate) mod balance_secrets {
    use sea_orm::entity::prelude::*;
    #[derive(Clone, Debug, DeriveEntityModel)]
    #[sea_orm(table_name = "aggregate_api_balance_secrets")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub aggregate_api_id: String,
        #[sea_orm(column_type = "Text")]
        pub access_token: String,
        pub created_at: i64,
        pub updated_at: i64,
    }
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}
    impl ActiveModelBehavior for ActiveModel {}
}
pub struct AggregateApisRepository;
impl AggregateApisRepository {
    pub async fn insert_aggregate_api(
        db: &impl ConnectionTrait,
        api: &AggregateApi,
    ) -> Result<(), DbErr> {
        let row: providers::ActiveModel = api.clone().into();
        providers::Entity::insert(row)
            .on_conflict(
                sea_orm::sea_query::OnConflict::column(providers::Column::Id)
                    .update_columns([
                        providers::Column::ProviderType,
                        providers::Column::SupplierName,
                        providers::Column::Sort,
                        providers::Column::Url,
                        providers::Column::AuthType,
                        providers::Column::AuthParamsJson,
                        providers::Column::Action,
                        providers::Column::ModelOverride,
                        providers::Column::UserAgent,
                        providers::Column::Status,
                        providers::Column::CreatedAt,
                        providers::Column::UpdatedAt,
                        providers::Column::LastTestAt,
                        providers::Column::LastTestStatus,
                        providers::Column::LastTestError,
                        providers::Column::BalanceQueryEnabled,
                        providers::Column::BalanceQueryTemplate,
                        providers::Column::BalanceQueryBaseUrl,
                        providers::Column::BalanceQueryUserId,
                        providers::Column::BalanceQueryConfigJson,
                        providers::Column::LastBalanceAt,
                        providers::Column::LastBalanceStatus,
                        providers::Column::LastBalanceError,
                        providers::Column::LastBalanceJson,
                    ])
                    .to_owned(),
            )
            .exec(db)
            .await?;
        Ok(())
    }
    pub async fn list_aggregate_apis(
        db: &impl ConnectionTrait,
    ) -> Result<Vec<AggregateApi>, DbErr> {
        Ok(providers::Entity::find()
            .order_by_asc(providers::Column::Sort)
            .order_by_desc(providers::Column::UpdatedAt)
            .order_by_asc(providers::Column::Id)
            .all(db)
            .await?
            .into_iter()
            .map(Into::into)
            .collect())
    }
    pub async fn find_aggregate_api_by_id(
        db: &impl ConnectionTrait,
        id: &str,
    ) -> Result<Option<AggregateApi>, DbErr> {
        Ok(providers::Entity::find_by_id(id)
            .one(db)
            .await?
            .map(Into::into))
    }
    pub async fn aggregate_api_exists(db: &impl ConnectionTrait, id: &str) -> Result<bool, DbErr> {
        Ok(Self::find_aggregate_api_by_id(db, id).await?.is_some())
    }
    pub async fn list_aggregate_apis_for_ids(
        db: &impl ConnectionTrait,
        ids: &[String],
    ) -> Result<Vec<AggregateApi>, DbErr> {
        let ids = ids.iter().map(|s| s.trim()).collect::<HashSet<_>>();
        Ok(Self::list_aggregate_apis(db)
            .await?
            .into_iter()
            .filter(|a| ids.contains(a.id.as_str()))
            .collect())
    }
    pub async fn list_aggregate_api_summaries(
        db: &impl ConnectionTrait,
    ) -> Result<Vec<AggregateApiListSummary>, DbErr> {
        Ok(Self::list_aggregate_apis(db)
            .await?
            .into_iter()
            .map(|a| AggregateApiListSummary {
                id: a.id,
                provider_type: a.provider_type,
                supplier_name: a.supplier_name,
                sort: a.sort,
                url: a.url,
                auth_type: a.auth_type,
                auth_params_json: a.auth_params_json,
                action: a.action,
                model_override: a.model_override,
                user_agent: a.user_agent,
                status: a.status,
                created_at: a.created_at,
                updated_at: a.updated_at,
                last_test_at: a.last_test_at,
                last_test_status: a.last_test_status,
                last_test_error: a.last_test_error,
                balance_query_enabled: a.balance_query_enabled,
                balance_query_template: a.balance_query_template,
                balance_query_base_url: a.balance_query_base_url,
                balance_query_user_id: a.balance_query_user_id,
                balance_query_config_json: a.balance_query_config_json,
                last_balance_at: a.last_balance_at,
                last_balance_status: a.last_balance_status,
                last_balance_error: a.last_balance_error,
                last_balance_json: a.last_balance_json,
            })
            .collect())
    }
    pub async fn list_aggregate_api_quota_source_summaries(
        db: &impl ConnectionTrait,
    ) -> Result<Vec<AggregateApiQuotaSourceSummary>, DbErr> {
        Ok(Self::list_aggregate_apis(db)
            .await?
            .into_iter()
            .map(|a| AggregateApiQuotaSourceSummary {
                id: a.id,
                provider_type: a.provider_type,
                supplier_name: a.supplier_name,
                url: a.url,
                status: a.status,
                balance_query_enabled: a.balance_query_enabled,
                last_balance_at: a.last_balance_at,
                last_balance_status: a.last_balance_status,
                last_balance_error: a.last_balance_error,
                last_balance_json: a.last_balance_json,
            })
            .collect())
    }
    pub async fn list_aggregate_api_dashboard_source_metadata_for_ids(
        db: &impl ConnectionTrait,
        ids: &[String],
    ) -> Result<Vec<AggregateApiDashboardSourceMetadata>, DbErr> {
        Ok(Self::list_aggregate_apis_for_ids(db, ids)
            .await?
            .into_iter()
            .map(|a| AggregateApiDashboardSourceMetadata {
                id: a.id,
                provider_type: a.provider_type,
                supplier_name: a.supplier_name,
                url: a.url,
                status: a.status,
            })
            .collect())
    }
    pub async fn find_aggregate_api_supplier_identity_by_id(
        db: &impl ConnectionTrait,
        id: &str,
    ) -> Result<Option<AggregateApiSupplierIdentity>, DbErr> {
        Ok(Self::find_aggregate_api_by_id(db, id)
            .await?
            .map(|a| AggregateApiSupplierIdentity {
                id: a.id,
                provider_type: a.provider_type,
                supplier_name: a.supplier_name,
                url: a.url,
            }))
    }
    pub async fn find_aggregate_api_update_config_by_id(
        db: &impl ConnectionTrait,
        id: &str,
    ) -> Result<Option<AggregateApiUpdateConfig>, DbErr> {
        Ok(Self::find_aggregate_api_by_id(db, id)
            .await?
            .map(|a| AggregateApiUpdateConfig {
                auth_type: a.auth_type,
                user_agent: a.user_agent,
                balance_query_enabled: a.balance_query_enabled,
                balance_query_template: a.balance_query_template,
                balance_query_base_url: a.balance_query_base_url,
                balance_query_user_id: a.balance_query_user_id,
                balance_query_config_json: a.balance_query_config_json,
            }))
    }
    pub async fn find_aggregate_api_status_by_id(
        db: &impl ConnectionTrait,
        id: &str,
    ) -> Result<Option<String>, DbErr> {
        Ok(Self::find_aggregate_api_by_id(db, id)
            .await?
            .map(|a| a.status))
    }
    pub async fn find_aggregate_api_auth_type_by_id(
        db: &impl ConnectionTrait,
        id: &str,
    ) -> Result<Option<String>, DbErr> {
        Ok(Self::find_aggregate_api_by_id(db, id)
            .await?
            .map(|a| a.auth_type))
    }
    pub async fn list_aggregate_api_ids(db: &impl ConnectionTrait) -> Result<Vec<String>, DbErr> {
        Ok(Self::list_aggregate_apis(db)
            .await?
            .into_iter()
            .map(|a| a.id)
            .collect())
    }
    pub async fn list_active_aggregate_api_ids(
        db: &impl ConnectionTrait,
    ) -> Result<Vec<String>, DbErr> {
        Ok(Self::list_aggregate_apis(db)
            .await?
            .into_iter()
            .filter(|a| a.status.trim().eq_ignore_ascii_case("active"))
            .map(|a| a.id)
            .collect())
    }
    pub async fn list_balance_query_aggregate_api_ids(
        db: &impl ConnectionTrait,
    ) -> Result<Vec<String>, DbErr> {
        Ok(Self::list_aggregate_apis(db)
            .await?
            .into_iter()
            .filter(|a| a.balance_query_enabled)
            .map(|a| a.id)
            .collect())
    }
    pub async fn list_active_balance_query_aggregate_api_ids(
        db: &impl ConnectionTrait,
    ) -> Result<Vec<String>, DbErr> {
        Ok(Self::list_aggregate_apis(db)
            .await?
            .into_iter()
            .filter(|a| a.balance_query_enabled && a.status.trim().eq_ignore_ascii_case("active"))
            .map(|a| a.id)
            .collect())
    }
    pub async fn list_balance_query_aggregate_api_ids_for_ids(
        db: &impl ConnectionTrait,
        ids: &[String],
    ) -> Result<Vec<String>, DbErr> {
        Ok(Self::list_aggregate_apis_for_ids(db, ids)
            .await?
            .into_iter()
            .filter(|a| a.balance_query_enabled)
            .map(|a| a.id)
            .collect())
    }
    pub async fn list_active_aggregate_apis(
        db: &impl ConnectionTrait,
    ) -> Result<Vec<AggregateApi>, DbErr> {
        Ok(Self::list_aggregate_apis(db)
            .await?
            .into_iter()
            .filter(|a| a.status.trim().eq_ignore_ascii_case("active"))
            .collect())
    }
    pub async fn list_active_aggregate_apis_by_provider_type(
        db: &impl ConnectionTrait,
        kind: &str,
    ) -> Result<Vec<AggregateApi>, DbErr> {
        let kind = kind.trim().to_ascii_lowercase().replace('-', "_");
        Ok(Self::list_active_aggregate_apis(db)
            .await?
            .into_iter()
            .filter(|a| {
                a.provider_type
                    .trim()
                    .to_ascii_lowercase()
                    .replace('-', "_")
                    == kind
            })
            .collect())
    }
    pub async fn list_aggregate_api_balance_jsons(
        db: &impl ConnectionTrait,
    ) -> Result<Vec<String>, DbErr> {
        Ok(Self::list_aggregate_apis(db)
            .await?
            .into_iter()
            .filter_map(|a| a.last_balance_json)
            .filter(|s| !s.trim().is_empty())
            .collect())
    }
    pub async fn aggregate_api_overview_stats(
        db: &impl ConnectionTrait,
    ) -> Result<AggregateApiOverviewStats, DbErr> {
        let rows = Self::list_aggregate_apis(db).await?;
        Ok(AggregateApiOverviewStats {
            source_count: rows.len() as i64,
            enabled_balance_query_count: rows.iter().filter(|a| a.balance_query_enabled).count()
                as i64,
            ok_count: rows
                .iter()
                .filter(|a| a.last_balance_status.as_deref() == Some("success"))
                .count() as i64,
            error_count: rows
                .iter()
                .filter(|a| matches!(a.last_balance_status.as_deref(), Some("error" | "failed")))
                .count() as i64,
            last_refreshed_at: rows.iter().filter_map(|a| a.last_balance_at).max(),
        })
    }
    pub async fn update_aggregate_api(
        db: &impl ConnectionTrait,
        id: &str,
        value: &str,
    ) -> Result<(), DbErr> {
        providers::Entity::update_many()
            .col_expr(providers::Column::Url, Expr::value(value))
            .col_expr(providers::Column::UpdatedAt, Expr::value(now_ts()))
            .filter(providers::Column::Id.eq(id))
            .exec(db)
            .await?;
        Ok(())
    }
    pub async fn update_aggregate_api_supplier_name(
        db: &impl ConnectionTrait,
        id: &str,
        value: Option<&str>,
    ) -> Result<(), DbErr> {
        providers::Entity::update_many()
            .col_expr(providers::Column::SupplierName, Expr::value(value))
            .col_expr(providers::Column::UpdatedAt, Expr::value(now_ts()))
            .filter(providers::Column::Id.eq(id))
            .exec(db)
            .await?;
        Ok(())
    }
    pub async fn update_aggregate_api_sort(
        db: &impl ConnectionTrait,
        id: &str,
        value: i64,
    ) -> Result<(), DbErr> {
        providers::Entity::update_many()
            .col_expr(providers::Column::Sort, Expr::value(value))
            .col_expr(providers::Column::UpdatedAt, Expr::value(now_ts()))
            .filter(providers::Column::Id.eq(id))
            .exec(db)
            .await?;
        Ok(())
    }
    pub async fn update_aggregate_api_status(
        db: &impl ConnectionTrait,
        id: &str,
        value: &str,
    ) -> Result<(), DbErr> {
        providers::Entity::update_many()
            .col_expr(providers::Column::Status, Expr::value(value))
            .col_expr(providers::Column::UpdatedAt, Expr::value(now_ts()))
            .filter(providers::Column::Id.eq(id))
            .exec(db)
            .await?;
        Ok(())
    }
    pub async fn update_aggregate_api_type(
        db: &impl ConnectionTrait,
        id: &str,
        value: &str,
    ) -> Result<(), DbErr> {
        providers::Entity::update_many()
            .col_expr(providers::Column::ProviderType, Expr::value(value))
            .col_expr(providers::Column::UpdatedAt, Expr::value(now_ts()))
            .filter(providers::Column::Id.eq(id))
            .exec(db)
            .await?;
        Ok(())
    }
    pub async fn update_aggregate_api_auth_type(
        db: &impl ConnectionTrait,
        id: &str,
        value: &str,
    ) -> Result<(), DbErr> {
        providers::Entity::update_many()
            .col_expr(providers::Column::AuthType, Expr::value(value))
            .col_expr(providers::Column::UpdatedAt, Expr::value(now_ts()))
            .filter(providers::Column::Id.eq(id))
            .exec(db)
            .await?;
        Ok(())
    }
    pub async fn update_aggregate_api_auth_params_json(
        db: &impl ConnectionTrait,
        id: &str,
        value: Option<&str>,
    ) -> Result<(), DbErr> {
        providers::Entity::update_many()
            .col_expr(providers::Column::AuthParamsJson, Expr::value(value))
            .col_expr(providers::Column::UpdatedAt, Expr::value(now_ts()))
            .filter(providers::Column::Id.eq(id))
            .exec(db)
            .await?;
        Ok(())
    }
    pub async fn update_aggregate_api_action(
        db: &impl ConnectionTrait,
        id: &str,
        value: Option<&str>,
    ) -> Result<(), DbErr> {
        providers::Entity::update_many()
            .col_expr(providers::Column::Action, Expr::value(value))
            .col_expr(providers::Column::UpdatedAt, Expr::value(now_ts()))
            .filter(providers::Column::Id.eq(id))
            .exec(db)
            .await?;
        Ok(())
    }
    pub async fn update_aggregate_api_model_override(
        db: &impl ConnectionTrait,
        id: &str,
        value: Option<&str>,
    ) -> Result<(), DbErr> {
        providers::Entity::update_many()
            .col_expr(providers::Column::ModelOverride, Expr::value(value))
            .col_expr(providers::Column::UpdatedAt, Expr::value(now_ts()))
            .filter(providers::Column::Id.eq(id))
            .exec(db)
            .await?;
        Ok(())
    }
    pub async fn update_aggregate_api_user_agent(
        db: &impl ConnectionTrait,
        id: &str,
        value: Option<&str>,
    ) -> Result<(), DbErr> {
        providers::Entity::update_many()
            .col_expr(providers::Column::UserAgent, Expr::value(value))
            .col_expr(providers::Column::UpdatedAt, Expr::value(now_ts()))
            .filter(providers::Column::Id.eq(id))
            .exec(db)
            .await?;
        Ok(())
    }
    pub async fn upsert_aggregate_api_secret(
        db: &impl ConnectionTrait,
        id: &str,
        value: &str,
    ) -> Result<(), DbErr> {
        let now = now_ts();
        secrets::Entity::insert(secrets::ActiveModel {
            aggregate_api_id: Set(id.into()),
            secret_value: Set(value.into()),
            created_at: Set(now),
            updated_at: Set(now),
        })
        .on_conflict(
            sea_orm::sea_query::OnConflict::column(secrets::Column::AggregateApiId)
                .update_columns([secrets::Column::SecretValue, secrets::Column::UpdatedAt])
                .to_owned(),
        )
        .exec(db)
        .await?;
        Ok(())
    }
    pub async fn find_aggregate_api_secret_by_id(
        db: &impl ConnectionTrait,
        id: &str,
    ) -> Result<Option<String>, DbErr> {
        Ok(secrets::Entity::find_by_id(id)
            .one(db)
            .await?
            .map(|s| s.secret_value))
    }
    pub async fn upsert_aggregate_api_balance_secret(
        db: &impl ConnectionTrait,
        id: &str,
        value: &str,
    ) -> Result<(), DbErr> {
        let now = now_ts();
        balance_secrets::Entity::insert(balance_secrets::ActiveModel {
            aggregate_api_id: Set(id.into()),
            access_token: Set(value.into()),
            created_at: Set(now),
            updated_at: Set(now),
        })
        .on_conflict(
            sea_orm::sea_query::OnConflict::column(balance_secrets::Column::AggregateApiId)
                .update_columns([
                    balance_secrets::Column::AccessToken,
                    balance_secrets::Column::UpdatedAt,
                ])
                .to_owned(),
        )
        .exec(db)
        .await?;
        Ok(())
    }
    pub async fn find_aggregate_api_balance_secret_by_id(
        db: &impl ConnectionTrait,
        id: &str,
    ) -> Result<Option<String>, DbErr> {
        Ok(balance_secrets::Entity::find_by_id(id)
            .one(db)
            .await?
            .map(|s| s.access_token))
    }
    pub async fn delete_aggregate_api_balance_secret(
        db: &impl ConnectionTrait,
        id: &str,
    ) -> Result<(), DbErr> {
        balance_secrets::Entity::delete_by_id(id).exec(db).await?;
        Ok(())
    }
    pub async fn list_aggregate_api_secrets_for_ids(
        db: &impl ConnectionTrait,
        ids: &[String],
    ) -> Result<HashMap<String, String>, DbErr> {
        Ok(secrets::Entity::find()
            .filter(
                secrets::Column::AggregateApiId
                    .is_in(ids.iter().map(|s| s.trim().to_owned()).collect::<Vec<_>>()),
            )
            .all(db)
            .await?
            .into_iter()
            .map(|s| (s.aggregate_api_id, s.secret_value))
            .collect())
    }
    pub async fn find_aggregate_api_with_secrets_by_id(
        db: &impl ConnectionTrait,
        id: &str,
    ) -> Result<Option<AggregateApiWithSecrets>, DbErr> {
        let Some(api) = Self::find_aggregate_api_by_id(db, id).await? else {
            return Ok(None);
        };
        Ok(Some(AggregateApiWithSecrets {
            api,
            secret_value: Self::find_aggregate_api_secret_by_id(db, id).await?,
            balance_access_token: Self::find_aggregate_api_balance_secret_by_id(db, id).await?,
        }))
    }
    pub async fn find_aggregate_api_secret_config_by_id(
        db: &impl ConnectionTrait,
        id: &str,
    ) -> Result<Option<AggregateApiSecretConfig>, DbErr> {
        let Some(auth_type) = Self::find_aggregate_api_auth_type_by_id(db, id).await? else {
            return Ok(None);
        };
        Ok(Some(AggregateApiSecretConfig {
            auth_type,
            secret_value: Self::find_aggregate_api_secret_by_id(db, id).await?,
        }))
    }
    pub async fn delete_aggregate_api(db: &DatabaseConnection, id: &str) -> Result<(), DbErr> {
        let tx = db.begin().await?;
        secrets::Entity::delete_by_id(id).exec(&tx).await?;
        balance_secrets::Entity::delete_by_id(id).exec(&tx).await?;
        crate::model_catalog::routes::Entity::delete_many()
            .filter(crate::model_catalog::routes::Column::SourceKind.eq("aggregate_api"))
            .filter(crate::model_catalog::routes::Column::SourceId.eq(id))
            .exec(&tx)
            .await?;
        providers::Entity::delete_by_id(id).exec(&tx).await?;
        tx.commit().await
    }
    pub async fn update_aggregate_api_balance_query(
        db: &impl ConnectionTrait,
        id: &str,
        enabled: bool,
        template: Option<&str>,
        base_url: Option<&str>,
        user_id: Option<&str>,
        config_json: Option<&str>,
    ) -> Result<(), DbErr> {
        providers::Entity::update_many()
            .col_expr(providers::Column::BalanceQueryEnabled, Expr::value(enabled))
            .col_expr(
                providers::Column::BalanceQueryTemplate,
                Expr::value(template),
            )
            .col_expr(
                providers::Column::BalanceQueryBaseUrl,
                Expr::value(base_url),
            )
            .col_expr(providers::Column::BalanceQueryUserId, Expr::value(user_id))
            .col_expr(
                providers::Column::BalanceQueryConfigJson,
                Expr::value(config_json),
            )
            .col_expr(providers::Column::UpdatedAt, Expr::value(now_ts()))
            .filter(providers::Column::Id.eq(id))
            .exec(db)
            .await?;
        Ok(())
    }
    pub async fn update_aggregate_api_balance_result(
        db: &impl ConnectionTrait,
        id: &str,
        ok: bool,
        balance_json: Option<&str>,
        error: Option<&str>,
    ) -> Result<(), DbErr> {
        let now = now_ts();
        providers::Entity::update_many()
            .col_expr(providers::Column::LastBalanceAt, Expr::value(now))
            .col_expr(
                providers::Column::LastBalanceStatus,
                Expr::value(if ok { "success" } else { "failed" }),
            )
            .col_expr(providers::Column::LastBalanceError, Expr::value(error))
            .col_expr(
                providers::Column::LastBalanceJson,
                Expr::value(balance_json),
            )
            .col_expr(providers::Column::UpdatedAt, Expr::value(now))
            .filter(providers::Column::Id.eq(id))
            .exec(db)
            .await?;
        Ok(())
    }
    pub async fn update_aggregate_api_test_result(
        db: &impl ConnectionTrait,
        id: &str,
        ok: bool,
        status_code: Option<i64>,
        error: Option<&str>,
    ) -> Result<(), DbErr> {
        let now = now_ts();
        let error = if !ok && status_code.is_some() {
            status_code.map(|v| format!("http_status={v}"))
        } else {
            error.map(str::to_owned)
        };
        providers::Entity::update_many()
            .col_expr(providers::Column::LastTestAt, Expr::value(now))
            .col_expr(
                providers::Column::LastTestStatus,
                Expr::value(if ok { "success" } else { "failed" }),
            )
            .col_expr(providers::Column::LastTestError, Expr::value(error))
            .col_expr(providers::Column::UpdatedAt, Expr::value(now))
            .filter(providers::Column::Id.eq(id))
            .exec(db)
            .await?;
        Ok(())
    }
    pub async fn list_aggregate_api_supplier_models(
        db: &impl ConnectionTrait,
        supplier: Option<&str>,
        provider: Option<&str>,
    ) -> Result<Vec<AggregateApiSupplierModel>, DbErr> {
        let mut q = suppliers::Entity::find();
        if let Some(v) = supplier.map(str::trim).filter(|v| !v.is_empty()) {
            q = q.filter(suppliers::Column::SupplierKey.eq(v));
        }
        if let Some(v) = provider.map(str::trim).filter(|v| !v.is_empty()) {
            q = q.filter(suppliers::Column::ProviderType.eq(v));
        }
        Ok(q.order_by_asc(suppliers::Column::SupplierKey)
            .order_by_asc(suppliers::Column::ProviderType)
            .order_by_asc(suppliers::Column::UpstreamModel)
            .all(db)
            .await?
            .into_iter()
            .map(Into::into)
            .collect())
    }
    pub async fn upsert_aggregate_api_supplier_model(
        db: &impl ConnectionTrait,
        model: &AggregateApiSupplierModel,
    ) -> Result<(), DbErr> {
        let row: suppliers::ActiveModel = model.clone().into();
        suppliers::Entity::insert(row)
            .on_conflict(
                sea_orm::sea_query::OnConflict::columns([
                    suppliers::Column::SupplierKey,
                    suppliers::Column::ProviderType,
                    suppliers::Column::UpstreamModel,
                ])
                .update_columns([
                    suppliers::Column::DisplayName,
                    suppliers::Column::Status,
                    suppliers::Column::UpdatedAt,
                ])
                .to_owned(),
            )
            .exec(db)
            .await?;
        Ok(())
    }
    pub async fn delete_aggregate_api_supplier_model(
        db: &impl ConnectionTrait,
        supplier: &str,
        provider: &str,
        upstream: &str,
    ) -> Result<(), DbErr> {
        suppliers::Entity::delete_by_id((
            supplier.trim().to_owned(),
            provider.trim().to_owned(),
            upstream.trim().to_owned(),
        ))
        .exec(db)
        .await?;
        Ok(())
    }
}
