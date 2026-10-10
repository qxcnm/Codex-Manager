use codexmanager_core::storage::{ModelPriceTierV2, Storage};

#[derive(Debug, Clone)]
pub(crate) struct CatalogModelPrice {
    pub(crate) model_slug: String,
    pub(crate) provider: String,
    pub(crate) price_status: String,
    pub(crate) tiers: Vec<ModelPriceTierV2>,
}

#[derive(Debug, Clone)]
pub(crate) struct ModelPriceMatch {
    pub(crate) provider: String,
    pub(crate) input_price_per_1m: f64,
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) cached_input_price_per_1m: f64,
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) cache_write_price_per_1m: f64,
    pub(crate) output_price_per_1m: f64,
}

#[cfg(test)]
#[derive(Debug, Clone)]
pub(crate) struct CostEstimate {
    pub(crate) provider: Option<String>,
    pub(crate) cost_usd: Option<f64>,
    pub(crate) price_status: &'static str,
}

pub(crate) fn infer_provider(model_pattern: &str) -> &str {
    let normalized = model_pattern.trim().to_ascii_lowercase();
    if normalized.starts_with("claude") {
        "anthropic"
    } else if normalized.starts_with("gemini") {
        "google"
    } else if normalized.starts_with("gpt")
        || normalized.starts_with('o')
        || normalized.starts_with("codex")
    {
        "openai"
    } else {
        "custom"
    }
}

pub(crate) fn wildcard_matches(pattern: &str, value: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if !pattern.contains('*') {
        return pattern == value;
    }
    let mut remainder = value;
    let mut first = true;
    for part in pattern.split('*').filter(|part| !part.is_empty()) {
        if first && !pattern.starts_with('*') {
            let Some(stripped) = remainder.strip_prefix(part) else {
                return false;
            };
            remainder = stripped;
            first = false;
            continue;
        }
        first = false;
        let Some(index) = remainder.find(part) else {
            return false;
        };
        remainder = &remainder[index + part.len()..];
    }
    pattern.ends_with('*') || remainder.is_empty()
}

pub(crate) fn load_catalog_prices(storage: &Storage) -> Result<Vec<CatalogModelPrice>, String> {
    let storage = crate::account::remote_storage::AccountStorage::new(storage);
    storage
        .list_managed_models_v2(true)
        .map_err(|err| format!("list model catalog V2 prices failed: {err}"))
        .map(|models| {
            models
                .into_iter()
                .map(|model| CatalogModelPrice {
                    provider: model
                        .provider
                        .filter(|value| !value.trim().is_empty())
                        .unwrap_or_else(|| infer_provider(model.slug.as_str()).to_string()),
                    model_slug: model.slug,
                    price_status: model.price.price_status,
                    tiers: model.price_tiers,
                })
                .collect()
        })
}

pub(crate) fn resolve_model_price_from_catalog(
    prices: &[CatalogModelPrice],
    model: &str,
    input_tokens: i64,
) -> Option<ModelPriceMatch> {
    let normalized = model.trim();
    if normalized.is_empty() || normalized.eq_ignore_ascii_case("unknown") {
        return None;
    }
    let catalog_slug = crate::models_v2::policy_catalog_slug(normalized);
    let price = prices
        .iter()
        .find(|price| price.model_slug.eq_ignore_ascii_case(catalog_slug))?;
    if price.price_status == "missing" {
        return None;
    }
    let tier = price
        .tiers
        .iter()
        .filter(|tier| tier.min_input_tokens <= input_tokens.max(0))
        .max_by_key(|tier| tier.min_input_tokens)?;
    Some(ModelPriceMatch {
        provider: price.provider.clone(),
        input_price_per_1m: tier.input_microusd_per_1m as f64 / 1_000_000.0,
        cached_input_price_per_1m: tier.cached_input_microusd_per_1m as f64 / 1_000_000.0,
        cache_write_price_per_1m: tier
            .cache_write_microusd_per_1m
            .unwrap_or(tier.input_microusd_per_1m) as f64
            / 1_000_000.0,
        output_price_per_1m: tier.output_microusd_per_1m as f64 / 1_000_000.0,
    })
}

#[cfg(test)]
fn estimate_cost_from_price(
    price: ModelPriceMatch,
    input_tokens: i64,
    cached_input_tokens: i64,
    output_tokens: i64,
) -> CostEstimate {
    let input_total = input_tokens.max(0) as f64;
    let cached_input = (cached_input_tokens.max(0) as f64).min(input_total);
    let billable_input = (input_total - cached_input).max(0.0);
    let output = output_tokens.max(0) as f64;
    let cost = (billable_input / 1_000_000.0) * price.input_price_per_1m
        + (cached_input / 1_000_000.0) * price.cached_input_price_per_1m
        + (output / 1_000_000.0) * price.output_price_per_1m;
    CostEstimate {
        provider: Some(price.provider),
        cost_usd: Some(cost.max(0.0)),
        price_status: "ok",
    }
}

#[cfg(test)]
pub(crate) fn estimate_cost_with_catalog(
    prices: &[CatalogModelPrice],
    model: Option<&str>,
    input_tokens: i64,
    cached_input_tokens: i64,
    output_tokens: i64,
) -> CostEstimate {
    let Some(model) = model.map(str::trim).filter(|value| !value.is_empty()) else {
        return CostEstimate {
            provider: None,
            cost_usd: None,
            price_status: "missing",
        };
    };
    let Some(price) = resolve_model_price_from_catalog(prices, model, input_tokens) else {
        let provider = prices
            .iter()
            .find(|price| price.model_slug.eq_ignore_ascii_case(model))
            .map(|price| price.provider.clone());
        return CostEstimate {
            provider,
            cost_usd: None,
            price_status: "missing",
        };
    };
    estimate_cost_from_price(price, input_tokens, cached_input_tokens, output_tokens)
}

pub(crate) fn estimate_remaining_tokens_from_usd_with_catalog(
    prices: &[CatalogModelPrice],
    model: &str,
    balance_usd: f64,
) -> Option<i64> {
    if !balance_usd.is_finite() || balance_usd < 0.0 {
        return None;
    }
    if balance_usd == 0.0 {
        return Some(0);
    }
    let price = resolve_model_price_from_catalog(prices, model, 0)?;
    let blended_price_per_1m = price.input_price_per_1m * 0.7 + price.output_price_per_1m * 0.3;
    if blended_price_per_1m <= 0.0 {
        return None;
    }
    Some(((balance_usd / blended_price_per_1m) * 1_000_000.0).floor() as i64)
}

#[cfg(test)]
#[path = "model_pricing_tests.rs"]
mod tests;
