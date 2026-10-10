//! Optional Service-mode SeaORM storage adapter.
//!
//! This crate is deliberately independent from the HTTP layer.  Desktop
//! builds continue using `codexmanager-core::storage::Storage` (SQLite), while
//! Service deployments can opt into MySQL or PostgreSQL through Cargo
//! features and a database URL.

use codexmanager_core::storage::{StorageBackendKind, StorageHealth};
use sea_orm::{ConnectOptions, ConnectionTrait, Database, DatabaseConnection, Statement};
use std::time::Duration;

mod account_details;
mod account_tokens;
mod accounts_domain;
mod accounts_proxy;
mod accounts_warmups;
mod plugins;
mod proxy_history;
pub use plugins::PluginsRepository;
mod accounts;
mod api_key_details;
mod api_key_rollups;
mod api_keys;
mod desktop_history;
mod skill_repositories;
pub use skill_repositories::SkillRepositoriesRepository;
mod conversation_bindings;
pub use conversation_bindings::ConversationBindingsRepository;
mod quota_configuration;
pub use quota_configuration::QuotaConfigurationRepository;
mod billing;
mod migration;
mod model_catalog;
mod model_groups;
#[cfg(test)]
mod real_database_tests;
mod request_log_retention;
mod request_logs;
mod request_token_stats;
mod request_usage_summary;
mod reset_credit_operations;
mod usage_analytics;
pub use usage_analytics::UsageAnalyticsRepository;
mod settings;
pub mod transfer;
mod usage_snapshots;
mod users;
pub use account_tokens::{AccountTokenRecord, AccountTokensRepository};
pub use accounts::{AccountRecord, AccountsRepository};
pub use api_key_details::ApiKeyDetailsRepository;
pub use api_keys::{ApiKeyRecord, ApiKeysRepository};
pub use billing::BillingRepository;
pub use model_catalog::{
    CatalogModelRecord, CatalogPriceRecord, CatalogPriceTierRecord, CatalogRouteRecord,
    ManagedModelsRepository, ModelCatalogRepository, ModelPriceTiersRepository,
    ModelPricesRepository, ModelRoutesRepository,
};
pub use model_groups::{
    ModelGroupModelRecord, ModelGroupRecord, ModelGroupsRepository, UserModelGroupRecord,
};
pub use request_logs::{RequestLogFilter, RequestLogRecord, RequestLogsRepository};
pub use request_token_stats::{RequestTokenStatRecord, RequestTokenStatsRepository};
pub use reset_credit_operations::ResetCreditOperationsRepository;
pub use settings::{AppSetting, SettingsRepository};
pub use usage_snapshots::{UsageSnapshotRow, UsageSnapshotsRepository};
pub use users::UsersRepository;

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("unsupported database backend: {0}")]
    UnsupportedBackend(String),
    #[error("database URL is required for {0}")]
    MissingUrl(&'static str),
    #[error("database connection failed")]
    Connection,
    #[error("database migration failed")]
    Migration,
    #[error("database health check failed")]
    HealthCheck,
    #[error("database backend {backend} is not enabled in this build")]
    FeatureDisabled { backend: &'static str },
    #[error("database URL scheme does not match backend {backend}")]
    InvalidUrl { backend: &'static str },
    #[error("database pool configuration is invalid: {0}")]
    InvalidPoolConfig(String),
}

#[derive(Clone, PartialEq, Eq)]
pub struct StorageConfig {
    pub backend: StorageBackendKind,
    pub database_url: String,
    pub max_connections: u32,
    pub acquire_timeout_ms: u64,
}

impl std::fmt::Debug for StorageConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StorageConfig")
            .field("backend", &self.backend)
            .field("database_url", &"[REDACTED]")
            .field("max_connections", &self.max_connections)
            .field("acquire_timeout_ms", &self.acquire_timeout_ms)
            .finish()
    }
}

impl StorageConfig {
    pub fn from_env() -> Result<Self, StorageError> {
        let backend = match std::env::var("CODEXMANAGER_STORAGE_BACKEND")
            .unwrap_or_else(|_| "sqlite".to_owned())
            .to_ascii_lowercase()
            .as_str()
        {
            "sqlite" => StorageBackendKind::Sqlite,
            "mysql" => StorageBackendKind::Mysql,
            "postgres" | "postgresql" => StorageBackendKind::Postgres,
            other => return Err(StorageError::UnsupportedBackend(other.to_owned())),
        };
        let database_url = std::env::var("CODEXMANAGER_DATABASE_URL").unwrap_or_default();
        let max_connections = std::env::var("CODEXMANAGER_DB_MAX_CONNECTIONS")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(10);
        let acquire_timeout_ms = std::env::var("CODEXMANAGER_DB_ACQUIRE_TIMEOUT_MS")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(5_000);
        if max_connections == 0 {
            return Err(StorageError::InvalidPoolConfig(
                "max connections must be greater than zero".into(),
            ));
        }
        Ok(Self {
            backend,
            database_url,
            max_connections,
            acquire_timeout_ms,
        })
    }
}

#[derive(Clone)]
pub struct SeaOrmStorage {
    connection: DatabaseConnection,
    backend: StorageBackendKind,
}

impl SeaOrmStorage {
    pub async fn connect(backend: StorageBackendKind, url: &str) -> Result<Self, StorageError> {
        Self::connect_with_config(StorageConfig {
            backend,
            database_url: url.to_owned(),
            max_connections: 10,
            acquire_timeout_ms: 5_000,
        })
        .await
    }

    pub async fn connect_with_config(config: StorageConfig) -> Result<Self, StorageError> {
        config.validate()?;
        let backend = config.backend;
        let url = config.database_url;
        let url = url.trim();
        let mut options = ConnectOptions::new(url.to_owned());
        options.max_connections(config.max_connections);
        options.acquire_timeout(Duration::from_millis(config.acquire_timeout_ms));
        options.sqlx_logging(false);
        let connection = Database::connect(options)
            .await
            .map_err(|_| StorageError::Connection)?;
        Ok(Self {
            connection,
            backend,
        })
    }

    pub fn connection(&self) -> &DatabaseConnection {
        &self.connection
    }

    /// Migrate only the optional adapter; existing desktop migrations remain untouched.
    pub async fn migrate(&self) -> Result<(), StorageError> {
        migration::migrate(&self.connection)
            .await
            .map_err(|_| StorageError::Migration)
    }

    /// Reconcile the built-in catalog after schema migration for a live service database.
    pub async fn reconcile_builtin_model_catalog(&self) -> Result<(), StorageError> {
        model_catalog::reconcile_builtin_catalog(&self.connection)
            .await
            .map_err(|_| StorageError::Migration)
    }
    pub async fn health_check(&self) -> Result<StorageHealth, StorageError> {
        self.connection
            .execute(Statement::from_string(
                self.connection.get_database_backend(),
                "SELECT 1".to_owned(),
            ))
            .await
            .map_err(|_| StorageError::HealthCheck)?;
        Ok(StorageHealth {
            backend: self.backend,
            schema_ready: true,
        })
    }
}

impl StorageConfig {
    /// Validate deployment configuration before touching the network.
    pub fn validate(&self) -> Result<(), StorageError> {
        let url = self.database_url.trim();
        if url.is_empty() {
            return Err(StorageError::MissingUrl(self.backend.as_str()));
        }
        if self.max_connections == 0 {
            return Err(StorageError::InvalidPoolConfig(
                "max connections must be greater than zero".into(),
            ));
        }
        if self.acquire_timeout_ms == 0 {
            return Err(StorageError::InvalidPoolConfig(
                "acquire timeout must be greater than zero".into(),
            ));
        }
        let valid_scheme = match self.backend {
            StorageBackendKind::Sqlite => url.starts_with("sqlite:") || url == ":memory:",
            StorageBackendKind::Mysql => url.starts_with("mysql://"),
            StorageBackendKind::Postgres => {
                url.starts_with("postgres://") || url.starts_with("postgresql://")
            }
        };
        if !valid_scheme {
            return Err(StorageError::InvalidUrl {
                backend: self.backend.as_str(),
            });
        }
        #[cfg(not(feature = "sqlite"))]
        if self.backend == StorageBackendKind::Sqlite {
            return Err(StorageError::FeatureDisabled { backend: "sqlite" });
        }
        #[cfg(not(feature = "mysql"))]
        if self.backend == StorageBackendKind::Mysql {
            return Err(StorageError::FeatureDisabled { backend: "mysql" });
        }
        #[cfg(not(feature = "postgres"))]
        if self.backend == StorageBackendKind::Postgres {
            return Err(StorageError::FeatureDisabled {
                backend: "postgres",
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_url_is_rejected_without_connecting() {
        let result =
            futures_lite::future::block_on(SeaOrmStorage::connect(StorageBackendKind::Mysql, " "));
        assert!(matches!(result, Err(StorageError::MissingUrl("mysql"))));
    }

    #[test]
    fn configuration_rejects_mismatched_scheme_and_zero_pool() {
        let config = StorageConfig {
            backend: StorageBackendKind::Mysql,
            database_url: "sqlite::memory:".into(),
            max_connections: 1,
            acquire_timeout_ms: 1,
        };
        assert!(matches!(
            config.validate(),
            Err(StorageError::InvalidUrl { .. })
        ));
        let config = StorageConfig {
            backend: StorageBackendKind::Sqlite,
            database_url: "sqlite::memory:".into(),
            max_connections: 0,
            acquire_timeout_ms: 1,
        };
        assert!(matches!(
            config.validate(),
            Err(StorageError::InvalidPoolConfig(_))
        ));
    }

    #[tokio::test]
    async fn sqlite_adapter_migrates_and_reads_settings() {
        let storage = SeaOrmStorage::connect(StorageBackendKind::Sqlite, "sqlite::memory:")
            .await
            .expect("connect");
        storage.migrate().await.expect("migrate");
        storage.migrate().await.expect("repeat migrate");
        SettingsRepository::set(
            storage.connection(),
            AppSetting {
                key: "locale".into(),
                value: "zh-CN".into(),
                updated_at: 1,
            },
        )
        .await
        .expect("write");
        assert_eq!(
            SettingsRepository::get(storage.connection(), "locale")
                .await
                .expect("read")
                .expect("row")
                .value,
            "zh-CN"
        );
        AccountsRepository::upsert(
            storage.connection(),
            AccountRecord {
                id: "migration-account".into(),
                label: "Migration test".into(),
                issuer: "test".into(),
                chatgpt_account_id: None,
                workspace_id: None,
                subject_account_id: None,
                note: None,
                tags: None,
                group_name: None,
                sort: 0,
                status: "active".into(),
                created_at: 1,
                updated_at: 1,
            },
        )
        .await
        .expect("account write");
        assert!(
            AccountsRepository::get(storage.connection(), "migration-account")
                .await
                .expect("account read")
                .is_some()
        );
        ApiKeysRepository::upsert(
            storage.connection(),
            ApiKeyRecord {
                id: "migration-key".into(),
                name: Some("Migration key".into()),
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
                key_hash: "migration-hash".into(),
                status: "active".into(),
                created_at: 1,
                last_used_at: None,
            },
        )
        .await
        .expect("api key write");
        assert!(
            ApiKeysRepository::get(storage.connection(), "migration-key")
                .await
                .expect("api key read")
                .is_some()
        );
        RequestTokenStatsRepository::upsert(
            storage.connection(),
            RequestTokenStatRecord {
                request_log_id: 1,
                key_id: Some("migration-key".into()),
                account_id: None,
                model: Some("gpt-5".into()),
                actual_source_kind: None,
                actual_source_id: None,
                input_tokens: Some(1),
                cached_input_tokens: None,
                output_tokens: Some(1),
                total_tokens: Some(2),
                reasoning_output_tokens: None,
                estimated_cost_usd: Some(0.0),
                usage_included: true,
                created_at: 1,
            },
        )
        .await
        .expect("token stat write");
        assert!(RequestTokenStatsRepository::get(storage.connection(), 1)
            .await
            .expect("token stat read")
            .is_some());
    }

    #[tokio::test]
    #[ignore = "requires a user-provided isolated MySQL test database"]
    async fn mysql_real_database_health_check() {
        real_database_tests::run(StorageBackendKind::Mysql, "CODEXMANAGER_TEST_MYSQL_URL").await;
    }

    #[tokio::test]
    #[ignore = "requires a user-provided isolated PostgreSQL test database"]
    async fn postgres_real_database_health_check() {
        real_database_tests::run(
            StorageBackendKind::Postgres,
            "CODEXMANAGER_TEST_POSTGRES_URL",
        )
        .await;
    }

    #[test]
    fn configuration_debug_and_errors_redact_database_url() {
        let config = StorageConfig {
            backend: StorageBackendKind::Sqlite,
            database_url: "mysql://sensitive-user:secret-password@localhost/private".into(),
            max_connections: 1,
            acquire_timeout_ms: 1,
        };
        let error = config.validate().expect_err("scheme mismatch");
        let diagnostic = format!("{config:?} {error} {error:?}");
        for sensitive in ["sensitive-user", "secret-password", "localhost", "private"] {
            assert!(!diagnostic.contains(sensitive));
        }
    }
}

mod aggregate_apis;
pub use aggregate_apis::AggregateApisRepository;

#[cfg(test)]
mod accounts_domain_tests;

pub type AccountUsageTransaction = sea_orm::DatabaseTransaction;

pub mod domain;
pub use domain::SqliteDomainStorage;
