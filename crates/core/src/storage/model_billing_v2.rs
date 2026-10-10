use rusqlite::{params, OptionalExtension, Result};
use serde::{Deserialize, Serialize};

use super::{now_ts, Storage};

const HARDENING_MIGRATION_VERSION: &str = "113_model_billing_v2_hardening";

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ModelPriceTierV2 {
    #[serde(alias = "min_input_tokens")]
    pub min_input_tokens: i64,
    #[serde(alias = "input_microusd_per_1m")]
    pub input_microusd_per_1m: i64,
    #[serde(alias = "cached_input_microusd_per_1m")]
    pub cached_input_microusd_per_1m: i64,
    #[serde(default, alias = "cache_write_microusd_per_1m")]
    pub cache_write_microusd_per_1m: Option<i64>,
    #[serde(alias = "output_microusd_per_1m")]
    pub output_microusd_per_1m: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChargeComputationV2 {
    pub uncached_input_tokens: i64,
    pub normal_input_tokens: i64,
    pub cache_write_tokens: i64,
    pub numerator: i128,
    pub base_cost_microusd: i64,
    pub charged_cost_microusd: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChargeSnapshotInputV2 {
    pub request_log_id: i64,
    pub model_slug: String,
    /// Optional catalog slug used for price lookup while preserving `model_slug` in the snapshot.
    #[serde(default)]
    pub pricing_model_slug: Option<String>,
    pub usage_source: String,
    pub input_tokens: i64,
    pub cached_input_tokens: i64,
    #[serde(default)]
    pub cache_write_tokens: i64,
    pub output_tokens: i64,
    pub rate_multiplier_millis: i64,
    #[serde(default)]
    pub wallet_id: Option<String>,
    #[serde(default)]
    pub api_key_id: Option<String>,
    #[serde(default)]
    pub pricing_rule_id: Option<String>,
    #[serde(default)]
    pub raw_usage_json: Option<String>,
    #[serde(default)]
    pub ledger_note: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChargeSnapshotV2 {
    pub request_log_id: i64,
    pub model_id: Option<String>,
    pub model_slug: String,
    pub tier_min_input_tokens: i64,
    pub usage_source: String,
    pub input_tokens: i64,
    pub cached_input_tokens: i64,
    pub cache_write_tokens: i64,
    pub output_tokens: i64,
    pub input_microusd_per_1m: i64,
    pub cached_input_microusd_per_1m: i64,
    pub cache_write_microusd_per_1m: i64,
    pub output_microusd_per_1m: i64,
    pub rate_multiplier_millis: i64,
    pub base_cost_microusd: i64,
    pub charged_cost_microusd: i64,
    pub currency: String,
    pub created_at: i64,
}

fn checked_i64(value: i128, label: &str) -> Result<i64> {
    i64::try_from(value).map_err(|_| {
        rusqlite::Error::InvalidParameterName(format!("{label} exceeds SQLite INTEGER range"))
    })
}

fn ceil_div(value: i128, divisor: i128) -> i128 {
    if value <= 0 {
        0
    } else {
        value / divisor + i128::from(value % divisor != 0)
    }
}

pub fn compute_charge_v2(
    input_tokens: i64,
    cached_input_tokens: i64,
    cache_write_tokens: i64,
    output_tokens: i64,
    tier: &ModelPriceTierV2,
    rate_multiplier_millis: i64,
) -> Result<ChargeComputationV2> {
    if input_tokens < 0
        || cached_input_tokens < 0
        || cache_write_tokens < 0
        || output_tokens < 0
        || tier.input_microusd_per_1m < 0
        || tier.cached_input_microusd_per_1m < 0
        || tier
            .cache_write_microusd_per_1m
            .is_some_and(|rate| rate < 0)
        || tier.output_microusd_per_1m < 0
        || rate_multiplier_millis < 0
    {
        return Err(rusqlite::Error::InvalidParameterName(
            "tokens, rates, and multiplier must be non-negative".to_string(),
        ));
    }
    let cached_tokens_for_charge = cached_input_tokens.min(input_tokens);
    let uncached_input_tokens = input_tokens.saturating_sub(cached_tokens_for_charge);
    let cache_write_tokens_for_charge = cache_write_tokens.min(uncached_input_tokens);
    let normal_input_tokens = uncached_input_tokens.saturating_sub(cache_write_tokens_for_charge);
    let input_part = i128::from(normal_input_tokens)
        .checked_mul(i128::from(tier.input_microusd_per_1m))
        .ok_or_else(|| {
            rusqlite::Error::InvalidParameterName("input charge overflow".to_string())
        })?;
    let cached_part = i128::from(cached_tokens_for_charge)
        .checked_mul(i128::from(tier.cached_input_microusd_per_1m))
        .ok_or_else(|| {
            rusqlite::Error::InvalidParameterName("cached charge overflow".to_string())
        })?;
    let cache_write_part = i128::from(cache_write_tokens_for_charge)
        .checked_mul(i128::from(
            tier.cache_write_microusd_per_1m
                .unwrap_or(tier.input_microusd_per_1m),
        ))
        .ok_or_else(|| {
            rusqlite::Error::InvalidParameterName("cache write charge overflow".to_string())
        })?;
    let output_part = i128::from(output_tokens)
        .checked_mul(i128::from(tier.output_microusd_per_1m))
        .ok_or_else(|| {
            rusqlite::Error::InvalidParameterName("output charge overflow".to_string())
        })?;
    let numerator = input_part
        .checked_add(cached_part)
        .and_then(|value| value.checked_add(cache_write_part))
        .and_then(|value| value.checked_add(output_part))
        .ok_or_else(|| rusqlite::Error::InvalidParameterName("charge overflow".to_string()))?;
    let charged_numerator = numerator
        .checked_mul(i128::from(rate_multiplier_millis))
        .ok_or_else(|| {
            rusqlite::Error::InvalidParameterName("multiplied charge overflow".to_string())
        })?;
    Ok(ChargeComputationV2 {
        uncached_input_tokens,
        normal_input_tokens,
        cache_write_tokens: cache_write_tokens_for_charge,
        numerator,
        base_cost_microusd: checked_i64(ceil_div(numerator, 1_000_000), "base cost")?,
        charged_cost_microusd: checked_i64(
            ceil_div(charged_numerator, 1_000_000_000),
            "charged cost",
        )?,
    })
}

fn map_snapshot(row: &rusqlite::Row<'_>) -> Result<ChargeSnapshotV2> {
    Ok(ChargeSnapshotV2 {
        request_log_id: row.get(0)?,
        model_id: row.get(1)?,
        model_slug: row.get(2)?,
        tier_min_input_tokens: row.get(3)?,
        usage_source: row.get(4)?,
        input_tokens: row.get(5)?,
        cached_input_tokens: row.get(6)?,
        cache_write_tokens: row.get(7)?,
        output_tokens: row.get(8)?,
        input_microusd_per_1m: row.get(9)?,
        cached_input_microusd_per_1m: row.get(10)?,
        cache_write_microusd_per_1m: row.get(11)?,
        output_microusd_per_1m: row.get(12)?,
        rate_multiplier_millis: row.get(13)?,
        base_cost_microusd: row.get(14)?,
        charged_cost_microusd: row.get(15)?,
        currency: row.get(16)?,
        created_at: row.get(17)?,
    })
}

const SNAPSHOT_SELECT: &str = "SELECT request_log_id,model_id,model_slug,tier_min_input_tokens,
    usage_source,input_tokens,cached_input_tokens,cache_write_tokens,output_tokens,
    input_microusd_per_1m,cached_input_microusd_per_1m,cache_write_microusd_per_1m,
    output_microusd_per_1m,rate_multiplier_millis,
    base_cost_microusd,charged_cost_microusd,currency,created_at
  FROM request_charge_snapshots";

impl Storage {
    pub(super) fn apply_model_billing_v2_hardening_migration(&self) -> Result<()> {
        if self.has_migration(HARDENING_MIGRATION_VERSION)? {
            return Ok(());
        }
        let tx = self.conn.unchecked_transaction()?;
        tx.execute_batch(include_str!(
            "../../migrations/113_model_billing_v2_hardening.sql"
        ))?;
        let invalid_snapshots: i64 = tx.query_row(
            "SELECT COUNT(*) FROM request_charge_snapshots
             WHERE cached_input_tokens>input_tokens OR rate_multiplier_millis<0",
            [],
            |row| row.get(0),
        )?;
        if invalid_snapshots != 0 {
            return Err(rusqlite::Error::SqliteFailure(
                (),
                Some("model billing V2 hardening smoke check failed".to_string()),
            ));
        }
        tx.execute(
            "INSERT INTO model_catalog_v2_meta(key,value)
             VALUES('billing_hardening_state','complete')
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            [],
        )?;
        tx.execute(
            "INSERT INTO schema_migrations(version,applied_at) VALUES(?1,?2)",
            params![HARDENING_MIGRATION_VERSION, now_ts()],
        )?;
        tx.commit()?;
        if let Some(migrations) = self.migration_cache().as_mut() {
            migrations.insert(HARDENING_MIGRATION_VERSION.to_string());
        }
        Ok(())
    }

    pub fn select_model_price_tier_v2(
        &self,
        model_slug: &str,
        input_tokens: i64,
    ) -> Result<Option<(String, ModelPriceTierV2)>> {
        if input_tokens < 0 {
            return Err(rusqlite::Error::InvalidParameterName(
                "input tokens cannot be negative".to_string(),
            ));
        }
        self.conn
            .query_row(
                "SELECT m.id,t.min_input_tokens,t.input_microusd_per_1m,
                        t.cached_input_microusd_per_1m,t.cache_write_microusd_per_1m,
                        t.output_microusd_per_1m
                 FROM models m
                 JOIN model_prices p ON p.model_id=m.id AND p.price_status<>'missing'
                 JOIN model_price_tiers t ON t.model_id=m.id AND t.min_input_tokens<=?2
                 WHERE m.slug=?1 COLLATE NOCASE
                 ORDER BY t.min_input_tokens DESC LIMIT 1",
                params![model_slug.trim(), input_tokens],
                |row| {
                    Ok((
                        row.get(0)?,
                        ModelPriceTierV2 {
                            min_input_tokens: row.get(1)?,
                            input_microusd_per_1m: row.get(2)?,
                            cached_input_microusd_per_1m: row.get(3)?,
                            cache_write_microusd_per_1m: row.get(4)?,
                            output_microusd_per_1m: row.get(5)?,
                        },
                    ))
                },
            )
            .optional()
    }

    pub fn get_charge_snapshot_v2(&self, request_log_id: i64) -> Result<Option<ChargeSnapshotV2>> {
        self.conn
            .query_row(
                &format!("{SNAPSHOT_SELECT} WHERE request_log_id=?1"),
                [request_log_id],
                map_snapshot,
            )
            .optional()
    }

    pub fn record_charge_snapshot_v2(
        &self,
        input: &ChargeSnapshotInputV2,
    ) -> Result<ChargeSnapshotV2> {
        if !matches!(input.usage_source.as_str(), "actual" | "estimated") {
            return Err(rusqlite::Error::InvalidParameterName(
                "usage_source must be actual or estimated".to_string(),
            ));
        }
        let tx = self.conn.unchecked_transaction()?;
        if let Some(existing) = tx
            .query_row(
                &format!("{SNAPSHOT_SELECT} WHERE request_log_id=?1"),
                [input.request_log_id],
                map_snapshot,
            )
            .optional()?
        {
            tx.execute(
                "UPDATE request_token_stats
                 SET estimated_cost_usd=CAST(?2 AS REAL)/1000000.0
                 WHERE request_log_id=?1",
                params![input.request_log_id, existing.base_cost_microusd],
            )?;
            tx.commit()?;
            return Ok(existing);
        }
        let pricing_model_slug = input
            .pricing_model_slug
            .as_deref()
            .map(str::trim)
            .filter(|slug| !slug.is_empty())
            .unwrap_or_else(|| input.model_slug.trim());
        let price_status: Option<String> = tx
            .query_row(
                "SELECT p.price_status FROM models m JOIN model_prices p ON p.model_id=m.id
                 WHERE m.slug=?1 COLLATE NOCASE",
                [pricing_model_slug],
                |row| row.get(0),
            )
            .optional()?;
        match price_status.as_deref() {
            None => return Err(rusqlite::Error::QueryReturnedNoRows),
            Some("missing") => {
                return Err(rusqlite::Error::InvalidParameterName(
                    "model_price_missing".to_string(),
                ))
            }
            _ => {}
        }
        let (model_id, tier) = tx.query_row(
            "SELECT m.id,t.min_input_tokens,t.input_microusd_per_1m,
                    t.cached_input_microusd_per_1m,t.cache_write_microusd_per_1m,
                    t.output_microusd_per_1m
             FROM models m JOIN model_price_tiers t ON t.model_id=m.id AND t.min_input_tokens<=?2
             WHERE m.slug=?1 COLLATE NOCASE ORDER BY t.min_input_tokens DESC LIMIT 1",
            params![pricing_model_slug, input.input_tokens],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    ModelPriceTierV2 {
                        min_input_tokens: row.get(1)?,
                        input_microusd_per_1m: row.get(2)?,
                        cached_input_microusd_per_1m: row.get(3)?,
                        cache_write_microusd_per_1m: row.get(4)?,
                        output_microusd_per_1m: row.get(5)?,
                    },
                ))
            },
        )?;
        let computation = compute_charge_v2(
            input.input_tokens,
            input.cached_input_tokens,
            input.cache_write_tokens,
            input.output_tokens,
            &tier,
            input.rate_multiplier_millis,
        )?;
        let cached_input_tokens = input.cached_input_tokens.min(input.input_tokens);
        let cache_write_tokens = input
            .cache_write_tokens
            .min(input.input_tokens.saturating_sub(cached_input_tokens));
        let cache_write_microusd_per_1m = tier
            .cache_write_microusd_per_1m
            .unwrap_or(tier.input_microusd_per_1m);
        let now = now_ts();
        tx.execute(
            "INSERT INTO request_charge_snapshots(request_log_id,model_id,model_slug,
               tier_min_input_tokens,usage_source,input_tokens,cached_input_tokens,cache_write_tokens,
               output_tokens,input_microusd_per_1m,cached_input_microusd_per_1m,
               cache_write_microusd_per_1m,output_microusd_per_1m,
               rate_multiplier_millis,base_cost_microusd,charged_cost_microusd,currency,created_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,'USD',?17)",
            params![
                input.request_log_id,
                model_id,
                input.model_slug.trim(),
                tier.min_input_tokens,
                input.usage_source,
                input.input_tokens,
                cached_input_tokens,
                cache_write_tokens,
                input.output_tokens,
                tier.input_microusd_per_1m,
                tier.cached_input_microusd_per_1m,
                cache_write_microusd_per_1m,
                tier.output_microusd_per_1m,
                input.rate_multiplier_millis,
                computation.base_cost_microusd,
                computation.charged_cost_microusd,
                now
            ],
        )?;
        tx.execute(
            "UPDATE request_token_stats
             SET estimated_cost_usd=CAST(?2 AS REAL)/1000000.0
             WHERE request_log_id=?1",
            params![input.request_log_id, computation.base_cost_microusd],
        )?;
        if let Some(wallet_id) = input
            .wallet_id
            .as_deref()
            .filter(|id| !id.trim().is_empty())
        {
            let prior_ledger: Option<String> = tx
                .query_row(
                    "SELECT id FROM app_wallet_ledger_entries
                 WHERE request_log_id=?1 AND entry_kind='request_charge' LIMIT 1",
                    [input.request_log_id],
                    |row| row.get(0),
                )
                .optional()?;
            if prior_ledger.is_some() {
                return Err(rusqlite::Error::InvalidParameterName(
                    "request charge ledger exists without snapshot".to_string(),
                ));
            }
            let charge = computation.charged_cost_microusd;
            let changed = tx.execute(
                "UPDATE app_wallets SET balance_credit_micros=balance_credit_micros-?2,updated_at=?3
                 WHERE id=?1 AND status='active'
                   AND balance_credit_micros-?2>=frozen_credit_micros",
                params![wallet_id,charge,now],
            )?;
            if changed != 1 {
                return Err(rusqlite::Error::InvalidParameterName(
                    "wallet_insufficient_balance".to_string(),
                ));
            }
            let balance_after: i64 = tx.query_row(
                "SELECT balance_credit_micros FROM app_wallets WHERE id=?1",
                [wallet_id],
                |row| row.get(0),
            )?;
            tx.execute(
                "INSERT INTO app_wallet_ledger_entries(id,wallet_id,entry_kind,
                   amount_credit_micros,balance_after_credit_micros,request_log_id,api_key_id,
                   pricing_rule_id,raw_usage_json,note,created_by_user_id,created_at)
                 VALUES(?1,?2,'request_charge',?3,?4,?5,?6,?7,?8,?9,NULL,?10)",
                params![
                    format!("wl_request_{}", input.request_log_id),
                    wallet_id,
                    -charge,
                    balance_after,
                    input.request_log_id,
                    input.api_key_id,
                    input.pricing_rule_id,
                    input.raw_usage_json,
                    input.ledger_note.as_deref().unwrap_or("model_catalog_v2"),
                    now
                ],
            )?;
        }
        tx.commit()?;
        self.get_charge_snapshot_v2(input.request_log_id)?
            .ok_or(rusqlite::Error::QueryReturnedNoRows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_formula_charges_read_write_and_normal_input_once() {
        let tier = ModelPriceTierV2 {
            min_input_tokens: 0,
            input_microusd_per_1m: 2_000_000,
            cached_input_microusd_per_1m: 200_000,
            cache_write_microusd_per_1m: Some(2_500_000),
            output_microusd_per_1m: 10_000_000,
        };
        let result = compute_charge_v2(100, 40, 20, 10, &tier, 1_500).unwrap();
        assert_eq!(result.uncached_input_tokens, 60);
        assert_eq!(result.normal_input_tokens, 40);
        assert_eq!(result.cache_write_tokens, 20);
        assert_eq!(result.numerator, 238_000_000);
        assert_eq!(result.base_cost_microusd, 238);
        assert_eq!(result.charged_cost_microusd, 357);
        let free = compute_charge_v2(100, 40, 20, 10, &tier, 0).unwrap();
        assert_eq!(free.base_cost_microusd, 238);
        assert_eq!(free.charged_cost_microusd, 0);
    }

    #[test]
    fn missing_cache_write_rate_falls_back_to_input_rate() {
        let tier = ModelPriceTierV2 {
            min_input_tokens: 0,
            input_microusd_per_1m: 2_000_000,
            cached_input_microusd_per_1m: 200_000,
            cache_write_microusd_per_1m: None,
            output_microusd_per_1m: 10_000_000,
        };
        let result = compute_charge_v2(100, 40, 20, 10, &tier, 1_000).unwrap();
        assert_eq!(result.numerator, 228_000_000);
    }

    #[test]
    fn gpt56_cache_write_prices_apply_at_the_long_context_boundary() {
        let storage = Storage::open_in_memory().unwrap();
        storage.init().unwrap();
        let (_, short) = storage
            .select_model_price_tier_v2("gpt-5.6-sol", 272_000)
            .unwrap()
            .unwrap();
        let (_, long) = storage
            .select_model_price_tier_v2("gpt-5.6-sol", 272_001)
            .unwrap()
            .unwrap();
        assert_eq!(short.cache_write_microusd_per_1m, Some(6_250_000));
        assert_eq!(long.cache_write_microusd_per_1m, Some(12_500_000));

        storage.conn.execute("INSERT INTO request_logs(request_path,method,created_at) VALUES('/v1/responses','POST',1)",[]).unwrap();
        let snapshot = storage
            .record_charge_snapshot_v2(&ChargeSnapshotInputV2 {
                request_log_id: storage.conn.last_insert_rowid(),
                model_slug: "gpt-5.6-sol".into(),
                usage_source: "actual".into(),
                input_tokens: 100,
                cached_input_tokens: 40,
                cache_write_tokens: 20,
                output_tokens: 10,
                rate_multiplier_millis: 1_000,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(snapshot.cache_write_tokens, 20);
        assert_eq!(snapshot.cache_write_microusd_per_1m, 6_250_000);
        assert_eq!(snapshot.base_cost_microusd, 645);
    }

    #[test]
    fn snapshot_can_preserve_request_alias_while_using_catalog_pricing() {
        let storage = Storage::open_in_memory().unwrap();
        storage.init().unwrap();
        storage.conn.execute("INSERT INTO request_logs(request_path,method,model,created_at) VALUES('/v1/responses','POST','gpt-reserve',1)",[]).unwrap();

        let snapshot = storage
            .record_charge_snapshot_v2(&ChargeSnapshotInputV2 {
                request_log_id: storage.conn.last_insert_rowid(),
                model_slug: "gpt-reserve".into(),
                pricing_model_slug: Some("gpt-5.6-luna".into()),
                usage_source: "actual".into(),
                input_tokens: 1_000,
                output_tokens: 1_000,
                rate_multiplier_millis: 1_000,
                ..Default::default()
            })
            .unwrap();

        assert_eq!(snapshot.model_slug, "gpt-reserve");
        assert_eq!(snapshot.base_cost_microusd, 1_400);
    }

    #[test]
    fn extreme_integer_inputs_return_an_error_without_panicking() {
        let tier = ModelPriceTierV2 {
            min_input_tokens: 0,
            input_microusd_per_1m: i64::MAX,
            cached_input_microusd_per_1m: i64::MAX,
            cache_write_microusd_per_1m: None,
            output_microusd_per_1m: i64::MAX,
        };
        let error = compute_charge_v2(i64::MAX, 0, 0, i64::MAX, &tier, i64::MAX)
            .expect_err("overflow must be rejected");
        assert!(error.to_string().contains("overflow"));
    }

    #[test]
    fn cached_above_input_is_clamped_and_tier_boundary_is_exact() {
        let storage = Storage::open_in_memory().unwrap();
        storage.init().unwrap();
        let (_, low) = storage
            .select_model_price_tier_v2("gpt-5.5", 271_999)
            .unwrap()
            .unwrap();
        let (_, high) = storage
            .select_model_price_tier_v2("gpt-5.5", 272_000)
            .unwrap()
            .unwrap();
        assert_eq!(low.min_input_tokens, 0);
        assert_eq!(high.min_input_tokens, 272_000);
        let result = compute_charge_v2(10, 20, 20, 0, &low, 1_000).unwrap();
        assert_eq!(result.uncached_input_tokens, 0);
        assert_eq!(result.cache_write_tokens, 0);
        assert_eq!(
            result.numerator,
            10_i128 * i128::from(low.cached_input_microusd_per_1m)
        );
    }

    #[test]
    fn missing_price_is_rejected_and_snapshot_is_idempotent() {
        let storage = Storage::open_in_memory().unwrap();
        storage.init().unwrap();
        storage.conn.execute("INSERT INTO request_logs(request_path,method,created_at) VALUES('/v1/responses','POST',1)",[]).unwrap();
        let request_log_id = storage.conn.last_insert_rowid();
        let mut input = ChargeSnapshotInputV2 {
            request_log_id,
            model_slug: "codex-auto-review".into(),
            usage_source: "actual".into(),
            input_tokens: 1,
            cached_input_tokens: 0,
            output_tokens: 1,
            rate_multiplier_millis: 1_000,
            ..Default::default()
        };
        assert!(storage
            .record_charge_snapshot_v2(&input)
            .unwrap_err()
            .to_string()
            .contains("model_price_missing"));
        input.model_slug = "gpt-6-luna".into();
        input.input_tokens = 10;
        input.cached_input_tokens = 20;
        let first = storage.record_charge_snapshot_v2(&input).unwrap();
        let second = storage.record_charge_snapshot_v2(&input).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.cached_input_tokens, 10);
    }

    #[test]
    fn image25_composite_input_prices_do_not_create_unified_charge_snapshots() {
        let storage = Storage::open_in_memory().unwrap();
        storage.init().unwrap();

        for slug in ["gpt-image-2.5-sunburst", "gpt-image-2.5-flare"] {
            storage.conn.execute("INSERT INTO request_logs(request_path,method,model,created_at) VALUES('/v1/images/generations','POST',?1,1)",[slug]).unwrap();
            let request_log_id = storage.conn.last_insert_rowid();
            let error = storage
                .record_charge_snapshot_v2(&ChargeSnapshotInputV2 {
                    request_log_id,
                    model_slug: slug.into(),
                    usage_source: "actual".into(),
                    input_tokens: 1_000,
                    cached_input_tokens: 100,
                    output_tokens: 2_000,
                    rate_multiplier_millis: 1_000,
                    ..Default::default()
                })
                .expect_err("mixed text/image pricing must not use one input rate");
            assert!(error.to_string().contains("model_price_missing"));
            assert!(storage
                .get_charge_snapshot_v2(request_log_id)
                .unwrap()
                .is_none());
        }
    }

    #[test]
    fn zero_multiplier_records_a_free_ledger_and_is_idempotent() {
        let storage = Storage::open_in_memory().unwrap();
        storage.init().unwrap();
        storage
            .conn
            .execute(
                "INSERT INTO app_wallets(id,owner_kind,owner_id,balance_credit_micros,
                   frozen_credit_micros,status,created_at,updated_at)
                 VALUES('wallet-free','user','free-user',0,0,'active',1,1)",
                [],
            )
            .unwrap();
        storage.conn.execute("INSERT INTO request_logs(request_path,method,created_at) VALUES('/v1/responses','POST',1)",[]).unwrap();
        let request_log_id = storage.conn.last_insert_rowid();
        let input = ChargeSnapshotInputV2 {
            request_log_id,
            model_slug: "gpt-6-luna".into(),
            usage_source: "estimated".into(),
            input_tokens: 100,
            cached_input_tokens: 0,
            output_tokens: 0,
            rate_multiplier_millis: 0,
            wallet_id: Some("wallet-free".into()),
            ..Default::default()
        };
        let first = storage.record_charge_snapshot_v2(&input).unwrap();
        let second = storage.record_charge_snapshot_v2(&input).unwrap();
        assert_eq!(first, second);
        assert!(first.base_cost_microusd > 0);
        assert_eq!(first.charged_cost_microusd, 0);
        assert_eq!(storage.request_charge_ledger_entry_count().unwrap(), 1);
        let duplicate = storage.conn.execute(
            "INSERT INTO app_wallet_ledger_entries(id,wallet_id,entry_kind,
               amount_credit_micros,balance_after_credit_micros,request_log_id,created_at)
             VALUES('duplicate-charge','wallet-free','request_charge',0,0,?1,2)",
            [request_log_id],
        );
        assert!(
            duplicate.is_err(),
            "partial unique index must reject duplicates"
        );
        let balance: i64 = storage
            .conn
            .query_row(
                "SELECT balance_credit_micros FROM app_wallets WHERE id='wallet-free'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(balance, 0);
    }

    #[test]
    fn insufficient_wallet_balance_rolls_back_snapshot_and_ledger() {
        let storage = Storage::open_in_memory().unwrap();
        storage.init().unwrap();
        storage
            .conn
            .execute(
                "INSERT INTO app_wallets(id,owner_kind,owner_id,balance_credit_micros,
                   frozen_credit_micros,status,created_at,updated_at)
                 VALUES('wallet-low','user','low-user',1,0,'active',1,1)",
                [],
            )
            .unwrap();
        storage.conn.execute("INSERT INTO request_logs(request_path,method,created_at) VALUES('/v1/responses','POST',1)",[]).unwrap();
        let request_log_id = storage.conn.last_insert_rowid();
        let error = storage
            .record_charge_snapshot_v2(&ChargeSnapshotInputV2 {
                request_log_id,
                model_slug: "gpt-6-luna".into(),
                usage_source: "actual".into(),
                input_tokens: 1_000,
                cached_input_tokens: 0,
                output_tokens: 1_000,
                rate_multiplier_millis: 1_000,
                wallet_id: Some("wallet-low".into()),
                ..Default::default()
            })
            .expect_err("insufficient wallet must fail");
        assert!(error.to_string().contains("wallet_insufficient_balance"));
        assert!(storage
            .get_charge_snapshot_v2(request_log_id)
            .unwrap()
            .is_none());
        assert_eq!(storage.request_charge_ledger_entry_count().unwrap(), 0);
        let balance: i64 = storage
            .conn
            .query_row(
                "SELECT balance_credit_micros FROM app_wallets WHERE id='wallet-low'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(balance, 1);
    }
}
