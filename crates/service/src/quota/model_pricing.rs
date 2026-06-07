use codexmanager_core::storage::{now_ts, ModelPriceRule, Storage};

pub(crate) const PRICE_SEED_VERSION: &str = "2026-05-11";

#[derive(Debug, Clone, Copy)]
struct PriceSeed {
    provider: &'static str,
    model_pattern: &'static str,
    input_price_per_1m: f64,
    cached_input_price_per_1m: Option<f64>,
    output_price_per_1m: f64,
    long_context_threshold_tokens: Option<i64>,
    long_context_input_price_per_1m: Option<f64>,
    long_context_cached_input_price_per_1m: Option<f64>,
    long_context_output_price_per_1m: Option<f64>,
    source_url: &'static str,
}

#[derive(Debug, Clone)]
pub(crate) struct ModelPriceMatch {
    pub(crate) provider: String,
    pub(crate) input_price_per_1m: f64,
    pub(crate) cached_input_price_per_1m: f64,
    pub(crate) output_price_per_1m: f64,
}

#[derive(Debug, Clone)]
pub(crate) struct CostEstimate {
    pub(crate) provider: Option<String>,
    pub(crate) cost_usd: Option<f64>,
    pub(crate) price_status: &'static str,
}

const OPENAI_PRICE_SOURCE: &str = "https://developers.openai.com/api/docs/pricing";
const ANTHROPIC_PRICE_SOURCE: &str = "https://docs.claude.com/en/docs/about-claude/pricing";
const GEMINI_PRICE_SOURCE: &str = "https://ai.google.dev/gemini-api/docs/pricing";

const PRICE_SEEDS: &[PriceSeed] = &[
    PriceSeed {
        provider: "openai",
        model_pattern: "gpt-5.5-pro",
        input_price_per_1m: 30.0,
        cached_input_price_per_1m: None,
        output_price_per_1m: 180.0,
        long_context_threshold_tokens: Some(272_000),
        long_context_input_price_per_1m: Some(60.0),
        long_context_cached_input_price_per_1m: None,
        long_context_output_price_per_1m: Some(270.0),
        source_url: OPENAI_PRICE_SOURCE,
    },
    PriceSeed {
        provider: "openai",
        model_pattern: "gpt-5.5",
        input_price_per_1m: 5.0,
        cached_input_price_per_1m: Some(0.5),
        output_price_per_1m: 30.0,
        long_context_threshold_tokens: Some(272_000),
        long_context_input_price_per_1m: Some(10.0),
        long_context_cached_input_price_per_1m: Some(1.0),
        long_context_output_price_per_1m: Some(45.0),
        source_url: OPENAI_PRICE_SOURCE,
    },
    PriceSeed {
        provider: "openai",
        model_pattern: "gpt-5.4-pro",
        input_price_per_1m: 30.0,
        cached_input_price_per_1m: None,
        output_price_per_1m: 180.0,
        long_context_threshold_tokens: Some(272_000),
        long_context_input_price_per_1m: Some(60.0),
        long_context_cached_input_price_per_1m: None,
        long_context_output_price_per_1m: Some(270.0),
        source_url: OPENAI_PRICE_SOURCE,
    },
    PriceSeed {
        provider: "openai",
        model_pattern: "gpt-5.4-mini",
        input_price_per_1m: 0.75,
        cached_input_price_per_1m: Some(0.075),
        output_price_per_1m: 4.5,
        long_context_threshold_tokens: None,
        long_context_input_price_per_1m: None,
        long_context_cached_input_price_per_1m: None,
        long_context_output_price_per_1m: None,
        source_url: OPENAI_PRICE_SOURCE,
    },
    PriceSeed {
        provider: "openai",
        model_pattern: "gpt-5.4-nano",
        input_price_per_1m: 0.2,
        cached_input_price_per_1m: Some(0.02),
        output_price_per_1m: 1.25,
        long_context_threshold_tokens: None,
        long_context_input_price_per_1m: None,
        long_context_cached_input_price_per_1m: None,
        long_context_output_price_per_1m: None,
        source_url: OPENAI_PRICE_SOURCE,
    },
    PriceSeed {
        provider: "openai",
        model_pattern: "gpt-5.4",
        input_price_per_1m: 2.5,
        cached_input_price_per_1m: Some(0.25),
        output_price_per_1m: 15.0,
        long_context_threshold_tokens: Some(272_000),
        long_context_input_price_per_1m: Some(5.0),
        long_context_cached_input_price_per_1m: Some(0.5),
        long_context_output_price_per_1m: Some(22.5),
        source_url: OPENAI_PRICE_SOURCE,
    },
    PriceSeed {
        provider: "openai",
        model_pattern: "gpt-5.3-codex",
        input_price_per_1m: 1.75,
        cached_input_price_per_1m: Some(0.175),
        output_price_per_1m: 14.0,
        long_context_threshold_tokens: None,
        long_context_input_price_per_1m: None,
        long_context_cached_input_price_per_1m: None,
        long_context_output_price_per_1m: None,
        source_url: OPENAI_PRICE_SOURCE,
    },
    PriceSeed {
        provider: "openai",
        model_pattern: "gpt-5.2-pro",
        input_price_per_1m: 21.0,
        cached_input_price_per_1m: None,
        output_price_per_1m: 168.0,
        long_context_threshold_tokens: None,
        long_context_input_price_per_1m: None,
        long_context_cached_input_price_per_1m: None,
        long_context_output_price_per_1m: None,
        source_url: OPENAI_PRICE_SOURCE,
    },
    PriceSeed {
        provider: "openai",
        model_pattern: "gpt-5.2",
        input_price_per_1m: 1.75,
        cached_input_price_per_1m: Some(0.175),
        output_price_per_1m: 14.0,
        long_context_threshold_tokens: None,
        long_context_input_price_per_1m: None,
        long_context_cached_input_price_per_1m: None,
        long_context_output_price_per_1m: None,
        source_url: OPENAI_PRICE_SOURCE,
    },
    PriceSeed {
        provider: "openai",
        model_pattern: "gpt-5.1",
        input_price_per_1m: 1.25,
        cached_input_price_per_1m: Some(0.125),
        output_price_per_1m: 10.0,
        long_context_threshold_tokens: None,
        long_context_input_price_per_1m: None,
        long_context_cached_input_price_per_1m: None,
        long_context_output_price_per_1m: None,
        source_url: OPENAI_PRICE_SOURCE,
    },
    PriceSeed {
        provider: "openai",
        model_pattern: "gpt-5-pro",
        input_price_per_1m: 15.0,
        cached_input_price_per_1m: None,
        output_price_per_1m: 120.0,
        long_context_threshold_tokens: None,
        long_context_input_price_per_1m: None,
        long_context_cached_input_price_per_1m: None,
        long_context_output_price_per_1m: None,
        source_url: OPENAI_PRICE_SOURCE,
    },
    PriceSeed {
        provider: "openai",
        model_pattern: "gpt-5-mini",
        input_price_per_1m: 0.25,
        cached_input_price_per_1m: Some(0.025),
        output_price_per_1m: 2.0,
        long_context_threshold_tokens: None,
        long_context_input_price_per_1m: None,
        long_context_cached_input_price_per_1m: None,
        long_context_output_price_per_1m: None,
        source_url: OPENAI_PRICE_SOURCE,
    },
    PriceSeed {
        provider: "openai",
        model_pattern: "gpt-5-nano",
        input_price_per_1m: 0.05,
        cached_input_price_per_1m: Some(0.005),
        output_price_per_1m: 0.4,
        long_context_threshold_tokens: None,
        long_context_input_price_per_1m: None,
        long_context_cached_input_price_per_1m: None,
        long_context_output_price_per_1m: None,
        source_url: OPENAI_PRICE_SOURCE,
    },
    PriceSeed {
        provider: "openai",
        model_pattern: "gpt-5",
        input_price_per_1m: 1.25,
        cached_input_price_per_1m: Some(0.125),
        output_price_per_1m: 10.0,
        long_context_threshold_tokens: None,
        long_context_input_price_per_1m: None,
        long_context_cached_input_price_per_1m: None,
        long_context_output_price_per_1m: None,
        source_url: OPENAI_PRICE_SOURCE,
    },
    PriceSeed {
        provider: "openai",
        model_pattern: "gpt-4.1",
        input_price_per_1m: 2.0,
        cached_input_price_per_1m: Some(0.5),
        output_price_per_1m: 8.0,
        long_context_threshold_tokens: None,
        long_context_input_price_per_1m: None,
        long_context_cached_input_price_per_1m: None,
        long_context_output_price_per_1m: None,
        source_url: OPENAI_PRICE_SOURCE,
    },
    PriceSeed {
        provider: "openai",
        model_pattern: "gpt-4o",
        input_price_per_1m: 2.5,
        cached_input_price_per_1m: Some(1.25),
        output_price_per_1m: 10.0,
        long_context_threshold_tokens: None,
        long_context_input_price_per_1m: None,
        long_context_cached_input_price_per_1m: None,
        long_context_output_price_per_1m: None,
        source_url: OPENAI_PRICE_SOURCE,
    },
    PriceSeed {
        provider: "openai",
        model_pattern: "o4-mini",
        input_price_per_1m: 1.1,
        cached_input_price_per_1m: Some(0.275),
        output_price_per_1m: 4.4,
        long_context_threshold_tokens: None,
        long_context_input_price_per_1m: None,
        long_context_cached_input_price_per_1m: None,
        long_context_output_price_per_1m: None,
        source_url: OPENAI_PRICE_SOURCE,
    },
    PriceSeed {
        provider: "openai",
        model_pattern: "o3",
        input_price_per_1m: 2.0,
        cached_input_price_per_1m: Some(0.5),
        output_price_per_1m: 8.0,
        long_context_threshold_tokens: None,
        long_context_input_price_per_1m: None,
        long_context_cached_input_price_per_1m: None,
        long_context_output_price_per_1m: None,
        source_url: OPENAI_PRICE_SOURCE,
    },
    PriceSeed {
        provider: "anthropic",
        model_pattern: "claude-opus-4.7",
        input_price_per_1m: 5.0,
        cached_input_price_per_1m: Some(0.5),
        output_price_per_1m: 25.0,
        long_context_threshold_tokens: None,
        long_context_input_price_per_1m: None,
        long_context_cached_input_price_per_1m: None,
        long_context_output_price_per_1m: None,
        source_url: ANTHROPIC_PRICE_SOURCE,
    },
    PriceSeed {
        provider: "anthropic",
        model_pattern: "claude-opus-4.6",
        input_price_per_1m: 5.0,
        cached_input_price_per_1m: Some(0.5),
        output_price_per_1m: 25.0,
        long_context_threshold_tokens: None,
        long_context_input_price_per_1m: None,
        long_context_cached_input_price_per_1m: None,
        long_context_output_price_per_1m: None,
        source_url: ANTHROPIC_PRICE_SOURCE,
    },
    PriceSeed {
        provider: "anthropic",
        model_pattern: "claude-opus-4.5",
        input_price_per_1m: 5.0,
        cached_input_price_per_1m: Some(0.5),
        output_price_per_1m: 25.0,
        long_context_threshold_tokens: None,
        long_context_input_price_per_1m: None,
        long_context_cached_input_price_per_1m: None,
        long_context_output_price_per_1m: None,
        source_url: ANTHROPIC_PRICE_SOURCE,
    },
    PriceSeed {
        provider: "anthropic",
        model_pattern: "claude-opus-4",
        input_price_per_1m: 15.0,
        cached_input_price_per_1m: Some(1.5),
        output_price_per_1m: 75.0,
        long_context_threshold_tokens: None,
        long_context_input_price_per_1m: None,
        long_context_cached_input_price_per_1m: None,
        long_context_output_price_per_1m: None,
        source_url: ANTHROPIC_PRICE_SOURCE,
    },
    PriceSeed {
        provider: "anthropic",
        model_pattern: "claude-sonnet-4",
        input_price_per_1m: 3.0,
        cached_input_price_per_1m: Some(0.3),
        output_price_per_1m: 15.0,
        long_context_threshold_tokens: None,
        long_context_input_price_per_1m: None,
        long_context_cached_input_price_per_1m: None,
        long_context_output_price_per_1m: None,
        source_url: ANTHROPIC_PRICE_SOURCE,
    },
    PriceSeed {
        provider: "anthropic",
        model_pattern: "claude-haiku-4",
        input_price_per_1m: 1.0,
        cached_input_price_per_1m: Some(0.1),
        output_price_per_1m: 5.0,
        long_context_threshold_tokens: None,
        long_context_input_price_per_1m: None,
        long_context_cached_input_price_per_1m: None,
        long_context_output_price_per_1m: None,
        source_url: ANTHROPIC_PRICE_SOURCE,
    },
    PriceSeed {
        provider: "google",
        model_pattern: "gemini-2.5-pro",
        input_price_per_1m: 1.25,
        cached_input_price_per_1m: Some(0.125),
        output_price_per_1m: 10.0,
        long_context_threshold_tokens: Some(200_000),
        long_context_input_price_per_1m: Some(2.5),
        long_context_cached_input_price_per_1m: Some(0.25),
        long_context_output_price_per_1m: Some(15.0),
        source_url: GEMINI_PRICE_SOURCE,
    },
    PriceSeed {
        provider: "google",
        model_pattern: "gemini-2.5-flash",
        input_price_per_1m: 0.3,
        cached_input_price_per_1m: Some(0.03),
        output_price_per_1m: 2.5,
        long_context_threshold_tokens: None,
        long_context_input_price_per_1m: None,
        long_context_cached_input_price_per_1m: None,
        long_context_output_price_per_1m: None,
        source_url: GEMINI_PRICE_SOURCE,
    },
    PriceSeed {
        provider: "google",
        model_pattern: "gemini-2.5-flash-lite",
        input_price_per_1m: 0.1,
        cached_input_price_per_1m: Some(0.01),
        output_price_per_1m: 0.4,
        long_context_threshold_tokens: None,
        long_context_input_price_per_1m: None,
        long_context_cached_input_price_per_1m: None,
        long_context_output_price_per_1m: None,
        source_url: GEMINI_PRICE_SOURCE,
    },
];

pub(crate) fn infer_provider(model_pattern: &str) -> &str {
    let normalized = model_pattern.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return "openai";
    }
    PRICE_SEEDS
        .iter()
        .filter(|seed| normalized.starts_with(seed.model_pattern))
        .max_by_key(|seed| seed.model_pattern.len())
        .map(|seed| seed.provider)
        .unwrap_or("openai")
}

pub(crate) fn ensure_official_price_seed(storage: &Storage) -> Result<(), String> {
    let count = storage
        .count_model_price_rules_for_seed(PRICE_SEED_VERSION)
        .map_err(|err| format!("count model price seeds failed: {err}"))?;
    if count as usize >= PRICE_SEEDS.len() {
        return Ok(());
    }

    let now = now_ts();
    for (index, seed) in PRICE_SEEDS.iter().enumerate() {
        storage
            .upsert_model_price_rule(&ModelPriceRule {
                id: format!("official-{PRICE_SEED_VERSION}-{}", seed.model_pattern),
                provider: seed.provider.to_string(),
                model_pattern: seed.model_pattern.to_string(),
                match_type: "prefix".to_string(),
                billing_mode: "standard".to_string(),
                currency: "USD".to_string(),
                unit: "per_1m_tokens".to_string(),
                input_price_per_1m: Some(seed.input_price_per_1m),
                cached_input_price_per_1m: seed.cached_input_price_per_1m,
                output_price_per_1m: Some(seed.output_price_per_1m),
                reasoning_output_price_per_1m: None,
                cache_write_5m_price_per_1m: None,
                cache_write_1h_price_per_1m: None,
                cache_hit_price_per_1m: None,
                long_context_threshold_tokens: seed.long_context_threshold_tokens,
                long_context_input_price_per_1m: seed.long_context_input_price_per_1m,
                long_context_cached_input_price_per_1m: seed.long_context_cached_input_price_per_1m,
                long_context_output_price_per_1m: seed.long_context_output_price_per_1m,
                source: "official_seed".to_string(),
                source_url: Some(seed.source_url.to_string()),
                seed_version: Some(PRICE_SEED_VERSION.to_string()),
                enabled: true,
                priority: 10_000 - index as i64,
                created_at: now,
                updated_at: now,
            })
            .map_err(|err| format!("insert official model price seed failed: {err}"))?;
    }
    Ok(())
}

pub(crate) fn load_enabled_price_rules(storage: &Storage) -> Result<Vec<ModelPriceRule>, String> {
    ensure_official_price_seed(storage)?;
    storage
        .list_enabled_model_price_rules()
        .map_err(|err| format!("list enabled model price rules failed: {err}"))
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

fn rule_matches(rule: &ModelPriceRule, normalized_model: &str) -> bool {
    let pattern = rule.model_pattern.trim().to_ascii_lowercase();
    if pattern.is_empty() {
        return false;
    }
    match rule.match_type.trim().to_ascii_lowercase().as_str() {
        "exact" => normalized_model == pattern,
        "glob" | "wildcard" => wildcard_matches(&pattern, normalized_model),
        "prefix" | "" => normalized_model.starts_with(&pattern),
        _ => normalized_model.starts_with(&pattern),
    }
}

fn price_from_rule(rule: &ModelPriceRule, input_tokens: i64) -> Option<ModelPriceMatch> {
    if !rule.enabled
        || !rule.currency.eq_ignore_ascii_case("USD")
        || !rule.unit.eq_ignore_ascii_case("per_1m_tokens")
    {
        return None;
    }

    let mut input = rule.input_price_per_1m?;
    let mut cached = rule
        .cached_input_price_per_1m
        .or(rule.cache_hit_price_per_1m)
        .unwrap_or(input);
    let mut output = rule.output_price_per_1m?;

    if rule
        .long_context_threshold_tokens
        .is_some_and(|threshold| input_tokens >= threshold)
    {
        input = rule.long_context_input_price_per_1m.unwrap_or(input);
        cached = rule.long_context_cached_input_price_per_1m.unwrap_or(input);
        output = rule.long_context_output_price_per_1m.unwrap_or(output);
    }

    Some(ModelPriceMatch {
        provider: rule.provider.clone(),
        input_price_per_1m: input,
        cached_input_price_per_1m: cached,
        output_price_per_1m: output,
    })
}

pub(crate) fn resolve_model_price_from_rules(
    rules: &[ModelPriceRule],
    model: &str,
    input_tokens: i64,
) -> Option<ModelPriceMatch> {
    let normalized = model.trim().to_ascii_lowercase();
    if normalized.is_empty() || normalized == "unknown" {
        return None;
    }

    let matched = rules
        .iter()
        .filter(|rule| rule_matches(rule, &normalized))
        .max_by_key(|rule| (rule.priority, rule.model_pattern.len() as i64))?;

    price_from_rule(matched, input_tokens)
}

pub(crate) fn resolve_model_price(model: &str, input_tokens: i64) -> Option<ModelPriceMatch> {
    let normalized = model.trim().to_ascii_lowercase();
    if normalized.is_empty() || normalized == "unknown" {
        return None;
    }

    let matched = PRICE_SEEDS
        .iter()
        .filter(|seed| normalized.starts_with(seed.model_pattern))
        .max_by_key(|seed| seed.model_pattern.len())?;

    let mut input = matched.input_price_per_1m;
    let mut cached = matched
        .cached_input_price_per_1m
        .unwrap_or(matched.input_price_per_1m);
    let mut output = matched.output_price_per_1m;

    if matched
        .long_context_threshold_tokens
        .is_some_and(|threshold| input_tokens >= threshold)
    {
        input = matched
            .long_context_input_price_per_1m
            .unwrap_or(matched.input_price_per_1m);
        cached = matched
            .long_context_cached_input_price_per_1m
            .unwrap_or(input);
        output = matched
            .long_context_output_price_per_1m
            .unwrap_or(matched.output_price_per_1m);
    }

    Some(ModelPriceMatch {
        provider: matched.provider.to_string(),
        input_price_per_1m: input,
        cached_input_price_per_1m: cached,
        output_price_per_1m: output,
    })
}

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

pub(crate) fn estimate_cost(
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
    let Some(price) = resolve_model_price(model, input_tokens.max(0)) else {
        return CostEstimate {
            provider: None,
            cost_usd: None,
            price_status: "missing",
        };
    };

    estimate_cost_from_price(price, input_tokens, cached_input_tokens, output_tokens)
}

pub(crate) fn estimate_cost_with_rules(
    rules: &[ModelPriceRule],
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

    let Some(price) = resolve_model_price_from_rules(rules, model, input_tokens.max(0))
        .or_else(|| resolve_model_price(model, input_tokens.max(0)))
    else {
        return CostEstimate {
            provider: None,
            cost_usd: None,
            price_status: "missing",
        };
    };

    estimate_cost_from_price(price, input_tokens, cached_input_tokens, output_tokens)
}

pub(crate) fn estimate_remaining_tokens_from_usd_with_rules(
    rules: &[ModelPriceRule],
    model: &str,
    balance_usd: f64,
) -> Option<i64> {
    if !balance_usd.is_finite() || balance_usd < 0.0 {
        return None;
    }
    let price = resolve_model_price_from_rules(rules, model, 0)
        .or_else(|| resolve_model_price(model, 0))?;
    if balance_usd == 0.0 {
        return Some(0);
    }
    let blended_price_per_1m = price.input_price_per_1m * 0.7 + price.output_price_per_1m * 0.3;
    if blended_price_per_1m <= 0.0 {
        return None;
    }
    Some(((balance_usd / blended_price_per_1m) * 1_000_000.0).floor() as i64)
}

pub(crate) fn estimate_cost_usd_for_log(
    storage: &Storage,
    model: Option<&str>,
    input_tokens: Option<i64>,
    cached_input_tokens: Option<i64>,
    output_tokens: Option<i64>,
) -> f64 {
    let input = input_tokens.unwrap_or(0);
    let cached = cached_input_tokens.unwrap_or(0);
    let output = output_tokens.unwrap_or(0);
    let cost = storage
        .list_enabled_model_price_rules()
        .ok()
        .filter(|rules| !rules.is_empty())
        .map(|rules| estimate_cost_with_rules(&rules, model, input, cached, output))
        .unwrap_or_else(|| estimate_cost(model, input, cached, output));

    cost.cost_usd.unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_rule(
        id: &str,
        model_pattern: &str,
        match_type: &str,
        priority: i64,
        input: f64,
        cached: Option<f64>,
        output: f64,
    ) -> ModelPriceRule {
        ModelPriceRule {
            id: id.to_string(),
            provider: "test".to_string(),
            model_pattern: model_pattern.to_string(),
            match_type: match_type.to_string(),
            billing_mode: "standard".to_string(),
            currency: "USD".to_string(),
            unit: "per_1m_tokens".to_string(),
            input_price_per_1m: Some(input),
            cached_input_price_per_1m: cached,
            output_price_per_1m: Some(output),
            reasoning_output_price_per_1m: None,
            cache_write_5m_price_per_1m: None,
            cache_write_1h_price_per_1m: None,
            cache_hit_price_per_1m: None,
            long_context_threshold_tokens: None,
            long_context_input_price_per_1m: None,
            long_context_cached_input_price_per_1m: None,
            long_context_output_price_per_1m: None,
            source: "test".to_string(),
            source_url: None,
            seed_version: None,
            enabled: true,
            priority,
            created_at: 0,
            updated_at: 0,
        }
    }

    fn assert_close(actual: f64, expected: f64) {
        let delta = (actual - expected).abs();
        assert!(
            delta < 0.000_000_1,
            "expected {expected}, got {actual}, delta {delta}"
        );
    }

    #[test]
    fn resolves_exact_and_wildcard_database_rules() {
        let rules = vec![
            test_rule("wild", "vendor-*-mini", "wildcard", 10, 1.0, Some(0.1), 2.0),
            test_rule(
                "exact",
                "vendor-model-mini",
                "exact",
                100,
                3.0,
                Some(0.3),
                4.0,
            ),
        ];
        let exact =
            resolve_model_price_from_rules(&rules, "vendor-model-mini", 0).expect("exact rule");
        assert_close(exact.input_price_per_1m, 3.0);
        assert_close(exact.cached_input_price_per_1m, 0.3);
        assert_close(exact.output_price_per_1m, 4.0);

        let wildcard =
            resolve_model_price_from_rules(&rules, "vendor-other-mini", 0).expect("wildcard rule");
        assert_close(wildcard.input_price_per_1m, 1.0);
        assert_close(wildcard.output_price_per_1m, 2.0);
    }

    #[test]
    fn resolves_exact_and_snapshot_models() {
        let exact = resolve_model_price("gpt-5.4-mini", 0).expect("exact price");
        assert_eq!(exact.provider, "openai");
        assert_close(exact.input_price_per_1m, 0.75);
        assert_close(exact.cached_input_price_per_1m, 0.075);
        assert_close(exact.output_price_per_1m, 4.5);

        let snapshot = resolve_model_price("gpt-5.4-mini-2026-03-17", 0).expect("snapshot price");
        assert_close(snapshot.input_price_per_1m, 0.75);
        assert_close(snapshot.output_price_per_1m, 4.5);
    }

    #[test]
    fn prefers_more_specific_prefix_for_latest_claude_opus() {
        let latest = resolve_model_price("claude-opus-4.7-20260219", 0).expect("latest opus price");
        assert_eq!(latest.provider, "anthropic");
        assert_close(latest.input_price_per_1m, 5.0);
        assert_close(latest.cached_input_price_per_1m, 0.5);
        assert_close(latest.output_price_per_1m, 25.0);

        let legacy = resolve_model_price("claude-opus-4-20250514", 0).expect("opus 4 price");
        assert_close(legacy.input_price_per_1m, 15.0);
        assert_close(legacy.output_price_per_1m, 75.0);
    }

    #[test]
    fn returns_missing_for_unknown_models() {
        assert!(resolve_model_price("unknown-provider-model", 0).is_none());
        let cost = estimate_cost(Some("unknown-provider-model"), 100, 0, 100);
        assert_eq!(cost.price_status, "missing");
        assert!(cost.cost_usd.is_none());
        assert!(cost.provider.is_none());
    }

    #[test]
    fn zero_usd_balance_is_known_zero_tokens() {
        let tokens = estimate_remaining_tokens_from_usd_with_rules(&[], "gpt-5.4-mini", 0.0);
        assert_eq!(tokens, Some(0));
    }

    #[test]
    fn estimates_cost_with_cached_input_discount() {
        let cost = estimate_cost(Some("gpt-5.4"), 1_000, 400, 100);
        assert_eq!(cost.price_status, "ok");
        assert_eq!(cost.provider.as_deref(), Some("openai"));
        assert_close(cost.cost_usd.expect("cost"), 0.0031);
    }

    #[test]
    fn falls_back_cached_input_to_input_price_when_no_discount_exists() {
        let cost = estimate_cost(Some("gpt-5.5-pro"), 1_000, 200, 100);
        assert_eq!(cost.price_status, "ok");
        assert_close(cost.cost_usd.expect("cost"), 0.048);
    }

    #[test]
    fn applies_openai_long_context_pricing_at_threshold() {
        let standard = resolve_model_price("gpt-5.4", 271_999).expect("standard price");
        assert_close(standard.input_price_per_1m, 2.5);
        assert_close(standard.output_price_per_1m, 15.0);

        let long_context = resolve_model_price("gpt-5.4", 272_000).expect("long context price");
        assert_close(long_context.input_price_per_1m, 5.0);
        assert_close(long_context.cached_input_price_per_1m, 0.5);
        assert_close(long_context.output_price_per_1m, 22.5);
    }
}
