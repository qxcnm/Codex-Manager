use super::remote_storage::AccountStorage;
use crate::storage_helpers::{seaorm_block_on, seaorm_enabled};
use codexmanager_core::storage::{
    ManagedModelRouteEnsureResultV2, ManagedModelRouteEnsureV2, ManagedModelV2,
    ManagedModelV2Upsert,
};
use codexmanager_storage_seaorm::ManagedModelsRepository;
use std::ops::Deref;

fn error(message: String) -> rusqlite::Error {
    rusqlite::Error::SqliteFailure((), Some(message))
}

impl AccountStorage<'_> {
    pub(crate) fn get_managed_model_v2(
        &self,
        slug: &str,
    ) -> rusqlite::Result<Option<ManagedModelV2>> {
        crate::models_v2::managed_model(self.deref(), slug)
    }
    pub(crate) fn get_enabled_model_v2(
        &self,
        slug: &str,
    ) -> rusqlite::Result<Option<ManagedModelV2>> {
        crate::models_v2::enabled_model(self.deref(), slug)
    }
    pub(crate) fn list_managed_models_v2(
        &self,
        include_hidden: bool,
    ) -> rusqlite::Result<Vec<ManagedModelV2>> {
        if !seaorm_enabled() {
            return self.deref().list_managed_models_v2(include_hidden);
        }
        seaorm_block_on(move |s| async move {
            ManagedModelsRepository::list(s.connection(), include_hidden)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn list_api_models_v2(&self) -> rusqlite::Result<Vec<ManagedModelV2>> {
        if !seaorm_enabled() {
            return self.deref().list_api_models_v2();
        }
        self.list_managed_models_v2(false).map(|rows| {
            rows.into_iter()
                .filter(|m| m.enabled && m.supported_in_api)
                .collect()
        })
    }
    pub(crate) fn upsert_managed_models_v2(
        &self,
        inputs: &[ManagedModelV2Upsert],
    ) -> rusqlite::Result<Vec<ManagedModelV2>> {
        if !seaorm_enabled() {
            return self.deref().upsert_managed_models_v2(inputs);
        }
        let inputs = inputs.to_vec();
        seaorm_block_on(move |s| async move {
            ManagedModelsRepository::upsert_many(s.connection(), &inputs)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn upsert_missing_managed_models_and_ensure_routes_v2(
        &self,
        candidates: &[ManagedModelV2Upsert],
        inputs: &[ManagedModelRouteEnsureV2],
    ) -> rusqlite::Result<ManagedModelRouteEnsureResultV2> {
        if !seaorm_enabled() {
            return self
                .deref()
                .upsert_missing_managed_models_and_ensure_routes_v2(candidates, inputs);
        }
        let candidates = candidates.to_vec();
        let inputs = inputs.to_vec();
        seaorm_block_on(move |s| async move {
            ManagedModelsRepository::ensure_routes(s.connection(), &candidates, &inputs)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
}
