use crate::{
    account_details::{proxy_profiles as profiles, proxy_settings as settings},
    AccountsRepository, UsersRepository,
};
use codexmanager_core::storage::*;
use sea_orm::sea_query::{Expr, OnConflict};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseConnection, DbErr, EntityTrait,
    IntoActiveModel, QueryFilter, QueryOrder, Set, TransactionTrait,
};
fn optional(v: Option<&str>) -> Option<String> {
    v.map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
}
fn status(v: &str) -> String {
    if v.trim().is_empty() {
        "unchecked".into()
    } else {
        v.trim().into()
    }
}
impl AccountsRepository {
    pub async fn configure_account_proxy_settings(
        db: &impl ConnectionTrait,
        account_id: &str,
        enabled: bool,
        proxy_source: Option<&str>,
        proxy_profile_id: Option<&str>,
        proxy_url: Option<&str>,
        status_value: &str,
        latency_ms: Option<i64>,
        last_check_at: Option<i64>,
        last_error: Option<&str>,
        ip: Option<&str>,
        country_code: Option<&str>,
        country_name: Option<&str>,
        region_name: Option<&str>,
        city_name: Option<&str>,
        geo_checked_at: Option<i64>,
        geo_error: Option<&str>,
        asn: Option<i64>,
        as_org: Option<&str>,
        isp: Option<&str>,
        as_domain: Option<&str>,
        timezone_id: Option<&str>,
        timezone_offset: Option<i64>,
        timezone_utc: Option<&str>,
        flag_img_url: Option<&str>,
        flag_emoji: Option<&str>,
    ) -> Result<(), DbErr> {
        let now = now_ts();
        let row = settings::ActiveModel {
            account_id: Set(account_id.to_owned()),
            enabled: Set(enabled),
            proxy_source: Set(optional(proxy_source).map(|s| s.to_ascii_lowercase())),
            proxy_profile_id: Set(optional(proxy_profile_id)),
            proxy_url: Set(optional(proxy_url)),
            status: Set(status(status_value)),
            latency_ms: Set(latency_ms),
            last_check_at: Set(last_check_at),
            last_error: Set(optional(last_error)),
            ip: Set(optional(ip)),
            country_code: Set(optional(country_code).map(|s| s.to_ascii_uppercase())),
            country_name: Set(optional(country_name)),
            region_name: Set(optional(region_name)),
            city_name: Set(optional(city_name)),
            geo_checked_at: Set(geo_checked_at),
            geo_error: Set(optional(geo_error)),
            asn: Set(asn),
            as_org: Set(optional(as_org)),
            isp: Set(optional(isp)),
            as_domain: Set(optional(as_domain)),
            timezone_id: Set(optional(timezone_id)),
            timezone_offset: Set(timezone_offset),
            timezone_utc: Set(optional(timezone_utc)),
            flag_img_url: Set(optional(flag_img_url)),
            flag_emoji: Set(optional(flag_emoji)),
            last_download_mbps: Set(None),
            last_upload_mbps: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
        };
        settings::Entity::insert(row)
            .on_conflict(
                OnConflict::column(settings::Column::AccountId)
                    .update_columns([
                        settings::Column::Enabled,
                        settings::Column::ProxySource,
                        settings::Column::ProxyProfileId,
                        settings::Column::ProxyUrl,
                        settings::Column::Status,
                        settings::Column::LatencyMs,
                        settings::Column::LastCheckAt,
                        settings::Column::LastError,
                        settings::Column::Ip,
                        settings::Column::CountryCode,
                        settings::Column::CountryName,
                        settings::Column::RegionName,
                        settings::Column::CityName,
                        settings::Column::GeoCheckedAt,
                        settings::Column::GeoError,
                        settings::Column::Asn,
                        settings::Column::AsOrg,
                        settings::Column::Isp,
                        settings::Column::AsDomain,
                        settings::Column::TimezoneId,
                        settings::Column::TimezoneOffset,
                        settings::Column::TimezoneUtc,
                        settings::Column::FlagImgUrl,
                        settings::Column::FlagEmoji,
                        settings::Column::UpdatedAt,
                    ])
                    .to_owned(),
            )
            .exec(db)
            .await?;
        Ok(())
    }
    pub async fn update_account_proxy_check_status(
        db: &impl ConnectionTrait,
        account_id: &str,
        status_value: &str,
        latency_ms: Option<i64>,
        last_check_at: Option<i64>,
        last_error: Option<&str>,
        ip: Option<&str>,
        country_code: Option<&str>,
        country_name: Option<&str>,
        region_name: Option<&str>,
        city_name: Option<&str>,
        geo_checked_at: Option<i64>,
        geo_error: Option<&str>,
        asn: Option<i64>,
        as_org: Option<&str>,
        isp: Option<&str>,
        as_domain: Option<&str>,
        timezone_id: Option<&str>,
        timezone_offset: Option<i64>,
        timezone_utc: Option<&str>,
        flag_img_url: Option<&str>,
        flag_emoji: Option<&str>,
    ) -> Result<(), DbErr> {
        settings::Entity::update_many()
            .col_expr(settings::Column::Status, Expr::value(status(status_value)))
            .col_expr(settings::Column::LatencyMs, Expr::value(latency_ms))
            .col_expr(settings::Column::LastCheckAt, Expr::value(last_check_at))
            .col_expr(
                settings::Column::LastError,
                Expr::value(optional(last_error)),
            )
            .col_expr(settings::Column::Ip, Expr::value(optional(ip)))
            .col_expr(
                settings::Column::CountryCode,
                Expr::value(optional(country_code).map(|s| s.to_ascii_uppercase())),
            )
            .col_expr(
                settings::Column::CountryName,
                Expr::value(optional(country_name)),
            )
            .col_expr(
                settings::Column::RegionName,
                Expr::value(optional(region_name)),
            )
            .col_expr(settings::Column::CityName, Expr::value(optional(city_name)))
            .col_expr(settings::Column::GeoCheckedAt, Expr::value(geo_checked_at))
            .col_expr(settings::Column::GeoError, Expr::value(optional(geo_error)))
            .col_expr(settings::Column::Asn, Expr::value(asn))
            .col_expr(settings::Column::AsOrg, Expr::value(optional(as_org)))
            .col_expr(settings::Column::Isp, Expr::value(optional(isp)))
            .col_expr(settings::Column::AsDomain, Expr::value(optional(as_domain)))
            .col_expr(
                settings::Column::TimezoneId,
                Expr::value(optional(timezone_id)),
            )
            .col_expr(
                settings::Column::TimezoneOffset,
                Expr::value(timezone_offset),
            )
            .col_expr(
                settings::Column::TimezoneUtc,
                Expr::value(optional(timezone_utc)),
            )
            .col_expr(
                settings::Column::FlagImgUrl,
                Expr::value(optional(flag_img_url)),
            )
            .col_expr(
                settings::Column::FlagEmoji,
                Expr::value(optional(flag_emoji)),
            )
            .col_expr(settings::Column::UpdatedAt, Expr::value(now_ts()))
            .filter(settings::Column::AccountId.eq(account_id))
            .exec(db)
            .await?;
        Ok(())
    }
    pub async fn update_account_proxy_test_result(
        db: &impl ConnectionTrait,
        account_id: &str,
        status_value: &str,
        latency_ms: Option<i64>,
        last_download_mbps: Option<f64>,
        last_upload_mbps: Option<f64>,
        last_check_at: Option<i64>,
        last_error: Option<&str>,
    ) -> Result<(), DbErr> {
        settings::Entity::update_many()
            .col_expr(settings::Column::Status, Expr::value(status(status_value)))
            .col_expr(settings::Column::LatencyMs, Expr::value(latency_ms))
            .col_expr(
                settings::Column::LastDownloadMbps,
                Expr::value(last_download_mbps),
            )
            .col_expr(
                settings::Column::LastUploadMbps,
                Expr::value(last_upload_mbps),
            )
            .col_expr(settings::Column::LastCheckAt, Expr::value(last_check_at))
            .col_expr(
                settings::Column::LastError,
                Expr::value(optional(last_error)),
            )
            .col_expr(settings::Column::UpdatedAt, Expr::value(now_ts()))
            .filter(settings::Column::AccountId.eq(account_id))
            .exec(db)
            .await?;
        Ok(())
    }
    pub async fn list_account_ids_bound_to_proxy_profile(
        db: &impl ConnectionTrait,
        proxy_profile_id: &str,
    ) -> Result<Vec<String>, DbErr> {
        let mut q = settings::Entity::find()
            .filter(settings::Column::ProxyProfileId.eq(proxy_profile_id.trim()));
        q = q.filter(settings::Column::ProxySource.eq("profile"));
        Ok(q.order_by_asc(settings::Column::AccountId)
            .all(db)
            .await?
            .into_iter()
            .map(|r| r.account_id)
            .collect())
    }
    pub async fn create_proxy_profile(
        db: &impl ConnectionTrait,
        input: &ProxyProfileCreateInput,
    ) -> Result<ProxyProfile, DbErr> {
        let meta = derive_proxy_profile_url_metadata(input.proxy_url.trim());
        let now = now_ts();
        let row = profiles::ActiveModel {
            id: Set(input.id.trim().to_owned()),
            name: Set(input.name.trim().to_owned()),
            proxy_url: Set(input.proxy_url.trim().to_owned()),
            proxy_url_redacted: Set(meta.proxy_url_redacted),
            scheme: Set(meta.scheme),
            host: Set(meta.host),
            port: Set(meta.port),
            enabled: Set(input.enabled),
            status: Set("unchecked".into()),
            last_error: Set(None),
            last_url_latency_ms: Set(None),
            last_download_mbps: Set(None),
            last_upload_mbps: Set(None),
            last_tested_at: Set(None),
            ip: Set(None),
            country_code: Set(None),
            country_name: Set(None),
            region_name: Set(None),
            city_name: Set(None),
            asn: Set(None),
            as_org: Set(None),
            isp: Set(None),
            as_domain: Set(None),
            flag_img_url: Set(None),
            flag_emoji: Set(None),
            timezone_id: Set(None),
            timezone_offset: Set(None),
            timezone_utc: Set(None),
            tags_json: Set(optional(input.tags_json.as_deref())),
            notes: Set(optional(input.notes.as_deref())),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(db)
        .await?;
        Ok(row.into())
    }
    pub async fn update_proxy_profile(
        db: &DatabaseConnection,
        input: &ProxyProfileUpdateInput,
    ) -> Result<Option<ProxyProfile>, DbErr> {
        let tx = db.begin().await?;
        UsersRepository::lock(&tx, "accounts").await?;
        let Some(existing) = profiles::Entity::find_by_id(input.id.trim())
            .one(&tx)
            .await?
        else {
            return Ok(None);
        };
        let mut row = existing.into_active_model();
        if let Some(v) = input.name.as_deref() {
            row.name = Set(v.trim().to_owned());
        }
        if let Some(v) = input.proxy_url.as_deref() {
            row.proxy_url = Set(v.trim().to_owned());
        }
        if let Some(v) = input.enabled {
            row.enabled = Set(v);
        }
        if let Some(v) = input.status.as_deref() {
            row.status = Set(status(v));
        }
        if let Some(v) = input.last_error.as_deref() {
            row.last_error = Set(optional(Some(v)));
        }
        if let Some(v) = input.last_url_latency_ms {
            row.last_url_latency_ms = Set(Some(v));
        }
        if let Some(v) = input.last_download_mbps {
            row.last_download_mbps = Set(Some(v));
        }
        if let Some(v) = input.last_upload_mbps {
            row.last_upload_mbps = Set(Some(v));
        }
        if let Some(v) = input.last_tested_at {
            row.last_tested_at = Set(Some(v));
        }
        if let Some(v) = input.ip.as_deref() {
            row.ip = Set(optional(Some(v)));
        }
        if let Some(v) = input.country_code.as_deref() {
            row.country_code = Set(optional(Some(v)).map(|s| s.to_ascii_uppercase()));
        }
        if let Some(v) = input.country_name.as_deref() {
            row.country_name = Set(optional(Some(v)));
        }
        if let Some(v) = input.region_name.as_deref() {
            row.region_name = Set(optional(Some(v)));
        }
        if let Some(v) = input.city_name.as_deref() {
            row.city_name = Set(optional(Some(v)));
        }
        if let Some(v) = input.asn {
            row.asn = Set(Some(v));
        }
        if let Some(v) = input.as_org.as_deref() {
            row.as_org = Set(optional(Some(v)));
        }
        if let Some(v) = input.isp.as_deref() {
            row.isp = Set(optional(Some(v)));
        }
        if let Some(v) = input.as_domain.as_deref() {
            row.as_domain = Set(optional(Some(v)));
        }
        if let Some(v) = input.flag_img_url.as_deref() {
            row.flag_img_url = Set(optional(Some(v)));
        }
        if let Some(v) = input.flag_emoji.as_deref() {
            row.flag_emoji = Set(optional(Some(v)));
        }
        if let Some(v) = input.timezone_id.as_deref() {
            row.timezone_id = Set(optional(Some(v)));
        }
        if let Some(v) = input.timezone_offset {
            row.timezone_offset = Set(Some(v));
        }
        if let Some(v) = input.timezone_utc.as_deref() {
            row.timezone_utc = Set(optional(Some(v)));
        }
        if let Some(v) = input.tags_json.as_deref() {
            row.tags_json = Set(optional(Some(v)));
        }
        if let Some(v) = input.notes.as_deref() {
            row.notes = Set(optional(Some(v)));
        }
        if let Some(url) = &input.proxy_url {
            let meta = derive_proxy_profile_url_metadata(url.trim());
            row.proxy_url_redacted = Set(meta.proxy_url_redacted);
            row.scheme = Set(meta.scheme);
            row.host = Set(meta.host);
            row.port = Set(meta.port);
        }
        row.updated_at = Set(now_ts());
        let result = row.update(&tx).await?;
        tx.commit().await?;
        Ok(Some(result.into()))
    }
    pub async fn delete_proxy_profile(db: &DatabaseConnection, id: &str) -> Result<bool, DbErr> {
        let tx = db.begin().await?;
        UsersRepository::lock(&tx, "accounts").await?;
        let deleted = profiles::Entity::delete_by_id(id.trim())
            .exec(&tx)
            .await?
            .rows_affected
            > 0;
        if deleted {
            Self::delete_proxy_profile_url_tests_by_profile(&tx, id).await?;
            Self::delete_proxy_speed_tests_by_profile(&tx, id).await?;
            Self::delete_proxy_diagnostic_tests_by_profile(&tx, id).await?;
        }
        tx.commit().await?;
        Ok(deleted)
    }
}

#[cfg(test)]
#[path = "accounts_proxy_tests.rs"]
pub(crate) mod tests;
