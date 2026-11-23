use codexmanager_core::storage::{ManagedModelReorderV2Params, ManagedModelV2};

pub(crate) fn reorder(input: ManagedModelReorderV2Params) -> Result<Vec<ManagedModelV2>, String> {
    let models = if crate::storage_helpers::seaorm_enabled() {
        let models = super::seaorm::reorder(input)?;
        super::sync_active_gateway_catalog_for_current_backend_best_effort(Vec::new());
        models
    } else {
        let storage = crate::storage_helpers::open_storage()
            .ok_or_else(|| "storage unavailable".to_string())?;
        let models = storage
            .reorder_managed_models_v2(&input)
            .map_err(|err| format!("reorder managed models V2 failed: {err}"))?;
        super::sync_active_gateway_catalog_after_model_changes_best_effort(&storage, Vec::new());
        models
    };
    Ok(super::resolved_capabilities::decorate_models(models, None))
}
