use super::*;
use crate::storage_helpers::seaorm_block_on;
use codexmanager_core::storage::ManagedModelPriceV2Update;
use codexmanager_storage_seaorm::ManagedModelsRepository;

pub(super) fn list(include_hidden: bool) -> Result<ManagedModelListV2Result, String> {
    seaorm_block_on(move |storage| async move {
        let db = storage.connection();
        Ok(ManagedModelListV2Result {
            items: ManagedModelsRepository::list(db, include_hidden)
                .await
                .map_err(|e| e.to_string())?,
            stats: ManagedModelsRepository::stats(db)
                .await
                .map_err(|e| e.to_string())?,
        })
    })
}
pub(super) fn get(slug: &str) -> Result<Option<ManagedModelV2>, String> {
    let slug = slug.to_owned();
    seaorm_block_on(move |storage| async move {
        ManagedModelsRepository::get(storage.connection(), &slug)
            .await
            .map_err(|e| e.to_string())
    })
}
pub(super) fn upsert_many(
    inputs: Vec<ManagedModelV2Upsert>,
) -> Result<Vec<ManagedModelV2>, String> {
    seaorm_block_on(move |storage| async move {
        ManagedModelsRepository::upsert_many(storage.connection(), &inputs)
            .await
            .map_err(|e| e.to_string())
    })
}
pub(super) fn update_prices(
    updates: Vec<ManagedModelPriceV2Update>,
    allow_custom_override: bool,
) -> Result<Vec<String>, String> {
    seaorm_block_on(move |storage| async move {
        ManagedModelsRepository::update_prices_with_custom_override(
            storage.connection(),
            &updates,
            allow_custom_override,
        )
        .await
        .map_err(|e| e.to_string())
    })
}
pub(super) fn update_states(
    input: ManagedModelBatchStateV2Update,
) -> Result<Vec<ManagedModelV2>, String> {
    seaorm_block_on(move |storage| async move {
        ManagedModelsRepository::update_states(storage.connection(), &input)
            .await
            .map_err(|e| e.to_string())
    })
}
pub(super) fn delete(slug: &str) -> Result<(), String> {
    let slug = slug.to_owned();
    seaorm_block_on(move |storage| async move {
        ManagedModelsRepository::delete(storage.connection(), &slug)
            .await
            .map_err(|e| e.to_string())
    })
}
