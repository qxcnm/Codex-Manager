//! Account adjunct tables; entities stay inside the adapter.
pub(crate) mod metadata {
    use sea_orm::entity::prelude::*;
    #[derive(Clone, Debug, DeriveEntityModel)]
    #[sea_orm(table_name = "account_metadata")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub account_id: String,
        #[sea_orm(column_type = "Text", nullable)]
        pub note: Option<String>,
        #[sea_orm(column_type = "Text", nullable)]
        pub tags: Option<String>,
        pub updated_at: i64,
    }
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}
    impl ActiveModelBehavior for ActiveModel {}
}
impl From<metadata::Model> for codexmanager_core::storage::AccountMetadata {
    fn from(row: metadata::Model) -> Self {
        Self {
            account_id: row.account_id,
            note: row.note,
            tags: row.tags,
            updated_at: row.updated_at,
        }
    }
}
impl From<codexmanager_core::storage::AccountMetadata> for metadata::ActiveModel {
    fn from(row: codexmanager_core::storage::AccountMetadata) -> Self {
        Self {
            account_id: sea_orm::Set(row.account_id),
            note: sea_orm::Set(row.note),
            tags: sea_orm::Set(row.tags),
            updated_at: sea_orm::Set(row.updated_at),
        }
    }
}
pub(crate) mod subscriptions {
    use sea_orm::entity::prelude::*;
    #[derive(Clone, Debug, DeriveEntityModel)]
    #[sea_orm(table_name = "account_subscriptions")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub account_id: String,
        pub has_subscription: bool,
        #[sea_orm(column_type = "Text", nullable)]
        pub account_plan_type: Option<String>,
        #[sea_orm(column_type = "Text", nullable)]
        pub plan_type: Option<String>,
        pub expires_at: Option<i64>,
        pub renews_at: Option<i64>,
        pub updated_at: i64,
    }
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}
    impl ActiveModelBehavior for ActiveModel {}
}
impl From<subscriptions::Model> for codexmanager_core::storage::AccountSubscription {
    fn from(row: subscriptions::Model) -> Self {
        Self {
            account_id: row.account_id,
            has_subscription: row.has_subscription,
            account_plan_type: row.account_plan_type,
            plan_type: row.plan_type,
            expires_at: row.expires_at,
            renews_at: row.renews_at,
            updated_at: row.updated_at,
        }
    }
}
impl From<codexmanager_core::storage::AccountSubscription> for subscriptions::ActiveModel {
    fn from(row: codexmanager_core::storage::AccountSubscription) -> Self {
        Self {
            account_id: sea_orm::Set(row.account_id),
            has_subscription: sea_orm::Set(row.has_subscription),
            account_plan_type: sea_orm::Set(row.account_plan_type),
            plan_type: sea_orm::Set(row.plan_type),
            expires_at: sea_orm::Set(row.expires_at),
            renews_at: sea_orm::Set(row.renews_at),
            updated_at: sea_orm::Set(row.updated_at),
        }
    }
}
pub(crate) mod proxy_settings {
    use sea_orm::entity::prelude::*;
    #[derive(Clone, Debug, DeriveEntityModel)]
    #[sea_orm(table_name = "account_proxy_settings")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub account_id: String,
        pub enabled: bool,
        #[sea_orm(column_type = "Text", nullable)]
        pub proxy_source: Option<String>,
        #[sea_orm(column_type = "Text", nullable)]
        pub proxy_profile_id: Option<String>,
        #[sea_orm(column_type = "Text", nullable)]
        pub proxy_url: Option<String>,
        #[sea_orm(column_type = "Text")]
        pub status: String,
        pub latency_ms: Option<i64>,
        pub last_download_mbps: Option<f64>,
        pub last_upload_mbps: Option<f64>,
        pub last_check_at: Option<i64>,
        #[sea_orm(column_type = "Text", nullable)]
        pub last_error: Option<String>,
        #[sea_orm(column_type = "Text", nullable)]
        pub ip: Option<String>,
        #[sea_orm(column_type = "Text", nullable)]
        pub country_code: Option<String>,
        #[sea_orm(column_type = "Text", nullable)]
        pub country_name: Option<String>,
        #[sea_orm(column_type = "Text", nullable)]
        pub region_name: Option<String>,
        #[sea_orm(column_type = "Text", nullable)]
        pub city_name: Option<String>,
        pub geo_checked_at: Option<i64>,
        #[sea_orm(column_type = "Text", nullable)]
        pub geo_error: Option<String>,
        pub asn: Option<i64>,
        #[sea_orm(column_type = "Text", nullable)]
        pub as_org: Option<String>,
        #[sea_orm(column_type = "Text", nullable)]
        pub isp: Option<String>,
        #[sea_orm(column_type = "Text", nullable)]
        pub as_domain: Option<String>,
        #[sea_orm(column_type = "Text", nullable)]
        pub timezone_id: Option<String>,
        pub timezone_offset: Option<i64>,
        #[sea_orm(column_type = "Text", nullable)]
        pub timezone_utc: Option<String>,
        #[sea_orm(column_type = "Text", nullable)]
        pub flag_img_url: Option<String>,
        #[sea_orm(column_type = "Text", nullable)]
        pub flag_emoji: Option<String>,
        pub created_at: i64,
        pub updated_at: i64,
    }
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}
    impl ActiveModelBehavior for ActiveModel {}
}
impl From<proxy_settings::Model> for codexmanager_core::storage::AccountProxySettings {
    fn from(row: proxy_settings::Model) -> Self {
        Self {
            account_id: row.account_id,
            enabled: row.enabled,
            proxy_source: row.proxy_source,
            proxy_profile_id: row.proxy_profile_id,
            proxy_url: row.proxy_url,
            status: row.status,
            latency_ms: row.latency_ms,
            last_download_mbps: row.last_download_mbps,
            last_upload_mbps: row.last_upload_mbps,
            last_check_at: row.last_check_at,
            last_error: row.last_error,
            ip: row.ip,
            country_code: row.country_code,
            country_name: row.country_name,
            region_name: row.region_name,
            city_name: row.city_name,
            geo_checked_at: row.geo_checked_at,
            geo_error: row.geo_error,
            asn: row.asn,
            as_org: row.as_org,
            isp: row.isp,
            as_domain: row.as_domain,
            timezone_id: row.timezone_id,
            timezone_offset: row.timezone_offset,
            timezone_utc: row.timezone_utc,
            flag_img_url: row.flag_img_url,
            flag_emoji: row.flag_emoji,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}
impl From<codexmanager_core::storage::AccountProxySettings> for proxy_settings::ActiveModel {
    fn from(row: codexmanager_core::storage::AccountProxySettings) -> Self {
        Self {
            account_id: sea_orm::Set(row.account_id),
            enabled: sea_orm::Set(row.enabled),
            proxy_source: sea_orm::Set(row.proxy_source),
            proxy_profile_id: sea_orm::Set(row.proxy_profile_id),
            proxy_url: sea_orm::Set(row.proxy_url),
            status: sea_orm::Set(row.status),
            latency_ms: sea_orm::Set(row.latency_ms),
            last_download_mbps: sea_orm::Set(row.last_download_mbps),
            last_upload_mbps: sea_orm::Set(row.last_upload_mbps),
            last_check_at: sea_orm::Set(row.last_check_at),
            last_error: sea_orm::Set(row.last_error),
            ip: sea_orm::Set(row.ip),
            country_code: sea_orm::Set(row.country_code),
            country_name: sea_orm::Set(row.country_name),
            region_name: sea_orm::Set(row.region_name),
            city_name: sea_orm::Set(row.city_name),
            geo_checked_at: sea_orm::Set(row.geo_checked_at),
            geo_error: sea_orm::Set(row.geo_error),
            asn: sea_orm::Set(row.asn),
            as_org: sea_orm::Set(row.as_org),
            isp: sea_orm::Set(row.isp),
            as_domain: sea_orm::Set(row.as_domain),
            timezone_id: sea_orm::Set(row.timezone_id),
            timezone_offset: sea_orm::Set(row.timezone_offset),
            timezone_utc: sea_orm::Set(row.timezone_utc),
            flag_img_url: sea_orm::Set(row.flag_img_url),
            flag_emoji: sea_orm::Set(row.flag_emoji),
            created_at: sea_orm::Set(row.created_at),
            updated_at: sea_orm::Set(row.updated_at),
        }
    }
}
pub(crate) mod proxy_profiles {
    use sea_orm::entity::prelude::*;
    #[derive(Clone, Debug, DeriveEntityModel)]
    #[sea_orm(table_name = "proxy_profiles")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        #[sea_orm(column_type = "Text")]
        pub name: String,
        #[sea_orm(column_type = "Text")]
        pub proxy_url: String,
        #[sea_orm(column_type = "Text")]
        pub proxy_url_redacted: String,
        #[sea_orm(column_type = "Text", nullable)]
        pub scheme: Option<String>,
        #[sea_orm(column_type = "Text", nullable)]
        pub host: Option<String>,
        pub port: Option<i64>,
        pub enabled: bool,
        #[sea_orm(column_type = "Text")]
        pub status: String,
        #[sea_orm(column_type = "Text", nullable)]
        pub last_error: Option<String>,
        pub last_url_latency_ms: Option<i64>,
        pub last_download_mbps: Option<f64>,
        pub last_upload_mbps: Option<f64>,
        pub last_tested_at: Option<i64>,
        #[sea_orm(column_type = "Text", nullable)]
        pub ip: Option<String>,
        #[sea_orm(column_type = "Text", nullable)]
        pub country_code: Option<String>,
        #[sea_orm(column_type = "Text", nullable)]
        pub country_name: Option<String>,
        #[sea_orm(column_type = "Text", nullable)]
        pub region_name: Option<String>,
        #[sea_orm(column_type = "Text", nullable)]
        pub city_name: Option<String>,
        pub asn: Option<i64>,
        #[sea_orm(column_type = "Text", nullable)]
        pub as_org: Option<String>,
        #[sea_orm(column_type = "Text", nullable)]
        pub isp: Option<String>,
        #[sea_orm(column_type = "Text", nullable)]
        pub as_domain: Option<String>,
        #[sea_orm(column_type = "Text", nullable)]
        pub flag_img_url: Option<String>,
        #[sea_orm(column_type = "Text", nullable)]
        pub flag_emoji: Option<String>,
        #[sea_orm(column_type = "Text", nullable)]
        pub timezone_id: Option<String>,
        pub timezone_offset: Option<i64>,
        #[sea_orm(column_type = "Text", nullable)]
        pub timezone_utc: Option<String>,
        #[sea_orm(column_type = "Text", nullable)]
        pub tags_json: Option<String>,
        #[sea_orm(column_type = "Text", nullable)]
        pub notes: Option<String>,
        pub created_at: i64,
        pub updated_at: i64,
    }
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}
    impl ActiveModelBehavior for ActiveModel {}
}
impl From<proxy_profiles::Model> for codexmanager_core::storage::ProxyProfile {
    fn from(row: proxy_profiles::Model) -> Self {
        Self {
            id: row.id,
            name: row.name,
            proxy_url: row.proxy_url,
            proxy_url_redacted: row.proxy_url_redacted,
            scheme: row.scheme,
            host: row.host,
            port: row.port,
            enabled: row.enabled,
            status: row.status,
            last_error: row.last_error,
            last_url_latency_ms: row.last_url_latency_ms,
            last_download_mbps: row.last_download_mbps,
            last_upload_mbps: row.last_upload_mbps,
            last_tested_at: row.last_tested_at,
            ip: row.ip,
            country_code: row.country_code,
            country_name: row.country_name,
            region_name: row.region_name,
            city_name: row.city_name,
            asn: row.asn,
            as_org: row.as_org,
            isp: row.isp,
            as_domain: row.as_domain,
            flag_img_url: row.flag_img_url,
            flag_emoji: row.flag_emoji,
            timezone_id: row.timezone_id,
            timezone_offset: row.timezone_offset,
            timezone_utc: row.timezone_utc,
            tags_json: row.tags_json,
            notes: row.notes,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}
impl From<codexmanager_core::storage::ProxyProfile> for proxy_profiles::ActiveModel {
    fn from(row: codexmanager_core::storage::ProxyProfile) -> Self {
        Self {
            id: sea_orm::Set(row.id),
            name: sea_orm::Set(row.name),
            proxy_url: sea_orm::Set(row.proxy_url),
            proxy_url_redacted: sea_orm::Set(row.proxy_url_redacted),
            scheme: sea_orm::Set(row.scheme),
            host: sea_orm::Set(row.host),
            port: sea_orm::Set(row.port),
            enabled: sea_orm::Set(row.enabled),
            status: sea_orm::Set(row.status),
            last_error: sea_orm::Set(row.last_error),
            last_url_latency_ms: sea_orm::Set(row.last_url_latency_ms),
            last_download_mbps: sea_orm::Set(row.last_download_mbps),
            last_upload_mbps: sea_orm::Set(row.last_upload_mbps),
            last_tested_at: sea_orm::Set(row.last_tested_at),
            ip: sea_orm::Set(row.ip),
            country_code: sea_orm::Set(row.country_code),
            country_name: sea_orm::Set(row.country_name),
            region_name: sea_orm::Set(row.region_name),
            city_name: sea_orm::Set(row.city_name),
            asn: sea_orm::Set(row.asn),
            as_org: sea_orm::Set(row.as_org),
            isp: sea_orm::Set(row.isp),
            as_domain: sea_orm::Set(row.as_domain),
            flag_img_url: sea_orm::Set(row.flag_img_url),
            flag_emoji: sea_orm::Set(row.flag_emoji),
            timezone_id: sea_orm::Set(row.timezone_id),
            timezone_offset: sea_orm::Set(row.timezone_offset),
            timezone_utc: sea_orm::Set(row.timezone_utc),
            tags_json: sea_orm::Set(row.tags_json),
            notes: sea_orm::Set(row.notes),
            created_at: sea_orm::Set(row.created_at),
            updated_at: sea_orm::Set(row.updated_at),
        }
    }
}
pub(crate) mod quota_overrides {
    use sea_orm::entity::prelude::*;
    #[derive(Clone, Debug, DeriveEntityModel)]
    #[sea_orm(table_name = "account_quota_capacity_overrides")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub account_id: String,
        pub primary_window_tokens: Option<i64>,
        pub secondary_window_tokens: Option<i64>,
        pub updated_at: i64,
    }
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}
    impl ActiveModelBehavior for ActiveModel {}
}
impl From<quota_overrides::Model> for codexmanager_core::storage::AccountQuotaCapacityOverride {
    fn from(row: quota_overrides::Model) -> Self {
        Self {
            account_id: row.account_id,
            primary_window_tokens: row.primary_window_tokens,
            secondary_window_tokens: row.secondary_window_tokens,
            updated_at: row.updated_at,
        }
    }
}
impl From<codexmanager_core::storage::AccountQuotaCapacityOverride>
    for quota_overrides::ActiveModel
{
    fn from(row: codexmanager_core::storage::AccountQuotaCapacityOverride) -> Self {
        Self {
            account_id: sea_orm::Set(row.account_id),
            primary_window_tokens: sea_orm::Set(row.primary_window_tokens),
            secondary_window_tokens: sea_orm::Set(row.secondary_window_tokens),
            updated_at: sea_orm::Set(row.updated_at),
        }
    }
}
pub(crate) mod agent_identities {
    use sea_orm::entity::prelude::*;
    #[derive(Clone, Debug, DeriveEntityModel)]
    #[sea_orm(table_name = "account_agent_identities")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub account_id: String,
        #[sea_orm(column_type = "Text")]
        pub agent_runtime_id: String,
        #[sea_orm(column_type = "Text")]
        pub agent_private_key: String,
        #[sea_orm(column_type = "Text", nullable)]
        pub task_id: Option<String>,
        #[sea_orm(column_type = "Text")]
        pub chatgpt_user_id: String,
        pub chatgpt_account_is_fedramp: bool,
        #[sea_orm(column_type = "Text")]
        pub auth_mode: String,
        #[sea_orm(column_type = "Text", nullable)]
        pub workspace_id: Option<String>,
        pub created_at: i64,
        pub updated_at: i64,
    }
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}
    impl ActiveModelBehavior for ActiveModel {}
}
impl From<agent_identities::Model> for codexmanager_core::storage::AccountAgentIdentity {
    fn from(row: agent_identities::Model) -> Self {
        Self {
            account_id: row.account_id,
            agent_runtime_id: row.agent_runtime_id,
            agent_private_key: row.agent_private_key,
            task_id: row.task_id,
            chatgpt_user_id: row.chatgpt_user_id,
            chatgpt_account_is_fedramp: row.chatgpt_account_is_fedramp,
            auth_mode: row.auth_mode,
            workspace_id: row.workspace_id,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}
impl From<codexmanager_core::storage::AccountAgentIdentity> for agent_identities::ActiveModel {
    fn from(row: codexmanager_core::storage::AccountAgentIdentity) -> Self {
        Self {
            account_id: sea_orm::Set(row.account_id),
            agent_runtime_id: sea_orm::Set(row.agent_runtime_id),
            agent_private_key: sea_orm::Set(row.agent_private_key),
            task_id: sea_orm::Set(row.task_id),
            chatgpt_user_id: sea_orm::Set(row.chatgpt_user_id),
            chatgpt_account_is_fedramp: sea_orm::Set(row.chatgpt_account_is_fedramp),
            auth_mode: sea_orm::Set(row.auth_mode),
            workspace_id: sea_orm::Set(row.workspace_id),
            created_at: sea_orm::Set(row.created_at),
            updated_at: sea_orm::Set(row.updated_at),
        }
    }
}
pub(crate) mod login_sessions {
    use sea_orm::entity::prelude::*;
    #[derive(Clone, Debug, DeriveEntityModel)]
    #[sea_orm(table_name = "login_sessions")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub login_id: String,
        #[sea_orm(column_type = "Text")]
        pub code_verifier: String,
        #[sea_orm(column_type = "Text")]
        pub state: String,
        #[sea_orm(column_type = "Text")]
        pub status: String,
        #[sea_orm(column_type = "Text", nullable)]
        pub error: Option<String>,
        #[sea_orm(column_type = "Text", nullable)]
        pub workspace_id: Option<String>,
        #[sea_orm(column_type = "Text", nullable)]
        pub note: Option<String>,
        #[sea_orm(column_type = "Text", nullable)]
        pub tags: Option<String>,
        #[sea_orm(column_type = "Text", nullable)]
        pub group_name: Option<String>,
        pub created_at: i64,
        pub updated_at: i64,
    }
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}
    impl ActiveModelBehavior for ActiveModel {}
}
impl From<login_sessions::Model> for codexmanager_core::storage::LoginSession {
    fn from(row: login_sessions::Model) -> Self {
        Self {
            login_id: row.login_id,
            code_verifier: row.code_verifier,
            state: row.state,
            status: row.status,
            error: row.error,
            workspace_id: row.workspace_id,
            note: row.note,
            tags: row.tags,
            group_name: row.group_name,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}
impl From<codexmanager_core::storage::LoginSession> for login_sessions::ActiveModel {
    fn from(row: codexmanager_core::storage::LoginSession) -> Self {
        Self {
            login_id: sea_orm::Set(row.login_id),
            code_verifier: sea_orm::Set(row.code_verifier),
            state: sea_orm::Set(row.state),
            status: sea_orm::Set(row.status),
            error: sea_orm::Set(row.error),
            workspace_id: sea_orm::Set(row.workspace_id),
            note: sea_orm::Set(row.note),
            tags: sea_orm::Set(row.tags),
            group_name: sea_orm::Set(row.group_name),
            created_at: sea_orm::Set(row.created_at),
            updated_at: sea_orm::Set(row.updated_at),
        }
    }
}
pub(crate) mod events {
    use sea_orm::entity::prelude::*;
    #[derive(Clone, Debug, DeriveEntityModel)]
    #[sea_orm(table_name = "events")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub account_id: Option<String>,
        #[sea_orm(column_name = "type")]
        pub event_type: String,
        #[sea_orm(column_type = "Text")]
        pub message: String,
        pub created_at: i64,
    }
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}
    impl ActiveModelBehavior for ActiveModel {}
}
pub(crate) mod warmups {
    use sea_orm::entity::prelude::*;
    #[derive(Clone, Debug, DeriveEntityModel)]
    #[sea_orm(table_name = "account_reset_warmups")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub account_id: String,
        pub enabled: bool,
        pub pending_reset_at: Option<i64>,
        pub consumed_reset_at: Option<i64>,
        pub last_claimed_at: Option<i64>,
    }
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}
    impl ActiveModelBehavior for ActiveModel {}
}
