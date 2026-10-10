use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::time::Duration;

use codexmanager_core::storage::{
    ManagedModelPriceV2Update, ManagedModelV2, ModelPriceTierV2, ModelPriceV2,
};
use futures_util::StreamExt;
use reqwest::header::ACCEPT;
use serde::{Deserialize, Serialize};
use serde_json::Number;

pub(crate) const BASELLM_PRICE_SOURCE_URL: &str =
    "https://basellm.github.io/llm-metadata/api/all.json";
pub(crate) const MODELS_DEV_PRICE_SOURCE_URL: &str = "https://models.dev/api.json";
const SOURCE_RESPONSE_LIMIT_BYTES: usize = 8 * 1024 * 1024;
const SOURCE_REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const SOURCE_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

static PRICE_SYNC_PERMIT: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(1);

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ModelPriceSourceSyncSummary {
    pub name: String,
    pub url: String,
    pub status: String,
    pub providers: usize,
    pub models_seen: usize,
    pub priced_models: usize,
    pub skipped_non_usd: usize,
    pub skipped_missing_text_price: usize,
    pub skipped_subscription: usize,
    pub skipped_tiers: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ManagedModelPriceSyncV2Result {
    pub sources: Vec<ModelPriceSourceSyncSummary>,
    pub catalog_prices: usize,
    pub scanned_models: usize,
    pub updated: usize,
    pub unchanged: usize,
    pub preserved_custom: usize,
    pub unmatched: usize,
    pub ambiguous: usize,
    pub updated_slugs: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ManagedModelPriceSyncV2Params {
    #[serde(default, alias = "model_slugs")]
    pub model_slugs: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct SourceProvider {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    subscription: bool,
    #[serde(default)]
    models: BTreeMap<String, SourceModel>,
}

#[derive(Debug, Deserialize)]
struct SourceModel {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    cost: Option<SourceCost>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct SourceRates {
    #[serde(default)]
    input: Option<Number>,
    #[serde(default)]
    output: Option<Number>,
    #[serde(default)]
    cache_read: Option<Number>,
    #[serde(default)]
    cache_write: Option<Number>,
}

#[derive(Debug, Deserialize)]
struct SourceCost {
    #[serde(flatten)]
    rates: SourceRates,
    #[serde(default)]
    currency: Option<String>,
    #[serde(default)]
    tiers: Vec<SourceTier>,
    #[serde(default)]
    context_over_200k: Option<SourceRates>,
}

#[derive(Debug, Deserialize)]
struct SourceTier {
    #[serde(flatten)]
    rates: SourceRates,
    tier: SourceTierCondition,
}

#[derive(Debug, Deserialize)]
struct SourceTierCondition {
    #[serde(rename = "type")]
    kind: String,
    size: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NormalizedPrice {
    source_rank: u8,
    provider: String,
    provider_aliases: BTreeSet<String>,
    model: String,
    model_aliases: BTreeSet<String>,
    price: ModelPriceV2,
    price_tiers: Vec<ModelPriceTierV2>,
}

#[derive(Debug)]
struct ParsedSource {
    records: BTreeMap<(String, String), NormalizedPrice>,
    blocked_keys: HashSet<(String, String)>,
    summary: ModelPriceSourceSyncSummary,
}

#[derive(Debug, Default)]
struct PriceIndex {
    records: Vec<NormalizedPrice>,
    by_provider_model: HashMap<(String, String), BTreeSet<usize>>,
    by_model: HashMap<String, BTreeSet<usize>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MatchResult {
    Found(usize),
    Missing,
    Ambiguous,
}

impl PriceIndex {
    fn from_records(records: BTreeMap<(String, String), NormalizedPrice>) -> Self {
        let records = records.into_values().collect::<Vec<_>>();
        let mut index = Self {
            records,
            ..Self::default()
        };
        for (record_index, record) in index.records.iter().enumerate() {
            for provider in &record.provider_aliases {
                for model in &record.model_aliases {
                    index
                        .by_provider_model
                        .entry((provider.clone(), model.clone()))
                        .or_default()
                        .insert(record_index);
                }
            }
            for model in &record.model_aliases {
                index
                    .by_model
                    .entry(model.clone())
                    .or_default()
                    .insert(record_index);
            }
        }
        index
    }

    fn provider_matches(&self, provider: &str, model: &str) -> BTreeSet<usize> {
        self.by_provider_model
            .get(&(normalize_provider(provider), normalize_model(model)))
            .cloned()
            .unwrap_or_default()
    }

    fn global_matches(&self, model: &str) -> BTreeSet<usize> {
        self.by_model
            .get(&normalize_model(model))
            .cloned()
            .unwrap_or_default()
    }

    fn resolve_candidates(&self, candidates: BTreeSet<usize>) -> MatchResult {
        let Some(best_rank) = candidates
            .iter()
            .map(|index| self.records[*index].source_rank)
            .min()
        else {
            return MatchResult::Missing;
        };
        let mut best = candidates
            .into_iter()
            .filter(|index| self.records[*index].source_rank == best_rank);
        let Some(first) = best.next() else {
            return MatchResult::Missing;
        };
        if best.next().is_some() {
            MatchResult::Ambiguous
        } else {
            MatchResult::Found(first)
        }
    }

    fn match_model(&self, model: &ManagedModelV2) -> MatchResult {
        let explicit_provider = model
            .provider
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let provider_hint =
            explicit_provider.or_else(|| (model.origin == "builtin").then_some("openai"));
        if let Some(provider) = provider_hint {
            let exact = self.provider_matches(provider, &model.slug);
            match self.resolve_candidates(exact) {
                MatchResult::Missing => {}
                result => return result,
            }
        }

        let mut provider_candidates = BTreeSet::new();
        let mut global_candidates = BTreeSet::new();
        for route in model.routes.iter().filter(|route| route.enabled) {
            let upstream = route.upstream_model.trim();
            if upstream.is_empty() {
                continue;
            }
            if let Some(provider) = provider_hint {
                provider_candidates.extend(self.provider_matches(provider, upstream));
            }
            if let Some((provider, provider_model)) = upstream.split_once('/') {
                provider_candidates.extend(self.provider_matches(provider, provider_model));
            }
            global_candidates.extend(self.global_matches(upstream));
        }
        match self.resolve_candidates(provider_candidates) {
            MatchResult::Missing if explicit_provider.is_none() => {
                self.resolve_candidates(global_candidates)
            }
            MatchResult::Missing => MatchResult::Missing,
            result => result,
        }
    }
}

pub(crate) async fn sync_prices(
    params: ManagedModelPriceSyncV2Params,
) -> Result<ManagedModelPriceSyncV2Result, String> {
    let _permit = PRICE_SYNC_PERMIT
        .try_acquire()
        .map_err(|_| "managed model price sync is already running".to_string())?;
    let models = crate::runtime::blocking::run("model-price-sync-list", || super::list(true))
        .await??
        .items;
    let allow_custom_override = params
        .model_slugs
        .iter()
        .map(|slug| normalize_model(slug))
        .any(|slug| !slug.is_empty());
    let models = select_models_for_price_sync(&models, &params.model_slugs)?;
    let client = reqwest::Client::builder()
        .connect_timeout(SOURCE_CONNECT_TIMEOUT)
        .timeout(SOURCE_REQUEST_TIMEOUT)
        .user_agent(format!(
            "CodexManager/{}/model-price-sync",
            codexmanager_core::core_version()
        ))
        .build()
        .map_err(|error| format!("build model price sync client failed: {error}"))?;

    let (primary, fallback) = tokio::join!(
        load_source(&client, "basellm", BASELLM_PRICE_SOURCE_URL),
        load_source(&client, "models.dev", MODELS_DEV_PRICE_SOURCE_URL)
    );
    let (index, sources) = merge_source_results(primary, fallback)?;
    let (updates, mut result) =
        build_update_plan_with_options(&models, &index, sources, allow_custom_override);
    if updates.is_empty() {
        return Ok(result);
    }
    let applied = crate::runtime::blocking::run("model-price-sync-write", move || {
        persist_price_updates(updates, allow_custom_override)
    })
    .await??;
    let planned = result.updated;
    result.updated_slugs = applied;
    result
        .updated_slugs
        .sort_by_key(|slug| slug.to_ascii_lowercase());
    result.updated = result.updated_slugs.len();
    result.preserved_custom += planned.saturating_sub(result.updated);
    Ok(result)
}

fn select_models_for_price_sync(
    models: &[ManagedModelV2],
    requested_slugs: &[String],
) -> Result<Vec<ManagedModelV2>, String> {
    let requested = requested_slugs
        .iter()
        .map(|slug| normalize_model(slug))
        .filter(|slug| !slug.is_empty())
        .collect::<BTreeSet<_>>();
    if requested.is_empty() {
        return Ok(models.to_vec());
    }

    let mut found = BTreeSet::new();
    let selected = models
        .iter()
        .filter(|model| {
            let normalized = normalize_model(&model.slug);
            if requested.contains(&normalized) {
                found.insert(normalized);
                true
            } else {
                false
            }
        })
        .cloned()
        .collect::<Vec<_>>();
    let missing = requested.difference(&found).cloned().collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(format!("managed model not found: {}", missing.join(", ")));
    }
    Ok(selected)
}

fn persist_price_updates(
    updates: Vec<ManagedModelPriceV2Update>,
    allow_custom_override: bool,
) -> Result<Vec<String>, String> {
    if crate::storage_helpers::seaorm_enabled() {
        return super::seaorm::update_prices(updates, allow_custom_override);
    }
    let storage =
        crate::storage_helpers::open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    storage
        .update_managed_model_prices_v2_with_custom_override(&updates, allow_custom_override)
        .map_err(|error| format!("update managed model prices failed: {error}"))
}

async fn load_source(
    client: &reqwest::Client,
    name: &str,
    url: &str,
) -> Result<ParsedSource, String> {
    let bytes = fetch_source_body(client, url).await?;
    parse_source_body(name, url, &bytes)
}

async fn fetch_source_body(client: &reqwest::Client, url: &str) -> Result<Vec<u8>, String> {
    let response = client
        .get(url)
        .header(ACCEPT, "application/json")
        .send()
        .await
        .map_err(|error| format!("fetch {url} failed: {error}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "fetch {url} failed with status {}",
            response.status()
        ));
    }
    if response
        .content_length()
        .is_some_and(|length| length > SOURCE_RESPONSE_LIMIT_BYTES as u64)
    {
        return Err(format!(
            "fetch {url} exceeded {} bytes",
            SOURCE_RESPONSE_LIMIT_BYTES
        ));
    }
    let mut body = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| format!("read {url} response failed: {error}"))?;
        let next_len = body
            .len()
            .checked_add(chunk.len())
            .ok_or_else(|| format!("fetch {url} response size overflow"))?;
        if next_len > SOURCE_RESPONSE_LIMIT_BYTES {
            return Err(format!(
                "fetch {url} exceeded {} bytes",
                SOURCE_RESPONSE_LIMIT_BYTES
            ));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn parse_source_body(name: &str, url: &str, bytes: &[u8]) -> Result<ParsedSource, String> {
    parse_source_body_with_limit(name, url, bytes, SOURCE_RESPONSE_LIMIT_BYTES)
}

fn parse_source_body_with_limit(
    name: &str,
    url: &str,
    bytes: &[u8],
    limit: usize,
) -> Result<ParsedSource, String> {
    if bytes.len() > limit {
        return Err(format!("parse {name} response exceeded {limit} bytes"));
    }
    let providers: BTreeMap<String, SourceProvider> = serde_json::from_slice(bytes)
        .map_err(|error| format!("parse {name} response failed: {error}"))?;
    let mut parsed = ParsedSource {
        records: BTreeMap::new(),
        blocked_keys: HashSet::new(),
        summary: ModelPriceSourceSyncSummary {
            name: name.to_string(),
            url: url.to_string(),
            status: "ok".to_string(),
            providers: providers.len(),
            models_seen: 0,
            priced_models: 0,
            skipped_non_usd: 0,
            skipped_missing_text_price: 0,
            skipped_subscription: 0,
            skipped_tiers: 0,
            error: None,
        },
    };

    for (provider_key, provider) in providers {
        let canonical_provider = normalize_provider(
            provider
                .id
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or(&provider_key),
        );
        if canonical_provider.is_empty() {
            continue;
        }
        let mut provider_aliases = BTreeSet::from([canonical_provider.clone()]);
        provider_aliases.insert(normalize_provider(&provider_key));
        if let Some(provider_name) = provider.name.as_deref() {
            provider_aliases.insert(normalize_provider(provider_name));
        }
        provider_aliases.retain(|value| !value.is_empty());

        for (model_key, model) in provider.models {
            parsed.summary.models_seen += 1;
            let canonical_model = normalize_model(
                model
                    .id
                    .as_deref()
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or(&model_key),
            );
            if canonical_model.is_empty() {
                parsed.summary.skipped_missing_text_price += 1;
                continue;
            }
            let key = (canonical_provider.clone(), canonical_model.clone());
            if provider.subscription {
                parsed.summary.skipped_subscription += 1;
                parsed.blocked_keys.insert(key);
                continue;
            }
            let Some(cost) = model.cost else {
                parsed.summary.skipped_missing_text_price += 1;
                continue;
            };
            if cost
                .currency
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .is_some_and(|currency| !currency.eq_ignore_ascii_case("USD"))
            {
                parsed.summary.skipped_non_usd += 1;
                continue;
            }
            let Some((price, price_tiers, skipped_tiers)) =
                normalize_cost(&cost, url, &canonical_provider, &canonical_model)
            else {
                parsed.summary.skipped_missing_text_price += 1;
                continue;
            };
            parsed.summary.skipped_tiers += skipped_tiers;
            let mut model_aliases = BTreeSet::from([canonical_model.clone()]);
            model_aliases.insert(normalize_model(&model_key));
            model_aliases.retain(|value| !value.is_empty());
            parsed.records.insert(
                key,
                NormalizedPrice {
                    source_rank: 0,
                    provider: canonical_provider.clone(),
                    provider_aliases: provider_aliases.clone(),
                    model: canonical_model,
                    model_aliases,
                    price,
                    price_tiers,
                },
            );
            parsed.summary.priced_models += 1;
        }
    }
    Ok(parsed)
}

fn normalize_cost(
    cost: &SourceCost,
    source_url: &str,
    provider: &str,
    model: &str,
) -> Option<(ModelPriceV2, Vec<ModelPriceTierV2>, usize)> {
    let base = normalize_rates(&cost.rates, 0)?;
    // Some subscription catalogs expose their included models as 0/0 prices
    // without a subscription marker. Treating those values as a real metered
    // price would let wallet requests bypass billing when that source is used
    // as a fallback.
    if base.input_microusd_per_1m == 0 && base.output_microusd_per_1m == 0 {
        return None;
    }
    let mut tiers = BTreeMap::from([(0_i64, base.clone())]);
    let mut skipped_tiers = 0;

    let mut has_usable_context_tier = false;
    for source_tier in &cost.tiers {
        if !source_tier.tier.kind.eq_ignore_ascii_case("context") {
            continue;
        }
        let Some(min_input_tokens) = source_tier
            .tier
            .size
            .checked_add(1)
            .filter(|threshold| *threshold > 0)
        else {
            skipped_tiers += 1;
            continue;
        };
        match normalize_rates(&source_tier.rates, min_input_tokens) {
            Some(tier) => {
                tiers.insert(min_input_tokens, tier);
                has_usable_context_tier = true;
            }
            None => skipped_tiers += 1,
        }
    }
    if !has_usable_context_tier {
        if let Some(rates) = cost.context_over_200k.as_ref() {
            match normalize_rates(rates, 200_001) {
                Some(tier) => {
                    tiers.insert(tier.min_input_tokens, tier);
                }
                None => skipped_tiers += 1,
            }
        }
    }
    let price_tiers = tiers.into_values().collect::<Vec<_>>();
    let price = ModelPriceV2 {
        price_status: "estimated".to_string(),
        price_source: Some(format!("{source_url}#{provider}/{model}")),
        input_microusd_per_1m: Some(base.input_microusd_per_1m),
        cached_input_microusd_per_1m: Some(base.cached_input_microusd_per_1m),
        cache_write_microusd_per_1m: base.cache_write_microusd_per_1m,
        output_microusd_per_1m: Some(base.output_microusd_per_1m),
    };
    Some((price, price_tiers, skipped_tiers))
}

fn normalize_rates(rates: &SourceRates, min_input_tokens: i64) -> Option<ModelPriceTierV2> {
    let input = usd_per_million_to_microusd(rates.input.as_ref()?)?;
    let output = usd_per_million_to_microusd(rates.output.as_ref()?)?;
    let cached_input = rates
        .cache_read
        .as_ref()
        .map(usd_per_million_to_microusd)
        .transpose_option()?
        .unwrap_or(input);
    let cache_write = rates
        .cache_write
        .as_ref()
        .map(usd_per_million_to_microusd)
        .transpose_option()?;
    Some(ModelPriceTierV2 {
        min_input_tokens,
        input_microusd_per_1m: input,
        cached_input_microusd_per_1m: cached_input,
        cache_write_microusd_per_1m: cache_write,
        output_microusd_per_1m: output,
    })
}

trait TransposeOption<T> {
    fn transpose_option(self) -> Option<Option<T>>;
}

impl<T> TransposeOption<T> for Option<Option<T>> {
    fn transpose_option(self) -> Option<Option<T>> {
        match self {
            Some(Some(value)) => Some(Some(value)),
            Some(None) => None,
            None => Some(None),
        }
    }
}

fn usd_per_million_to_microusd(value: &Number) -> Option<i64> {
    let raw = value.to_string();
    if raw.starts_with('-') {
        return None;
    }
    let (mantissa, exponent) = raw
        .split_once(['e', 'E'])
        .map(|(mantissa, exponent)| (mantissa, exponent.parse::<i32>().ok()))
        .unwrap_or((&raw, Some(0)));
    let exponent = exponent?;
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    if whole.is_empty()
        || !whole.bytes().all(|byte| byte.is_ascii_digit())
        || !fraction.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    let digits = format!("{whole}{fraction}").parse::<i128>().ok()?;
    let scale = exponent
        .checked_sub(i32::try_from(fraction.len()).ok()?)?
        .checked_add(6)?;
    let scaled = if scale >= 0 {
        digits.checked_mul(10_i128.checked_pow(scale as u32)?)?
    } else {
        let divisor = 10_i128.checked_pow(scale.unsigned_abs())?;
        let quotient = digits / divisor;
        let remainder = digits % divisor;
        quotient.checked_add(i128::from(remainder.saturating_mul(2) >= divisor))?
    };
    i64::try_from(scaled).ok()
}

fn merge_source_results(
    primary: Result<ParsedSource, String>,
    fallback: Result<ParsedSource, String>,
) -> Result<(PriceIndex, Vec<ModelPriceSourceSyncSummary>), String> {
    if primary.is_err() && fallback.is_err() {
        return Err(format!(
            "all model price sources failed: basellm: {}; models.dev: {}",
            primary.as_ref().expect_err("primary error"),
            fallback.as_ref().expect_err("fallback error")
        ));
    }
    let mut summaries = Vec::with_capacity(2);
    let mut records = BTreeMap::new();
    let mut primary_blocked = HashSet::new();
    match primary {
        Ok(parsed) => {
            primary_blocked = parsed.blocked_keys;
            for (key, mut record) in parsed.records {
                record.source_rank = 0;
                records.insert(key, record);
            }
            summaries.push(parsed.summary);
        }
        Err(error) => summaries.push(error_summary("basellm", BASELLM_PRICE_SOURCE_URL, error)),
    }
    match fallback {
        Ok(parsed) => {
            for (key, mut record) in parsed.records {
                if !primary_blocked.contains(&key) {
                    record.source_rank = 1;
                    records.entry(key).or_insert(record);
                }
            }
            summaries.push(parsed.summary);
        }
        Err(error) => summaries.push(error_summary(
            "models.dev",
            MODELS_DEV_PRICE_SOURCE_URL,
            error,
        )),
    }
    Ok((PriceIndex::from_records(records), summaries))
}

fn error_summary(name: &str, url: &str, error: String) -> ModelPriceSourceSyncSummary {
    ModelPriceSourceSyncSummary {
        name: name.to_string(),
        url: url.to_string(),
        status: "error".to_string(),
        providers: 0,
        models_seen: 0,
        priced_models: 0,
        skipped_non_usd: 0,
        skipped_missing_text_price: 0,
        skipped_subscription: 0,
        skipped_tiers: 0,
        error: Some(error),
    }
}

#[cfg(test)]
fn build_update_plan(
    models: &[ManagedModelV2],
    index: &PriceIndex,
    sources: Vec<ModelPriceSourceSyncSummary>,
) -> (
    Vec<ManagedModelPriceV2Update>,
    ManagedModelPriceSyncV2Result,
) {
    build_update_plan_with_options(models, index, sources, false)
}

fn build_update_plan_with_options(
    models: &[ManagedModelV2],
    index: &PriceIndex,
    sources: Vec<ModelPriceSourceSyncSummary>,
    allow_custom_override: bool,
) -> (
    Vec<ManagedModelPriceV2Update>,
    ManagedModelPriceSyncV2Result,
) {
    let all_sources_loaded = sources.len() == 2
        && sources
            .iter()
            .all(|source| source.status == "ok" && source.models_seen > 0);
    let mut result = ManagedModelPriceSyncV2Result {
        sources,
        catalog_prices: index.records.len(),
        scanned_models: models.len(),
        ..Default::default()
    };
    let mut updates = Vec::new();
    for model in models {
        if model.price.price_status == "custom" && !allow_custom_override {
            result.preserved_custom += 1;
            continue;
        }
        let record = match index.match_model(model) {
            MatchResult::Found(record) => &index.records[record],
            MatchResult::Missing => {
                result.unmatched += 1;
                if model.price.price_status == "custom" {
                    result.preserved_custom += 1;
                }
                if all_sources_loaded && is_external_estimated_price(model) {
                    updates.push(ManagedModelPriceV2Update {
                        slug: model.slug.clone(),
                        price: ModelPriceV2 {
                            price_status: "missing".to_string(),
                            ..Default::default()
                        },
                        price_tiers: Vec::new(),
                    });
                }
                continue;
            }
            MatchResult::Ambiguous => {
                result.ambiguous += 1;
                if model.price.price_status == "custom" {
                    result.preserved_custom += 1;
                }
                continue;
            }
        };
        if model.price == record.price && model.price_tiers == record.price_tiers {
            result.unchanged += 1;
            continue;
        }
        updates.push(ManagedModelPriceV2Update {
            slug: model.slug.clone(),
            price: record.price.clone(),
            price_tiers: record.price_tiers.clone(),
        });
    }
    result.updated = updates.len();
    (updates, result)
}

fn is_external_estimated_price(model: &ManagedModelV2) -> bool {
    if model.price.price_status != "estimated" {
        return false;
    }
    model.price.price_source.as_deref().is_some_and(|source| {
        [BASELLM_PRICE_SOURCE_URL, MODELS_DEV_PRICE_SOURCE_URL]
            .iter()
            .any(|url| {
                source
                    .strip_prefix(url)
                    .is_some_and(|suffix| suffix.starts_with('#'))
            })
    })
}

fn normalize_provider(value: &str) -> String {
    value
        .trim()
        .bytes()
        .filter(|byte| byte.is_ascii_alphanumeric())
        .map(|byte| byte.to_ascii_lowercase() as char)
        .collect()
}

fn normalize_model(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use codexmanager_core::storage::{
        ManagedModelV2Upsert, ModelGroup, ModelGroupModel, ModelRouteV2, Storage,
    };

    const PRIMARY_FIXTURE: &[u8] = include_bytes!("fixtures/basellm-prices.json");
    const FALLBACK_FIXTURE: &[u8] = include_bytes!("fixtures/models-dev-prices.json");

    fn parsed_sources() -> (PriceIndex, Vec<ModelPriceSourceSyncSummary>) {
        merge_source_results(
            parse_source_body("basellm", BASELLM_PRICE_SOURCE_URL, PRIMARY_FIXTURE),
            parse_source_body("models.dev", MODELS_DEV_PRICE_SOURCE_URL, FALLBACK_FIXTURE),
        )
        .expect("fixture sources")
    }

    fn model(slug: &str, provider: Option<&str>, upstream: &str) -> ManagedModelV2 {
        ManagedModelV2 {
            slug: slug.to_string(),
            display_name: slug.to_string(),
            provider: provider.map(str::to_string),
            origin: "custom".to_string(),
            enabled: true,
            supported_in_api: true,
            visibility: "list".to_string(),
            instructions_mode: "passthrough".to_string(),
            price: ModelPriceV2 {
                price_status: "missing".to_string(),
                ..Default::default()
            },
            routes: vec![ModelRouteV2 {
                source_kind: "account_pool".to_string(),
                source_id: "default".to_string(),
                upstream_model: upstream.to_string(),
                enabled: true,
                weight: 1,
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    #[test]
    fn primary_prices_win_and_fallback_only_fills_gaps() {
        let (index, sources) = parsed_sources();
        assert_eq!(sources.len(), 2);
        assert!(sources.iter().all(|source| source.status == "ok"));

        let primary = index.match_model(&model("gpt-alpha", Some("OpenAI"), "gpt-alpha"));
        let MatchResult::Found(primary) = primary else {
            panic!("primary match");
        };
        assert_eq!(
            index.records[primary].price.input_microusd_per_1m,
            Some(1_250_000)
        );
        assert!(index.records[primary]
            .price
            .price_source
            .as_deref()
            .unwrap()
            .starts_with(BASELLM_PRICE_SOURCE_URL));

        let fallback =
            index.match_model(&model("local-alias", Some("openai"), "fallback-candidate"));
        let MatchResult::Found(fallback) = fallback else {
            panic!("fallback match");
        };
        assert!(index.records[fallback]
            .price
            .price_source
            .as_deref()
            .unwrap()
            .starts_with(MODELS_DEV_PRICE_SOURCE_URL));

        let cny_fallback = index.match_model(&model("cny-model", Some("openai"), "cny-model"));
        let MatchResult::Found(cny_fallback) = cny_fallback else {
            panic!("USD fallback should fill a non-USD primary record");
        };
        assert!(index.records[cny_fallback]
            .price
            .price_source
            .as_deref()
            .unwrap()
            .starts_with(MODELS_DEV_PRICE_SOURCE_URL));
    }

    #[test]
    fn explicit_context_tiers_override_legacy_and_start_after_the_boundary() {
        let primary =
            parse_source_body("basellm", BASELLM_PRICE_SOURCE_URL, PRIMARY_FIXTURE).unwrap();
        let alpha = primary
            .records
            .get(&(normalize_provider("openai"), normalize_model("gpt-alpha")))
            .unwrap();
        assert_eq!(
            alpha
                .price_tiers
                .iter()
                .map(|tier| tier.min_input_tokens)
                .collect::<Vec<_>>(),
            vec![0, 272_001]
        );
        assert_eq!(
            alpha.price_tiers[1].cached_input_microusd_per_1m,
            alpha.price_tiers[1].input_microusd_per_1m
        );

        let context = primary
            .records
            .get(&(
                normalize_provider("openai"),
                normalize_model("context-model"),
            ))
            .unwrap();
        assert_eq!(context.price_tiers[1].min_input_tokens, 200_001);
    }

    #[test]
    fn overflowing_context_boundary_is_skipped() {
        let body = br#"{
          "openai":{"models":{"overflow":{"cost":{"input":1,"output":2,
            "tiers":[{"input":2,"output":4,"tier":{"type":"context","size":9223372036854775807}}]}}}}
        }"#;
        let parsed = parse_source_body("fixture", "https://fixture.invalid", body).unwrap();
        let record = parsed.records.values().next().unwrap();
        assert_eq!(record.price_tiers.len(), 1);
        assert_eq!(parsed.summary.skipped_tiers, 1);
    }

    #[test]
    fn provider_hint_wins_and_cross_provider_route_is_ambiguous() {
        let (index, _) = parsed_sources();
        assert!(matches!(
            index.match_model(&model("shared-model", Some("anthropic"), "shared-model")),
            MatchResult::Found(_)
        ));
        assert_eq!(
            index.match_model(&model("local-shared", None, "shared-model")),
            MatchResult::Ambiguous
        );
    }

    #[test]
    fn explicit_model_provider_blocks_global_cross_provider_price_updates() {
        let (index, sources) = parsed_sources();
        let constrained = model("local-gpt-alpha", Some("anthropic"), "gpt-alpha");

        assert_eq!(index.match_model(&constrained), MatchResult::Missing);
        let (updates, result) = build_update_plan(&[constrained], &index, sources);
        assert!(updates.is_empty());
        assert_eq!(result.updated, 0);
        assert_eq!(result.unmatched, 1);

        let explicit_route = model("local-gpt-alpha", Some("anthropic"), "aggregator/gpt-alpha");
        let MatchResult::Found(record) = index.match_model(&explicit_route) else {
            panic!("route provider should remain authoritative");
        };
        assert_eq!(index.records[record].provider, "aggregator");
    }

    #[test]
    fn global_matches_prefer_primary_source_but_explicit_provider_can_use_fallback() {
        let (index, _) = parsed_sources();
        let MatchResult::Found(primary) = index.match_model(&model("gpt-alpha", None, "gpt-alpha"))
        else {
            panic!("providerless route should use primary-source price");
        };
        assert_eq!(index.records[primary].provider, "openai");
        assert_eq!(index.records[primary].source_rank, 0);

        let MatchResult::Found(fallback) =
            index.match_model(&model("local-gpt-alpha", None, "aggregator/gpt-alpha"))
        else {
            panic!("explicit provider route should use matching fallback-source price");
        };
        assert_eq!(index.records[fallback].provider, "aggregator");
        assert_eq!(index.records[fallback].source_rank, 1);
    }

    #[test]
    fn a_single_source_failure_falls_back_but_two_failures_abort() {
        let fallback =
            parse_source_body("models.dev", MODELS_DEV_PRICE_SOURCE_URL, FALLBACK_FIXTURE).unwrap();
        let (index, summaries) =
            merge_source_results(Err("primary offline".into()), Ok(fallback)).unwrap();
        assert!(!index.records.is_empty());
        assert_eq!(summaries[0].status, "error");
        assert!(merge_source_results(Err("primary".into()), Err("fallback".into())).is_err());
    }

    #[test]
    fn fallback_zero_prices_are_not_treated_as_metered_prices() {
        let fallback = parse_source_body(
            "models.dev",
            MODELS_DEV_PRICE_SOURCE_URL,
            br#"{
              "subscription-plan": {
                "id": "subscription-plan",
                "models": {
                  "free-looking": {"cost": {"input": 0, "output": 0}}
                }
              }
            }"#,
        )
        .unwrap();
        assert!(fallback.records.is_empty());
        assert_eq!(fallback.summary.skipped_missing_text_price, 1);

        let (index, _) = merge_source_results(Err("primary offline".into()), Ok(fallback)).unwrap();
        assert_eq!(
            index.match_model(&model(
                "free-looking",
                Some("subscription-plan"),
                "free-looking"
            )),
            MatchResult::Missing
        );
    }

    #[test]
    fn complete_sources_clear_withdrawn_external_estimates() {
        let (index, sources) = parsed_sources();
        let mut withdrawn = model("withdrawn-model", Some("openai"), "withdrawn-model");
        withdrawn.price = ModelPriceV2 {
            price_status: "estimated".to_string(),
            price_source: Some(format!(
                "{MODELS_DEV_PRICE_SOURCE_URL}#openai/withdrawn-model"
            )),
            input_microusd_per_1m: Some(1_000_000),
            cached_input_microusd_per_1m: Some(100_000),
            cache_write_microusd_per_1m: None,
            output_microusd_per_1m: Some(5_000_000),
        };
        withdrawn.price_tiers = vec![ModelPriceTierV2 {
            min_input_tokens: 0,
            input_microusd_per_1m: 1_000_000,
            cached_input_microusd_per_1m: 100_000,
            cache_write_microusd_per_1m: None,
            output_microusd_per_1m: 5_000_000,
        }];

        let (updates, result) = build_update_plan(&[withdrawn], &index, sources);
        assert_eq!(result.unmatched, 1);
        assert_eq!(result.updated, 1);
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].price.price_status, "missing");
        assert!(updates[0].price_tiers.is_empty());
    }

    #[test]
    fn partial_source_failure_does_not_clear_an_external_estimate() {
        let fallback =
            parse_source_body("models.dev", MODELS_DEV_PRICE_SOURCE_URL, FALLBACK_FIXTURE).unwrap();
        let (index, sources) =
            merge_source_results(Err("primary offline".into()), Ok(fallback)).unwrap();
        let mut uncertain = model("primary-only-model", Some("openai"), "primary-only-model");
        uncertain.price = ModelPriceV2 {
            price_status: "estimated".to_string(),
            price_source: Some(format!(
                "{BASELLM_PRICE_SOURCE_URL}#openai/primary-only-model"
            )),
            input_microusd_per_1m: Some(1_000_000),
            cached_input_microusd_per_1m: Some(1_000_000),
            cache_write_microusd_per_1m: None,
            output_microusd_per_1m: Some(2_000_000),
        };
        uncertain.price_tiers = vec![ModelPriceTierV2 {
            min_input_tokens: 0,
            input_microusd_per_1m: 1_000_000,
            cached_input_microusd_per_1m: 1_000_000,
            cache_write_microusd_per_1m: None,
            output_microusd_per_1m: 2_000_000,
        }];

        let (updates, result) = build_update_plan(&[uncertain], &index, sources);
        assert!(updates.is_empty());
        assert_eq!(result.unmatched, 1);
        assert_eq!(result.updated, 0);
    }

    #[test]
    fn empty_successful_source_does_not_trigger_mass_price_invalidation() {
        let primary =
            parse_source_body("basellm", BASELLM_PRICE_SOURCE_URL, PRIMARY_FIXTURE).unwrap();
        let empty_fallback =
            parse_source_body("models.dev", MODELS_DEV_PRICE_SOURCE_URL, br#"{}"#).unwrap();
        let (index, sources) = merge_source_results(Ok(primary), Ok(empty_fallback)).unwrap();
        let mut uncertain = model("withdrawn-model", Some("openai"), "withdrawn-model");
        uncertain.price = ModelPriceV2 {
            price_status: "estimated".to_string(),
            price_source: Some(format!("{BASELLM_PRICE_SOURCE_URL}#openai/withdrawn-model")),
            input_microusd_per_1m: Some(1_000_000),
            cached_input_microusd_per_1m: Some(1_000_000),
            cache_write_microusd_per_1m: None,
            output_microusd_per_1m: Some(2_000_000),
        };
        uncertain.price_tiers = vec![ModelPriceTierV2 {
            min_input_tokens: 0,
            input_microusd_per_1m: 1_000_000,
            cached_input_microusd_per_1m: 1_000_000,
            cache_write_microusd_per_1m: None,
            output_microusd_per_1m: 2_000_000,
        }];

        let (updates, result) = build_update_plan(&[uncertain], &index, sources);
        assert!(updates.is_empty());
        assert_eq!(result.unmatched, 1);
        assert_eq!(result.updated, 0);
    }

    #[test]
    fn fallback_only_uses_openai_hint_for_builtins_and_keeps_custom_models_ambiguous() {
        let fallback =
            parse_source_body("models.dev", MODELS_DEV_PRICE_SOURCE_URL, FALLBACK_FIXTURE).unwrap();
        let (index, _) = merge_source_results(Err("primary offline".into()), Ok(fallback)).unwrap();

        let mut builtin = model("gpt-alpha", None, "gpt-alpha");
        builtin.origin = "builtin".to_string();
        let MatchResult::Found(found) = index.match_model(&builtin) else {
            panic!("builtin should use the OpenAI fallback record");
        };
        assert_eq!(index.records[found].provider, "openai");
        assert_eq!(index.records[found].source_rank, 1);

        assert_eq!(
            index.match_model(&model("gpt-alpha", None, "gpt-alpha")),
            MatchResult::Ambiguous
        );
    }

    #[test]
    fn invalid_and_oversized_source_bodies_are_rejected() {
        assert!(parse_source_body("bad", "https://fixture.invalid", b"{").is_err());
        assert!(
            parse_source_body_with_limit("large", "https://fixture.invalid", b"123456789", 8)
                .unwrap_err()
                .contains("exceeded")
        );
    }

    #[test]
    fn cache_defaults_and_decimal_conversion_are_exact() {
        let value = serde_json::from_str::<Number>("1.234567").unwrap();
        assert_eq!(usd_per_million_to_microusd(&value), Some(1_234_567));
        let (index, _) = parsed_sources();
        let MatchResult::Found(record) =
            index.match_model(&model("fallback-candidate", Some("openai"), "unused"))
        else {
            panic!("fallback record");
        };
        let price = &index.records[record].price;
        assert_eq!(
            price.cached_input_microusd_per_1m,
            price.input_microusd_per_1m
        );
        assert_eq!(price.cache_write_microusd_per_1m, None);
    }

    #[test]
    fn update_plan_preserves_custom_prices() {
        let (index, sources) = parsed_sources();
        let mut custom = model("gpt-alpha", Some("openai"), "gpt-alpha");
        custom.price = ModelPriceV2 {
            price_status: "custom".to_string(),
            price_source: Some("local".to_string()),
            input_microusd_per_1m: Some(1),
            cached_input_microusd_per_1m: Some(1),
            cache_write_microusd_per_1m: None,
            output_microusd_per_1m: Some(1),
        };
        custom.price_tiers = vec![ModelPriceTierV2 {
            min_input_tokens: 0,
            input_microusd_per_1m: 1,
            cached_input_microusd_per_1m: 1,
            cache_write_microusd_per_1m: None,
            output_microusd_per_1m: 1,
        }];
        let (updates, result) = build_update_plan(&[custom], &index, sources);
        assert!(updates.is_empty());
        assert_eq!(result.preserved_custom, 1);
    }

    #[test]
    fn price_sync_selection_defaults_to_all_and_rejects_unknown_slugs() {
        let models = vec![
            model("gpt-alpha", Some("openai"), "gpt-alpha"),
            model("gpt-beta", Some("openai"), "gpt-beta"),
        ];

        let all = select_models_for_price_sync(&models, &[]).unwrap();
        assert_eq!(all.len(), models.len());

        let whitespace_only = select_models_for_price_sync(&models, &["  ".to_string()]).unwrap();
        assert_eq!(whitespace_only.len(), models.len());

        let selected = select_models_for_price_sync(
            &models,
            &[" GPT-BETA ".to_string(), "gpt-beta".to_string()],
        )
        .unwrap();
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].slug, "gpt-beta");

        let error = select_models_for_price_sync(&models, &["missing-model".to_string()])
            .expect_err("unknown selected model must be rejected");
        assert!(error.contains("missing-model"));
    }

    #[test]
    fn selected_price_sync_only_scans_selected_models_and_overwrites_custom_price() {
        let (index, sources) = parsed_sources();
        let mut custom = model("gpt-alpha", Some("openai"), "gpt-alpha");
        custom.price.price_status = "custom".to_string();
        custom.price.price_source = Some("local".to_string());
        let mut other = model("gpt-beta", Some("openai"), "gpt-beta");
        other.price.price_status = "custom".to_string();
        other.price.price_source = Some("local".to_string());
        let models = vec![custom.clone(), other];

        let selected = select_models_for_price_sync(&models, &["GPT-ALPHA".to_string()]).unwrap();
        let (updates, result) = build_update_plan_with_options(&selected, &index, sources, true);

        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].slug, "gpt-alpha");
        assert_eq!(updates[0].price.price_status, "estimated");
        assert_eq!(result.scanned_models, 1);
        assert_eq!(result.preserved_custom, 0);
    }

    #[test]
    fn selected_price_sync_does_not_update_unselected_models() {
        let (index, sources) = parsed_sources();
        let selected = model("gpt-alpha", Some("openai"), "gpt-alpha");
        let unselected = model("gpt-copy", Some("openai"), "gpt-alpha");
        let models = vec![selected, unselected];

        let selected = select_models_for_price_sync(&models, &["gpt-alpha".to_string()]).unwrap();
        let (updates, result) = build_update_plan(&selected, &index, sources);

        assert_eq!(result.scanned_models, 1);
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].slug, "gpt-alpha");
    }

    #[tokio::test]
    #[ignore = "requires live network access to external price catalogs"]
    async fn live_sources_parse_and_update_an_isolated_catalog() {
        let client = reqwest::Client::builder()
            .connect_timeout(SOURCE_CONNECT_TIMEOUT)
            .timeout(SOURCE_REQUEST_TIMEOUT)
            .user_agent("CodexManager/model-price-sync-contract-test")
            .build()
            .unwrap();
        let (primary, fallback) = tokio::join!(
            load_source(&client, "basellm", BASELLM_PRICE_SOURCE_URL),
            load_source(&client, "models.dev", MODELS_DEV_PRICE_SOURCE_URL)
        );
        let (index, sources) = merge_source_results(primary, fallback).unwrap();
        assert!(sources.iter().all(|source| source.status == "ok"));
        assert!(index.records.len() > 100);

        let storage = Storage::open_in_memory().unwrap();
        storage.init().unwrap();
        let models = storage.list_managed_models_v2(true).unwrap();
        let gpt_6_sol = models
            .iter()
            .find(|candidate| candidate.slug == "gpt-6-sol")
            .expect("seeded gpt-6-sol model");
        assert!(matches!(
            index.match_model(gpt_6_sol),
            MatchResult::Found(_)
        ));

        let (updates, result) = build_update_plan(&models, &index, sources);
        assert_eq!(result.scanned_models, models.len());
        assert_eq!(result.catalog_prices, index.records.len());
        let applied = storage.update_managed_model_prices_v2(&updates).unwrap();
        assert_eq!(applied.len(), updates.len());
        for slug in applied {
            let updated = storage.get_managed_model_v2(&slug).unwrap().unwrap();
            assert_eq!(updated.price.price_status, "estimated");
            assert!(updated.price.price_source.is_some());
        }
    }

    fn estimated_update(slug: &str, input: i64) -> ManagedModelPriceV2Update {
        ManagedModelPriceV2Update {
            slug: slug.to_string(),
            price: ModelPriceV2 {
                price_status: "estimated".to_string(),
                price_source: Some("fixture".to_string()),
                input_microusd_per_1m: Some(input),
                cached_input_microusd_per_1m: Some(input),
                cache_write_microusd_per_1m: None,
                output_microusd_per_1m: Some(input * 2),
            },
            price_tiers: vec![ModelPriceTierV2 {
                min_input_tokens: 0,
                input_microusd_per_1m: input,
                cached_input_microusd_per_1m: input,
                cache_write_microusd_per_1m: None,
                output_microusd_per_1m: input * 2,
            }],
        }
    }

    fn missing_update(slug: &str) -> ManagedModelPriceV2Update {
        ManagedModelPriceV2Update {
            slug: slug.to_string(),
            price: ModelPriceV2 {
                price_status: "missing".to_string(),
                ..Default::default()
            },
            price_tiers: Vec::new(),
        }
    }

    #[test]
    fn sqlite_price_updates_are_atomic_and_recheck_custom_status() {
        let storage = Storage::open_in_memory().unwrap();
        storage.init().unwrap();

        let mut custom = storage.get_managed_model_v2("gpt-6-sol").unwrap().unwrap();
        custom.price.price_status = "custom".to_string();
        custom.price.price_source = Some("local-ui".to_string());
        storage
            .upsert_managed_model_v2(&ManagedModelV2Upsert {
                previous_slug: None,
                model: custom.clone(),
            })
            .unwrap();

        let applied = storage
            .update_managed_model_prices_v2(&[
                estimated_update("gpt-6-sol", 11),
                estimated_update("gpt-5.5", 22),
            ])
            .unwrap();
        assert_eq!(applied, vec!["gpt-5.5"]);
        let custom_after = storage.get_managed_model_v2("gpt-6-sol").unwrap().unwrap();
        assert_eq!(custom_after.price.price_source.as_deref(), Some("local-ui"));
        assert_eq!(custom_after.price_tiers, custom.price_tiers);

        let forced = estimated_update("gpt-6-sol", 12);
        assert_eq!(
            storage
                .update_managed_model_prices_v2_with_custom_override(
                    std::slice::from_ref(&forced),
                    true,
                )
                .unwrap(),
            vec!["gpt-6-sol"]
        );
        let forced_after = storage.get_managed_model_v2("gpt-6-sol").unwrap().unwrap();
        assert_eq!(forced_after.price, forced.price);
        assert_eq!(forced_after.price_tiers, forced.price_tiers);

        let before = storage.get_managed_model_v2("gpt-5.5").unwrap().unwrap();
        assert!(storage
            .update_managed_model_prices_v2(&[
                estimated_update("gpt-5.5", 33),
                estimated_update("missing-model", 44),
            ])
            .is_err());
        let after = storage.get_managed_model_v2("gpt-5.5").unwrap().unwrap();
        assert_eq!(after.price, before.price);
        assert_eq!(after.price_tiers, before.price_tiers);

        let group_id = "price-sync-permission-group";
        storage
            .upsert_model_group(&ModelGroup {
                id: group_id.to_string(),
                name: "Price sync permission group".to_string(),
                description: None,
                status: "active".to_string(),
                sort: 10,
                is_default: false,
                rate_multiplier_millis: 1_000,
                created_at: 1,
                updated_at: 1,
            })
            .unwrap();
        storage
            .replace_model_group_models_v2(
                group_id,
                &[ModelGroupModel {
                    group_id: group_id.to_string(),
                    platform_model_slug: "gpt-5.5".to_string(),
                    enabled: true,
                    rate_multiplier_millis: None,
                    billing_model_slug: None,
                    note: None,
                    created_at: 1,
                    updated_at: 1,
                }],
            )
            .unwrap();
        let permission_before = storage.get_managed_model_v2("gpt-5.5").unwrap().unwrap();
        assert!(permission_before
            .permission_group_ids
            .contains(&group_id.to_string()));
        let error = storage
            .update_managed_model_prices_v2(&[
                missing_update("gpt-5.5"),
                estimated_update("missing-model", 55),
            ])
            .expect_err("late failure must restore the price and group membership");
        assert!(error.to_string().contains("model_not_found"));
        let permission_after = storage.get_managed_model_v2("gpt-5.5").unwrap().unwrap();
        assert_eq!(permission_after.price, permission_before.price);
        assert_eq!(permission_after.price_tiers, permission_before.price_tiers);
        assert!(permission_after
            .permission_group_ids
            .contains(&group_id.to_string()));

        assert_eq!(
            storage
                .update_managed_model_prices_v2(&[missing_update("gpt-5.5")])
                .unwrap(),
            vec!["gpt-5.5"]
        );
        let invalidated = storage.get_managed_model_v2("gpt-5.5").unwrap().unwrap();
        assert_eq!(invalidated.price.price_status, "missing");
        assert!(invalidated.price.price_source.is_none());
        assert!(invalidated.price.input_microusd_per_1m.is_none());
        assert!(invalidated.price.cached_input_microusd_per_1m.is_none());
        assert!(invalidated.price.cache_write_microusd_per_1m.is_none());
        assert!(invalidated.price.output_microusd_per_1m.is_none());
        assert!(invalidated.price_tiers.is_empty());
        assert!(!invalidated
            .permission_group_ids
            .contains(&group_id.to_string()));
    }
}
