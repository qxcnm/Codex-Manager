use super::remote_storage::AccountStorage;
use crate::storage_helpers::{seaorm_block_on, seaorm_enabled};
use codexmanager_core::storage::AccountBpsSettings;
use codexmanager_storage_seaorm::AccountsRepository;
use std::ops::Deref;

fn error(message: String) -> rusqlite::Error {
    rusqlite::Error::SqliteFailure((), Some(message))
}

impl AccountStorage<'_> {
    pub(crate) fn any_account_bps_enabled(&self) -> rusqlite::Result<bool> {
        if !seaorm_enabled() {
            return self.deref().any_account_bps_enabled();
        }
        seaorm_block_on(move |s| async move {
            AccountsRepository::any_account_bps_enabled(s.connection())
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn find_account_bps_settings(
        &self,
        id: &str,
    ) -> rusqlite::Result<Option<AccountBpsSettings>> {
        if !seaorm_enabled() {
            return self.deref().find_account_bps_settings(id);
        }
        let id = id.to_owned();
        seaorm_block_on(move |s| async move {
            AccountsRepository::find_account_bps_settings(s.connection(), &id)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn list_account_bps_settings(
        &self,
        ids: &[String],
    ) -> rusqlite::Result<Vec<AccountBpsSettings>> {
        if !seaorm_enabled() {
            return self.deref().list_account_bps_settings(ids);
        }
        let ids = ids.to_vec();
        seaorm_block_on(move |s| async move {
            AccountsRepository::list_account_bps_settings(s.connection(), &ids)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn save_account_bps_credentials(
        &self,
        settings: &AccountBpsSettings,
    ) -> rusqlite::Result<()> {
        if !seaorm_enabled() {
            return self.deref().save_account_bps_credentials(settings);
        }
        let settings = settings.clone();
        seaorm_block_on(move |s| async move {
            AccountsRepository::save_account_bps_credentials(s.connection(), &settings)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn set_account_bps_enabled(
        &self,
        ids: &[String],
        enabled: bool,
    ) -> rusqlite::Result<usize> {
        if !seaorm_enabled() {
            return self.deref().set_account_bps_enabled(ids, enabled);
        }
        let ids = ids.to_vec();
        seaorm_block_on(move |s| async move {
            AccountsRepository::set_account_bps_enabled(s.connection(), &ids, enabled)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
}
