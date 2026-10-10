use super::remote_storage::AccountStorage;
use crate::storage_helpers::{seaorm_block_on, seaorm_enabled};
use codexmanager_core::storage::*;
use codexmanager_storage_seaorm::AggregateApisRepository;
use std::{collections::HashMap, ops::Deref};
fn storage_error(message: String) -> rusqlite::Error {
    rusqlite::Error::SqliteFailure((), Some(message))
}
// Keep the full dual-backend adapter surface available for SeaORM runtime selection.
#[allow(dead_code)]
impl AccountStorage<'_> {
    pub(crate) fn insert_aggregate_api(&self, api: &AggregateApi) -> rusqlite::Result<()> {
        if !seaorm_enabled() {
            return self.deref().insert_aggregate_api(api);
        }
        let api = api.clone();
        seaorm_block_on(move |remote| async move {
            AggregateApisRepository::insert_aggregate_api(remote.connection(), &api)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn list_aggregate_apis(&self) -> rusqlite::Result<Vec<AggregateApi>> {
        if !seaorm_enabled() {
            return self.deref().list_aggregate_apis();
        }
        seaorm_block_on(move |remote| async move {
            AggregateApisRepository::list_aggregate_apis(remote.connection())
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn find_aggregate_api_by_id(
        &self,
        id: &str,
    ) -> rusqlite::Result<Option<AggregateApi>> {
        if !seaorm_enabled() {
            return self.deref().find_aggregate_api_by_id(id);
        }
        let id = id.to_owned();
        seaorm_block_on(move |remote| async move {
            AggregateApisRepository::find_aggregate_api_by_id(remote.connection(), &id)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn aggregate_api_exists(&self, id: &str) -> rusqlite::Result<bool> {
        if !seaorm_enabled() {
            return self.deref().aggregate_api_exists(id);
        }
        let id = id.to_owned();
        seaorm_block_on(move |remote| async move {
            AggregateApisRepository::aggregate_api_exists(remote.connection(), &id)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn list_aggregate_apis_for_ids(
        &self,
        ids: &[String],
    ) -> rusqlite::Result<Vec<AggregateApi>> {
        if !seaorm_enabled() {
            return self.deref().list_aggregate_apis_for_ids(ids);
        }
        let ids = ids.to_vec();
        seaorm_block_on(move |remote| async move {
            AggregateApisRepository::list_aggregate_apis_for_ids(remote.connection(), &ids)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn list_aggregate_api_summaries(
        &self,
    ) -> rusqlite::Result<Vec<AggregateApiListSummary>> {
        if !seaorm_enabled() {
            return self.deref().list_aggregate_api_summaries();
        }
        seaorm_block_on(move |remote| async move {
            AggregateApisRepository::list_aggregate_api_summaries(remote.connection())
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn list_aggregate_api_quota_source_summaries(
        &self,
    ) -> rusqlite::Result<Vec<AggregateApiQuotaSourceSummary>> {
        if !seaorm_enabled() {
            return self.deref().list_aggregate_api_quota_source_summaries();
        }
        seaorm_block_on(move |remote| async move {
            AggregateApisRepository::list_aggregate_api_quota_source_summaries(remote.connection())
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn list_aggregate_api_dashboard_source_metadata_for_ids(
        &self,
        ids: &[String],
    ) -> rusqlite::Result<Vec<AggregateApiDashboardSourceMetadata>> {
        if !seaorm_enabled() {
            return self
                .deref()
                .list_aggregate_api_dashboard_source_metadata_for_ids(ids);
        }
        let ids = ids.to_vec();
        seaorm_block_on(move |remote| async move {
            AggregateApisRepository::list_aggregate_api_dashboard_source_metadata_for_ids(
                remote.connection(),
                &ids,
            )
            .await
            .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn find_aggregate_api_supplier_identity_by_id(
        &self,
        id: &str,
    ) -> rusqlite::Result<Option<AggregateApiSupplierIdentity>> {
        if !seaorm_enabled() {
            return self.deref().find_aggregate_api_supplier_identity_by_id(id);
        }
        let id = id.to_owned();
        seaorm_block_on(move |remote| async move {
            AggregateApisRepository::find_aggregate_api_supplier_identity_by_id(
                remote.connection(),
                &id,
            )
            .await
            .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn find_aggregate_api_update_config_by_id(
        &self,
        id: &str,
    ) -> rusqlite::Result<Option<AggregateApiUpdateConfig>> {
        if !seaorm_enabled() {
            return self.deref().find_aggregate_api_update_config_by_id(id);
        }
        let id = id.to_owned();
        seaorm_block_on(move |remote| async move {
            AggregateApisRepository::find_aggregate_api_update_config_by_id(
                remote.connection(),
                &id,
            )
            .await
            .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn find_aggregate_api_status_by_id(
        &self,
        id: &str,
    ) -> rusqlite::Result<Option<String>> {
        if !seaorm_enabled() {
            return self.deref().find_aggregate_api_status_by_id(id);
        }
        let id = id.to_owned();
        seaorm_block_on(move |remote| async move {
            AggregateApisRepository::find_aggregate_api_status_by_id(remote.connection(), &id)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn find_aggregate_api_auth_type_by_id(
        &self,
        id: &str,
    ) -> rusqlite::Result<Option<String>> {
        if !seaorm_enabled() {
            return self.deref().find_aggregate_api_auth_type_by_id(id);
        }
        let id = id.to_owned();
        seaorm_block_on(move |remote| async move {
            AggregateApisRepository::find_aggregate_api_auth_type_by_id(remote.connection(), &id)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn list_aggregate_api_ids(&self) -> rusqlite::Result<Vec<String>> {
        if !seaorm_enabled() {
            return self.deref().list_aggregate_api_ids();
        }
        seaorm_block_on(move |remote| async move {
            AggregateApisRepository::list_aggregate_api_ids(remote.connection())
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn list_active_aggregate_api_ids(&self) -> rusqlite::Result<Vec<String>> {
        if !seaorm_enabled() {
            return self.deref().list_active_aggregate_api_ids();
        }
        seaorm_block_on(move |remote| async move {
            AggregateApisRepository::list_active_aggregate_api_ids(remote.connection())
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn list_balance_query_aggregate_api_ids(&self) -> rusqlite::Result<Vec<String>> {
        if !seaorm_enabled() {
            return self.deref().list_balance_query_aggregate_api_ids();
        }
        seaorm_block_on(move |remote| async move {
            AggregateApisRepository::list_balance_query_aggregate_api_ids(remote.connection())
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn list_active_balance_query_aggregate_api_ids(
        &self,
    ) -> rusqlite::Result<Vec<String>> {
        if !seaorm_enabled() {
            return self.deref().list_active_balance_query_aggregate_api_ids();
        }
        seaorm_block_on(move |remote| async move {
            AggregateApisRepository::list_active_balance_query_aggregate_api_ids(
                remote.connection(),
            )
            .await
            .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn list_balance_query_aggregate_api_ids_for_ids(
        &self,
        ids: &[String],
    ) -> rusqlite::Result<Vec<String>> {
        if !seaorm_enabled() {
            return self
                .deref()
                .list_balance_query_aggregate_api_ids_for_ids(ids);
        }
        let ids = ids.to_vec();
        seaorm_block_on(move |remote| async move {
            AggregateApisRepository::list_balance_query_aggregate_api_ids_for_ids(
                remote.connection(),
                &ids,
            )
            .await
            .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn list_active_aggregate_apis(&self) -> rusqlite::Result<Vec<AggregateApi>> {
        if !seaorm_enabled() {
            return self.deref().list_active_aggregate_apis();
        }
        seaorm_block_on(move |remote| async move {
            AggregateApisRepository::list_active_aggregate_apis(remote.connection())
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn list_active_aggregate_apis_by_provider_type(
        &self,
        kind: &str,
    ) -> rusqlite::Result<Vec<AggregateApi>> {
        if !seaorm_enabled() {
            return self
                .deref()
                .list_active_aggregate_apis_by_provider_type(kind);
        }
        let kind = kind.to_owned();
        seaorm_block_on(move |remote| async move {
            AggregateApisRepository::list_active_aggregate_apis_by_provider_type(
                remote.connection(),
                &kind,
            )
            .await
            .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn list_aggregate_api_balance_jsons(&self) -> rusqlite::Result<Vec<String>> {
        if !seaorm_enabled() {
            return self.deref().list_aggregate_api_balance_jsons();
        }
        seaorm_block_on(move |remote| async move {
            AggregateApisRepository::list_aggregate_api_balance_jsons(remote.connection())
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn aggregate_api_overview_stats(
        &self,
    ) -> rusqlite::Result<AggregateApiOverviewStats> {
        if !seaorm_enabled() {
            return self.deref().aggregate_api_overview_stats();
        }
        seaorm_block_on(move |remote| async move {
            AggregateApisRepository::aggregate_api_overview_stats(remote.connection())
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn update_aggregate_api(&self, id: &str, value: &str) -> rusqlite::Result<()> {
        if !seaorm_enabled() {
            return self.deref().update_aggregate_api(id, value);
        }
        let id = id.to_owned();
        let value = value.to_owned();
        seaorm_block_on(move |remote| async move {
            AggregateApisRepository::update_aggregate_api(remote.connection(), &id, &value)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn update_aggregate_api_supplier_name(
        &self,
        id: &str,
        value: Option<&str>,
    ) -> rusqlite::Result<()> {
        if !seaorm_enabled() {
            return self.deref().update_aggregate_api_supplier_name(id, value);
        }
        let id = id.to_owned();
        let value = value.map(str::to_owned);
        seaorm_block_on(move |remote| async move {
            AggregateApisRepository::update_aggregate_api_supplier_name(
                remote.connection(),
                &id,
                value.as_deref(),
            )
            .await
            .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn update_aggregate_api_sort(&self, id: &str, value: i64) -> rusqlite::Result<()> {
        if !seaorm_enabled() {
            return self.deref().update_aggregate_api_sort(id, value);
        }
        let id = id.to_owned();
        seaorm_block_on(move |remote| async move {
            AggregateApisRepository::update_aggregate_api_sort(remote.connection(), &id, value)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn update_aggregate_api_status(
        &self,
        id: &str,
        value: &str,
    ) -> rusqlite::Result<()> {
        if !seaorm_enabled() {
            return self.deref().update_aggregate_api_status(id, value);
        }
        let id = id.to_owned();
        let value = value.to_owned();
        seaorm_block_on(move |remote| async move {
            AggregateApisRepository::update_aggregate_api_status(remote.connection(), &id, &value)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn update_aggregate_api_type(&self, id: &str, value: &str) -> rusqlite::Result<()> {
        if !seaorm_enabled() {
            return self.deref().update_aggregate_api_type(id, value);
        }
        let id = id.to_owned();
        let value = value.to_owned();
        seaorm_block_on(move |remote| async move {
            AggregateApisRepository::update_aggregate_api_type(remote.connection(), &id, &value)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn update_aggregate_api_auth_type(
        &self,
        id: &str,
        value: &str,
    ) -> rusqlite::Result<()> {
        if !seaorm_enabled() {
            return self.deref().update_aggregate_api_auth_type(id, value);
        }
        let id = id.to_owned();
        let value = value.to_owned();
        seaorm_block_on(move |remote| async move {
            AggregateApisRepository::update_aggregate_api_auth_type(
                remote.connection(),
                &id,
                &value,
            )
            .await
            .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn update_aggregate_api_auth_params_json(
        &self,
        id: &str,
        value: Option<&str>,
    ) -> rusqlite::Result<()> {
        if !seaorm_enabled() {
            return self
                .deref()
                .update_aggregate_api_auth_params_json(id, value);
        }
        let id = id.to_owned();
        let value = value.map(str::to_owned);
        seaorm_block_on(move |remote| async move {
            AggregateApisRepository::update_aggregate_api_auth_params_json(
                remote.connection(),
                &id,
                value.as_deref(),
            )
            .await
            .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn update_aggregate_api_action(
        &self,
        id: &str,
        value: Option<&str>,
    ) -> rusqlite::Result<()> {
        if !seaorm_enabled() {
            return self.deref().update_aggregate_api_action(id, value);
        }
        let id = id.to_owned();
        let value = value.map(str::to_owned);
        seaorm_block_on(move |remote| async move {
            AggregateApisRepository::update_aggregate_api_action(
                remote.connection(),
                &id,
                value.as_deref(),
            )
            .await
            .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn update_aggregate_api_model_override(
        &self,
        id: &str,
        value: Option<&str>,
    ) -> rusqlite::Result<()> {
        if !seaorm_enabled() {
            return self.deref().update_aggregate_api_model_override(id, value);
        }
        let id = id.to_owned();
        let value = value.map(str::to_owned);
        seaorm_block_on(move |remote| async move {
            AggregateApisRepository::update_aggregate_api_model_override(
                remote.connection(),
                &id,
                value.as_deref(),
            )
            .await
            .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn update_aggregate_api_user_agent(
        &self,
        id: &str,
        value: Option<&str>,
    ) -> rusqlite::Result<()> {
        if !seaorm_enabled() {
            return self.deref().update_aggregate_api_user_agent(id, value);
        }
        let id = id.to_owned();
        let value = value.map(str::to_owned);
        seaorm_block_on(move |remote| async move {
            AggregateApisRepository::update_aggregate_api_user_agent(
                remote.connection(),
                &id,
                value.as_deref(),
            )
            .await
            .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn upsert_aggregate_api_secret(
        &self,
        id: &str,
        value: &str,
    ) -> rusqlite::Result<()> {
        if !seaorm_enabled() {
            return self.deref().upsert_aggregate_api_secret(id, value);
        }
        let id = id.to_owned();
        let value = value.to_owned();
        seaorm_block_on(move |remote| async move {
            AggregateApisRepository::upsert_aggregate_api_secret(remote.connection(), &id, &value)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn find_aggregate_api_secret_by_id(
        &self,
        id: &str,
    ) -> rusqlite::Result<Option<String>> {
        if !seaorm_enabled() {
            return self.deref().find_aggregate_api_secret_by_id(id);
        }
        let id = id.to_owned();
        seaorm_block_on(move |remote| async move {
            AggregateApisRepository::find_aggregate_api_secret_by_id(remote.connection(), &id)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn upsert_aggregate_api_balance_secret(
        &self,
        id: &str,
        value: &str,
    ) -> rusqlite::Result<()> {
        if !seaorm_enabled() {
            return self.deref().upsert_aggregate_api_balance_secret(id, value);
        }
        let id = id.to_owned();
        let value = value.to_owned();
        seaorm_block_on(move |remote| async move {
            AggregateApisRepository::upsert_aggregate_api_balance_secret(
                remote.connection(),
                &id,
                &value,
            )
            .await
            .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn find_aggregate_api_balance_secret_by_id(
        &self,
        id: &str,
    ) -> rusqlite::Result<Option<String>> {
        if !seaorm_enabled() {
            return self.deref().find_aggregate_api_balance_secret_by_id(id);
        }
        let id = id.to_owned();
        seaorm_block_on(move |remote| async move {
            AggregateApisRepository::find_aggregate_api_balance_secret_by_id(
                remote.connection(),
                &id,
            )
            .await
            .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn delete_aggregate_api_balance_secret(&self, id: &str) -> rusqlite::Result<()> {
        if !seaorm_enabled() {
            return self.deref().delete_aggregate_api_balance_secret(id);
        }
        let id = id.to_owned();
        seaorm_block_on(move |remote| async move {
            AggregateApisRepository::delete_aggregate_api_balance_secret(remote.connection(), &id)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn list_aggregate_api_secrets_for_ids(
        &self,
        ids: &[String],
    ) -> rusqlite::Result<HashMap<String, String>> {
        if !seaorm_enabled() {
            return self.deref().list_aggregate_api_secrets_for_ids(ids);
        }
        let ids = ids.to_vec();
        seaorm_block_on(move |remote| async move {
            AggregateApisRepository::list_aggregate_api_secrets_for_ids(remote.connection(), &ids)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn find_aggregate_api_with_secrets_by_id(
        &self,
        id: &str,
    ) -> rusqlite::Result<Option<AggregateApiWithSecrets>> {
        if !seaorm_enabled() {
            return self.deref().find_aggregate_api_with_secrets_by_id(id);
        }
        let id = id.to_owned();
        seaorm_block_on(move |remote| async move {
            AggregateApisRepository::find_aggregate_api_with_secrets_by_id(remote.connection(), &id)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn find_aggregate_api_secret_config_by_id(
        &self,
        id: &str,
    ) -> rusqlite::Result<Option<AggregateApiSecretConfig>> {
        if !seaorm_enabled() {
            return self.deref().find_aggregate_api_secret_config_by_id(id);
        }
        let id = id.to_owned();
        seaorm_block_on(move |remote| async move {
            AggregateApisRepository::find_aggregate_api_secret_config_by_id(
                remote.connection(),
                &id,
            )
            .await
            .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn delete_aggregate_api(&self, id: &str) -> rusqlite::Result<()> {
        if !seaorm_enabled() {
            return self.deref().delete_aggregate_api(id);
        }
        let id = id.to_owned();
        seaorm_block_on(move |remote| async move {
            AggregateApisRepository::delete_aggregate_api(remote.connection(), &id)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn update_aggregate_api_balance_query(
        &self,
        id: &str,
        enabled: bool,
        template: Option<&str>,
        base_url: Option<&str>,
        user_id: Option<&str>,
        config_json: Option<&str>,
    ) -> rusqlite::Result<()> {
        if !seaorm_enabled() {
            return self.deref().update_aggregate_api_balance_query(
                id,
                enabled,
                template,
                base_url,
                user_id,
                config_json,
            );
        }
        let id = id.to_owned();
        let template = template.map(str::to_owned);
        let base_url = base_url.map(str::to_owned);
        let user_id = user_id.map(str::to_owned);
        let config_json = config_json.map(str::to_owned);
        seaorm_block_on(move |remote| async move {
            AggregateApisRepository::update_aggregate_api_balance_query(
                remote.connection(),
                &id,
                enabled,
                template.as_deref(),
                base_url.as_deref(),
                user_id.as_deref(),
                config_json.as_deref(),
            )
            .await
            .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn update_aggregate_api_balance_result(
        &self,
        id: &str,
        ok: bool,
        balance_json: Option<&str>,
        error: Option<&str>,
    ) -> rusqlite::Result<()> {
        if !seaorm_enabled() {
            return self
                .deref()
                .update_aggregate_api_balance_result(id, ok, balance_json, error);
        }
        let id = id.to_owned();
        let balance_json = balance_json.map(str::to_owned);
        let error = error.map(str::to_owned);
        seaorm_block_on(move |remote| async move {
            AggregateApisRepository::update_aggregate_api_balance_result(
                remote.connection(),
                &id,
                ok,
                balance_json.as_deref(),
                error.as_deref(),
            )
            .await
            .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn update_aggregate_api_test_result(
        &self,
        id: &str,
        ok: bool,
        status_code: Option<i64>,
        error: Option<&str>,
    ) -> rusqlite::Result<()> {
        if !seaorm_enabled() {
            return self
                .deref()
                .update_aggregate_api_test_result(id, ok, status_code, error);
        }
        let id = id.to_owned();
        let error = error.map(str::to_owned);
        seaorm_block_on(move |remote| async move {
            AggregateApisRepository::update_aggregate_api_test_result(
                remote.connection(),
                &id,
                ok,
                status_code,
                error.as_deref(),
            )
            .await
            .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn list_aggregate_api_supplier_models(
        &self,
        supplier: Option<&str>,
        provider: Option<&str>,
    ) -> rusqlite::Result<Vec<AggregateApiSupplierModel>> {
        if !seaorm_enabled() {
            return self
                .deref()
                .list_aggregate_api_supplier_models(supplier, provider);
        }
        let supplier = supplier.map(str::to_owned);
        let provider = provider.map(str::to_owned);
        seaorm_block_on(move |remote| async move {
            AggregateApisRepository::list_aggregate_api_supplier_models(
                remote.connection(),
                supplier.as_deref(),
                provider.as_deref(),
            )
            .await
            .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn upsert_aggregate_api_supplier_model(
        &self,
        model: &AggregateApiSupplierModel,
    ) -> rusqlite::Result<()> {
        if !seaorm_enabled() {
            return self.deref().upsert_aggregate_api_supplier_model(model);
        }
        let model = model.clone();
        seaorm_block_on(move |remote| async move {
            AggregateApisRepository::upsert_aggregate_api_supplier_model(
                remote.connection(),
                &model,
            )
            .await
            .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn delete_aggregate_api_supplier_model(
        &self,
        supplier: &str,
        provider: &str,
        upstream: &str,
    ) -> rusqlite::Result<()> {
        if !seaorm_enabled() {
            return self
                .deref()
                .delete_aggregate_api_supplier_model(supplier, provider, upstream);
        }
        let supplier = supplier.to_owned();
        let provider = provider.to_owned();
        let upstream = upstream.to_owned();
        seaorm_block_on(move |remote| async move {
            AggregateApisRepository::delete_aggregate_api_supplier_model(
                remote.connection(),
                &supplier,
                &provider,
                &upstream,
            )
            .await
            .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn load_aggregate_api_list_snapshot(
        &self,
    ) -> rusqlite::Result<AggregateApiListSnapshot> {
        let items = self.list_aggregate_api_summaries()?;
        let ids = items.iter().map(|a| a.id.clone()).collect::<Vec<_>>();
        let model_assignments =
            self.list_quota_source_model_assignments_for_sources("aggregate_api", &ids)?;
        Ok(AggregateApiListSnapshot {
            items,
            model_assignments,
        })
    }
}
