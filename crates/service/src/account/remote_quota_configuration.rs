use super::remote_storage::AccountStorage;
use crate::storage_helpers::{seaorm_block_on, seaorm_enabled};
use codexmanager_core::storage::{AccountQuotaCapacityTemplate, QuotaSourceModelAssignment};
use codexmanager_storage_seaorm::QuotaConfigurationRepository;
use std::ops::Deref;
fn error(message: String) -> rusqlite::Error {
    rusqlite::Error::SqliteFailure((), Some(message))
}
// Keep the full dual-backend adapter surface available for SeaORM runtime selection.
#[allow(dead_code)]
impl AccountStorage<'_> {
    pub(crate) fn list_account_quota_capacity_templates(
        &self,
    ) -> rusqlite::Result<Vec<AccountQuotaCapacityTemplate>> {
        if !seaorm_enabled() {
            return self.deref().list_account_quota_capacity_templates();
        }
        seaorm_block_on(|storage| async move {
            QuotaConfigurationRepository::templates(storage.connection())
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn upsert_account_quota_capacity_template(
        &self,
        plan: &str,
        primary: Option<i64>,
        secondary: Option<i64>,
    ) -> rusqlite::Result<()> {
        if !seaorm_enabled() {
            return self
                .deref()
                .upsert_account_quota_capacity_template(plan, primary, secondary);
        }
        let plan = plan.to_owned();
        seaorm_block_on(move |storage| async move {
            QuotaConfigurationRepository::set_template(
                storage.connection(),
                &plan,
                primary,
                secondary,
            )
            .await
            .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn list_quota_source_model_assignments(
        &self,
    ) -> rusqlite::Result<Vec<QuotaSourceModelAssignment>> {
        if !seaorm_enabled() {
            return self.deref().list_quota_source_model_assignments();
        }
        seaorm_block_on(|storage| async move {
            QuotaConfigurationRepository::assignments(storage.connection(), None, None)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn list_quota_source_model_assignments_for_sources(
        &self,
        kind: &str,
        ids: &[String],
    ) -> rusqlite::Result<Vec<QuotaSourceModelAssignment>> {
        if !seaorm_enabled() {
            return self
                .deref()
                .list_quota_source_model_assignments_for_sources(kind, ids);
        }
        let kind = kind.to_owned();
        let ids = ids.to_vec();
        seaorm_block_on(move |storage| async move {
            QuotaConfigurationRepository::assignments(storage.connection(), Some(&kind), Some(&ids))
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
}
