//! Proxy test histories with identical domain normalization.
use crate::AccountsRepository;
use codexmanager_core::storage::*;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DbErr, EntityTrait, QueryFilter, QueryOrder,
    QuerySelect, Set,
};
fn optional(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToOwned::to_owned)
}
fn status(value: &str) -> String {
    if value.trim().is_empty() {
        "failed".into()
    } else {
        value.trim().into()
    }
}
pub(crate) mod proxy_profile_url_test {
    use sea_orm::entity::prelude::*;
    #[derive(Clone, Debug, DeriveEntityModel)]
    #[sea_orm(table_name = "proxy_profile_url_tests")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        #[sea_orm(column_type = "Text")]
        pub proxy_profile_id: String,
        #[sea_orm(column_type = "Text")]
        pub status: String,
        pub url_latency_ms: Option<i64>,
        pub status_code: Option<i64>,
        #[sea_orm(column_type = "Text")]
        pub test_url: String,
        #[sea_orm(column_type = "Text", nullable)]
        pub final_url: Option<String>,
        pub redirected: bool,
        pub tested_at: i64,
        #[sea_orm(column_type = "Text", nullable)]
        pub error_code: Option<String>,
        #[sea_orm(column_type = "Text", nullable)]
        pub error: Option<String>,
    }
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}
    impl ActiveModelBehavior for ActiveModel {}
}
impl From<proxy_profile_url_test::Model> for ProxyProfileUrlTest {
    fn from(r: proxy_profile_url_test::Model) -> Self {
        Self {
            id: r.id,
            proxy_profile_id: r.proxy_profile_id,
            status: r.status,
            url_latency_ms: r.url_latency_ms,
            status_code: r.status_code,
            test_url: r.test_url,
            final_url: r.final_url,
            redirected: r.redirected,
            tested_at: r.tested_at,
            error_code: r.error_code,
            error: r.error,
        }
    }
}
impl AccountsRepository {
    pub async fn insert_proxy_profile_url_test(
        db: &impl ConnectionTrait,
        input: &ProxyProfileUrlTestInsertInput,
    ) -> Result<ProxyProfileUrlTest, DbErr> {
        let row = proxy_profile_url_test::ActiveModel {
            proxy_profile_id: Set(input.proxy_profile_id.trim().to_owned()),
            status: Set(status(&input.status)),
            url_latency_ms: Set(input.url_latency_ms),
            status_code: Set(input.status_code),
            test_url: Set(input.test_url.trim().to_owned()),
            final_url: Set(optional(input.final_url.as_deref())),
            redirected: Set(input.redirected),
            tested_at: Set(input.tested_at),
            error_code: Set(optional(input.error_code.as_deref())),
            error: Set(optional(input.error.as_deref())),
            ..Default::default()
        }
        .insert(db)
        .await?;
        Ok(row.into())
    }
    pub async fn find_proxy_profile_url_test(
        db: &impl ConnectionTrait,
        id: i64,
    ) -> Result<Option<ProxyProfileUrlTest>, DbErr> {
        Ok(proxy_profile_url_test::Entity::find_by_id(id)
            .one(db)
            .await?
            .map(Into::into))
    }
    pub async fn list_proxy_profile_url_tests(
        db: &impl ConnectionTrait,
        value: &str,
        limit: usize,
    ) -> Result<Vec<ProxyProfileUrlTest>, DbErr> {
        Ok(proxy_profile_url_test::Entity::find()
            .filter(proxy_profile_url_test::Column::ProxyProfileId.eq(value.trim()))
            .order_by_desc(proxy_profile_url_test::Column::TestedAt)
            .order_by_desc(proxy_profile_url_test::Column::Id)
            .limit(limit.max(1).min(i64::MAX as usize) as u64)
            .all(db)
            .await?
            .into_iter()
            .map(Into::into)
            .collect())
    }
    pub async fn delete_proxy_profile_url_tests_by_profile(
        db: &impl ConnectionTrait,
        value: &str,
    ) -> Result<usize, DbErr> {
        Ok(proxy_profile_url_test::Entity::delete_many()
            .filter(proxy_profile_url_test::Column::ProxyProfileId.eq(value.trim()))
            .exec(db)
            .await?
            .rows_affected as usize)
    }
}
pub(crate) mod proxy_speed_test {
    use sea_orm::entity::prelude::*;
    #[derive(Clone, Debug, DeriveEntityModel)]
    #[sea_orm(table_name = "proxy_speed_tests")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        #[sea_orm(column_type = "Text")]
        pub scope: String,
        #[sea_orm(column_type = "Text", nullable)]
        pub proxy_profile_id: Option<String>,
        #[sea_orm(column_type = "Text", nullable)]
        pub account_id: Option<String>,
        #[sea_orm(column_type = "Text")]
        pub status: String,
        #[sea_orm(column_type = "Text")]
        pub provider: String,
        #[sea_orm(column_type = "Text", nullable)]
        pub observed_ip: Option<String>,
        #[sea_orm(column_type = "Text", nullable)]
        pub observed_country: Option<String>,
        #[sea_orm(column_type = "Text", nullable)]
        pub observed_colo: Option<String>,
        pub max_payload_bytes: Option<i64>,
        #[sea_orm(column_type = "Text", nullable)]
        pub samples_json: Option<String>,
        #[sea_orm(column_type = "Text", nullable)]
        pub download_summary_json: Option<String>,
        #[sea_orm(column_type = "Text", nullable)]
        pub upload_summary_json: Option<String>,
        pub started_at: i64,
        pub finished_at: i64,
        #[sea_orm(column_type = "Text", nullable)]
        pub error_code: Option<String>,
        #[sea_orm(column_type = "Text", nullable)]
        pub error: Option<String>,
    }
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}
    impl ActiveModelBehavior for ActiveModel {}
}
impl From<proxy_speed_test::Model> for ProxySpeedTest {
    fn from(r: proxy_speed_test::Model) -> Self {
        Self {
            id: r.id,
            scope: r.scope,
            proxy_profile_id: r.proxy_profile_id,
            account_id: r.account_id,
            status: r.status,
            provider: r.provider,
            observed_ip: r.observed_ip,
            observed_country: r.observed_country,
            observed_colo: r.observed_colo,
            max_payload_bytes: r.max_payload_bytes,
            samples_json: r.samples_json,
            download_summary_json: r.download_summary_json,
            upload_summary_json: r.upload_summary_json,
            started_at: r.started_at,
            finished_at: r.finished_at,
            error_code: r.error_code,
            error: r.error,
        }
    }
}
impl AccountsRepository {
    pub async fn insert_proxy_speed_test(
        db: &impl ConnectionTrait,
        input: &ProxySpeedTestInsertInput,
    ) -> Result<ProxySpeedTest, DbErr> {
        let row = proxy_speed_test::ActiveModel {
            scope: Set(input.scope.trim().to_owned()),
            proxy_profile_id: Set(optional(input.proxy_profile_id.as_deref())),
            account_id: Set(optional(input.account_id.as_deref())),
            status: Set(input.status.trim().to_owned()),
            provider: Set(input.provider.trim().to_owned()),
            observed_ip: Set(optional(input.observed_ip.as_deref())),
            observed_country: Set(optional(input.observed_country.as_deref())),
            observed_colo: Set(optional(input.observed_colo.as_deref())),
            max_payload_bytes: Set(input.max_payload_bytes),
            samples_json: Set(optional(input.samples_json.as_deref())),
            download_summary_json: Set(optional(input.download_summary_json.as_deref())),
            upload_summary_json: Set(optional(input.upload_summary_json.as_deref())),
            started_at: Set(input.started_at),
            finished_at: Set(input.finished_at),
            error_code: Set(optional(input.error_code.as_deref())),
            error: Set(optional(input.error.as_deref())),
            ..Default::default()
        }
        .insert(db)
        .await?;
        Ok(row.into())
    }
    pub async fn find_proxy_speed_test(
        db: &impl ConnectionTrait,
        id: i64,
    ) -> Result<Option<ProxySpeedTest>, DbErr> {
        Ok(proxy_speed_test::Entity::find_by_id(id)
            .one(db)
            .await?
            .map(Into::into))
    }
    pub async fn list_proxy_speed_tests_by_profile(
        db: &impl ConnectionTrait,
        value: &str,
        limit: usize,
    ) -> Result<Vec<ProxySpeedTest>, DbErr> {
        Ok(proxy_speed_test::Entity::find()
            .filter(proxy_speed_test::Column::ProxyProfileId.eq(value.trim()))
            .order_by_desc(proxy_speed_test::Column::StartedAt)
            .order_by_desc(proxy_speed_test::Column::Id)
            .limit(limit.max(1).min(i64::MAX as usize) as u64)
            .all(db)
            .await?
            .into_iter()
            .map(Into::into)
            .collect())
    }
    pub async fn list_proxy_speed_tests_by_account(
        db: &impl ConnectionTrait,
        value: &str,
        limit: usize,
    ) -> Result<Vec<ProxySpeedTest>, DbErr> {
        Ok(proxy_speed_test::Entity::find()
            .filter(proxy_speed_test::Column::AccountId.eq(value.trim()))
            .order_by_desc(proxy_speed_test::Column::StartedAt)
            .order_by_desc(proxy_speed_test::Column::Id)
            .limit(limit.max(1).min(i64::MAX as usize) as u64)
            .all(db)
            .await?
            .into_iter()
            .map(Into::into)
            .collect())
    }
    pub async fn delete_proxy_speed_tests_by_profile(
        db: &impl ConnectionTrait,
        value: &str,
    ) -> Result<usize, DbErr> {
        Ok(proxy_speed_test::Entity::delete_many()
            .filter(proxy_speed_test::Column::ProxyProfileId.eq(value.trim()))
            .exec(db)
            .await?
            .rows_affected as usize)
    }
}
pub(crate) mod proxy_diagnostic_test {
    use sea_orm::entity::prelude::*;
    #[derive(Clone, Debug, DeriveEntityModel)]
    #[sea_orm(table_name = "proxy_diagnostics_history")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        #[sea_orm(column_type = "Text")]
        pub scope: String,
        #[sea_orm(column_type = "Text", nullable)]
        pub proxy_profile_id: Option<String>,
        #[sea_orm(column_type = "Text", nullable)]
        pub account_id: Option<String>,
        #[sea_orm(column_type = "Text")]
        pub status: String,
        #[sea_orm(column_type = "Text")]
        pub provider: String,
        #[sea_orm(column_type = "Text")]
        pub file_size_id: String,
        pub downloaded_bytes: Option<i64>,
        pub duration_ms: Option<i64>,
        pub mbps: Option<f64>,
        pub tested_at: i64,
        #[sea_orm(column_type = "Text", nullable)]
        pub error: Option<String>,
    }
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}
    impl ActiveModelBehavior for ActiveModel {}
}
impl From<proxy_diagnostic_test::Model> for ProxyDiagnosticTest {
    fn from(r: proxy_diagnostic_test::Model) -> Self {
        Self {
            id: r.id,
            scope: r.scope,
            proxy_profile_id: r.proxy_profile_id,
            account_id: r.account_id,
            status: r.status,
            provider: r.provider,
            file_size_id: r.file_size_id,
            downloaded_bytes: r.downloaded_bytes,
            duration_ms: r.duration_ms,
            mbps: r.mbps,
            tested_at: r.tested_at,
            error: r.error,
        }
    }
}
impl AccountsRepository {
    pub async fn insert_proxy_diagnostic_test(
        db: &impl ConnectionTrait,
        input: &ProxyDiagnosticTestInsertInput,
    ) -> Result<ProxyDiagnosticTest, DbErr> {
        let row = proxy_diagnostic_test::ActiveModel {
            scope: Set(input.scope.trim().to_owned()),
            proxy_profile_id: Set(optional(input.proxy_profile_id.as_deref())),
            account_id: Set(optional(input.account_id.as_deref())),
            status: Set(input.status.trim().to_owned()),
            provider: Set(input.provider.trim().to_owned()),
            file_size_id: Set(input.file_size_id.trim().to_owned()),
            downloaded_bytes: Set(input.downloaded_bytes),
            duration_ms: Set(input.duration_ms),
            mbps: Set(input.mbps),
            tested_at: Set(input.tested_at),
            error: Set(optional(input.error.as_deref())),
            ..Default::default()
        }
        .insert(db)
        .await?;
        Ok(row.into())
    }
    pub async fn find_proxy_diagnostic_test(
        db: &impl ConnectionTrait,
        id: i64,
    ) -> Result<Option<ProxyDiagnosticTest>, DbErr> {
        Ok(proxy_diagnostic_test::Entity::find_by_id(id)
            .one(db)
            .await?
            .map(Into::into))
    }
    pub async fn list_proxy_diagnostic_tests_by_profile(
        db: &impl ConnectionTrait,
        value: &str,
        limit: usize,
    ) -> Result<Vec<ProxyDiagnosticTest>, DbErr> {
        Ok(proxy_diagnostic_test::Entity::find()
            .filter(proxy_diagnostic_test::Column::ProxyProfileId.eq(value.trim()))
            .order_by_desc(proxy_diagnostic_test::Column::TestedAt)
            .order_by_desc(proxy_diagnostic_test::Column::Id)
            .limit(limit.max(1).min(i64::MAX as usize) as u64)
            .all(db)
            .await?
            .into_iter()
            .map(Into::into)
            .collect())
    }
    pub async fn list_proxy_diagnostic_tests_by_account(
        db: &impl ConnectionTrait,
        value: &str,
        limit: usize,
    ) -> Result<Vec<ProxyDiagnosticTest>, DbErr> {
        Ok(proxy_diagnostic_test::Entity::find()
            .filter(proxy_diagnostic_test::Column::AccountId.eq(value.trim()))
            .order_by_desc(proxy_diagnostic_test::Column::TestedAt)
            .order_by_desc(proxy_diagnostic_test::Column::Id)
            .limit(limit.max(1).min(i64::MAX as usize) as u64)
            .all(db)
            .await?
            .into_iter()
            .map(Into::into)
            .collect())
    }
    pub async fn delete_proxy_diagnostic_tests_by_profile(
        db: &impl ConnectionTrait,
        value: &str,
    ) -> Result<usize, DbErr> {
        Ok(proxy_diagnostic_test::Entity::delete_many()
            .filter(proxy_diagnostic_test::Column::ProxyProfileId.eq(value.trim()))
            .exec(db)
            .await?
            .rows_affected as usize)
    }
}
pub(crate) mod account_proxy_url_test {
    use sea_orm::entity::prelude::*;
    #[derive(Clone, Debug, DeriveEntityModel)]
    #[sea_orm(table_name = "account_proxy_url_tests")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        #[sea_orm(column_type = "Text")]
        pub account_id: String,
        #[sea_orm(column_type = "Text")]
        pub status: String,
        pub url_latency_ms: Option<i64>,
        pub status_code: Option<i64>,
        #[sea_orm(column_type = "Text")]
        pub test_url: String,
        #[sea_orm(column_type = "Text", nullable)]
        pub final_url: Option<String>,
        pub redirected: bool,
        pub tested_at: i64,
        #[sea_orm(column_type = "Text", nullable)]
        pub error_code: Option<String>,
        #[sea_orm(column_type = "Text", nullable)]
        pub error: Option<String>,
    }
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}
    impl ActiveModelBehavior for ActiveModel {}
}
impl From<account_proxy_url_test::Model> for AccountProxyUrlTest {
    fn from(r: account_proxy_url_test::Model) -> Self {
        Self {
            id: r.id,
            account_id: r.account_id,
            status: r.status,
            url_latency_ms: r.url_latency_ms,
            status_code: r.status_code,
            test_url: r.test_url,
            final_url: r.final_url,
            redirected: r.redirected,
            tested_at: r.tested_at,
            error_code: r.error_code,
            error: r.error,
        }
    }
}
impl AccountsRepository {
    pub async fn insert_account_proxy_url_test(
        db: &impl ConnectionTrait,
        input: &AccountProxyUrlTestInsertInput,
    ) -> Result<AccountProxyUrlTest, DbErr> {
        let row = account_proxy_url_test::ActiveModel {
            account_id: Set(input.account_id.trim().to_owned()),
            status: Set(status(&input.status)),
            url_latency_ms: Set(input.url_latency_ms),
            status_code: Set(input.status_code),
            test_url: Set(input.test_url.trim().to_owned()),
            final_url: Set(optional(input.final_url.as_deref())),
            redirected: Set(input.redirected),
            tested_at: Set(input.tested_at),
            error_code: Set(optional(input.error_code.as_deref())),
            error: Set(optional(input.error.as_deref())),
            ..Default::default()
        }
        .insert(db)
        .await?;
        Ok(row.into())
    }
    pub async fn find_account_proxy_url_test(
        db: &impl ConnectionTrait,
        id: i64,
    ) -> Result<Option<AccountProxyUrlTest>, DbErr> {
        Ok(account_proxy_url_test::Entity::find_by_id(id)
            .one(db)
            .await?
            .map(Into::into))
    }
    pub async fn list_account_proxy_url_tests(
        db: &impl ConnectionTrait,
        value: &str,
        limit: usize,
    ) -> Result<Vec<AccountProxyUrlTest>, DbErr> {
        Ok(account_proxy_url_test::Entity::find()
            .filter(account_proxy_url_test::Column::AccountId.eq(value.trim()))
            .order_by_desc(account_proxy_url_test::Column::TestedAt)
            .order_by_desc(account_proxy_url_test::Column::Id)
            .limit(limit.max(1).min(i64::MAX as usize) as u64)
            .all(db)
            .await?
            .into_iter()
            .map(Into::into)
            .collect())
    }
}
impl AccountsRepository {
    pub async fn find_cached_proxy_flag_by_country(
        db: &impl ConnectionTrait,
        value: &str,
    ) -> Result<Option<String>, DbErr> {
        use crate::account_details::proxy_settings as p;
        Ok(p::Entity::find()
            .filter(p::Column::CountryCode.eq(value))
            .filter(p::Column::FlagImgUrl.like("data:image/%"))
            .one(db)
            .await?
            .and_then(|r| r.flag_img_url))
    }
}

impl AccountsRepository {
    pub async fn delete_account_related_history(
        db: &impl ConnectionTrait,
        id: &str,
    ) -> Result<(), DbErr> {
        account_proxy_url_test::Entity::delete_many()
            .filter(account_proxy_url_test::Column::AccountId.eq(id))
            .exec(db)
            .await?;
        proxy_speed_test::Entity::delete_many()
            .filter(proxy_speed_test::Column::AccountId.eq(id))
            .exec(db)
            .await?;
        proxy_diagnostic_test::Entity::delete_many()
            .filter(proxy_diagnostic_test::Column::AccountId.eq(id))
            .exec(db)
            .await?;
        Ok(())
    }
}
