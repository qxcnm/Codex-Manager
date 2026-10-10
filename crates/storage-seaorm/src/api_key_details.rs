//! Authoritative API-key secrets, quotas and atomic profile mutations.
use crate::{ApiKeyRecord, ApiKeysRepository, UsersRepository};
use sea_orm::{entity::prelude::*, ActiveModelTrait, Set, TransactionTrait};

pub(crate) mod secrets {
    use sea_orm::entity::prelude::*;
    #[derive(Clone, Debug, DeriveEntityModel)]
    #[sea_orm(table_name = "api_key_secrets")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub key_id: String,
        #[sea_orm(column_type = "Text")]
        pub key_value: String,
        pub created_at: i64,
        pub updated_at: i64,
    }
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}
    impl ActiveModelBehavior for ActiveModel {}
}

pub(crate) mod quotas {
    use sea_orm::entity::prelude::*;
    #[derive(Clone, Debug, DeriveEntityModel)]
    #[sea_orm(table_name = "api_key_quota_limits")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub key_id: String,
        pub quota_limit_tokens: i64,
        pub created_at: i64,
        pub updated_at: i64,
    }
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}
    impl ActiveModelBehavior for ActiveModel {}
}

pub struct ApiKeyDetailsRepository;

#[cfg(test)]
#[path = "api_key_details_tests.rs"]
pub(crate) mod tests;

impl ApiKeyDetailsRepository {
    pub async fn usage_by_key_model(
        db: &impl ConnectionTrait,
        start: Option<i64>,
        end: Option<i64>,
        keys: Option<&[String]>,
    ) -> Result<Vec<codexmanager_core::storage::ApiKeyModelTokenUsageSummary>, DbErr> {
        use sea_orm::{DatabaseBackend, Statement};
        if keys.is_some_and(|keys| keys.is_empty()) {
            return Ok(Vec::new());
        }
        let backend = db.get_database_backend();
        let mut values: Vec<sea_orm::Value> = Vec::new();
        let mut sources = Vec::new();
        for (table, clock, included) in [
            (
                "request_token_stats",
                "created_at",
                " AND usage_included = TRUE",
            ),
            ("request_token_stat_hourly_rollups", "bucket_start", ""),
            ("request_token_stat_rollups", "", ""),
        ] {
            if clock.is_empty() && (start.is_some() || end.is_some()) {
                continue;
            }
            let mut query = format!("SELECT key_id, COALESCE(NULLIF(TRIM(model), ''), 'unknown') AS model, input_tokens, cached_input_tokens, output_tokens, reasoning_output_tokens, total_tokens, estimated_cost_usd FROM {table} WHERE TRIM(COALESCE(key_id, '')) <> ''{included}");
            let mut parameter = |value: sea_orm::Value| {
                values.push(value);
                if backend == DatabaseBackend::Postgres {
                    format!("${}", values.len())
                } else {
                    "?".to_string()
                }
            };
            if let Some(keys) = keys {
                query.push_str(&format!(
                    " AND key_id IN ({})",
                    keys.iter()
                        .map(|key| parameter(key.as_str().into()))
                        .collect::<Vec<_>>()
                        .join(",")
                ));
            }
            if let Some(start) = start {
                query.push_str(&format!(" AND {clock} >= {}", parameter(start.into())));
            }
            if let Some(end) = end {
                query.push_str(&format!(" AND {clock} < {}", parameter(end.into())));
            }
            sources.push(query);
        }
        let integer_type = match backend {
            DatabaseBackend::MySql => "SIGNED",
            DatabaseBackend::Postgres => "BIGINT",
            _ => "INTEGER",
        };
        let derived_total = "COALESCE(input_tokens,0) - COALESCE(cached_input_tokens,0) + COALESCE(output_tokens,0)";
        let token_value = format!("CASE WHEN total_tokens IS NOT NULL THEN CASE WHEN total_tokens > 0 THEN total_tokens ELSE 0 END ELSE CASE WHEN {derived_total} > 0 THEN {derived_total} ELSE 0 END END");
        let sums = [
            "input_tokens",
            "cached_input_tokens",
            "output_tokens",
            "reasoning_output_tokens",
        ]
        .into_iter()
        .map(|name| {
            format!("CAST(COALESCE(SUM(COALESCE({name},0)),0) AS {integer_type}) AS {name}")
        })
        .collect::<Vec<_>>()
        .join(",");
        let sql = format!("SELECT key_id,model,{sums},CAST(COALESCE(SUM({token_value}),0) AS {integer_type}) AS total_tokens,COALESCE(SUM(COALESCE(estimated_cost_usd,0.0)),0.0) AS estimated_cost_usd FROM ({}) selected_stats GROUP BY key_id,model ORDER BY key_id,model", sources.join(" UNION ALL "));
        db.query_all(Statement::from_sql_and_values(backend, sql, values))
            .await?
            .into_iter()
            .map(|row| {
                Ok(codexmanager_core::storage::ApiKeyModelTokenUsageSummary {
                    key_id: row.try_get("", "key_id")?,
                    model: row.try_get("", "model")?,
                    input_tokens: row.try_get("", "input_tokens")?,
                    cached_input_tokens: row.try_get("", "cached_input_tokens")?,
                    output_tokens: row.try_get("", "output_tokens")?,
                    reasoning_output_tokens: row.try_get("", "reasoning_output_tokens")?,
                    total_tokens: row.try_get("", "total_tokens")?,
                    estimated_cost_usd: row.try_get("", "estimated_cost_usd")?,
                })
            })
            .collect()
    }

    pub async fn token_usage(db: &impl ConnectionTrait, id: &str) -> Result<i64, DbErr> {
        Ok(Self::usage(db, Some(id), None, None)
            .await?
            .into_iter()
            .next()
            .map(|row| row.total_tokens)
            .unwrap_or(0))
    }

    pub async fn usage_by_key(
        db: &impl ConnectionTrait,
        start: Option<i64>,
        end: Option<i64>,
    ) -> Result<Vec<codexmanager_core::storage::ApiKeyTokenUsageSummary>, DbErr> {
        Self::usage(db, None, start, end).await
    }

    async fn usage(
        db: &impl ConnectionTrait,
        id: Option<&str>,
        start: Option<i64>,
        end: Option<i64>,
    ) -> Result<Vec<codexmanager_core::storage::ApiKeyTokenUsageSummary>, DbErr> {
        use sea_orm::{DatabaseBackend, Statement};
        let backend = db.get_database_backend();
        let mut values: Vec<sea_orm::Value> = Vec::new();
        let mut sources = Vec::new();
        for (table, clock, included) in [
            (
                "request_token_stats",
                "created_at",
                " AND usage_included = TRUE",
            ),
            ("request_token_stat_hourly_rollups", "bucket_start", ""),
            ("request_token_stat_rollups", "", ""),
        ] {
            if clock.is_empty() && (start.is_some() || end.is_some()) {
                continue;
            }
            let mut query = format!("SELECT key_id, input_tokens, cached_input_tokens, output_tokens, total_tokens, estimated_cost_usd FROM {table} WHERE TRIM(COALESCE(key_id, '')) <> ''{included}");
            let mut parameter = |value: sea_orm::Value| {
                values.push(value);
                if backend == DatabaseBackend::Postgres {
                    format!("${}", values.len())
                } else {
                    "?".to_string()
                }
            };
            if let Some(id) = id {
                query.push_str(&format!(" AND key_id = {}", parameter(id.into())));
            }
            if let Some(start) = start {
                query.push_str(&format!(" AND {clock} >= {}", parameter(start.into())));
            }
            if let Some(end) = end {
                query.push_str(&format!(" AND {clock} < {}", parameter(end.into())));
            }
            sources.push(query);
        }
        let integer_type = match backend {
            DatabaseBackend::MySql => "SIGNED",
            DatabaseBackend::Postgres => "BIGINT",
            _ => "INTEGER",
        };
        let derived_total = "COALESCE(input_tokens,0) - COALESCE(cached_input_tokens,0) + COALESCE(output_tokens,0)";
        let token_value = format!("CASE WHEN total_tokens IS NOT NULL THEN CASE WHEN total_tokens > 0 THEN total_tokens ELSE 0 END ELSE CASE WHEN {derived_total} > 0 THEN {derived_total} ELSE 0 END END");
        let sql = format!("SELECT key_id, CAST(COALESCE(SUM({token_value}),0) AS {integer_type}) AS total_tokens, COALESCE(SUM(COALESCE(estimated_cost_usd,0.0)),0.0) AS estimated_cost_usd FROM ({}) selected_stats GROUP BY key_id ORDER BY key_id", sources.join(" UNION ALL "));
        db.query_all(Statement::from_sql_and_values(backend, sql, values))
            .await?
            .into_iter()
            .map(|row| {
                Ok(codexmanager_core::storage::ApiKeyTokenUsageSummary {
                    key_id: row.try_get("", "key_id")?,
                    total_tokens: row.try_get("", "total_tokens")?,
                    estimated_cost_usd: row.try_get("", "estimated_cost_usd")?,
                })
            })
            .collect()
    }

    pub async fn secret(db: &impl ConnectionTrait, id: &str) -> Result<Option<String>, DbErr> {
        Ok(secrets::Entity::find_by_id(id)
            .one(db)
            .await?
            .map(|row| row.key_value))
    }

    pub async fn quota(db: &impl ConnectionTrait, id: &str) -> Result<Option<i64>, DbErr> {
        Ok(quotas::Entity::find_by_id(id)
            .one(db)
            .await?
            .map(|row| row.quota_limit_tokens)
            .filter(|limit| *limit > 0))
    }

    async fn set_quota(
        db: &impl ConnectionTrait,
        id: &str,
        limit: Option<i64>,
    ) -> Result<(), DbErr> {
        match limit.filter(|limit| *limit > 0) {
            Some(limit) => {
                let current = quotas::Entity::find_by_id(id).one(db).await?;
                let now = codexmanager_core::storage::now_ts();
                let model = quotas::ActiveModel {
                    key_id: Set(id.to_owned()),
                    quota_limit_tokens: Set(limit),
                    created_at: Set(current.as_ref().map(|row| row.created_at).unwrap_or(now)),
                    updated_at: Set(now),
                };
                if current.is_some() {
                    model.update(db).await?;
                } else {
                    model.insert(db).await?;
                }
            }
            None => {
                quotas::Entity::delete_by_id(id).exec(db).await?;
            }
        }
        Ok(())
    }

    pub async fn create(
        db: &DatabaseConnection,
        key: ApiKeyRecord,
        secret: String,
        quota: Option<i64>,
    ) -> Result<(), DbErr> {
        Self::create_with_owner(db, key, secret, quota, None, None).await
    }

    pub async fn create_with_owner(
        db: &DatabaseConnection,
        mut key: ApiKeyRecord,
        secret: String,
        quota: Option<i64>,
        account_group_filter: Option<String>,
        owner_user_id: Option<String>,
    ) -> Result<(), DbErr> {
        let tx = db.begin().await?;
        UsersRepository::lock(&tx, "api_keys").await?;
        if ApiKeysRepository::find_by_hash(&tx, &key.key_hash)
            .await?
            .is_some()
            || ApiKeysRepository::get(&tx, &key.id).await?.is_some()
        {
            return Err(DbErr::Custom("custom api key already exists".into()));
        }
        key.account_group_filter = account_group_filter;
        let id = key.id.clone();
        let created_at = key.created_at;
        ApiKeysRepository::upsert(&tx, key).await?;
        secrets::ActiveModel {
            key_id: Set(id.clone()),
            key_value: Set(secret),
            created_at: Set(created_at),
            updated_at: Set(created_at),
        }
        .insert(&tx)
        .await?;
        Self::set_quota(&tx, &id, quota).await?;
        if let Some(user_id) = owner_user_id {
            let user = UsersRepository::get(&tx, &user_id)
                .await?
                .ok_or_else(|| DbErr::Custom("用户不存在".into()))?;
            if user.role == "admin" || user.status != "active" {
                return Err(DbErr::Custom(
                    "API key owner must be an active member".into(),
                ));
            }
            crate::BillingRepository::ensure_wallet(
                &tx,
                &format!("wlt_domain_user_{user_id}"),
                "user",
                &user_id,
            )
            .await?;
            UsersRepository::put_owner(
                &tx,
                codexmanager_core::storage::ApiKeyOwner {
                    key_id: id.clone(),
                    owner_kind: "user".into(),
                    owner_user_id: Some(user_id),
                    project_id: None,
                    updated_at: codexmanager_core::storage::now_ts(),
                },
            )
            .await?;
        }
        tx.commit().await
    }

    /// Mutate a current profile while holding the same cross-process lock used
    /// by key creation/deletion, avoiding lost administrative routing updates.
    pub async fn update<F>(
        db: &DatabaseConnection,
        id: &str,
        quota: Option<Option<i64>>,
        update: F,
    ) -> Result<(), DbErr>
    where
        F: FnOnce(&mut ApiKeyRecord) -> Result<(), String>,
    {
        Self::update_with_owner(db, id, quota, None, update).await
    }

    /// Apply a key patch while checking a member owner under the same
    /// transaction/lock as the row update. The caller may still perform an
    /// early authorization check for response semantics, but this check is
    /// the concurrency boundary that prevents an owner change between the
    /// read and write from bypassing authorization.
    pub async fn update_with_owner<F>(
        db: &DatabaseConnection,
        id: &str,
        quota: Option<Option<i64>>,
        owner_user_id: Option<&str>,
        update: F,
    ) -> Result<(), DbErr>
    where
        F: FnOnce(&mut ApiKeyRecord) -> Result<(), String>,
    {
        let tx = db.begin().await?;
        UsersRepository::lock(&tx, "api_keys").await?;
        let mut key = ApiKeysRepository::get(&tx, id)
            .await?
            .ok_or_else(|| DbErr::Custom("api key not found".into()))?;
        if let Some(user_id) = owner_user_id {
            let owner = UsersRepository::owner(&tx, id).await?;
            if !owner.is_some_and(|owner| {
                owner.owner_kind == "user" && owner.owner_user_id.as_deref() == Some(user_id)
            }) {
                return Err(DbErr::Custom("permission_denied: apikey".into()));
            }
        }
        update(&mut key).map_err(DbErr::Custom)?;
        ApiKeysRepository::upsert(&tx, key).await?;
        if let Some(quota) = quota {
            Self::set_quota(&tx, id, quota).await?;
        }
        tx.commit().await
    }

    pub async fn delete(db: &DatabaseConnection, id: &str) -> Result<(), DbErr> {
        let tx = db.begin().await?;
        UsersRepository::lock(&tx, "api_keys").await?;
        secrets::Entity::delete_by_id(id).exec(&tx).await?;
        quotas::Entity::delete_by_id(id).exec(&tx).await?;
        crate::users::owners::Entity::delete_by_id(id)
            .exec(&tx)
            .await?;
        ApiKeysRepository::delete(&tx, id).await?;
        tx.commit().await
    }
}
