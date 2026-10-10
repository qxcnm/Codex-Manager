use super::remote_storage::AccountStorage;
use crate::storage_helpers::{seaorm_block_on, seaorm_enabled};
use codexmanager_core::storage::AccountResetWarmupTarget;
use codexmanager_storage_seaorm::AccountsRepository;
use std::ops::Deref;
fn error(message: String) -> rusqlite::Error {
    rusqlite::Error::SqliteFailure((), Some(message))
}
impl AccountStorage<'_> {
    pub(crate) fn list_account_reset_warmup_targets(
        &self,
        now: i64,
        limit: usize,
    ) -> rusqlite::Result<Vec<AccountResetWarmupTarget>> {
        if !seaorm_enabled() {
            return self.deref().list_account_reset_warmup_targets(now, limit);
        }

        seaorm_block_on(move |s| async move {
            AccountsRepository::list_account_reset_warmup_targets(s.connection(), now, limit)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn claim_account_reset_warmup(
        &self,
        account_id: &str,
        reset_at: i64,
        now: i64,
    ) -> rusqlite::Result<bool> {
        if !seaorm_enabled() {
            return self
                .deref()
                .claim_account_reset_warmup(account_id, reset_at, now);
        }
        let account_id = account_id.to_owned();
        seaorm_block_on(move |s| async move {
            AccountsRepository::claim_account_reset_warmup(
                s.connection(),
                &account_id,
                reset_at,
                now,
            )
            .await
            .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn set_account_reset_warmup_enabled(
        &self,
        ids: &[String],
        enabled: bool,
    ) -> rusqlite::Result<usize> {
        if !seaorm_enabled() {
            return self.deref().set_account_reset_warmup_enabled(ids, enabled);
        }
        let ids = ids.to_vec();
        seaorm_block_on(move |s| async move {
            AccountsRepository::set_account_reset_warmup_enabled(s.connection(), &ids, enabled)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn list_account_reset_warmup_settings_for_accounts(
        &self,
        ids: &[String],
    ) -> rusqlite::Result<Vec<(String, bool)>> {
        if !seaorm_enabled() {
            return self
                .deref()
                .list_account_reset_warmup_settings_for_accounts(ids);
        }
        let ids = ids.to_vec();
        seaorm_block_on(move |s| async move {
            AccountsRepository::list_account_reset_warmup_settings_for_accounts(
                s.connection(),
                &ids,
            )
            .await
            .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
}
