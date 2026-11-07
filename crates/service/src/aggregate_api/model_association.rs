//! Append model discoveries without writing back a stale catalog snapshot.
use std::collections::{BTreeMap, HashSet};

use codexmanager_core::rpc::types::AggregateApiAssociateModelsResult;
use codexmanager_core::storage::{
    AggregateApi, ManagedModelRouteEnsureV2, ManagedModelV2, ManagedModelV2Upsert,
    ModelFastPolicyV2, ModelPriceV2, ModelRouteV2,
};
use serde_json::json;

use super::{
    normalize_fetched_model_id, normalize_provider_type_value, AGGREGATE_API_PROVIDER_GEMINI,
};

struct AssociationPlan {
    candidates: Vec<ManagedModelV2Upsert>,
    routes: Vec<ManagedModelRouteEnsureV2>,
}

pub(super) fn associate(
    api_id: &str,
    upstream_models: Vec<String>,
    display_names: BTreeMap<String, String>,
) -> Result<AggregateApiAssociateModelsResult, String> {
    let api_id = api_id.trim();
    if api_id.is_empty() {
        return Err("aggregate api id required".to_owned());
    }
    let storage =
        crate::storage_helpers::open_storage().ok_or_else(|| "storage unavailable".to_owned())?;
    let storage = crate::account::remote_storage::AccountStorage::new(&storage);
    let api = storage
        .find_aggregate_api_by_id(api_id)
        .map_err(|err| err.to_string())?
        .ok_or_else(|| "aggregate api not found".to_owned())?;
    let models = storage
        .list_managed_models_v2(true)
        .map_err(|err| format!("read model catalog V2 failed: {err}"))?;
    let plan = prepare(&api, &models, upstream_models, &display_names)?;
    let result = storage
        .upsert_missing_managed_models_and_ensure_routes_v2(&plan.candidates, &plan.routes)
        .map_err(|err| format!("associate models transaction failed: {err}"))?;
    Ok(AggregateApiAssociateModelsResult {
        created_models: result.created_models,
        added_routes: result.added_routes,
        unchanged_routes: result.unchanged_routes,
    })
}

fn prepare(
    api: &AggregateApi,
    models: &[ManagedModelV2],
    upstream_models: Vec<String>,
    display_names: &BTreeMap<String, String>,
) -> Result<AssociationPlan, String> {
    let provider_type = normalize_provider_type_value(&api.provider_type);
    let mut requested = Vec::new();
    let mut seen = HashSet::new();
    for raw in upstream_models {
        let model =
            normalize_fetched_model_id(&raw, provider_type == AGGREGATE_API_PROVIDER_GEMINI)
                .ok_or_else(|| "invalid upstream model id".to_owned())?;
        if seen.insert(model.to_ascii_lowercase()) {
            requested.push(model);
        }
    }
    let next_sort = models
        .iter()
        .map(|model| model.sort_order)
        .max()
        .unwrap_or(0)
        + 1;
    let provider = api
        .supplier_name
        .clone()
        .filter(|value| !value.trim().is_empty())
        .or(Some(provider_type));
    let mut plan = AssociationPlan {
        candidates: Vec::new(),
        routes: Vec::new(),
    };
    for (index, upstream_model) in requested.into_iter().enumerate() {
        let existing = models
            .iter()
            .find(|model| model.slug.eq_ignore_ascii_case(&upstream_model));
        let (slug, priority, weight) = if let Some(model) = existing {
            let (priority, weight) = model
                .routes
                .iter()
                .find(|route| route.source_kind == "aggregate_api" && route.source_id == api.id)
                .map(|route| (route.priority, route.weight.max(1)))
                .unwrap_or((0, 1));
            (model.slug.clone(), priority, weight)
        } else {
            let display_name = display_names
                .iter()
                .find(|(key, _)| key.eq_ignore_ascii_case(&upstream_model))
                .map(|(_, value)| value.trim().to_owned())
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| upstream_model.clone());
            plan.candidates.push(ManagedModelV2Upsert {
                model: ManagedModelV2 {
                    slug: upstream_model.clone(),
                    display_name,
                    provider: provider.clone(),
                    origin: "custom".to_owned(),
                    enabled: true,
                    supported_in_api: true,
                    visibility: "list".to_owned(),
                    sort_order: next_sort + index as i64,
                    capabilities: json!({
                        "supports_text_generation": true,
                        "input_modalities": ["text"],
                        "output_modalities": ["text"]
                    }),
                    instructions_mode: "passthrough".to_owned(),
                    fast_policy: ModelFastPolicyV2::Passthrough,
                    price: ModelPriceV2 {
                        price_status: "missing".to_owned(),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                ..Default::default()
            });
            (upstream_model.clone(), 0, 1)
        };
        plan.routes.push(ManagedModelRouteEnsureV2 {
            model_slug: slug,
            route: ModelRouteV2 {
                source_kind: "aggregate_api".to_owned(),
                source_id: api.id.clone(),
                upstream_model,
                enabled: true,
                priority,
                weight,
                ..Default::default()
            },
        });
    }
    Ok(plan)
}

#[cfg(test)]
#[path = "model_association_tests.rs"]
mod tests;
