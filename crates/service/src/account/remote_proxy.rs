use super::remote_storage::AccountStorage;
use crate::storage_helpers::{seaorm_block_on, seaorm_enabled};
use codexmanager_core::storage::*;
use codexmanager_storage_seaorm::AccountsRepository;
use std::ops::Deref;
fn error(message: String) -> rusqlite::Error {
    rusqlite::Error::SqliteFailure((), Some(message))
}
impl AccountStorage<'_> {
    pub(crate) fn upsert_account_proxy_settings(
        &self,
        account_id: &str,
        enabled: bool,
        proxy_source: Option<&str>,
        proxy_profile_id: Option<&str>,
        proxy_url: Option<&str>,
        status: &str,
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
    ) -> rusqlite::Result<()> {
        if !seaorm_enabled() {
            return self.deref().upsert_account_proxy_settings(
                account_id,
                enabled,
                proxy_source,
                proxy_profile_id,
                proxy_url,
                status,
                latency_ms,
                last_check_at,
                last_error,
                ip,
                country_code,
                country_name,
                region_name,
                city_name,
                geo_checked_at,
                geo_error,
                asn,
                as_org,
                isp,
                as_domain,
                timezone_id,
                timezone_offset,
                timezone_utc,
                flag_img_url,
                flag_emoji,
            );
        }
        let account_id = account_id.to_owned();
        let proxy_source = proxy_source.map(ToOwned::to_owned);
        let proxy_profile_id = proxy_profile_id.map(ToOwned::to_owned);
        let proxy_url = proxy_url.map(ToOwned::to_owned);
        let status = status.to_owned();
        let last_error = last_error.map(ToOwned::to_owned);
        let ip = ip.map(ToOwned::to_owned);
        let country_code = country_code.map(ToOwned::to_owned);
        let country_name = country_name.map(ToOwned::to_owned);
        let region_name = region_name.map(ToOwned::to_owned);
        let city_name = city_name.map(ToOwned::to_owned);
        let geo_error = geo_error.map(ToOwned::to_owned);
        let as_org = as_org.map(ToOwned::to_owned);
        let isp = isp.map(ToOwned::to_owned);
        let as_domain = as_domain.map(ToOwned::to_owned);
        let timezone_id = timezone_id.map(ToOwned::to_owned);
        let timezone_utc = timezone_utc.map(ToOwned::to_owned);
        let flag_img_url = flag_img_url.map(ToOwned::to_owned);
        let flag_emoji = flag_emoji.map(ToOwned::to_owned);
        seaorm_block_on(move |s| async move {
            AccountsRepository::configure_account_proxy_settings(
                s.connection(),
                &account_id,
                enabled,
                proxy_source.as_deref(),
                proxy_profile_id.as_deref(),
                proxy_url.as_deref(),
                &status,
                latency_ms,
                last_check_at,
                last_error.as_deref(),
                ip.as_deref(),
                country_code.as_deref(),
                country_name.as_deref(),
                region_name.as_deref(),
                city_name.as_deref(),
                geo_checked_at,
                geo_error.as_deref(),
                asn,
                as_org.as_deref(),
                isp.as_deref(),
                as_domain.as_deref(),
                timezone_id.as_deref(),
                timezone_offset,
                timezone_utc.as_deref(),
                flag_img_url.as_deref(),
                flag_emoji.as_deref(),
            )
            .await
            .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn update_account_proxy_check_status(
        &self,
        account_id: &str,
        status: &str,
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
    ) -> rusqlite::Result<()> {
        if !seaorm_enabled() {
            return self.deref().update_account_proxy_check_status(
                account_id,
                status,
                latency_ms,
                last_check_at,
                last_error,
                ip,
                country_code,
                country_name,
                region_name,
                city_name,
                geo_checked_at,
                geo_error,
                asn,
                as_org,
                isp,
                as_domain,
                timezone_id,
                timezone_offset,
                timezone_utc,
                flag_img_url,
                flag_emoji,
            );
        }
        let account_id = account_id.to_owned();
        let status = status.to_owned();
        let last_error = last_error.map(ToOwned::to_owned);
        let ip = ip.map(ToOwned::to_owned);
        let country_code = country_code.map(ToOwned::to_owned);
        let country_name = country_name.map(ToOwned::to_owned);
        let region_name = region_name.map(ToOwned::to_owned);
        let city_name = city_name.map(ToOwned::to_owned);
        let geo_error = geo_error.map(ToOwned::to_owned);
        let as_org = as_org.map(ToOwned::to_owned);
        let isp = isp.map(ToOwned::to_owned);
        let as_domain = as_domain.map(ToOwned::to_owned);
        let timezone_id = timezone_id.map(ToOwned::to_owned);
        let timezone_utc = timezone_utc.map(ToOwned::to_owned);
        let flag_img_url = flag_img_url.map(ToOwned::to_owned);
        let flag_emoji = flag_emoji.map(ToOwned::to_owned);
        seaorm_block_on(move |s| async move {
            AccountsRepository::update_account_proxy_check_status(
                s.connection(),
                &account_id,
                &status,
                latency_ms,
                last_check_at,
                last_error.as_deref(),
                ip.as_deref(),
                country_code.as_deref(),
                country_name.as_deref(),
                region_name.as_deref(),
                city_name.as_deref(),
                geo_checked_at,
                geo_error.as_deref(),
                asn,
                as_org.as_deref(),
                isp.as_deref(),
                as_domain.as_deref(),
                timezone_id.as_deref(),
                timezone_offset,
                timezone_utc.as_deref(),
                flag_img_url.as_deref(),
                flag_emoji.as_deref(),
            )
            .await
            .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn update_account_proxy_test_result(
        &self,
        account_id: &str,
        status: &str,
        latency_ms: Option<i64>,
        last_download_mbps: Option<f64>,
        last_upload_mbps: Option<f64>,
        last_check_at: Option<i64>,
        last_error: Option<&str>,
    ) -> rusqlite::Result<()> {
        if !seaorm_enabled() {
            return self.deref().update_account_proxy_test_result(
                account_id,
                status,
                latency_ms,
                last_download_mbps,
                last_upload_mbps,
                last_check_at,
                last_error,
            );
        }
        let account_id = account_id.to_owned();
        let status = status.to_owned();
        let last_error = last_error.map(ToOwned::to_owned);
        seaorm_block_on(move |s| async move {
            AccountsRepository::update_account_proxy_test_result(
                s.connection(),
                &account_id,
                &status,
                latency_ms,
                last_download_mbps,
                last_upload_mbps,
                last_check_at,
                last_error.as_deref(),
            )
            .await
            .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn list_account_ids_bound_to_proxy_profile(
        &self,
        proxy_profile_id: &str,
    ) -> rusqlite::Result<Vec<String>> {
        if !seaorm_enabled() {
            return self
                .deref()
                .list_account_ids_bound_to_proxy_profile(proxy_profile_id);
        }
        let proxy_profile_id = proxy_profile_id.to_owned();
        seaorm_block_on(move |s| async move {
            AccountsRepository::list_account_ids_bound_to_proxy_profile(
                s.connection(),
                &proxy_profile_id,
            )
            .await
            .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn create_proxy_profile(
        &self,
        input: &ProxyProfileCreateInput,
    ) -> rusqlite::Result<ProxyProfile> {
        if !seaorm_enabled() {
            return self.deref().create_proxy_profile(input);
        }
        let input = input.clone();
        seaorm_block_on(move |s| async move {
            AccountsRepository::create_proxy_profile(s.connection(), &input)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn update_proxy_profile(
        &self,
        input: &ProxyProfileUpdateInput,
    ) -> rusqlite::Result<Option<ProxyProfile>> {
        if !seaorm_enabled() {
            return self.deref().update_proxy_profile(input);
        }
        let input = input.clone();
        seaorm_block_on(move |s| async move {
            AccountsRepository::update_proxy_profile(s.connection(), &input)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn delete_proxy_profile(&self, id: &str) -> rusqlite::Result<bool> {
        if !seaorm_enabled() {
            return self.deref().delete_proxy_profile(id);
        }
        let id = id.to_owned();
        seaorm_block_on(move |s| async move {
            AccountsRepository::delete_proxy_profile(s.connection(), &id)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
}
