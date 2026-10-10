use super::remote_storage::AccountStorage;
use crate::storage_helpers::{seaorm_block_on, seaorm_enabled};
use codexmanager_core::storage::*;
use codexmanager_storage_seaorm::AccountsRepository;
use std::ops::Deref;
fn error(message: String) -> rusqlite::Error {
    rusqlite::Error::SqliteFailure((), Some(message))
}
// Keep the full dual-backend adapter surface available for SeaORM runtime selection.
#[allow(dead_code)]
impl AccountStorage<'_> {
    pub(crate) fn insert_proxy_profile_url_test(
        &self,
        input: &ProxyProfileUrlTestInsertInput,
    ) -> rusqlite::Result<ProxyProfileUrlTest> {
        if !seaorm_enabled() {
            return self.deref().insert_proxy_profile_url_test(input);
        }
        let input = input.clone();
        seaorm_block_on(move |s| async move {
            AccountsRepository::insert_proxy_profile_url_test(s.connection(), &input)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn find_proxy_profile_url_test(
        &self,
        id: i64,
    ) -> rusqlite::Result<Option<ProxyProfileUrlTest>> {
        if !seaorm_enabled() {
            return self.deref().find_proxy_profile_url_test(id);
        }
        seaorm_block_on(move |s| async move {
            AccountsRepository::find_proxy_profile_url_test(s.connection(), id)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn list_proxy_profile_url_tests(
        &self,
        value: &str,
        limit: usize,
    ) -> rusqlite::Result<Vec<ProxyProfileUrlTest>> {
        if !seaorm_enabled() {
            return self.deref().list_proxy_profile_url_tests(value, limit);
        }
        let value = value.to_owned();
        seaorm_block_on(move |s| async move {
            AccountsRepository::list_proxy_profile_url_tests(s.connection(), &value, limit)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn delete_proxy_profile_url_tests_by_profile(
        &self,
        value: &str,
    ) -> rusqlite::Result<usize> {
        if !seaorm_enabled() {
            return self
                .deref()
                .delete_proxy_profile_url_tests_by_profile(value);
        }
        let value = value.to_owned();
        seaorm_block_on(move |s| async move {
            AccountsRepository::delete_proxy_profile_url_tests_by_profile(s.connection(), &value)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn insert_proxy_speed_test(
        &self,
        input: &ProxySpeedTestInsertInput,
    ) -> rusqlite::Result<ProxySpeedTest> {
        if !seaorm_enabled() {
            return self.deref().insert_proxy_speed_test(input);
        }
        let input = input.clone();
        seaorm_block_on(move |s| async move {
            AccountsRepository::insert_proxy_speed_test(s.connection(), &input)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn find_proxy_speed_test(
        &self,
        id: i64,
    ) -> rusqlite::Result<Option<ProxySpeedTest>> {
        if !seaorm_enabled() {
            return self.deref().find_proxy_speed_test(id);
        }
        seaorm_block_on(move |s| async move {
            AccountsRepository::find_proxy_speed_test(s.connection(), id)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn list_proxy_speed_tests_by_profile(
        &self,
        value: &str,
        limit: usize,
    ) -> rusqlite::Result<Vec<ProxySpeedTest>> {
        if !seaorm_enabled() {
            return self.deref().list_proxy_speed_tests_by_profile(value, limit);
        }
        let value = value.to_owned();
        seaorm_block_on(move |s| async move {
            AccountsRepository::list_proxy_speed_tests_by_profile(s.connection(), &value, limit)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn list_proxy_speed_tests_by_account(
        &self,
        value: &str,
        limit: usize,
    ) -> rusqlite::Result<Vec<ProxySpeedTest>> {
        if !seaorm_enabled() {
            return self.deref().list_proxy_speed_tests_by_account(value, limit);
        }
        let value = value.to_owned();
        seaorm_block_on(move |s| async move {
            AccountsRepository::list_proxy_speed_tests_by_account(s.connection(), &value, limit)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn delete_proxy_speed_tests_by_profile(
        &self,
        value: &str,
    ) -> rusqlite::Result<usize> {
        if !seaorm_enabled() {
            return self.deref().delete_proxy_speed_tests_by_profile(value);
        }
        let value = value.to_owned();
        seaorm_block_on(move |s| async move {
            AccountsRepository::delete_proxy_speed_tests_by_profile(s.connection(), &value)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn insert_proxy_diagnostic_test(
        &self,
        input: &ProxyDiagnosticTestInsertInput,
    ) -> rusqlite::Result<ProxyDiagnosticTest> {
        if !seaorm_enabled() {
            return self.deref().insert_proxy_diagnostic_test(input);
        }
        let input = input.clone();
        seaorm_block_on(move |s| async move {
            AccountsRepository::insert_proxy_diagnostic_test(s.connection(), &input)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn find_proxy_diagnostic_test(
        &self,
        id: i64,
    ) -> rusqlite::Result<Option<ProxyDiagnosticTest>> {
        if !seaorm_enabled() {
            return self.deref().find_proxy_diagnostic_test(id);
        }
        seaorm_block_on(move |s| async move {
            AccountsRepository::find_proxy_diagnostic_test(s.connection(), id)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn list_proxy_diagnostic_tests_by_profile(
        &self,
        value: &str,
        limit: usize,
    ) -> rusqlite::Result<Vec<ProxyDiagnosticTest>> {
        if !seaorm_enabled() {
            return self
                .deref()
                .list_proxy_diagnostic_tests_by_profile(value, limit);
        }
        let value = value.to_owned();
        seaorm_block_on(move |s| async move {
            AccountsRepository::list_proxy_diagnostic_tests_by_profile(
                s.connection(),
                &value,
                limit,
            )
            .await
            .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn list_proxy_diagnostic_tests_by_account(
        &self,
        value: &str,
        limit: usize,
    ) -> rusqlite::Result<Vec<ProxyDiagnosticTest>> {
        if !seaorm_enabled() {
            return self
                .deref()
                .list_proxy_diagnostic_tests_by_account(value, limit);
        }
        let value = value.to_owned();
        seaorm_block_on(move |s| async move {
            AccountsRepository::list_proxy_diagnostic_tests_by_account(
                s.connection(),
                &value,
                limit,
            )
            .await
            .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn delete_proxy_diagnostic_tests_by_profile(
        &self,
        value: &str,
    ) -> rusqlite::Result<usize> {
        if !seaorm_enabled() {
            return self.deref().delete_proxy_diagnostic_tests_by_profile(value);
        }
        let value = value.to_owned();
        seaorm_block_on(move |s| async move {
            AccountsRepository::delete_proxy_diagnostic_tests_by_profile(s.connection(), &value)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn insert_account_proxy_url_test(
        &self,
        input: &AccountProxyUrlTestInsertInput,
    ) -> rusqlite::Result<AccountProxyUrlTest> {
        if !seaorm_enabled() {
            return self.deref().insert_account_proxy_url_test(input);
        }
        let input = input.clone();
        seaorm_block_on(move |s| async move {
            AccountsRepository::insert_account_proxy_url_test(s.connection(), &input)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn find_account_proxy_url_test(
        &self,
        id: i64,
    ) -> rusqlite::Result<Option<AccountProxyUrlTest>> {
        if !seaorm_enabled() {
            return self.deref().find_account_proxy_url_test(id);
        }
        seaorm_block_on(move |s| async move {
            AccountsRepository::find_account_proxy_url_test(s.connection(), id)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn list_account_proxy_url_tests(
        &self,
        value: &str,
        limit: usize,
    ) -> rusqlite::Result<Vec<AccountProxyUrlTest>> {
        if !seaorm_enabled() {
            return self.deref().list_account_proxy_url_tests(value, limit);
        }
        let value = value.to_owned();
        seaorm_block_on(move |s| async move {
            AccountsRepository::list_account_proxy_url_tests(s.connection(), &value, limit)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn find_cached_proxy_flag_by_country(
        &self,
        value: &str,
    ) -> rusqlite::Result<Option<String>> {
        if !seaorm_enabled() {
            return self.deref().find_cached_proxy_flag_by_country(value);
        }
        let value = value.to_owned();
        seaorm_block_on(move |s| async move {
            AccountsRepository::find_cached_proxy_flag_by_country(s.connection(), &value)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
}
