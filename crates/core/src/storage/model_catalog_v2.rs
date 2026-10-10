use std::collections::HashSet;

use rusqlite::{params, Connection, OptionalExtension, Result, Transaction};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::{now_ts, ModelGroupAccess, ModelGroupModel, ModelPriceTierV2, Storage};

const MIGRATION_VERSION: &str = "112_model_catalog_v2";
const GPT56_PRICING_MIGRATION_VERSION: &str = "114_model_catalog_gpt56_prices";
const CODEX_METADATA_MIGRATION_VERSION: &str = "115_model_catalog_codex_metadata";
const GPT56_OFFICIAL_PRICING_MIGRATION_VERSION: &str = "121_model_catalog_gpt56_official_prices";
const GPT56_CURRENT_PRICING_MIGRATION_VERSION: &str = "126_model_catalog_gpt56_current_prices";
const GPT56_OFFICIAL_PRICE_SOURCE: &str = "https://developers.openai.com/api/docs/models/compare";
const GPT6_ASTRA_MIGRATION_VERSION: &str = "131_model_catalog_gpt6_astra";
const GPT6_ASTRA_MIGRATION_REVISION: i64 = 8;
const GPT56_METADATA_FIX_MIGRATION_VERSION: &str = "132_model_catalog_gpt56_metadata_fix";
const MODEL_CATALOG_REVISION9_MIGRATION_VERSION: &str = "137_model_catalog_revision9";
const MODEL_CATALOG_REVISION9: i64 = 9;
const MODEL_CATALOG_REVISION10_MIGRATION_VERSION: &str = "138_model_catalog_gpt61_sol";
const MODEL_CATALOG_REVISION10: i64 = 10;
const RETIRED_CODEX_MODEL_SLUGS_REVISION9: &[&str] = &["gpt-5.4", "gpt-5.4-mini", "gpt-5.2"];
const GPT56_METADATA_FIX_CONTEXT_WINDOW: i64 = 272_000;
const GPT56_METADATA_FIX_MAX_CONTEXT_WINDOW: i64 = 872_000;
const GPT56_METADATA_FIX_SHELL_TYPE: &str = "unified_exec";
const GPT6_ASTRA_SLUG: &str = "gpt-6-astra";
const GPT6_ASTRA_PRICE_SOURCE: &str = "https://developers.openai.com/api/docs/models/gpt-6-astra";
const GPT61_SOL_SLUG: &str = "gpt-6.1-sol";
const GPT61_SOL_PRICE_SOURCE: &str = "https://developers.openai.com/api/docs/models/gpt-6.1-sol";
#[cfg(test)]
const GPT_IMAGE_2_PRICE_SOURCE: &str =
    "https://developers.openai.com/api/docs/pricing#image-generation";
const DEFAULT_MODEL_GROUP_ID: &str = "mg_default";
const DELETED_BUILTIN_META_PREFIX: &str = "deleted_builtin_model:";
const KNOWN_MODEL_CATALOG_SOURCE_SHA256: &[&str] = &[
    "2e714bc5650848e1ca1a7684f331e42d1933ea8935593b78f1805e519be7deb0",
    "4914710f243f67c3b32895bfc91e51798cddd2f4020f4c82230fef2798eb0076",
    "bd20b8f4dca9c2408ad3b1f41723689d03e98d450beef29549d08a550512e64a",
    "ccc924d8388571e3ecfa722589e383b5de8038e0b02a096dd618c140066bced3",
    "ff99a8e61a58adaebb872d1970f746994cdbd1191e7adf74587cc1e6e01503d2",
    "e526f3b9048f16c07d3c1e15a1d43eec16a78f7fe18e6552419d02d8ccc8e064",
    "e993784040f785e736674a573347b353d6cd69b648398dbc56a5a548a72d6dd8",
];
#[derive(Debug, Clone, Deserialize)]
struct BuiltinCatalogFixture {
    revision: i64,
    source_sha256: String,
    models: Vec<BuiltinModelSeed>,
}

#[derive(Debug, Clone, Deserialize)]
struct BuiltinModelSeed {
    slug: String,
    display_name: String,
    description: String,
    visibility: String,
    priority: i64,
    default_reasoning_effort: Option<String>,
    context_window: Option<i64>,
    max_context_window: Option<i64>,
    capabilities: Value,
    price_status: String,
    price_source: Option<String>,
    price_tiers: Vec<ModelPriceTierV2>,
}

#[derive(Debug, Clone, Deserialize)]
struct Gpt6AstraMigrationFixture {
    revision: i64,
    source_sha256: String,
    astra: BuiltinModelSeed,
    sol_slug: String,
    sol_capabilities: Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ModelPriceV2 {
    pub price_status: String,
    #[serde(default)]
    pub price_source: Option<String>,
    #[serde(default)]
    pub input_microusd_per_1m: Option<i64>,
    #[serde(default)]
    pub cached_input_microusd_per_1m: Option<i64>,
    #[serde(default)]
    pub cache_write_microusd_per_1m: Option<i64>,
    #[serde(default)]
    pub output_microusd_per_1m: Option<i64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ModelRouteV2 {
    #[serde(default)]
    pub id: String,
    pub source_kind: String,
    pub source_id: String,
    pub upstream_model: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub priority: i64,
    #[serde(default = "default_weight")]
    pub weight: i64,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ModelFastPolicyV2 {
    #[default]
    Passthrough,
    Filter,
    Force,
    Block,
}

impl ModelFastPolicyV2 {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Passthrough => "passthrough",
            Self::Filter => "filter",
            Self::Force => "force",
            Self::Block => "block",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedModelV2 {
    pub id: String,
    pub slug: String,
    pub display_name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub family: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub origin: String,
    pub enabled: bool,
    pub supported_in_api: bool,
    pub visibility: String,
    pub sort_order: i64,
    #[serde(default)]
    pub context_window: Option<i64>,
    #[serde(default)]
    pub max_context_window: Option<i64>,
    #[serde(default)]
    pub default_reasoning_effort: Option<String>,
    #[serde(default)]
    pub capabilities: Value,
    pub instructions_mode: String,
    #[serde(default)]
    pub instructions_text: Option<String>,
    #[serde(default)]
    pub fast_policy: ModelFastPolicyV2,
    #[serde(default)]
    pub builtin_revision: Option<i64>,
    #[serde(default)]
    pub user_edited: bool,
    pub price: ModelPriceV2,
    #[serde(default)]
    pub price_tiers: Vec<ModelPriceTierV2>,
    #[serde(default)]
    pub routes: Vec<ModelRouteV2>,
    #[serde(default)]
    pub permission_group_ids: Vec<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedModelV2Upsert {
    #[serde(default)]
    pub previous_slug: Option<String>,
    pub model: ManagedModelV2,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ManagedModelPriceV2Update {
    pub slug: String,
    pub price: ModelPriceV2,
    #[serde(default)]
    pub price_tiers: Vec<ModelPriceTierV2>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedModelRouteEnsureV2 {
    pub model_slug: String,
    pub route: ModelRouteV2,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ManagedModelRouteEnsureResultV2 {
    pub created_models: Vec<String>,
    pub added_routes: Vec<String>,
    pub unchanged_routes: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ManagedModelStateV2Update {
    pub slug: String,
    pub enabled: bool,
    pub visibility: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ManagedModelBatchStateV2Update {
    pub slugs: Vec<String>,
    pub enabled: bool,
    pub visibility: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ModelCatalogV2Stats {
    pub total: i64,
    pub enabled: i64,
    pub builtin: i64,
    pub custom: i64,
    pub price_missing: i64,
    pub missing_route: i64,
}

fn default_true() -> bool {
    true
}

fn default_weight() -> i64 {
    1
}

fn legacy_fixture() -> BuiltinCatalogFixture {
    serde_json::from_str(include_str!("../../seeds/model_catalog_v2_2026_07_10.json"))
        .expect("legacy model catalog V2 fixture must be valid")
}

fn revision9_fixture() -> BuiltinCatalogFixture {
    let fixture: BuiltinCatalogFixture =
        serde_json::from_str(include_str!("../../seeds/model_catalog_v2_2026_09_24.json"))
            .expect("model catalog V2 revision 9 fixture must be valid");
    assert_eq!(
        fixture.revision, MODEL_CATALOG_REVISION9,
        "model catalog V2 revision 9 fixture must remain frozen"
    );
    fixture
}

fn fixture() -> BuiltinCatalogFixture {
    gpt61_sol_migration_fixture()
}

fn gpt61_sol_migration_fixture() -> BuiltinCatalogFixture {
    let fixture: BuiltinCatalogFixture =
        serde_json::from_str(include_str!("../../seeds/model_catalog_v2_2026_09_30.json"))
            .expect("GPT-6.1 Sol migration fixture must be valid");
    assert_eq!(
        fixture.revision, MODEL_CATALOG_REVISION10,
        "GPT-6.1 Sol migration fixture revision must remain frozen"
    );
    let sol = fixture
        .models
        .iter()
        .find(|model| model.slug == GPT61_SOL_SLUG)
        .expect("GPT-6.1 Sol must be in revision 10");
    assert_eq!(sol.price_source.as_deref(), Some(GPT61_SOL_PRICE_SOURCE));
    fixture
}

fn should_replace_fixture_source_hash(
    current: Option<&str>,
    fixture: &BuiltinCatalogFixture,
) -> bool {
    match current.map(str::trim).filter(|value| !value.is_empty()) {
        None => true,
        Some(value) if value == fixture.source_sha256 => true,
        Some(value) => KNOWN_MODEL_CATALOG_SOURCE_SHA256.contains(&value),
    }
}

fn gpt6_astra_migration_fixture() -> Gpt6AstraMigrationFixture {
    let fixture: Gpt6AstraMigrationFixture =
        serde_json::from_str(include_str!("../../seeds/model_catalog_gpt6_astra_v8.json"))
            .expect("GPT-6 Astra migration fixture must be valid");
    assert_eq!(
        fixture.revision, GPT6_ASTRA_MIGRATION_REVISION,
        "GPT-6 Astra migration fixture revision must remain frozen"
    );
    assert!(fixture.astra.slug.eq_ignore_ascii_case(GPT6_ASTRA_SLUG));
    assert_eq!(
        fixture.astra.price_source.as_deref(),
        Some(GPT6_ASTRA_PRICE_SOURCE)
    );
    fixture
}

fn sol_matches_gpt6_astra_migration_or_metadata_fix(
    model: &ManagedModelV2,
    migration_fixture: &Gpt6AstraMigrationFixture,
) -> bool {
    if model.capabilities == migration_fixture.sol_capabilities {
        return true;
    }

    let mut fixed_capabilities = migration_fixture.sol_capabilities.clone();
    fixed_capabilities["shell_type"] = Value::String(GPT56_METADATA_FIX_SHELL_TYPE.to_string());
    model.context_window == Some(GPT56_METADATA_FIX_CONTEXT_WINDOW)
        && model.max_context_window == Some(GPT56_METADATA_FIX_MAX_CONTEXT_WINDOW)
        && model.capabilities == fixed_capabilities
}

fn clean_optional(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.trim().is_empty())
}

fn stable_hash(value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    digest
        .iter()
        .take(12)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn connection_has_column(conn: &Connection, table: &str, column: &str) -> Result<bool> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let columns = stmt.query_map([], |row| row.get::<_, String>(1))?;
    for current in columns {
        if current?.eq_ignore_ascii_case(column) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn builtin_id(slug: &str) -> String {
    format!("builtin:{}", slug.trim().to_ascii_lowercase())
}

fn custom_id(slug: &str) -> String {
    format!("custom:{}", stable_hash(&slug.trim().to_ascii_lowercase()))
}

fn route_id(model_id: &str, route: &ModelRouteV2) -> String {
    let identity = format!(
        "{model_id}\0{}\0{}\0{}",
        route.source_kind, route.source_id, route.upstream_model
    );
    format!("route:{}", stable_hash(&identity))
}

pub fn validate_managed_model_price_v2(
    price: &ModelPriceV2,
    price_tiers: &[ModelPriceTierV2],
) -> Result<()> {
    let status = price.price_status.as_str();
    if !matches!(status, "official" | "estimated" | "custom" | "missing") {
        return Err(rusqlite::Error::InvalidParameterName(
            "invalid price_status".to_string(),
        ));
    }
    let required_rates = [
        price.input_microusd_per_1m,
        price.cached_input_microusd_per_1m,
        price.output_microusd_per_1m,
    ];
    if required_rates.iter().flatten().any(|rate| *rate < 0)
        || price
            .cache_write_microusd_per_1m
            .is_some_and(|rate| rate < 0)
    {
        return Err(rusqlite::Error::InvalidParameterName(
            "model prices cannot be negative".to_string(),
        ));
    }
    if status == "missing" {
        if required_rates.iter().any(Option::is_some)
            || price.cache_write_microusd_per_1m.is_some()
            || !price_tiers.is_empty()
        {
            return Err(rusqlite::Error::InvalidParameterName(
                "missing price must not contain rates or tiers".to_string(),
            ));
        }
        return Ok(());
    }
    if required_rates.iter().any(Option::is_none) {
        return Err(rusqlite::Error::InvalidParameterName(
            "priced model requires input, cached input, and output rates".to_string(),
        ));
    }
    let base = price_tiers
        .iter()
        .find(|tier| tier.min_input_tokens == 0)
        .ok_or_else(|| {
            rusqlite::Error::InvalidParameterName(
                "priced model requires a min_input_tokens=0 tier".to_string(),
            )
        })?;
    if Some(base.input_microusd_per_1m) != price.input_microusd_per_1m
        || Some(base.cached_input_microusd_per_1m) != price.cached_input_microusd_per_1m
        || base.cache_write_microusd_per_1m != price.cache_write_microusd_per_1m
        || Some(base.output_microusd_per_1m) != price.output_microusd_per_1m
    {
        return Err(rusqlite::Error::InvalidParameterName(
            "model base price must match the zero-threshold tier".to_string(),
        ));
    }
    let mut thresholds = HashSet::new();
    for tier in price_tiers {
        if tier.min_input_tokens < 0
            || tier.input_microusd_per_1m < 0
            || tier.cached_input_microusd_per_1m < 0
            || tier
                .cache_write_microusd_per_1m
                .is_some_and(|rate| rate < 0)
            || tier.output_microusd_per_1m < 0
            || !thresholds.insert(tier.min_input_tokens)
        {
            return Err(rusqlite::Error::InvalidParameterName(
                "invalid or duplicate model price tier".to_string(),
            ));
        }
    }
    Ok(())
}

fn validate_price(model: &ManagedModelV2) -> Result<()> {
    validate_managed_model_price_v2(&model.price, &model.price_tiers)?;
    if model.price.price_status == "missing" && !model.permission_group_ids.is_empty() {
        return Err(rusqlite::Error::InvalidParameterName(
            "model_price_missing: missing-price models cannot join billing groups".to_string(),
        ));
    }
    Ok(())
}

fn validate_route(route: &ModelRouteV2) -> Result<()> {
    if !matches!(route.source_kind.as_str(), "account_pool" | "aggregate_api")
        || route.source_id.trim().is_empty()
        || route.upstream_model.trim().is_empty()
        || route.weight <= 0
    {
        return Err(rusqlite::Error::InvalidParameterName(
            "invalid model route".to_string(),
        ));
    }
    Ok(())
}

pub fn validate_managed_model_v2(model: &ManagedModelV2) -> Result<()> {
    if model.slug.trim().is_empty() || model.display_name.trim().is_empty() {
        return Err(rusqlite::Error::InvalidParameterName(
            "model slug and display name are required".to_string(),
        ));
    }
    if !matches!(model.origin.as_str(), "builtin" | "custom") {
        return Err(rusqlite::Error::InvalidParameterName(
            "invalid model origin".to_string(),
        ));
    }
    if !matches!(model.visibility.as_str(), "list" | "hide") {
        return Err(rusqlite::Error::InvalidParameterName(
            "invalid model visibility".to_string(),
        ));
    }
    if !matches!(
        model.instructions_mode.as_str(),
        "passthrough" | "fallback" | "override"
    ) {
        return Err(rusqlite::Error::InvalidParameterName(
            "invalid instructions mode".to_string(),
        ));
    }
    if model.instructions_mode == "override"
        && model
            .instructions_text
            .as_deref()
            .is_none_or(|text| text.trim().is_empty())
    {
        return Err(rusqlite::Error::InvalidParameterName(
            "override instructions require non-empty text".to_string(),
        ));
    }
    for route in &model.routes {
        validate_route(route)?;
    }
    validate_price(model)
}

fn map_model(conn: &Connection, row: &rusqlite::Row<'_>) -> Result<ManagedModelV2> {
    let id: String = row.get(0)?;
    let tags_json: String = row.get(7)?;
    let capabilities_json: String = row.get(16)?;
    let mut model = ManagedModelV2 {
        id: id.clone(),
        slug: row.get(1)?,
        display_name: row.get(2)?,
        description: row.get(3)?,
        provider: row.get(4)?,
        family: row.get(5)?,
        category: row.get(6)?,
        tags: serde_json::from_str(&tags_json).unwrap_or_default(),
        origin: row.get(8)?,
        enabled: row.get::<_, i64>(9)? != 0,
        supported_in_api: row.get::<_, i64>(10)? != 0,
        visibility: row.get(11)?,
        sort_order: row.get(12)?,
        context_window: row.get(13)?,
        max_context_window: row.get(14)?,
        default_reasoning_effort: row.get(15)?,
        capabilities: serde_json::from_str(&capabilities_json)
            .unwrap_or(Value::Object(Default::default())),
        instructions_mode: row.get(17)?,
        instructions_text: row.get(18)?,
        fast_policy: match row.get::<_, String>(29)?.as_str() {
            "passthrough" => ModelFastPolicyV2::Passthrough,
            "filter" => ModelFastPolicyV2::Filter,
            "force" => ModelFastPolicyV2::Force,
            "block" => ModelFastPolicyV2::Block,
            value => {
                return Err(rusqlite::Error::InvalidParameterName(format!(
                    "invalid model fast policy: {value}"
                )))
            }
        },
        builtin_revision: row.get(19)?,
        user_edited: row.get::<_, i64>(20)? != 0,
        created_at: row.get(21)?,
        updated_at: row.get(22)?,
        price: ModelPriceV2 {
            price_status: row.get(23)?,
            price_source: row.get(24)?,
            input_microusd_per_1m: row.get(25)?,
            cached_input_microusd_per_1m: row.get(26)?,
            cache_write_microusd_per_1m: row.get(27)?,
            output_microusd_per_1m: row.get(28)?,
        },
        price_tiers: Vec::new(),
        routes: Vec::new(),
        permission_group_ids: Vec::new(),
    };
    model.price_tiers = list_tiers(conn, &id)?;
    model.routes = list_routes(conn, &id)?;
    model.permission_group_ids = list_permission_groups(conn, &id)?;
    Ok(model)
}

const MODEL_SELECT: &str = "SELECT
    m.id,m.slug,m.display_name,m.description,m.provider,m.family,m.category,m.tags_json,
    m.origin,m.enabled,m.supported_in_api,m.visibility,m.sort_order,m.context_window,
    m.max_context_window,m.default_reasoning_effort,m.capabilities_json,
    m.instructions_mode,m.instructions_text,m.builtin_revision,m.user_edited,m.created_at,m.updated_at,
    p.price_status,p.price_source,p.input_microusd_per_1m,p.cached_input_microusd_per_1m,
    p.cache_write_microusd_per_1m,p.output_microusd_per_1m,m.fast_policy
  FROM models m JOIN model_prices p ON p.model_id=m.id";

fn list_tiers(conn: &Connection, model_id: &str) -> Result<Vec<ModelPriceTierV2>> {
    let mut stmt = conn.prepare(
        "SELECT min_input_tokens,input_microusd_per_1m,cached_input_microusd_per_1m,
                cache_write_microusd_per_1m,output_microusd_per_1m
         FROM model_price_tiers WHERE model_id=?1 ORDER BY min_input_tokens ASC",
    )?;
    stmt.query_map([model_id], |row| {
        Ok(ModelPriceTierV2 {
            min_input_tokens: row.get(0)?,
            input_microusd_per_1m: row.get(1)?,
            cached_input_microusd_per_1m: row.get(2)?,
            cache_write_microusd_per_1m: row.get(3)?,
            output_microusd_per_1m: row.get(4)?,
        })
    })?
    .collect()
}

fn list_routes(conn: &Connection, model_id: &str) -> Result<Vec<ModelRouteV2>> {
    let mut stmt = conn.prepare(
        "SELECT id,source_kind,source_id,upstream_model,enabled,priority,weight
         FROM model_routes WHERE model_id=?1 ORDER BY priority DESC,id ASC",
    )?;
    stmt.query_map([model_id], |row| {
        Ok(ModelRouteV2 {
            id: row.get(0)?,
            source_kind: row.get(1)?,
            source_id: row.get(2)?,
            upstream_model: row.get(3)?,
            enabled: row.get::<_, i64>(4)? != 0,
            priority: row.get(5)?,
            weight: row.get(6)?,
        })
    })?
    .collect()
}

fn list_permission_groups(conn: &Connection, model_id: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT group_id FROM model_group_models_v2
         WHERE model_id=?1 AND enabled=1 ORDER BY group_id ASC",
    )?;
    stmt.query_map([model_id], |row| row.get(0))?.collect()
}

fn get_managed_model_v2_with_conn(conn: &Connection, slug: &str) -> Result<Option<ManagedModelV2>> {
    let sql = format!("{MODEL_SELECT} WHERE m.slug=?1 COLLATE NOCASE");
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query([slug.trim()])?;
    rows.next()?.map(|row| map_model(conn, row)).transpose()
}

fn insert_seed(
    conn: &Connection,
    fixture: &BuiltinCatalogFixture,
    seed: &BuiltinModelSeed,
    now: i64,
) -> Result<()> {
    let deleted_meta_key = deleted_builtin_meta_key(&seed.slug);
    let was_deleted = conn
        .query_row(
            "SELECT 1 FROM model_catalog_v2_meta WHERE key=?1",
            [&deleted_meta_key],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if was_deleted {
        return Ok(());
    }
    let proposed_id = builtin_id(&seed.slug);
    let inserted_model = conn.execute(
        "INSERT OR IGNORE INTO models (
           id,slug,display_name,description,origin,enabled,supported_in_api,visibility,
           sort_order,context_window,max_context_window,default_reasoning_effort,
           capabilities_json,instructions_mode,instructions_text,builtin_revision,user_edited,
           created_at,updated_at
        ) VALUES (?1,?2,?3,?4,'builtin',1,1,?5,?6,?7,?8,?9,?10,'passthrough',NULL,?11,0,?12,?12)",
        params![
            proposed_id,
            seed.slug,
            seed.display_name,
            seed.description,
            seed.visibility,
            seed.priority,
            seed.context_window,
            seed.max_context_window,
            seed.default_reasoning_effort,
            serde_json::to_string(&seed.capabilities).expect("serialize seed capabilities"),
            fixture.revision,
            now
        ],
    )?;
    let (id, origin, user_edited) = conn.query_row(
        "SELECT id,origin,user_edited FROM models WHERE slug=?1 COLLATE NOCASE",
        [&seed.slug],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, bool>(2)?,
            ))
        },
    )?;
    if origin != "builtin" {
        // A user-created model owns its slug. Keep it intact when a later
        // builtin fixture introduces the same slug; the builtin can be
        // restored after the custom row is explicitly deleted.
        if origin == "custom" {
            return Ok(());
        }
        return Err(rusqlite::Error::InvalidParameterName(format!(
            "builtin seed slug {} is owned by a custom model",
            seed.slug
        )));
    }
    // Existing catalog rows own their dependent pricing, tiers, and routes.
    // Seed data initializes those records only when the model itself is new.
    if inserted_model == 0 || user_edited {
        return Ok(());
    }
    let base = seed
        .price_tiers
        .iter()
        .find(|tier| tier.min_input_tokens == 0);
    let inserted_price = conn.execute(
        "INSERT OR IGNORE INTO model_prices (
           model_id,currency,input_microusd_per_1m,cached_input_microusd_per_1m,
           output_microusd_per_1m,price_status,price_source,created_at,updated_at
         ) VALUES (?1,'USD',?2,?3,?4,?5,?6,?7,?7)",
        params![
            id,
            base.map(|tier| tier.input_microusd_per_1m),
            base.map(|tier| tier.cached_input_microusd_per_1m),
            base.map(|tier| tier.output_microusd_per_1m),
            seed.price_status,
            seed.price_source,
            now
        ],
    )?;
    let has_cache_write_price =
        connection_has_column(conn, "model_prices", "cache_write_microusd_per_1m")?;
    if inserted_price > 0 && has_cache_write_price {
        conn.execute(
            "UPDATE model_prices SET cache_write_microusd_per_1m=?2 WHERE model_id=?1",
            params![id, base.and_then(|tier| tier.cache_write_microusd_per_1m)],
        )?;
    }
    for tier in &seed.price_tiers {
        let inserted_tier = conn.execute(
            "INSERT OR IGNORE INTO model_price_tiers (
               model_id,min_input_tokens,input_microusd_per_1m,
               cached_input_microusd_per_1m,output_microusd_per_1m
             ) VALUES (?1,?2,?3,?4,?5)",
            params![
                id,
                tier.min_input_tokens,
                tier.input_microusd_per_1m,
                tier.cached_input_microusd_per_1m,
                tier.output_microusd_per_1m
            ],
        )?;
        if inserted_tier > 0 && has_cache_write_price {
            conn.execute(
                "UPDATE model_price_tiers SET cache_write_microusd_per_1m=?3
                 WHERE model_id=?1 AND min_input_tokens=?2",
                params![id, tier.min_input_tokens, tier.cache_write_microusd_per_1m],
            )?;
        }
    }
    let route = ModelRouteV2 {
        source_kind: "account_pool".to_string(),
        source_id: "default".to_string(),
        upstream_model: seed.slug.clone(),
        enabled: true,
        priority: 0,
        weight: 1,
        ..Default::default()
    };
    conn.execute(
        "INSERT OR IGNORE INTO model_routes (
           id,model_id,source_kind,source_id,upstream_model,enabled,priority,weight,created_at,updated_at
         ) VALUES (?1,?2,?3,?4,?5,1,0,1,?6,?6)",
        params![route_id(&id, &route), id, route.source_kind, route.source_id, route.upstream_model, now],
    )?;
    Ok(())
}

fn seed_missing_with_fixture(conn: &Connection, fixture: &BuiltinCatalogFixture) -> Result<()> {
    let stored_catalog_revision = conn
        .query_row(
            "SELECT CAST(value AS INTEGER) FROM model_catalog_v2_meta
             WHERE key='builtin_revision'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .optional()?
        .unwrap_or_default();
    let max_unedited_builtin_revision = conn.query_row(
        "SELECT COALESCE(MAX(builtin_revision),0) FROM models
         WHERE origin='builtin' AND user_edited=0",
        [],
        |row| row.get::<_, i64>(0),
    )?;
    let effective_catalog_revision = stored_catalog_revision.max(max_unedited_builtin_revision);
    if effective_catalog_revision > fixture.revision {
        if stored_catalog_revision < effective_catalog_revision {
            conn.execute(
                "INSERT INTO model_catalog_v2_meta(key,value) VALUES('builtin_revision',?1)
                 ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                [effective_catalog_revision.to_string()],
            )?;
        }
        return Ok(());
    }

    let now = now_ts();
    for seed in &fixture.models {
        insert_seed(conn, fixture, seed, now)?;
    }
    conn.execute(
        "INSERT INTO model_catalog_v2_meta(key,value) VALUES('builtin_revision',?1)
         ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        [fixture.revision.to_string()],
    )?;
    let current_source_sha256: Option<String> = conn
        .query_row(
            "SELECT value FROM model_catalog_v2_meta WHERE key='fixture_sha256'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    if should_replace_fixture_source_hash(current_source_sha256.as_deref(), fixture) {
        conn.execute(
            "INSERT INTO model_catalog_v2_meta(key,value) VALUES('fixture_sha256',?1)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            [fixture.source_sha256.as_str()],
        )?;
    }
    Ok(())
}

fn seed_missing(conn: &Connection) -> Result<()> {
    let fixture = fixture();
    seed_missing_with_fixture(conn, &fixture)
}

fn reconcile_unedited_builtin_seed_revision9(
    conn: &Connection,
    previous_seed: Option<&BuiltinModelSeed>,
    seed: &BuiltinModelSeed,
    now: i64,
) -> Result<()> {
    let Some(current) = get_managed_model_v2_with_conn(conn, &seed.slug)? else {
        return Ok(());
    };
    if current.origin != "builtin"
        || current.user_edited
        || current.builtin_revision.unwrap_or_default() >= MODEL_CATALOG_REVISION9
    {
        return Ok(());
    }

    // Price sync and route management do not necessarily set user_edited.
    // Only advance pricing that still exactly matches the previous built-in
    // fixture, and never rebuild an existing model's routes during catalog
    // reconciliation.
    let replace_pricing = previous_seed
        .map(|previous| builtin_seed_pricing_matches(&current, previous))
        .unwrap_or(false);
    conn.execute(
        "UPDATE models
         SET display_name=?2,description=?3,visibility=?4,sort_order=?5,
             context_window=?6,max_context_window=?7,default_reasoning_effort=?8,
             capabilities_json=?9,builtin_revision=?10,updated_at=?11
         WHERE id=?1 AND origin='builtin' AND user_edited=0
           AND COALESCE(builtin_revision,0) < ?10",
        params![
            current.id,
            seed.display_name,
            seed.description,
            seed.visibility,
            seed.priority,
            seed.context_window,
            seed.max_context_window,
            seed.default_reasoning_effort,
            serde_json::to_string(&seed.capabilities)
                .expect("serialize built-in seed capabilities"),
            MODEL_CATALOG_REVISION9,
            now
        ],
    )?;
    if replace_pricing
        && seed
            .price_tiers
            .iter()
            .any(|tier| tier.min_input_tokens == 0)
    {
        replace_builtin_seed_pricing(conn, &current.id, seed, now)?;
    }
    Ok(())
}

fn builtin_seed_metadata_matches(model: &ManagedModelV2, seed: &BuiltinModelSeed) -> bool {
    model.enabled
        && model.supported_in_api
        && model.display_name == seed.display_name
        && model.description.as_deref() == Some(seed.description.as_str())
        && model.visibility == seed.visibility
        && model.sort_order == seed.priority
        && model.context_window == seed.context_window
        && model.max_context_window == seed.max_context_window
        && model.default_reasoning_effort == seed.default_reasoning_effort
        && model.capabilities == seed.capabilities
        && model.instructions_mode == "passthrough"
        && model
            .instructions_text
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .is_none()
        && model.fast_policy == ModelFastPolicyV2::Passthrough
}

fn has_only_default_builtin_route(model: &ManagedModelV2) -> bool {
    matches!(
        model.routes.as_slice(),
        [route]
            if route.source_kind == "account_pool"
                && route.source_id == "default"
                && route.upstream_model.eq_ignore_ascii_case(&model.slug)
                && route.enabled
                && route.priority == 0
                && route.weight == 1
    )
}

fn retire_codex_model_revision9(
    conn: &Connection,
    previous_seed: &BuiltinModelSeed,
    now: i64,
) -> Result<()> {
    conn.execute(
        "INSERT INTO model_catalog_v2_meta(key,value) VALUES(?1,?2)
         ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![
            deleted_builtin_meta_key(&previous_seed.slug),
            now.to_string()
        ],
    )?;

    let Some(current) = get_managed_model_v2_with_conn(conn, &previous_seed.slug)? else {
        return Ok(());
    };
    if current.origin != "builtin" && current.origin != "custom" {
        return Err(rusqlite::Error::InvalidParameterName(format!(
            "retired model {} has unsupported origin {}",
            current.slug, current.origin
        )));
    }
    if current.origin == "custom" {
        return Ok(());
    }
    let api_key_referenced = conn.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM api_keys
             WHERE TRIM(model_slug)=?1 COLLATE NOCASE
           ) OR EXISTS(
             SELECT 1 FROM api_key_profiles
             WHERE TRIM(default_model)=?1 COLLATE NOCASE
           )",
        [current.slug.trim()],
        |row| row.get::<_, bool>(0),
    )?;
    let permission_group_referenced = conn.query_row(
        "SELECT EXISTS(
           SELECT 1 FROM model_group_models_v2 WHERE model_id=?1
         )",
        [&current.id],
        |row| row.get::<_, bool>(0),
    )?;
    if !current.user_edited
        && builtin_seed_metadata_matches(&current, previous_seed)
        && builtin_seed_pricing_matches(&current, previous_seed)
        && has_only_default_builtin_route(&current)
        && !permission_group_referenced
        && !api_key_referenced
    {
        conn.execute("DELETE FROM models WHERE id=?1", [&current.id])?;
        return Ok(());
    }

    conn.execute(
        "UPDATE models
         SET origin='custom',builtin_revision=NULL,user_edited=1,updated_at=?2
         WHERE id=?1",
        params![current.id, now],
    )?;
    conn.execute(
        "DELETE FROM model_routes
         WHERE model_id=?1 AND source_kind='account_pool' AND source_id='default'
           AND upstream_model=?2 COLLATE NOCASE AND enabled=1 AND priority=0 AND weight=1",
        params![current.id, previous_seed.slug],
    )?;
    Ok(())
}

fn deleted_builtin_meta_key(slug: &str) -> String {
    format!(
        "{DELETED_BUILTIN_META_PREFIX}{}",
        slug.trim().to_ascii_lowercase()
    )
}

fn migrate_legacy_catalog(conn: &Connection) -> Result<()> {
    let mut stmt = conn.prepare(
        "SELECT slug,display_name,source_kind,user_edited,description,default_reasoning_level,
                visibility,supported_in_api,context_window,extra_json,sort_index,updated_at
         FROM model_catalog_models
         WHERE scope='default' AND TRIM(slug) <> ''
         ORDER BY sort_index ASC,slug ASC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, bool>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, Option<String>>(5)?,
            row.get::<_, Option<String>>(6)?,
            row.get::<_, Option<bool>>(7)?,
            row.get::<_, Option<i64>>(8)?,
            row.get::<_, String>(9)?,
            row.get::<_, i64>(10)?,
            row.get::<_, i64>(11)?,
        ))
    })?;
    let legacy = rows.collect::<Result<Vec<_>>>()?;
    drop(stmt);
    for (
        slug,
        display_name,
        source_kind,
        user_edited,
        description,
        reasoning,
        visibility,
        supported,
        context,
        extra,
        sort,
        updated_at,
    ) in legacy
    {
        let existing: Option<(String, String)> = conn
            .query_row(
                "SELECT id,origin FROM models WHERE slug=?1 COLLATE NOCASE",
                [&slug],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if let Some((id, origin)) = existing {
            if origin == "builtin" && user_edited {
                let normalized_visibility = match visibility.as_deref() {
                    Some("hide" | "hidden") => "hide",
                    _ => "list",
                };
                let enabled = !matches!(visibility.as_deref(), Some("disabled" | "unavailable"));
                conn.execute(
                    "UPDATE models SET display_name=?2,description=?3,enabled=?4,visibility=?5,
                       supported_in_api=?6,context_window=COALESCE(?7,context_window),
                       default_reasoning_effort=COALESCE(?8,default_reasoning_effort),
                       sort_order=?9,user_edited=1,updated_at=?10 WHERE id=?1",
                    params![
                        id,
                        display_name,
                        description,
                        enabled,
                        normalized_visibility,
                        supported.unwrap_or(true),
                        context,
                        reasoning,
                        sort,
                        updated_at
                    ],
                )?;
            }
            continue;
        }
        if source_kind != "custom" && !user_edited {
            continue;
        }
        let id = custom_id(&slug);
        let capabilities = serde_json::from_str::<Value>(&extra)
            .ok()
            .filter(|value| value.is_object())
            .unwrap_or(Value::Object(Default::default()));
        conn.execute(
            "INSERT INTO models (
               id,slug,display_name,description,origin,enabled,supported_in_api,visibility,
               sort_order,context_window,max_context_window,default_reasoning_effort,
               capabilities_json,instructions_mode,instructions_text,builtin_revision,user_edited,
               created_at,updated_at
             ) VALUES (?1,?2,?3,?4,'custom',?5,?6,?7,?8,?9,?9,?10,?11,'passthrough',NULL,NULL,1,?12,?12)",
            params![id, slug, display_name, description,
                !matches!(visibility.as_deref(), Some("disabled" | "unavailable")),
                supported.unwrap_or(true), if matches!(visibility.as_deref(), Some("hide" | "hidden")) { "hide" } else { "list" },
                sort, context, reasoning, capabilities.to_string(), updated_at],
        )?;
        conn.execute(
            "INSERT INTO model_prices(model_id,currency,price_status,created_at,updated_at)
             VALUES(?1,'USD','missing',?2,?2)",
            params![id, updated_at],
        )?;
    }
    Ok(())
}

fn usd_rate_to_microusd(value: f64) -> Option<i64> {
    if !value.is_finite() || value < 0.0 {
        return None;
    }
    let scaled = value * 1_000_000.0;
    if scaled > i64::MAX as f64 {
        return None;
    }
    Some(scaled.round() as i64)
}

fn migrate_legacy_prices(conn: &Connection) -> Result<()> {
    let mut stmt = conn.prepare(
        "SELECT model_pattern,input_price_per_1m,cached_input_price_per_1m,output_price_per_1m,
                long_context_threshold_tokens,long_context_input_price_per_1m,
                long_context_cached_input_price_per_1m,long_context_output_price_per_1m,
                source,updated_at
         FROM model_price_rules
         WHERE enabled=1 AND match_type='exact' AND source='custom'
         ORDER BY priority ASC,updated_at ASC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<f64>>(1)?,
            row.get::<_, Option<f64>>(2)?,
            row.get::<_, Option<f64>>(3)?,
            row.get::<_, Option<i64>>(4)?,
            row.get::<_, Option<f64>>(5)?,
            row.get::<_, Option<f64>>(6)?,
            row.get::<_, Option<f64>>(7)?,
            row.get::<_, String>(8)?,
            row.get::<_, i64>(9)?,
        ))
    })?;
    let rules = rows.collect::<Result<Vec<_>>>()?;
    drop(stmt);
    for (
        slug,
        input,
        cached,
        output,
        threshold,
        long_input,
        long_cached,
        long_output,
        source,
        updated_at,
    ) in rules
    {
        let (Some(input), Some(cached), Some(output)) = (
            input.and_then(usd_rate_to_microusd),
            cached.and_then(usd_rate_to_microusd),
            output.and_then(usd_rate_to_microusd),
        ) else {
            continue;
        };
        let Some(model_id) = conn
            .query_row(
                "SELECT id FROM models WHERE slug=?1 COLLATE NOCASE",
                [&slug],
                |row| row.get::<_, String>(0),
            )
            .optional()?
        else {
            continue;
        };
        conn.execute(
            "UPDATE model_prices SET input_microusd_per_1m=?2,cached_input_microusd_per_1m=?3,
               output_microusd_per_1m=?4,price_status='custom',price_source=?5,updated_at=?6
             WHERE model_id=?1",
            params![model_id, input, cached, output, source, updated_at],
        )?;
        conn.execute(
            "DELETE FROM model_price_tiers WHERE model_id=?1",
            [&model_id],
        )?;
        conn.execute(
            "INSERT INTO model_price_tiers(model_id,min_input_tokens,input_microusd_per_1m,
               cached_input_microusd_per_1m,output_microusd_per_1m) VALUES(?1,0,?2,?3,?4)",
            params![model_id, input, cached, output],
        )?;
        if let (Some(threshold), Some(long_input), Some(long_cached), Some(long_output)) = (
            threshold,
            long_input.and_then(usd_rate_to_microusd),
            long_cached.and_then(usd_rate_to_microusd),
            long_output.and_then(usd_rate_to_microusd),
        ) {
            if threshold > 0 {
                conn.execute(
                    "INSERT INTO model_price_tiers(model_id,min_input_tokens,input_microusd_per_1m,
                       cached_input_microusd_per_1m,output_microusd_per_1m) VALUES(?1,?2,?3,?4,?5)",
                    params![model_id, threshold, long_input, long_cached, long_output],
                )?;
            }
        }
    }
    Ok(())
}

fn migrate_legacy_routes(conn: &Connection) -> Result<()> {
    let now = now_ts();
    let mut stmt = conn.prepare(
        "SELECT r.platform_model_slug,r.source_kind,r.source_id,r.upstream_model,
                r.enabled,r.priority,r.weight
         FROM model_source_mappings r
         JOIN aggregate_apis a ON a.id=r.source_id
         LEFT JOIN model_source_mapping_preferences pref
           ON pref.source_kind=r.source_kind AND pref.source_id=r.source_id
          AND pref.upstream_model=r.upstream_model
         WHERE r.enabled=1 AND r.source_kind='aggregate_api' AND pref.preference IS NULL
           AND (r.upstream_model<>r.platform_model_slug OR r.priority<>0 OR r.weight<>1
             OR EXISTS(SELECT 1 FROM quota_source_model_assignments q
                       WHERE q.source_kind=r.source_kind AND q.source_id=r.source_id
                         AND q.model_slug=r.platform_model_slug))
         ORDER BY r.priority DESC,r.id ASC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, bool>(4)?,
            row.get::<_, i64>(5)?,
            row.get::<_, i64>(6)?,
        ))
    })?;
    let mappings = rows.collect::<Result<Vec<_>>>()?;
    drop(stmt);
    for (slug, old_kind, source_id, upstream_model, enabled, priority, weight) in mappings {
        let Some(model_id) = conn
            .query_row(
                "SELECT id FROM models WHERE slug=?1 COLLATE NOCASE",
                [&slug],
                |row| row.get::<_, String>(0),
            )
            .optional()?
        else {
            continue;
        };
        let (source_kind, source_id) = match old_kind.as_str() {
            "openai_account" => ("account_pool", "default".to_string()),
            "aggregate_api" => ("aggregate_api", source_id),
            _ => continue,
        };
        let route = ModelRouteV2 {
            source_kind: source_kind.to_string(),
            source_id,
            upstream_model,
            enabled,
            priority,
            weight: weight.max(1),
            ..Default::default()
        };
        conn.execute(
            "INSERT OR IGNORE INTO model_routes(id,model_id,source_kind,source_id,upstream_model,
               enabled,priority,weight,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?9)",
            params![route_id(&model_id,&route),model_id,route.source_kind,route.source_id,route.upstream_model,route.enabled,route.priority,route.weight,now],
        )?;
    }
    Ok(())
}

fn migrate_legacy_groups(conn: &Connection) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO model_group_models_v2(group_id,model_id,enabled,
           rate_multiplier_millis,created_at,updated_at)
         SELECT old.group_id,m.id,old.enabled,old.rate_multiplier_millis,old.created_at,old.updated_at
         FROM model_group_models old
         JOIN model_groups g ON g.id=old.group_id AND g.is_default=0
         JOIN models m ON m.slug=old.platform_model_slug COLLATE NOCASE
         JOIN model_prices p ON p.model_id=m.id AND p.price_status<>'missing'",
        [],
    )?;
    Ok(())
}

fn migration_smoke_for_fixture(conn: &Connection, fixture: &BuiltinCatalogFixture) -> Result<()> {
    let expected_seed_count = fixture.models.len() as i64;
    let mut covered_seed_count = 0_i64;
    for seed in &fixture.models {
        let origin = conn
            .query_row(
                "SELECT origin FROM models WHERE slug=?1 COLLATE NOCASE",
                [&seed.slug],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        let tombstoned = conn
            .query_row(
                "SELECT 1 FROM model_catalog_v2_meta WHERE key=?1",
                [deleted_builtin_meta_key(&seed.slug)],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        let covered = matches!(origin.as_deref(), Some("builtin") | Some("custom")) || tombstoned;
        if covered {
            covered_seed_count += 1;
        }
    }
    let orphan_count: i64 = conn.query_row(
        "SELECT (SELECT COUNT(*) FROM model_prices p LEFT JOIN models m ON m.id=p.model_id WHERE m.id IS NULL)
          +(SELECT COUNT(*) FROM model_price_tiers p LEFT JOIN models m ON m.id=p.model_id WHERE m.id IS NULL)
          +(SELECT COUNT(*) FROM model_routes r LEFT JOIN models m ON m.id=r.model_id WHERE m.id IS NULL)",
        [], |row| row.get(0))?;
    if covered_seed_count != expected_seed_count || orphan_count != 0 {
        return Err(rusqlite::Error::SqliteFailure(
            (),
            Some("model catalog V2 smoke check failed".to_string()),
        ));
    }
    Ok(())
}

fn builtin_seed_pricing_matches(model: &ManagedModelV2, seed: &BuiltinModelSeed) -> bool {
    let Some(base_price) = seed
        .price_tiers
        .iter()
        .find(|tier| tier.min_input_tokens == 0)
    else {
        return false;
    };
    let expected_price = ModelPriceV2 {
        price_status: seed.price_status.clone(),
        price_source: seed.price_source.clone(),
        input_microusd_per_1m: Some(base_price.input_microusd_per_1m),
        cached_input_microusd_per_1m: Some(base_price.cached_input_microusd_per_1m),
        cache_write_microusd_per_1m: base_price.cache_write_microusd_per_1m,
        output_microusd_per_1m: Some(base_price.output_microusd_per_1m),
    };

    model.price == expected_price && model.price_tiers == seed.price_tiers
}

fn replace_builtin_seed_pricing(
    conn: &Connection,
    id: &str,
    seed: &BuiltinModelSeed,
    now: i64,
) -> Result<()> {
    let base = seed
        .price_tiers
        .iter()
        .find(|tier| tier.min_input_tokens == 0)
        .ok_or_else(|| {
            rusqlite::Error::InvalidParameterName(
                "priced built-in seed requires a min_input_tokens=0 tier".to_string(),
            )
        })?;
    conn.execute(
        "INSERT INTO model_prices(
           model_id,currency,input_microusd_per_1m,cached_input_microusd_per_1m,
           cache_write_microusd_per_1m,output_microusd_per_1m,price_status,price_source,
           created_at,updated_at
         ) VALUES(?1,'USD',?2,?3,?4,?5,?6,?7,?8,?8)
         ON CONFLICT(model_id) DO UPDATE SET
           currency='USD',input_microusd_per_1m=excluded.input_microusd_per_1m,
           cached_input_microusd_per_1m=excluded.cached_input_microusd_per_1m,
           cache_write_microusd_per_1m=excluded.cache_write_microusd_per_1m,
           output_microusd_per_1m=excluded.output_microusd_per_1m,
           price_status=excluded.price_status,price_source=excluded.price_source,
           updated_at=excluded.updated_at",
        params![
            id,
            base.input_microusd_per_1m,
            base.cached_input_microusd_per_1m,
            base.cache_write_microusd_per_1m,
            base.output_microusd_per_1m,
            seed.price_status,
            seed.price_source,
            now
        ],
    )?;
    conn.execute("DELETE FROM model_price_tiers WHERE model_id=?1", [id])?;
    for tier in &seed.price_tiers {
        conn.execute(
            "INSERT INTO model_price_tiers(
               model_id,min_input_tokens,input_microusd_per_1m,
               cached_input_microusd_per_1m,cache_write_microusd_per_1m,
               output_microusd_per_1m
             ) VALUES(?1,?2,?3,?4,?5,?6)",
            params![
                id,
                tier.min_input_tokens,
                tier.input_microusd_per_1m,
                tier.cached_input_microusd_per_1m,
                tier.cache_write_microusd_per_1m,
                tier.output_microusd_per_1m
            ],
        )?;
    }
    Ok(())
}

fn replace_unedited_builtin_seed(
    conn: &Connection,
    revision: i64,
    seed: &BuiltinModelSeed,
    now: i64,
) -> Result<()> {
    let id = conn.query_row(
        "SELECT id FROM models
         WHERE slug=?1 COLLATE NOCASE AND origin='builtin' AND user_edited=0
           AND COALESCE(builtin_revision,0) <= ?2",
        params![seed.slug.as_str(), revision],
        |row| row.get::<_, String>(0),
    )?;
    conn.execute(
        "UPDATE models
         SET display_name=?2,description=?3,provider=NULL,family=NULL,category=NULL,
             tags_json='[]',enabled=1,supported_in_api=1,visibility=?4,sort_order=?5,
             context_window=?6,max_context_window=?7,default_reasoning_effort=?8,
             capabilities_json=?9,instructions_mode='passthrough',instructions_text=NULL,
             builtin_revision=?10,fast_policy='passthrough',updated_at=?11
         WHERE id=?1 AND origin='builtin' AND user_edited=0
           AND COALESCE(builtin_revision,0) <= ?10",
        params![
            id,
            seed.display_name,
            seed.description,
            seed.visibility,
            seed.priority,
            seed.context_window,
            seed.max_context_window,
            seed.default_reasoning_effort,
            serde_json::to_string(&seed.capabilities)
                .expect("serialize built-in seed capabilities"),
            revision,
            now
        ],
    )?;
    replace_builtin_seed_pricing(conn, &id, seed, now)?;
    conn.execute("DELETE FROM model_routes WHERE model_id=?1", [&id])?;
    let route = ModelRouteV2 {
        source_kind: "account_pool".to_string(),
        source_id: "default".to_string(),
        upstream_model: seed.slug.clone(),
        enabled: true,
        priority: 0,
        weight: 1,
        ..Default::default()
    };
    conn.execute(
        "INSERT INTO model_routes(
           id,model_id,source_kind,source_id,upstream_model,enabled,priority,weight,
           created_at,updated_at
         ) VALUES(?1,?2,?3,?4,?5,1,0,1,?6,?6)",
        params![
            route_id(&id, &route),
            id,
            route.source_kind,
            route.source_id,
            route.upstream_model,
            now
        ],
    )?;
    Ok(())
}

fn migration_smoke(conn: &Connection) -> Result<()> {
    migration_smoke_for_fixture(conn, &fixture())
}

fn seeded_builtin_matches(model: &ManagedModelV2, revision: i64, seed: &BuiltinModelSeed) -> bool {
    let Some(base_price) = seed
        .price_tiers
        .iter()
        .find(|tier| tier.min_input_tokens == 0)
    else {
        return false;
    };
    let expected_price = ModelPriceV2 {
        price_status: seed.price_status.clone(),
        price_source: seed.price_source.clone(),
        input_microusd_per_1m: Some(base_price.input_microusd_per_1m),
        cached_input_microusd_per_1m: Some(base_price.cached_input_microusd_per_1m),
        cache_write_microusd_per_1m: base_price.cache_write_microusd_per_1m,
        output_microusd_per_1m: Some(base_price.output_microusd_per_1m),
    };
    let expected_route = ModelRouteV2 {
        source_kind: "account_pool".to_string(),
        source_id: "default".to_string(),
        upstream_model: seed.slug.clone(),
        enabled: true,
        priority: 0,
        weight: 1,
        ..Default::default()
    };

    model.slug.eq_ignore_ascii_case(&seed.slug)
        && model.display_name == seed.display_name
        && model.description.as_deref() == Some(seed.description.as_str())
        && model.provider.is_none()
        && model.family.is_none()
        && model.category.is_none()
        && model.tags.is_empty()
        && model.origin == "builtin"
        && model.enabled
        && model.supported_in_api
        && model.visibility == seed.visibility
        && model.sort_order == seed.priority
        && model.context_window == seed.context_window
        && model.max_context_window == seed.max_context_window
        && model.default_reasoning_effort == seed.default_reasoning_effort
        && model.capabilities == seed.capabilities
        && model.instructions_mode == "passthrough"
        && model.instructions_text.is_none()
        && model.fast_policy == ModelFastPolicyV2::Passthrough
        && model.builtin_revision == Some(revision)
        && !model.user_edited
        && model.price == expected_price
        && model.price_tiers == seed.price_tiers
        && model.routes.len() == 1
        && model.routes[0].source_kind == expected_route.source_kind
        && model.routes[0].source_id == expected_route.source_id
        && model.routes[0]
            .upstream_model
            .eq_ignore_ascii_case(&expected_route.upstream_model)
        && model.routes[0].enabled == expected_route.enabled
        && model.routes[0].priority == expected_route.priority
        && model.routes[0].weight == expected_route.weight
}

fn write_model(tx: &Transaction<'_>, input: &ManagedModelV2Upsert) -> Result<String> {
    let mut model = input.model.clone();
    model.slug = model.slug.trim().to_string();
    model.display_name = model.display_name.trim().to_string();
    model.description = clean_optional(model.description);
    model.provider = clean_optional(model.provider);
    model.family = clean_optional(model.family);
    model.category = clean_optional(model.category);
    model.instructions_text = clean_optional(model.instructions_text);
    validate_managed_model_v2(&model)?;

    let previous = input.previous_slug.as_deref().unwrap_or(&model.slug).trim();
    let existing = tx
        .query_row(
            "SELECT id,origin,created_at FROM models WHERE slug=?1 COLLATE NOCASE",
            [previous],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )
        .optional()?;
    let now = now_ts();
    let (id, origin, created_at) = match existing {
        Some((id, origin, created_at)) => {
            if origin == "builtin" && model.slug.to_lowercase() != previous.to_lowercase() {
                return Err(rusqlite::Error::InvalidParameterName(
                    "builtin model slug cannot be renamed".to_string(),
                ));
            }
            if model.origin != origin {
                return Err(rusqlite::Error::InvalidParameterName(
                    "model origin cannot be changed".to_string(),
                ));
            }
            (id, origin, created_at)
        }
        None => {
            if model.origin != "custom" {
                return Err(rusqlite::Error::InvalidParameterName(
                    "only custom models can be created".to_string(),
                ));
            }
            (
                if model.id.trim().is_empty() {
                    custom_id(&model.slug)
                } else {
                    model.id.clone()
                },
                "custom".to_string(),
                now,
            )
        }
    };
    tx.execute(
        "INSERT INTO models(id,slug,display_name,description,provider,family,category,tags_json,
           origin,enabled,supported_in_api,visibility,sort_order,context_window,max_context_window,
           default_reasoning_effort,capabilities_json,instructions_mode,instructions_text,
           builtin_revision,user_edited,created_at,updated_at,fast_policy)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,1,?21,?22,?23)
         ON CONFLICT(id) DO UPDATE SET slug=excluded.slug,display_name=excluded.display_name,
           description=excluded.description,provider=excluded.provider,family=excluded.family,
           category=excluded.category,tags_json=excluded.tags_json,enabled=excluded.enabled,
           supported_in_api=excluded.supported_in_api,visibility=excluded.visibility,
           sort_order=excluded.sort_order,context_window=excluded.context_window,
           max_context_window=excluded.max_context_window,
           default_reasoning_effort=excluded.default_reasoning_effort,
           capabilities_json=excluded.capabilities_json,instructions_mode=excluded.instructions_mode,
           instructions_text=excluded.instructions_text,fast_policy=excluded.fast_policy,
           user_edited=1,updated_at=excluded.updated_at",
        params![id,model.slug,model.display_name,model.description,model.provider,model.family,model.category,
            serde_json::to_string(&model.tags)
                .map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?,origin,model.enabled,
            model.supported_in_api,model.visibility,model.sort_order,model.context_window,model.max_context_window,
            model.default_reasoning_effort,model.capabilities.to_string(),model.instructions_mode,model.instructions_text,
            model.builtin_revision,created_at,now,model.fast_policy.as_str()],
    )?;
    tx.execute(
        "INSERT INTO model_prices(model_id,currency,input_microusd_per_1m,
           cached_input_microusd_per_1m,cache_write_microusd_per_1m,
           output_microusd_per_1m,price_status,price_source,created_at,updated_at)
         VALUES(?1,'USD',?2,?3,?4,?5,?6,?7,?8,?9)
         ON CONFLICT(model_id) DO UPDATE SET input_microusd_per_1m=excluded.input_microusd_per_1m,
           cached_input_microusd_per_1m=excluded.cached_input_microusd_per_1m,
           cache_write_microusd_per_1m=excluded.cache_write_microusd_per_1m,
           output_microusd_per_1m=excluded.output_microusd_per_1m,
           price_status=excluded.price_status,price_source=excluded.price_source,
           updated_at=excluded.updated_at",
        params![
            id,
            model.price.input_microusd_per_1m,
            model.price.cached_input_microusd_per_1m,
            model.price.cache_write_microusd_per_1m,
            model.price.output_microusd_per_1m,
            model.price.price_status,
            model.price.price_source,
            created_at,
            now
        ],
    )?;
    tx.execute("DELETE FROM model_price_tiers WHERE model_id=?1", [&id])?;
    for tier in &model.price_tiers {
        tx.execute(
            "INSERT INTO model_price_tiers(model_id,min_input_tokens,input_microusd_per_1m,
            cached_input_microusd_per_1m,cache_write_microusd_per_1m,
            output_microusd_per_1m) VALUES(?1,?2,?3,?4,?5,?6)",
            params![
                id,
                tier.min_input_tokens,
                tier.input_microusd_per_1m,
                tier.cached_input_microusd_per_1m,
                tier.cache_write_microusd_per_1m,
                tier.output_microusd_per_1m
            ],
        )?;
    }
    tx.execute("DELETE FROM model_routes WHERE model_id=?1", [&id])?;
    for route in &model.routes {
        let route_id = if route.id.trim().is_empty() {
            route_id(&id, route)
        } else {
            route.id.clone()
        };
        tx.execute(
            "INSERT INTO model_routes(id,model_id,source_kind,source_id,upstream_model,
            enabled,priority,weight,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?9)",
            params![
                route_id,
                id,
                route.source_kind,
                route.source_id,
                route.upstream_model,
                route.enabled,
                route.priority,
                route.weight,
                now
            ],
        )?;
    }
    tx.execute("DELETE FROM model_group_models_v2 WHERE model_id=?1", [&id])?;
    for group_id in &model.permission_group_ids {
        if group_id == DEFAULT_MODEL_GROUP_ID {
            continue;
        }
        tx.execute(
            "INSERT INTO model_group_models_v2(group_id,model_id,enabled,created_at,updated_at)
            VALUES(?1,?2,1,?3,?3)",
            params![group_id, id, now],
        )?;
    }
    Ok(id)
}

impl Storage {
    pub(super) fn apply_model_catalog_v2_migration(&self) -> Result<()> {
        if self.has_migration(MIGRATION_VERSION)? {
            return Ok(());
        }
        let has_legacy_catalog = self.has_table("model_catalog_models")?;
        let has_legacy_prices = self.has_table("model_price_rules")?;
        let has_legacy_routes = self.has_table("model_source_mappings")?
            && self.has_table("aggregate_apis")?
            && self.has_table("model_source_mapping_preferences")?
            && self.has_table("quota_source_model_assignments")?;
        let has_legacy_groups = self.has_table("model_group_models")?;
        let legacy_fixture = legacy_fixture();
        let tx = self.conn.unchecked_transaction()?;
        tx.execute_batch(include_str!("../../migrations/112_model_catalog_v2.sql"))?;
        seed_missing_with_fixture(&self.conn, &legacy_fixture)?;
        if has_legacy_catalog {
            migrate_legacy_catalog(&self.conn)?;
        }
        if has_legacy_prices {
            migrate_legacy_prices(&self.conn)?;
        }
        if has_legacy_routes {
            migrate_legacy_routes(&self.conn)?;
        }
        if has_legacy_groups {
            migrate_legacy_groups(&self.conn)?;
        }
        migration_smoke_for_fixture(&self.conn, &legacy_fixture)?;
        tx.execute(
            "INSERT INTO model_catalog_v2_meta(key,value) VALUES('cutover_state','complete')",
            [],
        )?;
        tx.execute(
            "INSERT INTO schema_migrations(version,applied_at) VALUES(?1,?2)",
            params![MIGRATION_VERSION, now_ts()],
        )?;
        tx.commit()?;
        if let Some(migrations) = self.migration_cache().as_mut() {
            migrations.insert(MIGRATION_VERSION.to_string());
        }
        Ok(())
    }

    pub(super) fn seed_missing_builtin_models_v2(&self) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        seed_missing(&self.conn)?;
        tx.commit()
    }

    pub(super) fn apply_gpt56_pricing_migration(&self) -> Result<()> {
        if self.has_migration(GPT56_PRICING_MIGRATION_VERSION)? {
            return Ok(());
        }
        let tx = self.conn.unchecked_transaction()?;
        tx.execute_batch(include_str!(
            "../../migrations/114_model_catalog_gpt56_prices.sql"
        ))?;
        let invalid_rows: i64 = tx.query_row(
            "SELECT COUNT(*)
             FROM models m
             JOIN model_prices p ON p.model_id=m.id
             LEFT JOIN model_price_tiers t
               ON t.model_id=m.id AND t.min_input_tokens=0
             WHERE p.price_source='user_provided_openai_gpt-5.6_2026-07-14_cached_at_input_rate'
               AND (m.origin<>'builtin' OR p.price_status<>'estimated'
                 OR p.input_microusd_per_1m IS NULL
                 OR p.cached_input_microusd_per_1m IS NULL
                 OR p.output_microusd_per_1m IS NULL
                 OR t.model_id IS NULL
                 OR t.input_microusd_per_1m<>p.input_microusd_per_1m
                 OR t.cached_input_microusd_per_1m<>p.cached_input_microusd_per_1m
                 OR t.output_microusd_per_1m<>p.output_microusd_per_1m)",
            [],
            |row| row.get(0),
        )?;
        if invalid_rows != 0 {
            return Err(rusqlite::Error::SqliteFailure(
                (),
                Some("GPT-5.6 pricing migration smoke check failed".to_string()),
            ));
        }
        tx.execute(
            "INSERT INTO schema_migrations(version,applied_at) VALUES(?1,?2)",
            params![GPT56_PRICING_MIGRATION_VERSION, now_ts()],
        )?;
        tx.commit()?;
        if let Some(migrations) = self.migration_cache().as_mut() {
            migrations.insert(GPT56_PRICING_MIGRATION_VERSION.to_string());
        }
        Ok(())
    }

    pub(super) fn apply_model_catalog_codex_metadata_migration(&self) -> Result<()> {
        if self.has_migration(CODEX_METADATA_MIGRATION_VERSION)? {
            return Ok(());
        }
        let fixture = legacy_fixture();
        let tx = self.conn.unchecked_transaction()?;
        let now = now_ts();
        for seed in &fixture.models {
            tx.execute(
                "UPDATE models
                 SET capabilities_json=?1,builtin_revision=?2,updated_at=?3
                 WHERE origin='builtin' AND user_edited=0
                   AND slug=?4 COLLATE NOCASE
                   AND COALESCE(builtin_revision,0) < ?2",
                params![
                    serde_json::to_string(&seed.capabilities)
                        .expect("serialize builtin Codex capabilities"),
                    fixture.revision,
                    now,
                    seed.slug
                ],
            )?;
        }
        tx.execute_batch(include_str!(
            "../../migrations/115_model_catalog_codex_metadata.sql"
        ))?;
        let stale_rows: i64 = tx.query_row(
            "SELECT COUNT(*) FROM models
             WHERE origin='builtin' AND user_edited=0
               AND COALESCE(builtin_revision,0) < ?1",
            [fixture.revision],
            |row| row.get(0),
        )?;
        if stale_rows != 0 {
            return Err(rusqlite::Error::SqliteFailure(
                (),
                Some("Codex model metadata migration smoke check failed".to_string()),
            ));
        }
        tx.execute(
            "INSERT INTO schema_migrations(version,applied_at) VALUES(?1,?2)",
            params![CODEX_METADATA_MIGRATION_VERSION, now],
        )?;
        tx.commit()?;
        if let Some(migrations) = self.migration_cache().as_mut() {
            migrations.insert(CODEX_METADATA_MIGRATION_VERSION.to_string());
        }
        Ok(())
    }

    pub(super) fn apply_gpt56_official_pricing_migration(&self) -> Result<()> {
        if self.has_migration(GPT56_OFFICIAL_PRICING_MIGRATION_VERSION)? {
            return Ok(());
        }
        let tx = self.conn.unchecked_transaction()?;
        tx.execute_batch(include_str!(
            "../../migrations/121_model_catalog_gpt56_official_prices.sql"
        ))?;
        let invalid_rows: i64 = tx.query_row(
            "SELECT COUNT(*)
             FROM _gpt56_official_price_candidates candidate
             JOIN models m ON m.id=candidate.model_id
             JOIN model_prices p ON p.model_id=m.id
             WHERE m.origin<>'builtin'
                OR p.price_status<>'official'
                OR p.price_source<>?1
                OR p.input_microusd_per_1m<>CASE lower(m.slug)
                     WHEN 'gpt-5.6-sol' THEN 5000000
                     WHEN 'gpt-5.6-terra' THEN 2500000
                     WHEN 'gpt-5.6-luna' THEN 1000000
                   END
                OR p.cached_input_microusd_per_1m<>CASE lower(m.slug)
                     WHEN 'gpt-5.6-sol' THEN 500000
                     WHEN 'gpt-5.6-terra' THEN 250000
                     WHEN 'gpt-5.6-luna' THEN 100000
                   END
                OR p.output_microusd_per_1m<>CASE lower(m.slug)
                     WHEN 'gpt-5.6-sol' THEN 30000000
                     WHEN 'gpt-5.6-terra' THEN 15000000
                     WHEN 'gpt-5.6-luna' THEN 6000000
                   END
                OR COALESCE(m.builtin_revision,0)<4
                OR (SELECT COUNT(*) FROM model_price_tiers tier
                    WHERE tier.model_id=m.id)<>2
                OR NOT EXISTS(
                     SELECT 1 FROM model_price_tiers base
                     WHERE base.model_id=m.id AND base.min_input_tokens=0
                       AND base.input_microusd_per_1m=p.input_microusd_per_1m
                       AND base.cached_input_microusd_per_1m=p.cached_input_microusd_per_1m
                       AND base.output_microusd_per_1m=p.output_microusd_per_1m
                   )
                OR NOT EXISTS(
                     SELECT 1 FROM model_price_tiers long_tier
                     WHERE long_tier.model_id=m.id AND long_tier.min_input_tokens=272000
                       AND long_tier.input_microusd_per_1m=CASE lower(m.slug)
                         WHEN 'gpt-5.6-sol' THEN 10000000
                         WHEN 'gpt-5.6-terra' THEN 5000000
                         WHEN 'gpt-5.6-luna' THEN 2000000
                       END
                       AND long_tier.cached_input_microusd_per_1m=CASE lower(m.slug)
                         WHEN 'gpt-5.6-sol' THEN 1000000
                         WHEN 'gpt-5.6-terra' THEN 500000
                         WHEN 'gpt-5.6-luna' THEN 200000
                       END
                       AND long_tier.output_microusd_per_1m=CASE lower(m.slug)
                         WHEN 'gpt-5.6-sol' THEN 45000000
                         WHEN 'gpt-5.6-terra' THEN 22500000
                         WHEN 'gpt-5.6-luna' THEN 9000000
                       END
                   )",
            [GPT56_OFFICIAL_PRICE_SOURCE],
            |row| row.get(0),
        )?;
        if invalid_rows != 0 {
            return Err(rusqlite::Error::SqliteFailure(
                (),
                Some("GPT-5.6 official pricing migration smoke check failed".to_string()),
            ));
        }
        tx.execute("DROP TABLE _gpt56_official_price_candidates", [])?;
        tx.execute(
            "INSERT INTO schema_migrations(version,applied_at) VALUES(?1,?2)",
            params![GPT56_OFFICIAL_PRICING_MIGRATION_VERSION, now_ts()],
        )?;
        tx.commit()?;
        if let Some(migrations) = self.migration_cache().as_mut() {
            migrations.insert(GPT56_OFFICIAL_PRICING_MIGRATION_VERSION.to_string());
        }
        Ok(())
    }

    pub(super) fn apply_gpt56_current_pricing_migration(&self) -> Result<()> {
        if self.has_migration(GPT56_CURRENT_PRICING_MIGRATION_VERSION)? {
            return Ok(());
        }
        let tx = self.conn.unchecked_transaction()?;
        tx.execute_batch(include_str!(
            "../../migrations/126_model_catalog_gpt56_current_prices.sql"
        ))?;
        let invalid_rows: i64 = tx.query_row(
            "SELECT COUNT(*)
             FROM _gpt56_current_price_candidates candidate
             JOIN models m ON m.id=candidate.model_id
             JOIN model_prices p ON p.model_id=m.id
             WHERE m.origin<>'builtin'
                OR m.user_edited<>0
                OR p.price_status<>'official'
                OR p.price_source<>?1
                OR p.input_microusd_per_1m<>CASE lower(m.slug)
                     WHEN 'gpt-5.6-sol' THEN 5000000
                     WHEN 'gpt-5.6-terra' THEN 2000000
                     WHEN 'gpt-5.6-luna' THEN 200000
                   END
                OR p.cached_input_microusd_per_1m<>CASE lower(m.slug)
                     WHEN 'gpt-5.6-sol' THEN 500000
                     WHEN 'gpt-5.6-terra' THEN 200000
                     WHEN 'gpt-5.6-luna' THEN 20000
                   END
                OR p.output_microusd_per_1m<>CASE lower(m.slug)
                     WHEN 'gpt-5.6-sol' THEN 30000000
                     WHEN 'gpt-5.6-terra' THEN 12000000
                     WHEN 'gpt-5.6-luna' THEN 1200000
                   END
                OR COALESCE(m.builtin_revision,0)<6
                OR COALESCE((SELECT value FROM model_catalog_v2_meta
                             WHERE key='builtin_revision'),'')<>'6'
                OR (SELECT COUNT(*) FROM model_price_tiers tier
                    WHERE tier.model_id=m.id)<>2
                OR NOT EXISTS(
                     SELECT 1 FROM model_price_tiers base
                     WHERE base.model_id=m.id AND base.min_input_tokens=0
                       AND base.input_microusd_per_1m=CASE lower(m.slug)
                         WHEN 'gpt-5.6-sol' THEN 5000000
                         WHEN 'gpt-5.6-terra' THEN 2000000
                         WHEN 'gpt-5.6-luna' THEN 200000
                       END
                       AND base.cached_input_microusd_per_1m=CASE lower(m.slug)
                         WHEN 'gpt-5.6-sol' THEN 500000
                         WHEN 'gpt-5.6-terra' THEN 200000
                         WHEN 'gpt-5.6-luna' THEN 20000
                       END
                       AND base.output_microusd_per_1m=CASE lower(m.slug)
                         WHEN 'gpt-5.6-sol' THEN 30000000
                         WHEN 'gpt-5.6-terra' THEN 12000000
                         WHEN 'gpt-5.6-luna' THEN 1200000
                       END
                   )
                OR NOT EXISTS(
                     SELECT 1 FROM model_price_tiers long_tier
                     WHERE long_tier.model_id=m.id AND long_tier.min_input_tokens=272000
                       AND long_tier.input_microusd_per_1m=CASE lower(m.slug)
                         WHEN 'gpt-5.6-sol' THEN 10000000
                         WHEN 'gpt-5.6-terra' THEN 4000000
                         WHEN 'gpt-5.6-luna' THEN 400000
                       END
                       AND long_tier.cached_input_microusd_per_1m=CASE lower(m.slug)
                         WHEN 'gpt-5.6-sol' THEN 1000000
                         WHEN 'gpt-5.6-terra' THEN 400000
                         WHEN 'gpt-5.6-luna' THEN 40000
                       END
                       AND long_tier.output_microusd_per_1m=CASE lower(m.slug)
                         WHEN 'gpt-5.6-sol' THEN 45000000
                         WHEN 'gpt-5.6-terra' THEN 18000000
                         WHEN 'gpt-5.6-luna' THEN 1800000
                       END
                   )",
            [GPT56_OFFICIAL_PRICE_SOURCE],
            |row| row.get(0),
        )?;
        if invalid_rows != 0 {
            return Err(rusqlite::Error::SqliteFailure(
                (),
                Some("GPT-5.6 current pricing migration smoke check failed".to_string()),
            ));
        }
        tx.execute("DROP TABLE _gpt56_current_price_candidates", [])?;
        tx.execute(
            "INSERT INTO schema_migrations(version,applied_at) VALUES(?1,?2)",
            params![GPT56_CURRENT_PRICING_MIGRATION_VERSION, now_ts()],
        )?;
        tx.commit()?;
        if let Some(migrations) = self.migration_cache().as_mut() {
            migrations.insert(GPT56_CURRENT_PRICING_MIGRATION_VERSION.to_string());
        }
        Ok(())
    }

    pub(super) fn apply_model_catalog_gpt6_astra_migration(&self) -> Result<()> {
        if self.has_migration(GPT6_ASTRA_MIGRATION_VERSION)? {
            return Ok(());
        }

        let migration_fixture = gpt6_astra_migration_fixture();
        let astra = &migration_fixture.astra;
        let scoped_fixture = BuiltinCatalogFixture {
            revision: migration_fixture.revision,
            source_sha256: migration_fixture.source_sha256.clone(),
            models: vec![astra.clone()],
        };

        let tx = self.conn.unchecked_transaction()?;
        let now = now_ts();
        let stored_catalog_revision = tx
            .query_row(
                "SELECT CAST(value AS INTEGER) FROM model_catalog_v2_meta
                 WHERE key='builtin_revision'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .unwrap_or_default();
        let max_unedited_builtin_revision = tx.query_row(
            "SELECT COALESCE(MAX(builtin_revision),0) FROM models
             WHERE origin='builtin' AND user_edited=0",
            [],
            |row| row.get::<_, i64>(0),
        )?;
        let effective_catalog_revision = stored_catalog_revision.max(max_unedited_builtin_revision);
        let astra_state = tx
            .query_row(
                "SELECT origin,user_edited,builtin_revision
                 FROM models WHERE slug=?1 COLLATE NOCASE",
                [GPT6_ASTRA_SLUG],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, bool>(1)?,
                        row.get::<_, Option<i64>>(2)?,
                    ))
                },
            )
            .optional()?;
        match astra_state.as_ref() {
            None if effective_catalog_revision <= migration_fixture.revision => {
                insert_seed(&self.conn, &scoped_fixture, astra, now)?;
            }
            Some((origin, user_edited, revision))
                if origin == "builtin"
                    && !user_edited
                    && revision.unwrap_or_default() <= GPT6_ASTRA_MIGRATION_REVISION =>
            {
                replace_unedited_builtin_seed(&self.conn, migration_fixture.revision, astra, now)?;
            }
            _ => {}
        }
        tx.execute(
            "UPDATE models
             SET capabilities_json=?1,builtin_revision=?2,updated_at=?3
             WHERE origin='builtin' AND user_edited=0
               AND slug=?4 COLLATE NOCASE
               AND COALESCE(builtin_revision,0) < ?2",
            params![
                serde_json::to_string(&migration_fixture.sol_capabilities)
                    .expect("serialize GPT-5.6 Sol capabilities"),
                migration_fixture.revision,
                now,
                migration_fixture.sol_slug.as_str()
            ],
        )?;
        if effective_catalog_revision <= migration_fixture.revision {
            tx.execute(
                "INSERT INTO model_catalog_v2_meta(key,value) VALUES('builtin_revision',?1)
                 ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                [migration_fixture.revision.to_string()],
            )?;
            tx.execute(
                "INSERT INTO model_catalog_v2_meta(key,value) VALUES('fixture_sha256',?1)
                 ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                [migration_fixture.source_sha256.as_str()],
            )?;
        } else if stored_catalog_revision < effective_catalog_revision {
            tx.execute(
                "INSERT INTO model_catalog_v2_meta(key,value) VALUES('builtin_revision',?1)
                 ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                [effective_catalog_revision.to_string()],
            )?;
        }
        tx.execute_batch(include_str!(
            "../../migrations/131_model_catalog_gpt6_astra.sql"
        ))?;

        let migrated = get_managed_model_v2_with_conn(&self.conn, GPT6_ASTRA_SLUG)?;
        match migrated.as_ref() {
            Some(model) if model.origin == "builtin" && !model.user_edited => {
                let migrated_revision = model.builtin_revision.unwrap_or_default();
                if migrated_revision < migration_fixture.revision
                    || (migrated_revision == migration_fixture.revision
                        && !seeded_builtin_matches(model, migration_fixture.revision, astra))
                {
                    return Err(rusqlite::Error::SqliteFailure(
                        (),
                        Some("GPT-6 Astra catalog migration smoke check failed".to_string()),
                    ));
                }
            }
            None if effective_catalog_revision <= migration_fixture.revision => {
                return Err(rusqlite::Error::QueryReturnedNoRows);
            }
            _ => {}
        }
        let migrated_sol =
            get_managed_model_v2_with_conn(&self.conn, migration_fixture.sol_slug.as_str())?;
        match migrated_sol.as_ref() {
            Some(model) if model.origin == "builtin" && !model.user_edited => {
                let migrated_revision = model.builtin_revision.unwrap_or_default();
                if migrated_revision < migration_fixture.revision
                    || (migrated_revision == migration_fixture.revision
                        && !sol_matches_gpt6_astra_migration_or_metadata_fix(
                            model,
                            &migration_fixture,
                        ))
                {
                    return Err(rusqlite::Error::SqliteFailure(
                        (),
                        Some("GPT-5.6 Sol catalog migration smoke check failed".to_string()),
                    ));
                }
            }
            Some(model)
                if model.origin != "builtin"
                    && effective_catalog_revision <= migration_fixture.revision =>
            {
                return Err(rusqlite::Error::SqliteFailure(
                    (),
                    Some("GPT-5.6 Sol catalog migration smoke check failed".to_string()),
                ));
            }
            None if effective_catalog_revision <= migration_fixture.revision => {
                return Err(rusqlite::Error::QueryReturnedNoRows);
            }
            _ => {}
        }
        if effective_catalog_revision <= migration_fixture.revision {
            migration_smoke_for_fixture(&self.conn, &scoped_fixture)?;
        }
        tx.execute(
            "INSERT INTO schema_migrations(version,applied_at) VALUES(?1,?2)",
            params![GPT6_ASTRA_MIGRATION_VERSION, now],
        )?;
        tx.commit()?;
        if let Some(migrations) = self.migration_cache().as_mut() {
            migrations.insert(GPT6_ASTRA_MIGRATION_VERSION.to_string());
        }
        Ok(())
    }

    pub(super) fn apply_model_catalog_gpt56_metadata_fix_migration(&self) -> Result<()> {
        self.apply_sql_migration(
            GPT56_METADATA_FIX_MIGRATION_VERSION,
            include_str!("../../migrations/132_model_catalog_gpt56_metadata_fix.sql"),
        )
    }

    pub(super) fn apply_model_catalog_revision9_migration(&self) -> Result<()> {
        if self.has_migration(MODEL_CATALOG_REVISION9_MIGRATION_VERSION)? {
            return Ok(());
        }

        let migration_fixture = revision9_fixture();

        let tx = self.conn.unchecked_transaction()?;
        let stored_catalog_revision = tx
            .query_row(
                "SELECT CAST(value AS INTEGER) FROM model_catalog_v2_meta
                 WHERE key='builtin_revision'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .unwrap_or_default();
        let max_unedited_builtin_revision = tx.query_row(
            "SELECT COALESCE(MAX(builtin_revision),0) FROM models
             WHERE origin='builtin' AND user_edited=0",
            [],
            |row| row.get::<_, i64>(0),
        )?;
        let effective_catalog_revision = stored_catalog_revision.max(max_unedited_builtin_revision);
        let now = now_ts();
        if effective_catalog_revision > migration_fixture.revision {
            if stored_catalog_revision < effective_catalog_revision {
                tx.execute(
                    "INSERT INTO model_catalog_v2_meta(key,value) VALUES('builtin_revision',?1)
                     ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                    [effective_catalog_revision.to_string()],
                )?;
            }
            tx.execute(
                "INSERT INTO schema_migrations(version,applied_at) VALUES(?1,?2)",
                params![MODEL_CATALOG_REVISION9_MIGRATION_VERSION, now],
            )?;
            tx.commit()?;
            if let Some(migrations) = self.migration_cache().as_mut() {
                migrations.insert(MODEL_CATALOG_REVISION9_MIGRATION_VERSION.to_string());
            }
            return Ok(());
        }
        let previous_fixture = legacy_fixture();

        // Seed new rows before reconciling the existing catalog. The
        // connection participates in the transaction above, so a failed
        // reconciliation rolls back both inserts and metadata updates.
        seed_missing_with_fixture(&self.conn, &migration_fixture)?;

        // Refresh unedited rows that already existed in the revision-8
        // catalog. Custom and user-edited rows remain untouched.
        for seed in &migration_fixture.models {
            let previous_seed = previous_fixture
                .models
                .iter()
                .find(|previous| previous.slug.eq_ignore_ascii_case(&seed.slug));
            reconcile_unedited_builtin_seed_revision9(&self.conn, previous_seed, seed, now)?;
        }

        for slug in RETIRED_CODEX_MODEL_SLUGS_REVISION9 {
            let previous_seed = previous_fixture
                .models
                .iter()
                .find(|seed| seed.slug.eq_ignore_ascii_case(slug))
                .ok_or_else(|| {
                    rusqlite::Error::InvalidParameterName(format!(
                        "revision 8 fixture is missing retired model {slug}"
                    ))
                })?;
            retire_codex_model_revision9(&self.conn, previous_seed, now)?;
        }

        tx.execute(
            "UPDATE models
             SET builtin_revision=?1
             WHERE origin='builtin' AND user_edited=0
               AND COALESCE(builtin_revision,0) < ?1",
            [MODEL_CATALOG_REVISION9],
        )?;

        tx.execute_batch(include_str!(
            "../../migrations/137_model_catalog_revision9.sql"
        ))?;
        migration_smoke_for_fixture(&self.conn, &migration_fixture)?;
        tx.execute(
            "INSERT INTO schema_migrations(version,applied_at) VALUES(?1,?2)",
            params![MODEL_CATALOG_REVISION9_MIGRATION_VERSION, now],
        )?;
        tx.commit()?;
        if let Some(migrations) = self.migration_cache().as_mut() {
            migrations.insert(MODEL_CATALOG_REVISION9_MIGRATION_VERSION.to_string());
        }
        Ok(())
    }

    pub(super) fn apply_model_catalog_revision10_migration(&self) -> Result<()> {
        if self.has_migration(MODEL_CATALOG_REVISION10_MIGRATION_VERSION)? {
            return Ok(());
        }

        let migration_fixture = gpt61_sol_migration_fixture();
        let tx = self.conn.unchecked_transaction()?;
        let stored_catalog_revision = tx
            .query_row(
                "SELECT CAST(value AS INTEGER) FROM model_catalog_v2_meta
                 WHERE key='builtin_revision'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .unwrap_or_default();
        let max_unedited_builtin_revision = tx.query_row(
            "SELECT COALESCE(MAX(builtin_revision),0) FROM models
             WHERE origin='builtin' AND user_edited=0",
            [],
            |row| row.get::<_, i64>(0),
        )?;
        let effective_catalog_revision = stored_catalog_revision.max(max_unedited_builtin_revision);
        let now = now_ts();
        seed_missing_with_fixture(&self.conn, &migration_fixture)?;
        if effective_catalog_revision <= migration_fixture.revision {
            migration_smoke_for_fixture(&self.conn, &migration_fixture)?;
        }
        tx.execute_batch(include_str!(
            "../../migrations/138_model_catalog_gpt61_sol.sql"
        ))?;
        tx.execute(
            "INSERT INTO schema_migrations(version,applied_at) VALUES(?1,?2)",
            params![MODEL_CATALOG_REVISION10_MIGRATION_VERSION, now],
        )?;
        tx.commit()?;
        if let Some(migrations) = self.migration_cache().as_mut() {
            migrations.insert(MODEL_CATALOG_REVISION10_MIGRATION_VERSION.to_string());
        }
        Ok(())
    }

    pub fn smoke_check_model_catalog_v2(&self) -> Result<()> {
        migration_smoke(&self.conn)
    }

    pub fn list_managed_models_v2(&self, include_hidden: bool) -> Result<Vec<ManagedModelV2>> {
        let sql = if include_hidden {
            format!("{MODEL_SELECT} ORDER BY m.sort_order ASC,m.slug ASC")
        } else {
            format!("{MODEL_SELECT} WHERE m.visibility='list' ORDER BY m.sort_order ASC,m.slug ASC")
        };
        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = stmt.query([])?;
        let mut models = Vec::new();
        while let Some(row) = rows.next()? {
            models.push(map_model(&self.conn, row)?);
        }
        Ok(models)
    }

    pub fn list_api_models_v2(&self) -> Result<Vec<ManagedModelV2>> {
        let sql = format!("{MODEL_SELECT} WHERE m.enabled=1 AND m.supported_in_api=1 AND m.visibility='list' ORDER BY m.sort_order ASC,m.slug ASC");
        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = stmt.query([])?;
        let mut models = Vec::new();
        while let Some(row) = rows.next()? {
            models.push(map_model(&self.conn, row)?);
        }
        Ok(models)
    }

    pub fn get_managed_model_v2(&self, slug: &str) -> Result<Option<ManagedModelV2>> {
        get_managed_model_v2_with_conn(&self.conn, slug)
    }

    pub fn get_enabled_model_v2(&self, slug: &str) -> Result<Option<ManagedModelV2>> {
        let sql = format!("{MODEL_SELECT} WHERE m.slug=?1 COLLATE NOCASE AND m.enabled=1 AND m.supported_in_api=1");
        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = stmt.query([slug.trim()])?;
        rows.next()?
            .map(|row| map_model(&self.conn, row))
            .transpose()
    }

    pub fn model_catalog_v2_stats(&self) -> Result<ModelCatalogV2Stats> {
        self.conn.query_row(
            "SELECT COUNT(*),COALESCE(SUM(m.enabled),0),
                    COALESCE(SUM(m.origin='builtin'),0),COALESCE(SUM(m.origin='custom'),0),
                    COALESCE(SUM(p.price_status='missing'),0),
                    COALESCE(SUM(NOT EXISTS(SELECT 1 FROM model_routes r WHERE r.model_id=m.id AND r.enabled=1)),0)
             FROM models m JOIN model_prices p ON p.model_id=m.id",
            [],
            |row| Ok(ModelCatalogV2Stats { total:row.get(0)?,enabled:row.get(1)?,builtin:row.get(2)?,custom:row.get(3)?,price_missing:row.get(4)?,missing_route:row.get(5)? }),
        )
    }

    pub fn upsert_managed_model_v2(&self, input: &ManagedModelV2Upsert) -> Result<ManagedModelV2> {
        let tx = self.conn.unchecked_transaction()?;
        let id = write_model(&tx, input)?;
        tx.commit()?;
        self.get_managed_model_v2(&input.model.slug)?
            .ok_or(rusqlite::Error::QueryReturnedNoRows)
            .map(|mut model| {
                model.id = id;
                model
            })
    }

    pub fn upsert_managed_models_v2(
        &self,
        inputs: &[ManagedModelV2Upsert],
    ) -> Result<Vec<ManagedModelV2>> {
        let tx = self.conn.unchecked_transaction()?;
        for input in inputs {
            write_model(&tx, input)?;
        }
        tx.commit()?;
        inputs
            .iter()
            .map(|input| {
                self.get_managed_model_v2(&input.model.slug)?
                    .ok_or(rusqlite::Error::QueryReturnedNoRows)
            })
            .collect()
    }

    /// Replaces base and tier prices as one transaction while preserving
    /// user-owned custom prices, including a custom price written after the
    /// caller prepared its update plan.
    pub fn update_managed_model_prices_v2(
        &self,
        updates: &[ManagedModelPriceV2Update],
    ) -> Result<Vec<String>> {
        self.update_managed_model_prices_v2_with_custom_override(updates, false)
    }

    /// Replaces base and tier prices as one transaction. When
    /// `allow_custom_override` is true, the caller has explicitly selected
    /// these models for a price sync and its external price may replace a
    /// custom price as well.
    pub fn update_managed_model_prices_v2_with_custom_override(
        &self,
        updates: &[ManagedModelPriceV2Update],
        allow_custom_override: bool,
    ) -> Result<Vec<String>> {
        let mut seen = HashSet::new();
        for update in updates {
            let slug = update.slug.trim();
            if slug.is_empty() || !seen.insert(slug.to_ascii_lowercase()) {
                return Err(rusqlite::Error::InvalidParameterName(
                    "invalid or duplicate managed model price slug".to_string(),
                ));
            }
            validate_managed_model_price_v2(&update.price, &update.price_tiers)?;
        }

        let tx = self.conn.unchecked_transaction()?;
        let now = now_ts();
        let mut updated_slugs = Vec::new();
        for update in updates {
            let (model_id, canonical_slug) = tx
                .query_row(
                    "SELECT id,slug FROM models WHERE slug=?1 COLLATE NOCASE",
                    [update.slug.trim()],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                )
                .optional()?
                .ok_or_else(|| {
                    rusqlite::Error::InvalidParameterName(format!(
                        "model_not_found: {}",
                        update.slug.trim()
                    ))
                })?;
            let changed = tx.execute(
                "UPDATE model_prices SET input_microusd_per_1m=?2,
                 cached_input_microusd_per_1m=?3,cache_write_microusd_per_1m=?4,
                   output_microusd_per_1m=?5,price_status=?6,price_source=?7,updated_at=?8
                 WHERE model_id=?1 AND (?9 OR price_status<>'custom')",
                params![
                    model_id,
                    update.price.input_microusd_per_1m,
                    update.price.cached_input_microusd_per_1m,
                    update.price.cache_write_microusd_per_1m,
                    update.price.output_microusd_per_1m,
                    update.price.price_status,
                    update.price.price_source,
                    now,
                    if allow_custom_override { 1_i64 } else { 0_i64 }
                ],
            )?;
            if changed == 0 {
                continue;
            }
            if update.price.price_status == "missing" {
                tx.execute(
                    "DELETE FROM model_group_models_v2 WHERE model_id=?1",
                    [&model_id],
                )?;
            }
            tx.execute(
                "DELETE FROM model_price_tiers WHERE model_id=?1",
                [&model_id],
            )?;
            for tier in &update.price_tiers {
                tx.execute(
                    "INSERT INTO model_price_tiers(model_id,min_input_tokens,
                       input_microusd_per_1m,cached_input_microusd_per_1m,
                       cache_write_microusd_per_1m,output_microusd_per_1m)
                     VALUES(?1,?2,?3,?4,?5,?6)",
                    params![
                        model_id,
                        tier.min_input_tokens,
                        tier.input_microusd_per_1m,
                        tier.cached_input_microusd_per_1m,
                        tier.cache_write_microusd_per_1m,
                        tier.output_microusd_per_1m
                    ],
                )?;
            }
            updated_slugs.push(canonical_slug);
        }
        tx.commit()?;
        Ok(updated_slugs)
    }

    pub fn upsert_missing_managed_models_and_ensure_routes_v2(
        &self,
        create_candidates: &[ManagedModelV2Upsert],
        route_inputs: &[ManagedModelRouteEnsureV2],
    ) -> Result<ManagedModelRouteEnsureResultV2> {
        let tx = self.conn.unchecked_transaction()?;
        let mut candidate_slugs = HashSet::new();
        for candidate in create_candidates {
            let slug = candidate.model.slug.trim();
            if slug.is_empty()
                || !candidate_slugs.insert(slug.to_ascii_lowercase())
                || candidate.previous_slug.is_some()
                || !candidate.model.id.trim().is_empty()
                || !candidate.model.routes.is_empty()
            {
                return Err(rusqlite::Error::InvalidParameterName(
                    "invalid managed model creation candidate".to_string(),
                ));
            }
        }

        let mut result = ManagedModelRouteEnsureResultV2::default();
        let mut created_model_ids = HashSet::new();
        for input in route_inputs {
            let model_slug = input.model_slug.trim();
            if model_slug.is_empty() {
                return Err(rusqlite::Error::InvalidParameterName(
                    "managed model route slug is required".to_string(),
                ));
            }

            let mut route = input.route.clone();
            route.id.clear();
            route.source_kind = route.source_kind.trim().to_string();
            route.source_id = route.source_id.trim().to_string();
            route.upstream_model = route.upstream_model.trim().to_string();
            validate_route(&route)?;

            let existing = tx
                .query_row(
                    "SELECT id,slug FROM models WHERE slug=?1 COLLATE NOCASE",
                    [model_slug],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                )
                .optional()?;
            let (model_id, canonical_slug) = match existing {
                Some(existing) => existing,
                None => {
                    let candidate = create_candidates
                        .iter()
                        .find(|candidate| {
                            candidate.model.slug.trim().eq_ignore_ascii_case(model_slug)
                        })
                        .ok_or_else(|| {
                            rusqlite::Error::InvalidParameterName(format!(
                                "missing managed model creation candidate: {model_slug}"
                            ))
                        })?;
                    let model_id = write_model(&tx, candidate)?;
                    let canonical_slug =
                        tx.query_row("SELECT slug FROM models WHERE id=?1", [&model_id], |row| {
                            row.get::<_, String>(0)
                        })?;
                    if created_model_ids.insert(model_id.clone()) {
                        result.created_models.push(canonical_slug.clone());
                    }
                    (model_id, canonical_slug)
                }
            };

            let exact_route_exists = |tx: &Transaction<'_>| -> Result<bool> {
                Ok(tx
                    .query_row(
                        "SELECT 1 FROM model_routes
                         WHERE model_id=?1 AND source_kind=?2 AND source_id=?3
                           AND upstream_model=?4 COLLATE NOCASE",
                        params![
                            &model_id,
                            &route.source_kind,
                            &route.source_id,
                            &route.upstream_model
                        ],
                        |row| row.get::<_, i64>(0),
                    )
                    .optional()?
                    .is_some())
            };
            if exact_route_exists(&tx)? {
                result.unchanged_routes.push(canonical_slug);
                continue;
            }

            let now = now_ts();
            let inserted = tx.execute(
                "INSERT OR IGNORE INTO model_routes(
                    id,model_id,source_kind,source_id,upstream_model,
                    enabled,priority,weight,created_at,updated_at
                 ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?9)",
                params![
                    route_id(&model_id, &route),
                    &model_id,
                    &route.source_kind,
                    &route.source_id,
                    &route.upstream_model,
                    route.enabled,
                    route.priority,
                    route.weight,
                    now
                ],
            )?;
            if inserted > 0 {
                result.added_routes.push(canonical_slug);
            } else if exact_route_exists(&tx)? {
                result.unchanged_routes.push(canonical_slug);
            } else {
                return Err(rusqlite::Error::SqliteFailure(
                    (),
                    Some("managed model route id collision".to_string()),
                ));
            }
        }
        tx.commit()?;
        Ok(result)
    }

    pub fn update_managed_model_state_v2(
        &self,
        input: &ManagedModelStateV2Update,
    ) -> Result<ManagedModelV2> {
        let visibility = input.visibility.trim();
        if !matches!(visibility, "list" | "hide") {
            return Err(rusqlite::Error::InvalidParameterName(
                "invalid model visibility".to_string(),
            ));
        }
        let updated = self.conn.execute(
            "UPDATE models
             SET enabled=?2,visibility=?3,user_edited=1,updated_at=?4
             WHERE slug=?1 COLLATE NOCASE",
            params![input.slug.trim(), input.enabled, visibility, now_ts()],
        )?;
        if updated == 0 {
            return Err(rusqlite::Error::QueryReturnedNoRows);
        }
        self.get_managed_model_v2(&input.slug)?
            .ok_or(rusqlite::Error::QueryReturnedNoRows)
    }

    pub fn update_managed_models_state_v2(
        &self,
        input: &ManagedModelBatchStateV2Update,
    ) -> Result<Vec<ManagedModelV2>> {
        let visibility = input.visibility.trim();
        if !matches!(visibility, "list" | "hide") {
            return Err(rusqlite::Error::InvalidParameterName(
                "invalid model visibility".to_string(),
            ));
        }
        let mut seen = HashSet::new();
        let slugs = input
            .slugs
            .iter()
            .filter_map(|slug| {
                let slug = slug.trim();
                if slug.is_empty() || !seen.insert(slug.to_ascii_lowercase()) {
                    None
                } else {
                    Some(slug.to_string())
                }
            })
            .collect::<Vec<_>>();
        if slugs.is_empty() {
            return Err(rusqlite::Error::InvalidParameterName(
                "managed model slugs must not be empty".to_string(),
            ));
        }

        let tx = self.conn.unchecked_transaction()?;
        let updated_at = now_ts();
        for slug in &slugs {
            let updated = tx.execute(
                "UPDATE models
                 SET enabled=?2,visibility=?3,user_edited=1,updated_at=?4
                 WHERE slug=?1 COLLATE NOCASE",
                params![slug, input.enabled, visibility, updated_at],
            )?;
            if updated == 0 {
                return Err(rusqlite::Error::InvalidParameterName(format!(
                    "managed model not found: {slug}"
                )));
            }
        }
        let models = slugs
            .iter()
            .map(|slug| {
                self.get_managed_model_v2(slug)?
                    .ok_or(rusqlite::Error::QueryReturnedNoRows)
            })
            .collect::<Result<Vec<_>>>()?;
        tx.commit()?;
        Ok(models)
    }

    pub fn delete_managed_model_v2(&self, slug: &str) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        let origin = tx
            .query_row(
                "SELECT origin FROM models WHERE slug=?1 COLLATE NOCASE",
                [slug.trim()],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        let Some(origin) = origin else {
            return Err(rusqlite::Error::QueryReturnedNoRows);
        };
        if origin == "builtin" {
            tx.execute(
                "INSERT INTO model_catalog_v2_meta(key,value) VALUES(?1,?2)
                 ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                params![deleted_builtin_meta_key(slug), now_ts().to_string()],
            )?;
        }
        tx.execute(
            "DELETE FROM models WHERE slug=?1 COLLATE NOCASE",
            [slug.trim()],
        )?;
        tx.commit()
    }

    pub fn list_enabled_routes_v2(&self, slug: &str) -> Result<Vec<ModelRouteV2>> {
        let Some(model_id) = self.conn.query_row(
            "SELECT id FROM models WHERE slug=?1 COLLATE NOCASE AND enabled=1 AND supported_in_api=1",
            [slug.trim()],|row| row.get::<_,String>(0)).optional()? else { return Ok(Vec::new()) };
        Ok(list_routes(&self.conn, &model_id)?
            .into_iter()
            .filter(|route| route.enabled)
            .collect())
    }

    pub fn allowed_model_slugs_for_user_v2(&self, user_id: &str, now: i64) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT m.slug
             FROM user_model_groups u JOIN model_groups g ON g.id=u.group_id
             JOIN models m ON m.enabled=1 AND m.supported_in_api=1
             JOIN model_prices p ON p.model_id=m.id
             LEFT JOIN model_group_models_v2 gm ON gm.group_id=g.id AND gm.model_id=m.id AND gm.enabled=1
             WHERE u.user_id=?1 AND u.status='active' AND (u.expires_at IS NULL OR u.expires_at>?2)
               AND g.status='active' AND ((g.is_default=1) OR (gm.model_id IS NOT NULL AND p.price_status<>'missing'))
             ORDER BY m.slug ASC")?;
        stmt.query_map(params![user_id, now], |row| row.get(0))?
            .collect()
    }

    pub fn list_model_group_models_v2(&self) -> Result<Vec<ModelGroupModel>> {
        let mut stmt = self.conn.prepare(
            "SELECT gm.group_id,m.slug,gm.enabled,gm.rate_multiplier_millis,
                    gm.created_at,gm.updated_at
             FROM model_group_models_v2 gm JOIN models m ON m.id=gm.model_id
             ORDER BY gm.group_id ASC,m.slug ASC",
        )?;
        stmt.query_map([], |row| {
            Ok(ModelGroupModel {
                group_id: row.get(0)?,
                platform_model_slug: row.get(1)?,
                enabled: row.get::<_, i64>(2)? != 0,
                rate_multiplier_millis: row.get(3)?,
                billing_model_slug: None,
                note: Some("model_catalog_v2".to_string()),
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
            })
        })?
        .collect()
    }

    pub fn replace_model_group_models_v2(
        &self,
        group_id: &str,
        models: &[ModelGroupModel],
    ) -> Result<()> {
        let is_default = self.conn.query_row(
            "SELECT is_default FROM model_groups WHERE id=?1",
            [group_id],
            |row| row.get::<_, i64>(0),
        )? != 0;
        if is_default && !models.is_empty() {
            return Err(rusqlite::Error::InvalidParameterName(
                "default model group uses all_enabled_models and cannot store explicit models"
                    .to_string(),
            ));
        }
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "DELETE FROM model_group_models_v2 WHERE group_id=?1",
            [group_id],
        )?;
        for model in models {
            let (model_id, price_status) = tx.query_row(
                "SELECT m.id,p.price_status FROM models m JOIN model_prices p ON p.model_id=m.id
                 WHERE m.slug=?1 COLLATE NOCASE AND m.enabled=1 AND m.supported_in_api=1",
                [model.platform_model_slug.as_str()],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )?;
            if price_status == "missing" {
                return Err(rusqlite::Error::InvalidParameterName(format!(
                    "model_price_missing: {}",
                    model.platform_model_slug
                )));
            }
            tx.execute(
                "INSERT INTO model_group_models_v2(group_id,model_id,enabled,
                   rate_multiplier_millis,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6)",
                params![
                    group_id,
                    model_id,
                    model.enabled,
                    model.rate_multiplier_millis,
                    model.created_at,
                    model.updated_at
                ],
            )?;
        }
        tx.commit()
    }

    pub fn resolve_model_group_access_for_user_v2(
        &self,
        user_id: &str,
        slug: &str,
        now: i64,
    ) -> Result<Option<ModelGroupAccess>> {
        let mut stmt = self.conn.prepare(
            "SELECT g.id,g.name,m.slug,g.rate_multiplier_millis,gm.rate_multiplier_millis
             FROM user_model_groups u JOIN model_groups g ON g.id=u.group_id
             JOIN models m ON m.slug=?2 COLLATE NOCASE AND m.enabled=1 AND m.supported_in_api=1
             JOIN model_prices p ON p.model_id=m.id
             LEFT JOIN model_group_models_v2 gm ON gm.group_id=g.id AND gm.model_id=m.id AND gm.enabled=1
             WHERE u.user_id=?1 AND u.status='active' AND (u.expires_at IS NULL OR u.expires_at>?3)
               AND g.status='active' AND ((g.is_default=1) OR (gm.model_id IS NOT NULL AND p.price_status<>'missing'))",
        )?;
        let rows = stmt.query_map(params![user_id, slug.trim(), now], |row| {
            let group_rate = row.get::<_, i64>(3)?.max(0);
            let model_rate = row.get::<_, Option<i64>>(4)?.unwrap_or(1_000).max(0);
            let effective = i128::from(group_rate) * i128::from(model_rate) / 1_000;
            Ok(ModelGroupAccess {
                group_id: row.get(0)?,
                group_name: row.get(1)?,
                platform_model_slug: row.get(2)?,
                rate_multiplier_millis: i64::try_from(effective).map_err(|_| {
                    rusqlite::Error::InvalidParameterName(
                        "model group multiplier exceeds SQLite INTEGER range".to_string(),
                    )
                })?,
                billing_model_slug: None,
            })
        })?;
        let mut best = None;
        for row in rows {
            let access = row?;
            if best.as_ref().is_none_or(|current: &ModelGroupAccess| {
                access.rate_multiplier_millis < current.rate_multiplier_millis
            }) {
                best = Some(access);
            }
        }
        Ok(best)
    }

    pub fn resolve_model_group_rate_v2(
        &self,
        user_id: &str,
        slug: &str,
        now: i64,
    ) -> Result<Option<i64>> {
        let mut stmt = self.conn.prepare(
            "SELECT g.rate_multiplier_millis,gm.rate_multiplier_millis
             FROM user_model_groups u JOIN model_groups g ON g.id=u.group_id
             JOIN models m ON m.slug=?2 COLLATE NOCASE AND m.enabled=1 AND m.supported_in_api=1
             JOIN model_prices p ON p.model_id=m.id
             LEFT JOIN model_group_models_v2 gm ON gm.group_id=g.id AND gm.model_id=m.id AND gm.enabled=1
             WHERE u.user_id=?1 AND u.status='active' AND (u.expires_at IS NULL OR u.expires_at>?3)
               AND g.status='active' AND ((g.is_default=1) OR (gm.model_id IS NOT NULL AND p.price_status<>'missing'))",
        )?;
        let rows = stmt.query_map(params![user_id, slug.trim(), now], |row| {
            let group_rate = row.get::<_, i64>(0)?.max(0);
            let model_rate = row.get::<_, Option<i64>>(1)?.unwrap_or(1_000).max(0);
            let effective = i128::from(group_rate) * i128::from(model_rate) / 1_000;
            i64::try_from(effective).map_err(|_| {
                rusqlite::Error::InvalidParameterName(
                    "model group multiplier exceeds SQLite INTEGER range".to_string(),
                )
            })
        })?;
        let mut best = None;
        for row in rows {
            let rate = row?;
            if best.is_none_or(|current| rate < current) {
                best = Some(rate);
            }
        }
        Ok(best)
    }
}

#[cfg(test)]
#[path = "model_catalog_revision10_tests.rs"]
mod revision10_tests;

#[cfg(test)]
mod tests {
    use super::*;

    fn storage() -> Storage {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        storage
    }

    fn reset_to_revision8_catalog(storage: &Storage) {
        storage
            .conn
            .execute(
                "DELETE FROM schema_migrations WHERE version=?1",
                [MODEL_CATALOG_REVISION9_MIGRATION_VERSION],
            )
            .unwrap();
        storage.migration_cache().take();
        storage.conn.execute("DELETE FROM models", []).unwrap();
        storage
            .conn
            .execute(
                "DELETE FROM model_catalog_v2_meta WHERE key LIKE ?1",
                [format!("{DELETED_BUILTIN_META_PREFIX}%")],
            )
            .unwrap();
        storage
            .conn
            .execute(
                "UPDATE model_catalog_v2_meta SET value='8' WHERE key='builtin_revision'",
                [],
            )
            .unwrap();
        let legacy_fixture = legacy_fixture();
        storage
            .conn
            .execute(
                "UPDATE model_catalog_v2_meta SET value=?1 WHERE key='fixture_sha256'",
                [legacy_fixture.source_sha256.as_str()],
            )
            .unwrap();
        seed_missing_with_fixture(&storage.conn, &legacy_fixture).unwrap();
    }

    fn custom_creation_candidate(slug: &str) -> ManagedModelV2Upsert {
        ManagedModelV2Upsert {
            model: ManagedModelV2 {
                slug: slug.to_string(),
                display_name: format!("Discovered {slug}"),
                provider: Some("OpenAI".to_string()),
                origin: "custom".to_string(),
                enabled: true,
                supported_in_api: true,
                visibility: "list".to_string(),
                capabilities: serde_json::json!({
                    "supports_text_generation": true,
                    "input_modalities": ["text"],
                    "output_modalities": ["text"]
                }),
                instructions_mode: "passthrough".to_string(),
                fast_policy: ModelFastPolicyV2::Passthrough,
                price: ModelPriceV2 {
                    price_status: "missing".to_string(),
                    ..Default::default()
                },
                ..Default::default()
            },
            ..Default::default()
        }
    }

    fn account_route_ensure(model_slug: &str, upstream_model: &str) -> ManagedModelRouteEnsureV2 {
        ManagedModelRouteEnsureV2 {
            model_slug: model_slug.to_string(),
            route: ModelRouteV2 {
                source_kind: "account_pool".to_string(),
                source_id: "default".to_string(),
                upstream_model: upstream_model.to_string(),
                enabled: true,
                priority: 0,
                weight: 1,
                ..Default::default()
            },
        }
    }

    #[test]
    fn fixture_contains_no_prompt_fields() {
        let raw = include_str!("../../seeds/model_catalog_v2_2026_07_10.json");
        let value: Value = serde_json::from_str(raw).expect("parse fixture");
        assert_eq!(value["models"].as_array().map(Vec::len), Some(10));
        assert_eq!(value["revision"].as_i64(), Some(8));
        assert_eq!(
            value["source_sha256"].as_str(),
            Some("ccc924d8388571e3ecfa722589e383b5de8038e0b02a096dd618c140066bced3")
        );
        assert!(!raw.contains("base_instructions"));
        assert!(!raw.contains("instructions_template"));
        assert!(!raw.contains("instructions_text"));
        for model in value["models"].as_array().expect("fixture models") {
            let capabilities = model["capabilities"].as_object().expect("capabilities");
            assert_eq!(
                capabilities
                    .get("include_skills_usage_instructions")
                    .and_then(Value::as_bool),
                Some(false)
            );
            if model["slug"] == "gpt-image-2" {
                assert!(model["context_window"].is_null());
                assert!(model["max_context_window"].is_null());
                assert!(model["default_reasoning_effort"].is_null());
                assert_eq!(capabilities["snapshot"], "gpt-image-2-2026-04-21");
                assert_eq!(capabilities["supports_text_generation"], false);
                assert_eq!(capabilities["supports_image_generation"], true);
                assert_eq!(capabilities["supports_image_editing"], true);
                assert_eq!(
                    capabilities["supported_endpoints"],
                    serde_json::json!(["/v1/images/generations", "/v1/images/edits"])
                );
                continue;
            }
            for key in [
                "additional_speed_tiers",
                "default_verbosity",
                "apply_patch_tool_type",
                "truncation_mode",
                "truncation_limit",
                "max_context_window",
            ] {
                if key == "max_context_window" {
                    assert!(model.get(key).is_some(), "{} missing {key}", model["slug"]);
                } else {
                    assert!(
                        capabilities.contains_key(key),
                        "{} missing capability {key}",
                        model["slug"]
                    );
                }
            }
        }
        for slug in ["gpt-5.6-sol", "gpt-5.6-terra"] {
            let model = value["models"]
                .as_array()
                .unwrap()
                .iter()
                .find(|model| model["slug"] == slug)
                .unwrap();
            assert_eq!(model["capabilities"]["multi_agent_version"], "v2");
            assert_eq!(model["capabilities"]["tool_mode"], "code_mode_only");
            assert_eq!(model["capabilities"]["use_responses_lite"], true);
        }

        let latest_raw = include_str!("../../seeds/model_catalog_v2_2026_09_24.json");
        let latest: Value = serde_json::from_str(latest_raw).expect("parse latest fixture");
        assert_eq!(latest["revision"].as_i64(), Some(9));
        assert_eq!(latest["models"].as_array().map(Vec::len), Some(11));
        let latest_slugs = latest["models"]
            .as_array()
            .unwrap()
            .iter()
            .map(|model| model["slug"].as_str().unwrap())
            .collect::<HashSet<_>>();
        assert_eq!(
            latest_slugs,
            HashSet::from([
                "gpt-6-astra",
                "gpt-6-sol",
                "gpt-6-luna",
                "gpt-5.6-sol",
                "gpt-5.6-terra",
                "gpt-5.6-luna",
                "gpt-5.5",
                "gpt-image-2",
                "gpt-image-2.5-sunburst",
                "gpt-image-2.5-flare",
                "codex-auto-review",
            ])
        );
        for slug in RETIRED_CODEX_MODEL_SLUGS_REVISION9 {
            assert!(!latest_slugs.contains(slug));
        }
        for slug in [
            "gpt-6-sol",
            "gpt-6-luna",
            "gpt-image-2.5-sunburst",
            "gpt-image-2.5-flare",
        ] {
            assert!(latest["models"]
                .as_array()
                .unwrap()
                .iter()
                .any(|model| model["slug"] == slug));
        }
        for (slug, description, price_source) in [
            (
                "gpt-6-sol",
                "Latest frontier agentic coding model.",
                "https://developers.openai.com/api/docs/models/gpt-6-sol",
            ),
            (
                "gpt-6-luna",
                "Fast and affordable agentic coding model.",
                "https://developers.openai.com/api/docs/models/gpt-6-luna",
            ),
            (
                "gpt-image-2.5-sunburst",
                "Advanced image generation with high instruction following and image fidelity.",
                "https://developers.openai.com/api/docs/models/gpt-image-2.5-sunburst",
            ),
            (
                "gpt-image-2.5-flare",
                "Fast, high-quality everyday image generation.",
                "https://developers.openai.com/api/docs/models/gpt-image-2.5-flare",
            ),
        ] {
            let model = latest["models"]
                .as_array()
                .unwrap()
                .iter()
                .find(|model| model["slug"] == slug)
                .unwrap();
            assert_eq!(model["description"], description);
            assert_eq!(model["price_source"], price_source);
        }
        for slug in ["gpt-image-2.5-sunburst", "gpt-image-2.5-flare"] {
            let model = latest["models"]
                .as_array()
                .unwrap()
                .iter()
                .find(|model| model["slug"] == slug)
                .unwrap();
            assert_eq!(model["price_status"], "missing");
            assert_eq!(model["price_tiers"], serde_json::json!([]));
        }
        let gpt6_luna = latest["models"]
            .as_array()
            .unwrap()
            .iter()
            .find(|model| model["slug"] == "gpt-6-luna")
            .unwrap();
        assert_eq!(gpt6_luna["default_reasoning_effort"], "high");
        for (slug, description) in [
            ("gpt-5.6-sol", "Latest frontier agentic coding model."),
            (
                "gpt-5.6-terra",
                "Balanced agentic coding model for everyday work.",
            ),
            ("gpt-5.6-luna", "Fast and affordable agentic coding model."),
        ] {
            let model = latest["models"]
                .as_array()
                .unwrap()
                .iter()
                .find(|model| model["slug"] == slug)
                .unwrap();
            assert_eq!(model["description"], description);
        }
        let expected_source_sha256 = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&latest["models"]).expect("serialize models"))
        );
        assert_eq!(
            latest["source_sha256"].as_str(),
            Some(expected_source_sha256.as_str())
        );
    }

    #[test]
    fn fresh_catalog_seeds_prices_routes_and_hidden_model() {
        let storage = storage();
        let all = storage.list_managed_models_v2(true).expect("list all");
        let visible = storage.list_api_models_v2().expect("list visible");
        assert_eq!(all.len(), 12);
        assert_eq!(visible.len(), 11);
        for slug in RETIRED_CODEX_MODEL_SLUGS_REVISION9 {
            assert!(all.iter().all(|model| model.slug != *slug));
        }
        assert_eq!(
            all.iter()
                .filter(|model| model.price.price_status == "missing")
                .count(),
            3
        );
        for (
            slug,
            price_source,
            input,
            cached,
            cache_write,
            output,
            long_input,
            long_cached,
            long_cache_write,
            long_output,
        ) in [
            (
                "gpt-6.1-sol",
                GPT61_SOL_PRICE_SOURCE,
                2_000_000,
                100_000,
                2_500_000,
                10_000_000,
                4_000_000,
                200_000,
                5_000_000,
                15_000_000,
            ),
            (
                "gpt-5.6-sol",
                GPT56_OFFICIAL_PRICE_SOURCE,
                5_000_000,
                500_000,
                6_250_000,
                30_000_000,
                10_000_000,
                1_000_000,
                12_500_000,
                45_000_000,
            ),
            (
                "gpt-5.6-terra",
                GPT56_OFFICIAL_PRICE_SOURCE,
                2_000_000,
                200_000,
                2_500_000,
                12_000_000,
                4_000_000,
                400_000,
                5_000_000,
                18_000_000,
            ),
            (
                "gpt-5.6-luna",
                GPT56_OFFICIAL_PRICE_SOURCE,
                200_000,
                20_000,
                250_000,
                1_200_000,
                400_000,
                40_000,
                500_000,
                1_800_000,
            ),
        ] {
            let model = all.iter().find(|model| model.slug == slug).unwrap();
            assert_eq!(model.price.price_status, "official");
            assert_eq!(model.price.price_source.as_deref(), Some(price_source));
            assert_eq!(model.price.input_microusd_per_1m, Some(input));
            assert_eq!(model.price.cached_input_microusd_per_1m, Some(cached));
            assert_eq!(model.price.cache_write_microusd_per_1m, Some(cache_write));
            assert_eq!(model.price.output_microusd_per_1m, Some(output));
            assert_eq!(model.price_tiers.len(), 2);
            assert_eq!(model.price_tiers[1].min_input_tokens, 272_001);
            assert_eq!(model.price_tiers[1].input_microusd_per_1m, long_input);
            assert_eq!(
                model.price_tiers[1].cached_input_microusd_per_1m,
                long_cached
            );
            assert_eq!(
                model.price_tiers[1].cache_write_microusd_per_1m,
                Some(long_cache_write)
            );
            assert_eq!(model.price_tiers[1].output_microusd_per_1m, long_output);
        }
        for slug in [
            "gpt-6.1-sol",
            "gpt-5.6-sol",
            "gpt-5.6-terra",
            "gpt-5.6-luna",
        ] {
            let model = all.iter().find(|model| model.slug == slug).unwrap();
            assert_eq!(model.context_window, Some(272_000));
            assert_eq!(model.max_context_window, Some(872_000));
        }
        for slug in [
            "gpt-6.1-sol",
            "gpt-5.6-sol",
            "gpt-5.6-terra",
            "gpt-5.6-luna",
            "gpt-5.5",
        ] {
            let model = all.iter().find(|model| model.slug == slug).unwrap();
            assert_eq!(model.capabilities["shell_type"], "unified_exec");
        }
        let sol = all
            .iter()
            .find(|model| model.slug == "gpt-5.6-sol")
            .unwrap();
        assert_eq!(sol.builtin_revision, Some(9));
        assert_eq!(sol.capabilities["multi_agent_version"], "v2");
        assert_eq!(sol.capabilities["tool_mode"], "code_mode_only");
        assert_eq!(sol.capabilities["use_responses_lite"], true);
        assert_eq!(sol.capabilities["comp_hash"], "3000");
        assert_eq!(
            sol.capabilities["service_tiers"],
            serde_json::json!(["priority", "ultrafast"])
        );
        let sol61 = all
            .iter()
            .find(|model| model.slug == GPT61_SOL_SLUG)
            .unwrap();
        assert_eq!(sol61.display_name, "GPT-6.1 Sol");
        assert_eq!(
            sol61.description.as_deref(),
            Some("Near-Astra performance for complex work at a lower cost.")
        );
        assert_eq!(sol61.builtin_revision, Some(MODEL_CATALOG_REVISION10));
        assert_eq!(sol61.default_reasoning_effort.as_deref(), Some("medium"));
        assert_eq!(
            sol61.capabilities["reasoning_efforts"],
            serde_json::json!(["low", "medium", "high", "xhigh", "max"])
        );
        assert_eq!(sol61.capabilities["shell_type"], "unified_exec");
        assert_eq!(sol61.price.price_status, "official");
        assert_eq!(
            sol61.price.price_source.as_deref(),
            Some(GPT61_SOL_PRICE_SOURCE)
        );
        assert_eq!(sol61.price.input_microusd_per_1m, Some(2_000_000));
        assert_eq!(sol61.price.cached_input_microusd_per_1m, Some(100_000));
        assert_eq!(sol61.price.cache_write_microusd_per_1m, Some(2_500_000));
        assert_eq!(sol61.price.output_microusd_per_1m, Some(10_000_000));
        assert_eq!(sol61.price_tiers.len(), 2);
        assert_eq!(sol61.price_tiers[1].min_input_tokens, 272_001);
        assert_eq!(sol61.price_tiers[1].input_microusd_per_1m, 4_000_000);
        assert_eq!(sol61.price_tiers[1].cached_input_microusd_per_1m, 200_000);
        assert_eq!(
            sol61.price_tiers[1].cache_write_microusd_per_1m,
            Some(5_000_000)
        );
        assert_eq!(sol61.price_tiers[1].output_microusd_per_1m, 15_000_000);
        assert_eq!(sol61.routes.len(), 1);
        assert_eq!(sol61.routes[0].upstream_model, GPT61_SOL_SLUG);
        let metadata_fix_applied: i64 = storage
            .conn
            .query_row(
                "SELECT COUNT(*) FROM schema_migrations WHERE version=?1",
                [GPT56_METADATA_FIX_MIGRATION_VERSION],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(metadata_fix_applied, 1);
        let astra = all
            .iter()
            .find(|model| model.slug == GPT6_ASTRA_SLUG)
            .unwrap();
        assert_eq!(astra.display_name, "GPT-6-Astra");
        assert_eq!(
            astra.description.as_deref(),
            Some("Our most capable model for complex, demanding work.")
        );
        assert_eq!(astra.sort_order, 1);
        assert_eq!(astra.builtin_revision, Some(9));
        assert_eq!(astra.default_reasoning_effort.as_deref(), Some("low"));
        assert_eq!(astra.context_window, Some(272_000));
        assert_eq!(astra.max_context_window, Some(872_000));
        assert_eq!(
            astra.capabilities["reasoning_efforts"],
            serde_json::json!(["low", "medium", "high", "xhigh", "max", "ultra"])
        );
        assert_eq!(
            astra.capabilities["service_tiers"],
            serde_json::json!(["priority"])
        );
        assert_eq!(astra.capabilities["api_context_window"], 1_050_000);
        assert_eq!(astra.capabilities["max_output_tokens"], 128_000);
        assert_eq!(
            astra.capabilities["supported_endpoints"],
            serde_json::json!(["/v1/responses", "/v1/chat/completions"])
        );
        assert_eq!(astra.price.price_status, "official");
        assert_eq!(
            astra.price.price_source.as_deref(),
            Some(GPT6_ASTRA_PRICE_SOURCE)
        );
        assert_eq!(astra.price.input_microusd_per_1m, Some(10_000_000));
        assert_eq!(astra.price.cached_input_microusd_per_1m, Some(1_000_000));
        assert_eq!(astra.price.cache_write_microusd_per_1m, Some(12_500_000));
        assert_eq!(astra.price.output_microusd_per_1m, Some(50_000_000));
        assert_eq!(astra.price_tiers.len(), 2);
        assert_eq!(astra.price_tiers[1].min_input_tokens, 272_001);
        assert_eq!(astra.price_tiers[1].input_microusd_per_1m, 20_000_000);
        assert_eq!(astra.price_tiers[1].cached_input_microusd_per_1m, 2_000_000);
        assert_eq!(
            astra.price_tiers[1].cache_write_microusd_per_1m,
            Some(25_000_000)
        );
        assert_eq!(astra.price_tiers[1].output_microusd_per_1m, 75_000_000);
        assert_eq!(astra.routes.len(), 1);
        assert_eq!(astra.routes[0].source_kind, "account_pool");
        assert_eq!(astra.routes[0].upstream_model, GPT6_ASTRA_SLUG);
        let luna = all.iter().find(|model| model.slug == "gpt-6-luna").unwrap();
        assert_eq!(luna.default_reasoning_effort.as_deref(), Some("high"));
        let image = all
            .iter()
            .find(|model| model.slug == "gpt-image-2")
            .unwrap();
        assert_eq!(image.display_name, "GPT Image 2");
        assert_eq!(image.builtin_revision, Some(9));
        assert_eq!(image.context_window, None);
        assert_eq!(image.max_context_window, None);
        assert_eq!(image.default_reasoning_effort, None);
        assert_eq!(image.price.price_status, "official");
        assert_eq!(
            image.price.price_source.as_deref(),
            Some(GPT_IMAGE_2_PRICE_SOURCE)
        );
        assert_eq!(image.price.input_microusd_per_1m, Some(8_000_000));
        assert_eq!(image.price.cached_input_microusd_per_1m, Some(2_000_000));
        assert_eq!(image.price.output_microusd_per_1m, Some(30_000_000));
        assert_eq!(image.price_tiers.len(), 1);
        assert_eq!(image.capabilities["supports_text_generation"], false);
        assert_eq!(
            image.capabilities["output_modalities"],
            serde_json::json!(["image"])
        );
        assert_eq!(image.routes.len(), 1);
        assert_eq!(image.routes[0].upstream_model, "gpt-image-2");
        for slug in ["gpt-image-2.5-sunburst", "gpt-image-2.5-flare"] {
            let image = all.iter().find(|model| model.slug == slug).unwrap();
            assert_eq!(image.price.price_status, "missing");
            assert!(image.price_tiers.is_empty());
            assert!(image.price.input_microusd_per_1m.is_none());
            assert!(image.price.cached_input_microusd_per_1m.is_none());
            assert!(image.price.output_microusd_per_1m.is_none());
            assert_eq!(image.routes.len(), 1);
            assert_eq!(image.routes[0].upstream_model, slug);
            assert!(visible.iter().any(|model| model.slug == slug));
        }
        assert!(all
            .iter()
            .all(|model| model.instructions_mode == "passthrough"
                && model.instructions_text.is_none()));
        assert!(all
            .iter()
            .all(|model| model.fast_policy == ModelFastPolicyV2::Passthrough));
    }

    #[test]
    fn revision9_migration_reconciles_new_and_user_owned_models() {
        let storage = storage();
        reset_to_revision8_catalog(&storage);

        // Price sync and route creation can customize dependent state without
        // toggling user_edited. Reconciliation must update model metadata while
        // preserving the custom base price, deleted tiers, and extra routes.
        storage
            .conn
            .execute_batch(
                "UPDATE models
                 SET enabled=1,supported_in_api=1,visibility='list',
                     builtin_revision=8,user_edited=0
                 WHERE slug='gpt-5.4';
                 UPDATE model_prices
                 SET input_microusd_per_1m=1111111,
                     cached_input_microusd_per_1m=222222,
                     cache_write_microusd_per_1m=333333,
                     output_microusd_per_1m=4444444,
                     price_status='estimated',
                     price_source='runtime-custom'
                 WHERE model_id=(SELECT id FROM models WHERE slug='gpt-5.4');
                 DELETE FROM model_price_tiers
                 WHERE model_id=(SELECT id FROM models WHERE slug='gpt-5.4');
                 INSERT INTO model_routes(
                   id,model_id,source_kind,source_id,upstream_model,enabled,
                   priority,weight,created_at,updated_at
                 ) VALUES(
                   'revision9-custom-route',
                   (SELECT id FROM models WHERE slug='gpt-5.4'),
                   'aggregate_api','aggregate-test','vendor-gpt-5.4',1,7,3,1,1
                 );",
            )
            .unwrap();

        // A retained built-in that the user edited owns all of its dependent
        // catalog state as well. Seeding must not restore deleted tiers/routes
        // or replace its custom base price.
        storage
            .conn
            .execute_batch(
                "UPDATE models
                 SET display_name='Edited Terra',builtin_revision=8,user_edited=1
                 WHERE slug='gpt-5.6-terra';
                 UPDATE model_prices
                 SET input_microusd_per_1m=1234567,
                     cached_input_microusd_per_1m=234567,
                     cache_write_microusd_per_1m=345678,
                     output_microusd_per_1m=7654321,
                     price_source='user-edited'
                 WHERE model_id=(SELECT id FROM models WHERE slug='gpt-5.6-terra');
                 DELETE FROM model_price_tiers
                 WHERE model_id=(SELECT id FROM models WHERE slug='gpt-5.6-terra');
                 DELETE FROM model_routes
                 WHERE model_id=(SELECT id FROM models WHERE slug='gpt-5.6-terra');",
            )
            .unwrap();

        // These image models are not retired as of this catalog revision.
        // Preserve an older database's rows until their actual shutdown date.
        for (slug, display_name) in [
            ("gpt-image-1", "GPT Image 1"),
            ("gpt-image-1-mini", "GPT Image 1 Mini"),
            ("gpt-image-1.5", "GPT Image 1.5"),
            ("chatgpt-image-latest", "ChatGPT Image Latest"),
        ] {
            let id = format!("legacy:{slug}");
            storage
                .conn
                .execute(
                    "INSERT INTO models(
                       id,slug,display_name,description,origin,enabled,supported_in_api,
                       visibility,sort_order,context_window,max_context_window,
                       default_reasoning_effort,capabilities_json,instructions_mode,
                       instructions_text,builtin_revision,user_edited,created_at,updated_at
                     ) VALUES(?1,?2,?3,NULL,'builtin',1,1,'list',90,NULL,NULL,NULL,
                              '{\"supports_image_generation\":true}','passthrough',NULL,8,0,1,1)",
                    params![id.as_str(), slug, display_name],
                )
                .unwrap();
            storage
                .conn
                .execute(
                    "INSERT INTO model_prices(
                       model_id,currency,input_microusd_per_1m,cached_input_microusd_per_1m,
                       cache_write_microusd_per_1m,output_microusd_per_1m,price_status,
                       price_source,created_at,updated_at
                     ) VALUES(?1,'USD',NULL,NULL,NULL,NULL,'missing',NULL,1,1)",
                    [id],
                )
                .unwrap();
        }
        storage
            .conn
            .execute(
                "UPDATE models
                 SET enabled=1,supported_in_api=1,visibility='list',
                     builtin_revision=8,user_edited=1
                 WHERE slug='gpt-5.4-mini'",
                [],
            )
            .unwrap();

        let mut custom_luna = custom_creation_candidate("gpt-6-luna");
        custom_luna.model.display_name = "Custom Luna".to_string();
        storage
            .upsert_managed_model_v2(&custom_luna)
            .expect("create custom slug collision");

        storage
            .apply_model_catalog_revision9_migration()
            .expect("apply revision 9 migration");

        let sol = storage
            .get_managed_model_v2("gpt-6-sol")
            .unwrap()
            .expect("seeded GPT-6 Sol");
        assert_eq!(sol.origin, "builtin");
        assert_eq!(sol.builtin_revision, Some(9));
        assert_eq!(sol.default_reasoning_effort.as_deref(), Some("medium"));
        assert_eq!(sol.capabilities["shell_type"], "shell_command");
        assert_eq!(sol.capabilities["reasoning_efforts"][5], "ultra");
        assert_eq!(sol.price.input_microusd_per_1m, Some(2_000_000));
        assert_eq!(sol.routes.len(), 1);

        let custom_luna = storage
            .get_managed_model_v2("gpt-6-luna")
            .unwrap()
            .expect("preserved custom Luna");
        assert_eq!(custom_luna.origin, "custom");
        assert!(custom_luna.user_edited);
        assert_eq!(custom_luna.display_name, "Custom Luna");

        let image = storage
            .get_managed_model_v2("gpt-image-2.5-sunburst")
            .unwrap()
            .expect("seeded image 2.5 model");
        assert_eq!(image.builtin_revision, Some(9));
        assert_eq!(image.price.price_status, "missing");
        assert!(image.price.input_microusd_per_1m.is_none());
        assert!(image.price_tiers.is_empty());
        assert_eq!(image.routes.len(), 1);
        assert_eq!(image.routes[0].upstream_model, "gpt-image-2.5-sunburst");
        assert_eq!(image.capabilities["supports_image_generation"], true);
        assert_eq!(image.capabilities["supports_image_editing"], true);
        assert_eq!(
            image.capabilities["quality_settings"],
            serde_json::json!(["low", "medium", "high", "xhigh", "max", "auto"])
        );
        assert_eq!(
            image.capabilities["snapshot"],
            "gpt-image-2.5-sunburst-2026-09-08"
        );

        let existing = storage
            .get_managed_model_v2("gpt-5.4")
            .unwrap()
            .expect("customized retired model remains available");
        assert_eq!(existing.origin, "custom");
        assert!(existing.user_edited);
        assert_eq!(existing.builtin_revision, None);
        assert!(existing.enabled);
        assert!(existing.supported_in_api);
        assert_eq!(existing.visibility, "list");
        assert_eq!(existing.capabilities["shell_type"], "shell_command");
        assert_eq!(existing.price.input_microusd_per_1m, Some(1_111_111));
        assert_eq!(existing.price.cached_input_microusd_per_1m, Some(222_222));
        assert_eq!(existing.price.cache_write_microusd_per_1m, Some(333_333));
        assert_eq!(existing.price.output_microusd_per_1m, Some(4_444_444));
        assert_eq!(existing.price.price_status, "estimated");
        assert_eq!(
            existing.price.price_source.as_deref(),
            Some("runtime-custom")
        );
        assert!(existing.price_tiers.is_empty());
        assert_eq!(existing.routes.len(), 1);
        assert!(existing.routes.iter().all(|route| {
            route.source_kind == "aggregate_api"
                && route.source_id == "aggregate-test"
                && route.upstream_model == "vendor-gpt-5.4"
                && route.priority == 7
                && route.weight == 3
        }));

        let user_edited = storage
            .get_managed_model_v2("gpt-5.4-mini")
            .unwrap()
            .expect("preserved user-edited retired model");
        assert_eq!(user_edited.origin, "custom");
        assert!(user_edited.user_edited);
        assert_eq!(user_edited.builtin_revision, None);
        assert!(user_edited.enabled);
        assert_eq!(user_edited.visibility, "list");
        assert!(user_edited.routes.is_empty());

        assert!(storage.get_managed_model_v2("gpt-5.2").unwrap().is_none());
        for slug in RETIRED_CODEX_MODEL_SLUGS_REVISION9 {
            let tombstoned = storage
                .conn
                .query_row(
                    "SELECT 1 FROM model_catalog_v2_meta WHERE key=?1",
                    [deleted_builtin_meta_key(slug)],
                    |_| Ok(()),
                )
                .optional()
                .unwrap()
                .is_some();
            assert!(tombstoned, "{slug} should have a retirement tombstone");
        }

        let edited_retained = storage
            .get_managed_model_v2("gpt-5.6-terra")
            .unwrap()
            .expect("preserved user-edited retained builtin");
        assert!(edited_retained.user_edited);
        assert_eq!(edited_retained.display_name, "Edited Terra");
        assert_eq!(edited_retained.price.input_microusd_per_1m, Some(1_234_567));
        assert_eq!(
            edited_retained.price.cached_input_microusd_per_1m,
            Some(234_567)
        );
        assert_eq!(
            edited_retained.price.cache_write_microusd_per_1m,
            Some(345_678)
        );
        assert_eq!(
            edited_retained.price.output_microusd_per_1m,
            Some(7_654_321)
        );
        assert_eq!(
            edited_retained.price.price_source.as_deref(),
            Some("user-edited")
        );
        assert!(edited_retained.price_tiers.is_empty());
        assert!(edited_retained.routes.is_empty());

        let visible_after_upgrade = storage.list_api_models_v2().unwrap();
        assert!(visible_after_upgrade
            .iter()
            .any(|model| model.slug == "gpt-5.4"));
        for slug in [
            "gpt-image-1",
            "gpt-image-1-mini",
            "gpt-image-1.5",
            "chatgpt-image-latest",
        ] {
            let preserved_image = storage
                .get_managed_model_v2(slug)
                .unwrap()
                .expect("preserved not-yet-retired image model");
            assert!(preserved_image.enabled);
            assert!(preserved_image.supported_in_api);
            assert_eq!(preserved_image.visibility, "list");
            assert!(visible_after_upgrade.iter().any(|model| model.slug == slug));
        }

        let count_after_first = storage.list_managed_models_v2(true).unwrap().len();
        storage
            .conn
            .execute(
                "DELETE FROM schema_migrations WHERE version=?1",
                [MODEL_CATALOG_REVISION9_MIGRATION_VERSION],
            )
            .unwrap();
        storage.migration_cache().take();
        storage
            .apply_model_catalog_revision9_migration()
            .expect("force replay revision 9 migration");
        assert_eq!(
            storage.list_managed_models_v2(true).unwrap().len(),
            count_after_first
        );
        assert_eq!(
            storage
                .get_managed_model_v2("gpt-6-luna")
                .unwrap()
                .unwrap()
                .display_name,
            "Custom Luna"
        );
    }

    #[test]
    fn revision9_migration_preserves_permission_and_custom_retired_models() {
        let storage = storage();
        reset_to_revision8_catalog(&storage);
        storage
            .conn
            .execute_batch(
                "INSERT INTO model_groups(
                   id,name,description,status,sort,is_default,rate_multiplier_millis,
                   created_at,updated_at
                 ) VALUES('mg-retired','Retired model access',NULL,'active',1,0,1000,1,1);
                 INSERT INTO model_group_models_v2(
                   group_id,model_id,enabled,rate_multiplier_millis,created_at,updated_at
                 ) SELECT 'mg-retired',id,1,1000,1,1
                   FROM models WHERE slug='gpt-5.4-mini';
                 UPDATE models
                 SET origin='custom',builtin_revision=NULL,user_edited=1
                 WHERE slug='gpt-5.2';
                 INSERT INTO model_routes(
                   id,model_id,source_kind,source_id,upstream_model,enabled,
                   priority,weight,created_at,updated_at
                 ) SELECT 'revision9-custom-retired-route',id,'aggregate_api',
                          'aggregate-retired','vendor-gpt-5.2',1,5,2,1,1
                   FROM models WHERE slug='gpt-5.2';",
            )
            .unwrap();

        storage
            .apply_model_catalog_revision9_migration()
            .expect("apply revision 9 migration");

        assert!(storage.get_managed_model_v2("gpt-5.4").unwrap().is_none());
        let permission_owned = storage
            .get_managed_model_v2("gpt-5.4-mini")
            .unwrap()
            .expect("permission-owned retired model remains");
        assert_eq!(permission_owned.origin, "custom");
        assert_eq!(permission_owned.builtin_revision, None);
        assert_eq!(permission_owned.permission_group_ids, ["mg-retired"]);
        assert!(permission_owned.routes.is_empty());

        let custom = storage
            .get_managed_model_v2("gpt-5.2")
            .unwrap()
            .expect("custom retired model remains");
        assert_eq!(custom.origin, "custom");
        assert_eq!(custom.builtin_revision, None);
        assert!(custom.user_edited);
        assert_eq!(custom.routes.len(), 2);
        assert!(custom.routes.iter().any(|route| {
            route.source_kind == "account_pool"
                && route.source_id == "default"
                && route.upstream_model == "gpt-5.2"
        }));
        assert!(custom.routes.iter().any(|route| {
            route.source_kind == "aggregate_api"
                && route.source_id == "aggregate-retired"
                && route.upstream_model == "vendor-gpt-5.2"
        }));
    }

    #[test]
    fn revision9_migration_preserves_custom_account_pool_routes() {
        let storage = storage();
        reset_to_revision8_catalog(&storage);
        storage
            .conn
            .execute(
                "INSERT INTO model_routes(
                   id,model_id,source_kind,source_id,upstream_model,enabled,
                   priority,weight,created_at,updated_at
                 ) SELECT 'revision9-custom-account-route',id,'account_pool',
                          'fallback-pool','gpt-6-luna',1,7,2,1,1
                   FROM models WHERE slug='gpt-5.4'",
                [],
            )
            .unwrap();

        storage
            .apply_model_catalog_revision9_migration()
            .expect("apply revision 9 migration");

        let model = storage
            .get_managed_model_v2("gpt-5.4")
            .unwrap()
            .expect("custom-routed retired model remains");
        assert_eq!(model.origin, "custom");
        assert_eq!(model.builtin_revision, None);
        assert!(model.user_edited);
        assert_eq!(model.routes.len(), 1);
        assert_eq!(model.routes[0].source_kind, "account_pool");
        assert_eq!(model.routes[0].source_id, "fallback-pool");
        assert_eq!(model.routes[0].upstream_model, "gpt-6-luna");
        assert_eq!(model.routes[0].priority, 7);
        assert_eq!(model.routes[0].weight, 2);
    }

    #[test]
    fn revision9_migration_preserves_disabled_permission_group_rows() {
        let storage = storage();
        reset_to_revision8_catalog(&storage);
        storage
            .conn
            .execute_batch(
                "INSERT INTO model_groups(
                   id,name,description,status,sort,is_default,rate_multiplier_millis,
                   created_at,updated_at
                 ) VALUES('mg-retired-disabled','Disabled retired model access',NULL,
                          'active',1,0,1000,1,1);
                 INSERT INTO model_group_models_v2(
                   group_id,model_id,enabled,rate_multiplier_millis,created_at,updated_at
                 ) SELECT 'mg-retired-disabled',id,0,1000,1,1
                   FROM models WHERE slug='gpt-5.4';",
            )
            .unwrap();

        storage
            .apply_model_catalog_revision9_migration()
            .expect("apply revision 9 migration");

        let model = storage
            .get_managed_model_v2("gpt-5.4")
            .unwrap()
            .expect("disabled permission row keeps retired model");
        assert_eq!(model.origin, "custom");
        assert!(model.permission_group_ids.is_empty());
        assert!(model.routes.is_empty());
        assert_eq!(
            storage
                .conn
                .query_row(
                    "SELECT enabled FROM model_group_models_v2
                     WHERE group_id='mg-retired-disabled' AND model_id=?1",
                    [model.id],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            0
        );
    }

    #[test]
    fn revision9_migration_preserves_api_key_bound_retired_models() {
        let storage = storage();
        reset_to_revision8_catalog(&storage);
        storage
            .conn
            .execute_batch(
                "INSERT INTO api_keys(
                   id,name,model_slug,key_hash,status,created_at,last_used_at
                 ) VALUES(
                   'key-legacy-retired','legacy retired','gpt-5.4',
                   'hash-legacy-retired','active',1,NULL
                 );
                 INSERT INTO api_keys(
                   id,name,model_slug,key_hash,status,created_at,last_used_at
                 ) VALUES(
                   'key-profile-retired','profile retired',NULL,
                   'hash-profile-retired','active',1,NULL
                 );
                 INSERT INTO api_key_profiles(
                   key_id,client_type,protocol_type,auth_scheme,upstream_base_url,
                   static_headers_json,default_model,reasoning_effort,service_tier,
                   created_at,updated_at
                 ) VALUES(
                   'key-profile-retired','codex','openai_compat','authorization_bearer',
                   NULL,NULL,'gpt-5.4-mini',NULL,NULL,1,1
                 );",
            )
            .unwrap();

        storage
            .apply_model_catalog_revision9_migration()
            .expect("apply revision 9 migration");

        for slug in ["gpt-5.4", "gpt-5.4-mini"] {
            let model = storage
                .get_managed_model_v2(slug)
                .unwrap()
                .expect("API-key-bound retired model remains");
            assert_eq!(model.origin, "custom");
            assert_eq!(model.builtin_revision, None);
            assert!(model.user_edited);
            assert!(model.routes.is_empty());
        }
        assert_eq!(
            storage
                .conn
                .query_row(
                    "SELECT model_slug FROM api_keys WHERE id='key-legacy-retired'",
                    [],
                    |row| row.get::<_, Option<String>>(0),
                )
                .unwrap()
                .as_deref(),
            Some("gpt-5.4")
        );
        assert_eq!(
            storage
                .conn
                .query_row(
                    "SELECT default_model FROM api_key_profiles
                     WHERE key_id='key-profile-retired'",
                    [],
                    |row| row.get::<_, Option<String>>(0),
                )
                .unwrap()
                .as_deref(),
            Some("gpt-5.4-mini")
        );
        assert!(storage.get_managed_model_v2("gpt-5.2").unwrap().is_none());
    }

    #[test]
    fn revision9_migration_respects_deleted_builtin_tombstones() {
        let storage = storage();
        reset_to_revision8_catalog(&storage);
        storage
            .delete_managed_model_v2("gpt-5.4")
            .expect("delete built-in model");

        storage
            .apply_model_catalog_revision9_migration()
            .expect("apply revision 9 with deleted built-in");

        assert!(storage.get_managed_model_v2("gpt-5.4").unwrap().is_none());
        assert!(storage.get_managed_model_v2("gpt-6-sol").unwrap().is_some());
        migration_smoke_for_fixture(&storage.conn, &revision9_fixture()).unwrap();
    }

    #[test]
    fn revision9_migration_does_not_downgrade_future_catalog_state() {
        let storage = storage();
        storage
            .conn
            .execute(
                "DELETE FROM schema_migrations WHERE version=?1",
                [MODEL_CATALOG_REVISION9_MIGRATION_VERSION],
            )
            .unwrap();
        storage.migration_cache().take();
        storage
            .conn
            .execute(
                "UPDATE models SET builtin_revision=11
                 WHERE origin='builtin' AND user_edited=0",
                [],
            )
            .unwrap();
        storage
            .conn
            .execute(
                "UPDATE models
                 SET display_name='Future GPT-6 Sol',capabilities_json=?1
                 WHERE slug='gpt-6-sol'",
                [r#"{"future":true}"#],
            )
            .unwrap();
        storage
            .conn
            .execute("DELETE FROM models WHERE slug='gpt-5.5'", [])
            .unwrap();
        storage
            .conn
            .execute(
                "UPDATE model_catalog_v2_meta SET value='11' WHERE key='builtin_revision'",
                [],
            )
            .unwrap();
        storage
            .conn
            .execute(
                "UPDATE model_catalog_v2_meta SET value='future-fixture-sha256'
                 WHERE key='fixture_sha256'",
                [],
            )
            .unwrap();

        storage
            .apply_model_catalog_revision9_migration()
            .expect("mark revision 9 migration without catalog downgrade");
        storage
            .seed_missing_builtin_models_v2()
            .expect("skip current fixture for future catalog");

        assert!(storage.get_managed_model_v2("gpt-5.5").unwrap().is_none());
        let sol = storage
            .get_managed_model_v2("gpt-6-sol")
            .unwrap()
            .expect("future Sol remains");
        assert_eq!(sol.display_name, "Future GPT-6 Sol");
        assert_eq!(sol.builtin_revision, Some(11));
        assert_eq!(sol.capabilities["future"], true);
        let revision: String = storage
            .conn
            .query_row(
                "SELECT value FROM model_catalog_v2_meta WHERE key='builtin_revision'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(revision, "11");
        let source_sha256: String = storage
            .conn
            .query_row(
                "SELECT value FROM model_catalog_v2_meta WHERE key='fixture_sha256'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(source_sha256, "future-fixture-sha256");
        assert!(storage
            .has_migration(MODEL_CATALOG_REVISION9_MIGRATION_VERSION)
            .unwrap());
    }

    #[test]
    fn model_fast_policy_round_trips() {
        let storage = storage();
        let mut model = storage
            .get_managed_model_v2("gpt-5.6-sol")
            .unwrap()
            .unwrap();
        model.fast_policy = ModelFastPolicyV2::Filter;

        let saved = storage
            .upsert_managed_model_v2(&ManagedModelV2Upsert {
                previous_slug: Some(model.slug.clone()),
                model,
            })
            .unwrap();

        assert_eq!(saved.fast_policy, ModelFastPolicyV2::Filter);
        assert_eq!(
            storage
                .get_managed_model_v2("gpt-5.6-sol")
                .unwrap()
                .unwrap()
                .fast_policy,
            ModelFastPolicyV2::Filter
        );
    }

    #[test]
    fn fast_policy_migration_defaults_existing_models() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE models(id TEXT PRIMARY KEY);
             INSERT INTO models(id) VALUES('existing');",
        )
        .unwrap();
        conn.execute_batch(include_str!("../../migrations/129_model_fast_policy.sql"))
            .unwrap();

        let policy: String = conn
            .query_row(
                "SELECT fast_policy FROM models WHERE id='existing'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(policy, "passthrough");
        assert!(conn
            .execute(
                "UPDATE models SET fast_policy='invalid' WHERE id='existing'",
                [],
            )
            .is_err());
    }

    #[test]
    fn seed_is_idempotent_and_does_not_overwrite_builtin_edits() {
        let storage = storage();
        storage.conn.execute("UPDATE models SET display_name='Edited',enabled=0,user_edited=1 WHERE slug='gpt-5.6-terra'",[]).unwrap();
        storage.seed_missing_builtin_models_v2().unwrap();
        let model = storage
            .get_managed_model_v2("gpt-5.6-terra")
            .unwrap()
            .unwrap();
        assert_eq!(model.display_name, "Edited");
        assert!(!model.enabled);
    }

    #[test]
    fn cache_write_price_migration_preserves_custom_gpt56_prices() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE models(
               id TEXT PRIMARY KEY,slug TEXT,origin TEXT,user_edited INTEGER,
               builtin_revision INTEGER,updated_at INTEGER
             );
             CREATE TABLE model_prices(
               model_id TEXT PRIMARY KEY,price_status TEXT,price_source TEXT,
               input_microusd_per_1m INTEGER,cached_input_microusd_per_1m INTEGER,
               output_microusd_per_1m INTEGER,updated_at INTEGER
             );
             CREATE TABLE model_price_tiers(
               model_id TEXT,min_input_tokens INTEGER,input_microusd_per_1m INTEGER,
               cached_input_microusd_per_1m INTEGER,output_microusd_per_1m INTEGER
             );
             CREATE TABLE request_charge_snapshots(input_microusd_per_1m INTEGER);
             CREATE TABLE model_catalog_v2_meta(key TEXT PRIMARY KEY,value TEXT);
             INSERT INTO models VALUES('terra','gpt-5.6-terra','builtin',1,6,1);
             INSERT INTO model_prices VALUES(
               'terra','custom','local-ui',9000000,8000000,7000000,1
             );
             INSERT INTO model_price_tiers VALUES('terra',0,9000000,8000000,7000000);
             INSERT INTO request_charge_snapshots VALUES(1234567);",
        )
        .unwrap();

        conn.execute_batch(include_str!(
            "../../migrations/127_model_catalog_cache_write_prices.sql"
        ))
        .unwrap();

        let (price, revision): (Option<i64>, i64) = conn
            .query_row(
                "SELECT p.cache_write_microusd_per_1m,m.builtin_revision
                 FROM model_prices p JOIN models m ON m.id=p.model_id",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(price, None);
        assert_eq!(revision, 6);
        let snapshot_rate: i64 = conn
            .query_row(
                "SELECT cache_write_microusd_per_1m FROM request_charge_snapshots",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(snapshot_rate, 1_234_567);
    }

    #[test]
    fn latest_revision_seeds_image_model_into_an_existing_revision_four_catalog() {
        let storage = storage();
        storage
            .conn
            .execute("DELETE FROM models WHERE slug='gpt-image-2'", [])
            .unwrap();
        storage
            .conn
            .execute(
                "UPDATE models SET builtin_revision=4 WHERE origin='builtin'",
                [],
            )
            .unwrap();
        storage
            .conn
            .execute(
                "UPDATE model_catalog_v2_meta SET value='4' WHERE key='builtin_revision'",
                [],
            )
            .unwrap();

        storage.seed_missing_builtin_models_v2().unwrap();
        storage.smoke_check_model_catalog_v2().unwrap();

        let image = storage
            .get_managed_model_v2("gpt-image-2")
            .unwrap()
            .unwrap();
        assert_eq!(image.origin, "builtin");
        assert_eq!(image.builtin_revision, Some(10));
        assert_eq!(image.routes.len(), 1);
        assert_eq!(image.routes[0].upstream_model, "gpt-image-2");
        let sol = storage
            .get_managed_model_v2("gpt-5.6-sol")
            .unwrap()
            .unwrap();
        assert_eq!(sol.builtin_revision, Some(4));
        let revision: String = storage
            .conn
            .query_row(
                "SELECT value FROM model_catalog_v2_meta WHERE key='builtin_revision'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(revision, "10");
    }

    #[test]
    fn image_seed_preserves_an_existing_custom_slug_collision() {
        let storage = storage();
        let mut custom = storage
            .get_managed_model_v2("gpt-image-2")
            .unwrap()
            .unwrap();
        storage
            .conn
            .execute("DELETE FROM models WHERE slug='gpt-image-2'", [])
            .unwrap();
        custom.id.clear();
        custom.display_name = "My Existing Image Route".to_string();
        custom.origin = "custom".to_string();
        custom.builtin_revision = None;
        custom.user_edited = true;
        custom.price.price_status = "custom".to_string();
        custom.price.price_source = Some("existing-user-config".to_string());
        custom.routes[0].id.clear();
        custom.routes[0].upstream_model = "my-image-upstream".to_string();
        let saved = storage
            .upsert_managed_model_v2(&ManagedModelV2Upsert {
                model: custom,
                ..Default::default()
            })
            .unwrap();

        storage.seed_missing_builtin_models_v2().unwrap();
        storage.smoke_check_model_catalog_v2().unwrap();

        let preserved = storage
            .get_managed_model_v2("gpt-image-2")
            .unwrap()
            .unwrap();
        assert_eq!(preserved.id, saved.id);
        assert_eq!(preserved.origin, "custom");
        assert_eq!(preserved.builtin_revision, None);
        assert_eq!(preserved.display_name, "My Existing Image Route");
        assert_eq!(
            preserved.price.price_source.as_deref(),
            Some("existing-user-config")
        );
        assert_eq!(preserved.routes.len(), 1);
        assert_eq!(preserved.routes[0].upstream_model, "my-image-upstream");

        storage
            .delete_managed_model_v2("gpt-image-2")
            .expect("delete preserved custom model");
        storage
            .seed_missing_builtin_models_v2()
            .expect("seed builtin after custom model is removed");
        let restored = storage
            .get_managed_model_v2("gpt-image-2")
            .unwrap()
            .unwrap();
        assert_eq!(restored.origin, "builtin");
        assert_eq!(restored.builtin_revision, Some(10));
        assert_eq!(restored.routes[0].upstream_model, "gpt-image-2");
    }

    #[test]
    fn astra_migration_upgrades_an_existing_revision_seven_catalog() {
        let storage = storage();
        let mut old_sol_capabilities = storage
            .get_managed_model_v2("gpt-5.6-sol")
            .unwrap()
            .unwrap()
            .capabilities;
        old_sol_capabilities["service_tiers"] = serde_json::json!(["priority"]);
        old_sol_capabilities["shell_type"] = serde_json::json!("shell_command");

        storage
            .conn
            .execute(
                "DELETE FROM schema_migrations WHERE version=?1",
                [GPT6_ASTRA_MIGRATION_VERSION],
            )
            .unwrap();
        storage.migration_cache().take();
        storage
            .conn
            .execute("DELETE FROM models WHERE slug=?1", [GPT6_ASTRA_SLUG])
            .unwrap();
        storage
            .conn
            .execute(
                "UPDATE models SET builtin_revision=7 WHERE origin='builtin'",
                [],
            )
            .unwrap();
        storage
            .conn
            .execute(
                "UPDATE models
                 SET context_window=372000,max_context_window=372000,capabilities_json=?1
                 WHERE slug='gpt-5.6-sol'",
                [old_sol_capabilities.to_string()],
            )
            .unwrap();
        storage
            .conn
            .execute(
                "UPDATE model_catalog_v2_meta SET value='7' WHERE key='builtin_revision'",
                [],
            )
            .unwrap();

        storage.apply_model_catalog_gpt6_astra_migration().unwrap();
        let legacy_fixture = legacy_fixture();
        seed_missing_with_fixture(&storage.conn, &legacy_fixture).unwrap();
        storage.apply_model_catalog_gpt6_astra_migration().unwrap();

        let astra = storage
            .get_managed_model_v2(GPT6_ASTRA_SLUG)
            .unwrap()
            .unwrap();
        assert_eq!(astra.origin, "builtin");
        assert_eq!(astra.builtin_revision, Some(8));
        assert_eq!(astra.context_window, Some(272_000));
        assert_eq!(astra.max_context_window, Some(872_000));
        assert_eq!(astra.capabilities["api_context_window"], 1_050_000);
        assert_eq!(astra.capabilities["max_output_tokens"], 128_000);
        assert_eq!(astra.routes.len(), 1);
        assert_eq!(astra.routes[0].upstream_model, GPT6_ASTRA_SLUG);

        let sol = storage
            .get_managed_model_v2("gpt-5.6-sol")
            .unwrap()
            .unwrap();
        assert_eq!(sol.builtin_revision, Some(8));
        assert_eq!(sol.context_window, Some(372_000));
        assert_eq!(sol.max_context_window, Some(372_000));
        assert_eq!(sol.capabilities["shell_type"], "shell_command");
        assert_eq!(
            sol.capabilities["service_tiers"],
            serde_json::json!(["priority", "ultrafast"])
        );
        let image = storage
            .get_managed_model_v2("gpt-image-2")
            .unwrap()
            .unwrap();
        assert_eq!(image.builtin_revision, Some(7));

        let revision: String = storage
            .conn
            .query_row(
                "SELECT value FROM model_catalog_v2_meta WHERE key='builtin_revision'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(revision, "8");
        let applied: i64 = storage
            .conn
            .query_row(
                "SELECT COUNT(*) FROM schema_migrations WHERE version=?1",
                [GPT6_ASTRA_MIGRATION_VERSION],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(applied, 1);
        let migration_revision: String = storage
            .conn
            .query_row(
                "SELECT value FROM model_catalog_v2_meta
                 WHERE key='gpt6_astra_catalog_revision'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(migration_revision, "2026-09-04-official");
        let price_source: String = storage
            .conn
            .query_row(
                "SELECT value FROM model_catalog_v2_meta
                 WHERE key='gpt6_astra_price_source'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(price_source, GPT6_ASTRA_PRICE_SOURCE);
    }

    #[test]
    fn gpt56_metadata_fix_upgrades_a_database_after_131_and_is_replay_idempotent() {
        let storage = storage();
        for slug in ["gpt-5.6-sol", "gpt-5.6-terra", "gpt-5.6-luna"] {
            let mut old_capabilities = storage
                .get_managed_model_v2(slug)
                .unwrap()
                .unwrap()
                .capabilities;
            old_capabilities["shell_type"] = serde_json::json!("shell_command");
            storage
                .conn
                .execute(
                    "UPDATE models
                     SET context_window=372000,max_context_window=372000,
                         capabilities_json=?1,builtin_revision=8,user_edited=0,updated_at=7
                     WHERE slug=?2",
                    params![old_capabilities.to_string(), slug],
                )
                .unwrap();
        }
        storage
            .conn
            .execute(
                "DELETE FROM schema_migrations WHERE version=?1",
                [GPT56_METADATA_FIX_MIGRATION_VERSION],
            )
            .unwrap();
        storage.migration_cache().take();

        let astra_migration_applied: i64 = storage
            .conn
            .query_row(
                "SELECT COUNT(*) FROM schema_migrations WHERE version=?1",
                [GPT6_ASTRA_MIGRATION_VERSION],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(astra_migration_applied, 1);

        storage
            .apply_model_catalog_gpt56_metadata_fix_migration()
            .unwrap();
        for (slug, expected_service_tiers) in [
            ("gpt-5.6-sol", serde_json::json!(["priority", "ultrafast"])),
            ("gpt-5.6-terra", serde_json::json!(["priority"])),
            ("gpt-5.6-luna", serde_json::json!(["priority"])),
        ] {
            let migrated = storage.get_managed_model_v2(slug).unwrap().unwrap();
            assert_eq!(migrated.builtin_revision, Some(8));
            assert_eq!(migrated.context_window, Some(272_000));
            assert_eq!(migrated.max_context_window, Some(872_000));
            assert_eq!(migrated.capabilities["shell_type"], "unified_exec");
            assert_eq!(
                migrated.capabilities["service_tiers"],
                expected_service_tiers
            );
        }

        storage
            .conn
            .execute(
                "UPDATE models SET updated_at=123456789
                 WHERE lower(slug) IN ('gpt-5.6-sol','gpt-5.6-terra','gpt-5.6-luna')",
                [],
            )
            .unwrap();
        storage
            .conn
            .execute(
                "DELETE FROM schema_migrations WHERE version=?1",
                [GPT56_METADATA_FIX_MIGRATION_VERSION],
            )
            .unwrap();
        storage.migration_cache().take();
        storage
            .apply_model_catalog_gpt56_metadata_fix_migration()
            .unwrap();

        for slug in ["gpt-5.6-sol", "gpt-5.6-terra", "gpt-5.6-luna"] {
            let replayed = storage.get_managed_model_v2(slug).unwrap().unwrap();
            assert_eq!(replayed.updated_at, 123_456_789);
        }
        let applied: i64 = storage
            .conn
            .query_row(
                "SELECT COUNT(*) FROM schema_migrations WHERE version=?1",
                [GPT56_METADATA_FIX_MIGRATION_VERSION],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(applied, 1);
    }

    #[test]
    fn gpt56_metadata_fix_preserves_user_edited_metadata() {
        let storage = storage();
        let custom_capabilities = serde_json::json!({
            "shell_type": "custom_shell",
            "service_tiers": ["private-fast"],
            "custom": true
        });
        storage
            .conn
            .execute(
                "UPDATE models
                 SET context_window=111111,max_context_window=222222,
                     capabilities_json=?1,builtin_revision=8,user_edited=1,updated_at=77
                 WHERE slug='gpt-5.6-terra'",
                [custom_capabilities.to_string()],
            )
            .unwrap();
        storage
            .conn
            .execute(
                "DELETE FROM schema_migrations WHERE version=?1",
                [GPT56_METADATA_FIX_MIGRATION_VERSION],
            )
            .unwrap();
        storage.migration_cache().take();

        storage
            .apply_model_catalog_gpt56_metadata_fix_migration()
            .unwrap();
        storage
            .apply_model_catalog_gpt56_metadata_fix_migration()
            .unwrap();

        let preserved = storage
            .get_managed_model_v2("gpt-5.6-terra")
            .unwrap()
            .unwrap();
        assert!(preserved.user_edited);
        assert_eq!(preserved.builtin_revision, Some(8));
        assert_eq!(preserved.context_window, Some(111_111));
        assert_eq!(preserved.max_context_window, Some(222_222));
        assert_eq!(preserved.capabilities, custom_capabilities);
        assert_eq!(preserved.updated_at, 77);
    }

    #[test]
    fn gpt56_metadata_fix_preserves_future_builtin_metadata() {
        let storage = storage();
        let future_capabilities = serde_json::json!({
            "shell_type": "future_shell",
            "service_tiers": ["future-tier"],
            "future": true
        });
        storage
            .conn
            .execute(
                "UPDATE models
                 SET context_window=999111,max_context_window=999222,
                     capabilities_json=?1,builtin_revision=9,user_edited=0,updated_at=88
                 WHERE slug='gpt-5.6-luna'",
                [future_capabilities.to_string()],
            )
            .unwrap();
        storage
            .conn
            .execute(
                "DELETE FROM schema_migrations WHERE version=?1",
                [GPT56_METADATA_FIX_MIGRATION_VERSION],
            )
            .unwrap();
        storage.migration_cache().take();

        storage
            .apply_model_catalog_gpt56_metadata_fix_migration()
            .unwrap();
        storage
            .apply_model_catalog_gpt56_metadata_fix_migration()
            .unwrap();

        let preserved = storage
            .get_managed_model_v2("gpt-5.6-luna")
            .unwrap()
            .unwrap();
        assert!(!preserved.user_edited);
        assert_eq!(preserved.builtin_revision, Some(9));
        assert_eq!(preserved.context_window, Some(999_111));
        assert_eq!(preserved.max_context_window, Some(999_222));
        assert_eq!(preserved.capabilities, future_capabilities);
        assert_eq!(preserved.updated_at, 88);
    }

    #[test]
    fn astra_migration_preserves_custom_slug_and_user_edited_sol_metadata() {
        let storage = storage();
        let mut custom = storage
            .get_managed_model_v2(GPT6_ASTRA_SLUG)
            .unwrap()
            .unwrap();
        storage
            .conn
            .execute("DELETE FROM models WHERE slug=?1", [GPT6_ASTRA_SLUG])
            .unwrap();
        custom.id.clear();
        custom.slug = "GpT-6-AsTrA".to_string();
        custom.display_name = "My Existing Astra Route".to_string();
        custom.origin = "custom".to_string();
        custom.builtin_revision = None;
        custom.user_edited = true;
        custom.price.price_status = "custom".to_string();
        custom.price.price_source = Some("existing-user-config".to_string());
        custom.routes[0].id.clear();
        custom.routes[0].upstream_model = "my-astra-upstream".to_string();
        let saved = storage
            .upsert_managed_model_v2(&ManagedModelV2Upsert {
                model: custom,
                ..Default::default()
            })
            .unwrap();
        storage
            .conn
            .execute(
                "UPDATE models
                 SET capabilities_json=?1,builtin_revision=7,user_edited=1
                 WHERE slug='gpt-5.6-sol'",
                [r#"{"custom":true}"#],
            )
            .unwrap();
        storage
            .conn
            .execute(
                "DELETE FROM schema_migrations WHERE version=?1",
                [GPT6_ASTRA_MIGRATION_VERSION],
            )
            .unwrap();
        storage.migration_cache().take();

        storage.apply_model_catalog_gpt6_astra_migration().unwrap();
        storage.apply_model_catalog_gpt6_astra_migration().unwrap();

        let preserved = storage
            .get_managed_model_v2(GPT6_ASTRA_SLUG)
            .unwrap()
            .unwrap();
        assert_eq!(preserved.id, saved.id);
        assert_eq!(preserved.origin, "custom");
        assert_eq!(preserved.builtin_revision, None);
        assert_eq!(preserved.display_name, "My Existing Astra Route");
        assert_eq!(
            preserved.price.price_source.as_deref(),
            Some("existing-user-config")
        );
        assert_eq!(preserved.routes.len(), 1);
        assert_eq!(preserved.routes[0].upstream_model, "my-astra-upstream");

        let sol = storage
            .get_managed_model_v2("gpt-5.6-sol")
            .unwrap()
            .unwrap();
        assert_eq!(sol.builtin_revision, Some(7));
        assert_eq!(sol.capabilities["custom"], true);
    }

    #[test]
    fn astra_migration_repairs_an_unedited_pre_release_builtin() {
        let storage = storage();
        storage
            .conn
            .execute(
                "DELETE FROM schema_migrations WHERE version=?1",
                [GPT6_ASTRA_MIGRATION_VERSION],
            )
            .unwrap();
        storage.migration_cache().take();
        storage
            .conn
            .execute(
                "UPDATE models
                 SET display_name='Pre-release Astra',capabilities_json='{}',builtin_revision=7
                 WHERE slug=?1",
                [GPT6_ASTRA_SLUG],
            )
            .unwrap();
        storage
            .conn
            .execute(
                "UPDATE model_prices SET cache_write_microusd_per_1m=1
                 WHERE model_id=(SELECT id FROM models WHERE slug=?1)",
                [GPT6_ASTRA_SLUG],
            )
            .unwrap();
        storage
            .conn
            .execute(
                "DELETE FROM model_price_tiers
                 WHERE model_id=(SELECT id FROM models WHERE slug=?1)
                   AND min_input_tokens>0",
                [GPT6_ASTRA_SLUG],
            )
            .unwrap();

        storage.apply_model_catalog_gpt6_astra_migration().unwrap();

        let migrated = storage
            .get_managed_model_v2(GPT6_ASTRA_SLUG)
            .unwrap()
            .unwrap();
        let fixture = gpt6_astra_migration_fixture();
        assert!(seeded_builtin_matches(
            &migrated,
            fixture.revision,
            &fixture.astra
        ));
    }

    #[test]
    fn astra_migration_does_not_downgrade_future_catalog_state() {
        let storage = storage();
        storage
            .conn
            .execute(
                "DELETE FROM schema_migrations WHERE version=?1",
                [GPT6_ASTRA_MIGRATION_VERSION],
            )
            .unwrap();
        storage.migration_cache().take();
        storage
            .conn
            .execute(
                "UPDATE models
                 SET display_name='Future Astra',capabilities_json=?1,builtin_revision=11
                 WHERE slug=?2",
                params![r#"{"future":true}"#, GPT6_ASTRA_SLUG],
            )
            .unwrap();
        storage
            .conn
            .execute(
                "UPDATE model_prices SET cache_write_microusd_per_1m=13000000
                 WHERE model_id=(SELECT id FROM models WHERE slug=?1)",
                [GPT6_ASTRA_SLUG],
            )
            .unwrap();
        storage
            .conn
            .execute(
                "UPDATE model_price_tiers
                 SET cache_write_microusd_per_1m=CASE min_input_tokens
                   WHEN 0 THEN 13000000 ELSE 26000000 END
                 WHERE model_id=(SELECT id FROM models WHERE slug=?1)",
                [GPT6_ASTRA_SLUG],
            )
            .unwrap();
        storage
            .conn
            .execute(
                "UPDATE models SET capabilities_json=?1,builtin_revision=11 WHERE slug=?2",
                params![r#"{"future_sol":true}"#, "gpt-5.6-sol"],
            )
            .unwrap();
        storage
            .conn
            .execute(
                "UPDATE model_catalog_v2_meta SET value='7' WHERE key='builtin_revision'",
                [],
            )
            .unwrap();
        storage
            .conn
            .execute(
                "UPDATE model_catalog_v2_meta SET value='future-fixture-sha256'
                 WHERE key='fixture_sha256'",
                [],
            )
            .unwrap();

        storage.apply_model_catalog_gpt6_astra_migration().unwrap();
        storage.seed_missing_builtin_models_v2().unwrap();

        let astra = storage
            .get_managed_model_v2(GPT6_ASTRA_SLUG)
            .unwrap()
            .unwrap();
        assert_eq!(astra.display_name, "Future Astra");
        assert_eq!(astra.builtin_revision, Some(11));
        assert_eq!(astra.capabilities["future"], true);
        assert_eq!(astra.price.cache_write_microusd_per_1m, Some(13_000_000));
        assert_eq!(
            astra.price_tiers[0].cache_write_microusd_per_1m,
            Some(13_000_000)
        );
        assert_eq!(
            astra.price_tiers[1].cache_write_microusd_per_1m,
            Some(26_000_000)
        );
        let sol = storage
            .get_managed_model_v2("gpt-5.6-sol")
            .unwrap()
            .unwrap();
        assert_eq!(sol.builtin_revision, Some(11));
        assert_eq!(sol.capabilities["future_sol"], true);
        let revision: String = storage
            .conn
            .query_row(
                "SELECT value FROM model_catalog_v2_meta WHERE key='builtin_revision'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(revision, "11");
        let source_sha256: String = storage
            .conn
            .query_row(
                "SELECT value FROM model_catalog_v2_meta WHERE key='fixture_sha256'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(source_sha256, "future-fixture-sha256");
    }

    #[test]
    fn astra_migration_rolls_back_all_writes_when_smoke_check_fails() {
        let storage = storage();
        storage
            .conn
            .execute(
                "DELETE FROM schema_migrations WHERE version=?1",
                [GPT6_ASTRA_MIGRATION_VERSION],
            )
            .unwrap();
        storage.migration_cache().take();
        storage
            .conn
            .execute(
                "DELETE FROM model_catalog_v2_meta
                 WHERE key IN ('gpt6_astra_catalog_revision','gpt6_astra_price_source')",
                [],
            )
            .unwrap();
        storage
            .conn
            .execute(
                "UPDATE model_catalog_v2_meta SET value='7' WHERE key='builtin_revision'",
                [],
            )
            .unwrap();
        storage
            .conn
            .execute(
                "UPDATE models SET builtin_revision=7
                 WHERE origin='builtin' AND user_edited=0",
                [],
            )
            .unwrap();
        storage
            .conn
            .execute(
                "UPDATE model_catalog_v2_meta SET value='revision-seven-sha256'
                 WHERE key='fixture_sha256'",
                [],
            )
            .unwrap();
        storage
            .conn
            .execute(
                "DELETE FROM models WHERE slug IN (?1,?2)",
                params![GPT6_ASTRA_SLUG, "gpt-5.6-sol"],
            )
            .unwrap();

        assert!(storage.apply_model_catalog_gpt6_astra_migration().is_err());

        assert!(storage
            .get_managed_model_v2(GPT6_ASTRA_SLUG)
            .unwrap()
            .is_none());
        let revision: String = storage
            .conn
            .query_row(
                "SELECT value FROM model_catalog_v2_meta WHERE key='builtin_revision'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(revision, "7");
        let source_sha256: String = storage
            .conn
            .query_row(
                "SELECT value FROM model_catalog_v2_meta WHERE key='fixture_sha256'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(source_sha256, "revision-seven-sha256");
        let migration_writes: i64 = storage
            .conn
            .query_row(
                "SELECT
                   (SELECT COUNT(*) FROM schema_migrations WHERE version=?1)
                   +(SELECT COUNT(*) FROM model_catalog_v2_meta
                     WHERE key IN ('gpt6_astra_catalog_revision','gpt6_astra_price_source'))",
                [GPT6_ASTRA_MIGRATION_VERSION],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(migration_writes, 0);
    }

    #[test]
    fn gpt56_pricing_migration_only_fills_unedited_missing_builtin_prices() {
        let storage = storage();
        storage
            .conn
            .execute(
                "DELETE FROM schema_migrations WHERE version=?1",
                [GPT56_PRICING_MIGRATION_VERSION],
            )
            .unwrap();
        storage.migration_cache().take();

        for slug in ["gpt-5.6-sol", "gpt-5.6-terra"] {
            storage
                .conn
                .execute(
                    "DELETE FROM model_price_tiers
                     WHERE model_id=(SELECT id FROM models WHERE slug=?1)",
                    [slug],
                )
                .unwrap();
            storage
                .conn
                .execute(
                    "UPDATE model_prices
                     SET input_microusd_per_1m=NULL,cached_input_microusd_per_1m=NULL,
                         output_microusd_per_1m=NULL,price_status='missing',price_source=NULL
                     WHERE model_id=(SELECT id FROM models WHERE slug=?1)",
                    [slug],
                )
                .unwrap();
        }
        storage
            .conn
            .execute(
                "UPDATE models SET user_edited=1 WHERE slug='gpt-5.6-terra'",
                [],
            )
            .unwrap();

        storage.apply_gpt56_pricing_migration().unwrap();
        storage.apply_gpt56_pricing_migration().unwrap();

        let sol = storage
            .get_managed_model_v2("gpt-5.6-sol")
            .unwrap()
            .unwrap();
        assert_eq!(sol.price.price_status, "estimated");
        assert_eq!(sol.price.input_microusd_per_1m, Some(5_000_000));
        assert_eq!(sol.price.cached_input_microusd_per_1m, Some(5_000_000));
        assert_eq!(sol.price.output_microusd_per_1m, Some(30_000_000));
        assert_eq!(sol.price_tiers.len(), 1);
        assert_eq!(sol.builtin_revision, Some(9));

        let terra = storage
            .get_managed_model_v2("gpt-5.6-terra")
            .unwrap()
            .unwrap();
        assert_eq!(terra.price.price_status, "missing");
        assert!(terra.price_tiers.is_empty());

        let applied: i64 = storage
            .conn
            .query_row(
                "SELECT COUNT(*) FROM schema_migrations WHERE version=?1",
                [GPT56_PRICING_MIGRATION_VERSION],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(applied, 1);
    }

    #[test]
    fn gpt56_official_pricing_migration_updates_seeded_estimates_only() {
        let storage = storage();
        storage
            .conn
            .execute(
                "DELETE FROM schema_migrations WHERE version=?1",
                [GPT56_OFFICIAL_PRICING_MIGRATION_VERSION],
            )
            .unwrap();
        storage.migration_cache().take();
        storage
            .conn
            .execute_batch(
                "DELETE FROM model_price_tiers
                 WHERE model_id IN (
                   SELECT id FROM models
                   WHERE lower(slug) IN ('gpt-5.6-sol','gpt-5.6-terra','gpt-5.6-luna')
                 );
                 INSERT INTO model_price_tiers(
                   model_id,min_input_tokens,input_microusd_per_1m,
                   cached_input_microusd_per_1m,output_microusd_per_1m
                 )
                 SELECT id,0,
                   CASE lower(slug)
                     WHEN 'gpt-5.6-sol' THEN 5000000
                     WHEN 'gpt-5.6-terra' THEN 2500000
                     WHEN 'gpt-5.6-luna' THEN 1000000
                   END,
                   CASE lower(slug)
                     WHEN 'gpt-5.6-sol' THEN 5000000
                     WHEN 'gpt-5.6-terra' THEN 2500000
                     WHEN 'gpt-5.6-luna' THEN 1000000
                   END,
                   CASE lower(slug)
                     WHEN 'gpt-5.6-sol' THEN 30000000
                     WHEN 'gpt-5.6-terra' THEN 15000000
                     WHEN 'gpt-5.6-luna' THEN 6000000
                   END
                 FROM models
                 WHERE lower(slug) IN ('gpt-5.6-sol','gpt-5.6-terra','gpt-5.6-luna');
                 UPDATE model_prices
                 SET input_microusd_per_1m=CASE lower((
                       SELECT slug FROM models WHERE id=model_prices.model_id
                     ))
                       WHEN 'gpt-5.6-sol' THEN 5000000
                       WHEN 'gpt-5.6-terra' THEN 2500000
                       WHEN 'gpt-5.6-luna' THEN 1000000
                     END,
                     cached_input_microusd_per_1m=CASE lower((
                       SELECT slug FROM models WHERE id=model_prices.model_id
                     ))
                       WHEN 'gpt-5.6-sol' THEN 5000000
                       WHEN 'gpt-5.6-terra' THEN 2500000
                       WHEN 'gpt-5.6-luna' THEN 1000000
                     END,
                     output_microusd_per_1m=CASE lower((
                       SELECT slug FROM models WHERE id=model_prices.model_id
                     ))
                       WHEN 'gpt-5.6-sol' THEN 30000000
                       WHEN 'gpt-5.6-terra' THEN 15000000
                       WHEN 'gpt-5.6-luna' THEN 6000000
                     END,
                     price_status='estimated',
                     price_source='user_provided_openai_gpt-5.6_2026-07-14_cached_at_input_rate'
                 WHERE model_id IN (
                   SELECT id FROM models
                   WHERE lower(slug) IN ('gpt-5.6-sol','gpt-5.6-terra','gpt-5.6-luna')
                 );
                 UPDATE models SET builtin_revision=3
                 WHERE lower(slug) IN ('gpt-5.6-sol','gpt-5.6-terra','gpt-5.6-luna');
                 UPDATE models
                 SET display_name='Edited Sol',user_edited=1
                 WHERE slug='gpt-5.6-sol';
                 UPDATE model_prices
                 SET cached_input_microusd_per_1m=777000,
                     price_status='custom',price_source='local-ui'
                 WHERE model_id=(SELECT id FROM models WHERE slug='gpt-5.6-terra');
                 UPDATE model_price_tiers
                 SET cached_input_microusd_per_1m=777000
                 WHERE model_id=(SELECT id FROM models WHERE slug='gpt-5.6-terra');",
            )
            .unwrap();

        storage.apply_gpt56_official_pricing_migration().unwrap();
        storage.apply_gpt56_official_pricing_migration().unwrap();

        for (slug, cached, long_input, long_cached, long_output) in [
            ("gpt-5.6-sol", 500_000, 10_000_000, 1_000_000, 45_000_000),
            ("gpt-5.6-luna", 100_000, 2_000_000, 200_000, 9_000_000),
        ] {
            let model = storage.get_managed_model_v2(slug).unwrap().unwrap();
            assert_eq!(model.price.price_status, "official");
            assert_eq!(
                model.price.price_source.as_deref(),
                Some(GPT56_OFFICIAL_PRICE_SOURCE)
            );
            assert_eq!(model.price.cached_input_microusd_per_1m, Some(cached));
            assert_eq!(model.price_tiers.len(), 2);
            assert_eq!(model.price_tiers[1].min_input_tokens, 272_000);
            assert_eq!(model.price_tiers[1].input_microusd_per_1m, long_input);
            assert_eq!(
                model.price_tiers[1].cached_input_microusd_per_1m,
                long_cached
            );
            assert_eq!(model.price_tiers[1].output_microusd_per_1m, long_output);
            assert_eq!(model.builtin_revision, Some(4));
        }

        let sol = storage
            .get_managed_model_v2("gpt-5.6-sol")
            .unwrap()
            .unwrap();
        assert_eq!(sol.display_name, "Edited Sol");
        assert!(sol.user_edited);

        let terra = storage
            .get_managed_model_v2("gpt-5.6-terra")
            .unwrap()
            .unwrap();
        assert_eq!(terra.price.price_status, "custom");
        assert_eq!(terra.price.price_source.as_deref(), Some("local-ui"));
        assert_eq!(terra.price.cached_input_microusd_per_1m, Some(777_000));
        assert_eq!(terra.price_tiers.len(), 1);
        assert_eq!(terra.builtin_revision, Some(3));

        let applied: i64 = storage
            .conn
            .query_row(
                "SELECT COUNT(*) FROM schema_migrations WHERE version=?1",
                [GPT56_OFFICIAL_PRICING_MIGRATION_VERSION],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(applied, 1);
    }

    #[test]
    fn gpt56_current_pricing_migration_updates_unedited_official_rows() {
        let storage = storage();
        storage
            .conn
            .execute(
                "DELETE FROM schema_migrations WHERE version=?1",
                [GPT56_CURRENT_PRICING_MIGRATION_VERSION],
            )
            .unwrap();
        storage.migration_cache().take();
        storage
            .conn
            .execute_batch(
                "DELETE FROM model_price_tiers
                 WHERE model_id IN (
                   SELECT id FROM models
                   WHERE lower(slug) IN ('gpt-5.6-sol','gpt-5.6-terra','gpt-5.6-luna')
                 );
                 INSERT INTO model_price_tiers(
                   model_id,min_input_tokens,input_microusd_per_1m,
                   cached_input_microusd_per_1m,output_microusd_per_1m
                 )
                 SELECT id,0,
                   CASE lower(slug)
                     WHEN 'gpt-5.6-sol' THEN 5000000
                     WHEN 'gpt-5.6-terra' THEN 2500000
                     WHEN 'gpt-5.6-luna' THEN 1000000
                   END,
                   CASE lower(slug)
                     WHEN 'gpt-5.6-sol' THEN 500000
                     WHEN 'gpt-5.6-terra' THEN 250000
                     WHEN 'gpt-5.6-luna' THEN 100000
                   END,
                   CASE lower(slug)
                     WHEN 'gpt-5.6-sol' THEN 30000000
                     WHEN 'gpt-5.6-terra' THEN 15000000
                     WHEN 'gpt-5.6-luna' THEN 6000000
                   END
                 FROM models
                 WHERE lower(slug) IN ('gpt-5.6-sol','gpt-5.6-terra','gpt-5.6-luna')
                 UNION ALL
                 SELECT id,272000,
                   CASE lower(slug)
                     WHEN 'gpt-5.6-sol' THEN 10000000
                     WHEN 'gpt-5.6-terra' THEN 5000000
                     WHEN 'gpt-5.6-luna' THEN 2000000
                   END,
                   CASE lower(slug)
                     WHEN 'gpt-5.6-sol' THEN 1000000
                     WHEN 'gpt-5.6-terra' THEN 500000
                     WHEN 'gpt-5.6-luna' THEN 200000
                   END,
                   CASE lower(slug)
                     WHEN 'gpt-5.6-sol' THEN 45000000
                     WHEN 'gpt-5.6-terra' THEN 22500000
                     WHEN 'gpt-5.6-luna' THEN 9000000
                   END
                 FROM models
                 WHERE lower(slug) IN ('gpt-5.6-sol','gpt-5.6-terra','gpt-5.6-luna');
                 UPDATE model_prices
                 SET input_microusd_per_1m=CASE lower((SELECT slug FROM models WHERE id=model_prices.model_id))
                       WHEN 'gpt-5.6-sol' THEN 5000000
                       WHEN 'gpt-5.6-terra' THEN 2500000
                       WHEN 'gpt-5.6-luna' THEN 1000000
                     END,
                     cached_input_microusd_per_1m=CASE lower((SELECT slug FROM models WHERE id=model_prices.model_id))
                       WHEN 'gpt-5.6-sol' THEN 500000
                       WHEN 'gpt-5.6-terra' THEN 250000
                       WHEN 'gpt-5.6-luna' THEN 100000
                     END,
                     output_microusd_per_1m=CASE lower((SELECT slug FROM models WHERE id=model_prices.model_id))
                       WHEN 'gpt-5.6-sol' THEN 30000000
                       WHEN 'gpt-5.6-terra' THEN 15000000
                       WHEN 'gpt-5.6-luna' THEN 6000000
                     END,
                     price_status='official',
                     price_source='https://developers.openai.com/api/docs/models/compare'
                 WHERE model_id IN (
                   SELECT id FROM models
                   WHERE lower(slug) IN ('gpt-5.6-sol','gpt-5.6-terra','gpt-5.6-luna')
                 );
                 UPDATE models SET builtin_revision=5,user_edited=0
                 WHERE lower(slug) IN ('gpt-5.6-sol','gpt-5.6-terra','gpt-5.6-luna');
                 UPDATE models SET user_edited=1 WHERE slug='gpt-5.6-terra';
                 UPDATE model_prices
                 SET cached_input_microusd_per_1m=777000,
                     price_status='custom',price_source='local-ui'
                 WHERE model_id=(SELECT id FROM models WHERE slug='gpt-5.6-terra');
                 UPDATE model_price_tiers
                 SET cached_input_microusd_per_1m=777000
                 WHERE model_id=(SELECT id FROM models WHERE slug='gpt-5.6-terra')
                   AND min_input_tokens=0;",
            )
            .unwrap();

        storage.apply_gpt56_current_pricing_migration().unwrap();
        storage.apply_gpt56_current_pricing_migration().unwrap();

        for (slug, input, cached, output, long_input, long_cached, long_output) in [
            (
                "gpt-5.6-sol",
                5_000_000,
                500_000,
                30_000_000,
                10_000_000,
                1_000_000,
                45_000_000,
            ),
            (
                "gpt-5.6-luna",
                200_000,
                20_000,
                1_200_000,
                400_000,
                40_000,
                1_800_000,
            ),
        ] {
            let model = storage.get_managed_model_v2(slug).unwrap().unwrap();
            assert_eq!(model.price.price_status, "official");
            assert_eq!(
                model.price.price_source.as_deref(),
                Some(GPT56_OFFICIAL_PRICE_SOURCE)
            );
            assert_eq!(model.price.input_microusd_per_1m, Some(input));
            assert_eq!(model.price.cached_input_microusd_per_1m, Some(cached));
            assert_eq!(model.price.output_microusd_per_1m, Some(output));
            assert_eq!(model.builtin_revision, Some(6));
            assert_eq!(model.price_tiers.len(), 2);
            assert_eq!(model.price_tiers[1].min_input_tokens, 272_000);
            assert_eq!(model.price_tiers[1].input_microusd_per_1m, long_input);
            assert_eq!(
                model.price_tiers[1].cached_input_microusd_per_1m,
                long_cached
            );
            assert_eq!(model.price_tiers[1].output_microusd_per_1m, long_output);
        }

        let terra = storage
            .get_managed_model_v2("gpt-5.6-terra")
            .unwrap()
            .unwrap();
        assert!(terra.user_edited);
        assert_eq!(terra.price.price_status, "custom");
        assert_eq!(terra.price.price_source.as_deref(), Some("local-ui"));
        assert_eq!(terra.price.input_microusd_per_1m, Some(2_500_000));
        assert_eq!(terra.price.cached_input_microusd_per_1m, Some(777_000));
        assert_eq!(terra.price.output_microusd_per_1m, Some(15_000_000));
        assert_eq!(terra.builtin_revision, Some(5));
        assert_eq!(terra.price_tiers.len(), 2);
        assert_eq!(terra.price_tiers[0].cached_input_microusd_per_1m, 777_000);

        let revision: String = storage
            .conn
            .query_row(
                "SELECT value FROM model_catalog_v2_meta WHERE key='gpt56_pricing_revision'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(revision, "2026-07-31-official");
        let applied: i64 = storage
            .conn
            .query_row(
                "SELECT COUNT(*) FROM schema_migrations WHERE version=?1",
                [GPT56_CURRENT_PRICING_MIGRATION_VERSION],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(applied, 1);
    }

    #[test]
    fn codex_metadata_migration_updates_only_unedited_builtin_rows() {
        let storage = storage();
        storage
            .conn
            .execute(
                "DELETE FROM schema_migrations WHERE version=?1",
                [CODEX_METADATA_MIGRATION_VERSION],
            )
            .unwrap();
        storage.migration_cache().take();
        storage
            .conn
            .execute(
                "UPDATE models SET capabilities_json='{}',builtin_revision=2,user_edited=0
                 WHERE slug='gpt-5.6-sol'",
                [],
            )
            .unwrap();
        storage
            .conn
            .execute(
                "UPDATE models SET capabilities_json=?1,builtin_revision=2,user_edited=1
                 WHERE slug='gpt-5.6-terra'",
                [r#"{"custom":true}"#],
            )
            .unwrap();

        storage
            .apply_model_catalog_codex_metadata_migration()
            .unwrap();
        storage
            .apply_model_catalog_codex_metadata_migration()
            .unwrap();

        let sol = storage
            .get_managed_model_v2("gpt-5.6-sol")
            .unwrap()
            .unwrap();
        assert_eq!(sol.builtin_revision, Some(8));
        assert_eq!(sol.capabilities["multi_agent_version"], "v2");
        assert_eq!(sol.capabilities["use_responses_lite"], true);

        let terra = storage
            .get_managed_model_v2("gpt-5.6-terra")
            .unwrap()
            .unwrap();
        assert_eq!(terra.builtin_revision, Some(2));
        assert_eq!(terra.capabilities["custom"], true);

        let applied: i64 = storage
            .conn
            .query_row(
                "SELECT COUNT(*) FROM schema_migrations WHERE version=?1",
                [CODEX_METADATA_MIGRATION_VERSION],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(applied, 1);
    }

    #[test]
    fn seed_reuses_legacy_builtin_id_for_existing_slug() {
        let storage = storage();
        let slug = "gpt-5.6-sol";
        let current_id = builtin_id(slug);
        let legacy_id = "builtin:gpt-5_6_sol";

        storage
            .conn
            .execute_batch("PRAGMA foreign_keys=OFF; BEGIN IMMEDIATE;")
            .expect("disable foreign keys for legacy fixture");
        storage
            .conn
            .execute(
                "UPDATE models SET id=?2 WHERE id=?1",
                params![current_id, legacy_id],
            )
            .expect("rewrite legacy model id");
        for table in ["model_prices", "model_price_tiers", "model_routes"] {
            storage
                .conn
                .execute(
                    &format!("UPDATE {table} SET model_id=?2 WHERE model_id=?1"),
                    params![current_id, legacy_id],
                )
                .expect("rewrite legacy child model id");
        }
        storage
            .conn
            .execute_batch("COMMIT; PRAGMA foreign_keys=ON;")
            .expect("commit legacy fixture");

        storage
            .seed_missing_builtin_models_v2()
            .expect("seed with legacy builtin id");

        let model = storage
            .get_managed_model_v2(slug)
            .expect("read model")
            .expect("model exists");
        assert_eq!(model.id, legacy_id);
        assert_eq!(model.price.price_status, "official");
        assert_eq!(model.price_tiers.len(), 2);
        assert_eq!(model.routes.len(), 1);
        let foreign_key_errors: i64 = storage
            .conn
            .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
                row.get(0)
            })
            .expect("foreign key check");
        assert_eq!(foreign_key_errors, 0);
    }

    #[test]
    fn builtin_and_custom_delete_remove_models_permanently() {
        let storage = storage();
        storage.delete_managed_model_v2("gpt-5.6-sol").unwrap();
        assert!(storage
            .get_managed_model_v2("gpt-5.6-sol")
            .unwrap()
            .is_none());
        storage.seed_missing_builtin_models_v2().unwrap();
        assert!(storage
            .get_managed_model_v2("gpt-5.6-sol")
            .unwrap()
            .is_none());
        let mut custom = storage
            .get_managed_model_v2("gpt-5.6-luna")
            .unwrap()
            .unwrap();
        custom.id = String::new();
        custom.slug = "local-model".into();
        custom.display_name = "Local".into();
        custom.origin = "custom".into();
        custom.builtin_revision = None;
        custom.user_edited = false;
        custom.price.price_status = "custom".into();
        custom.routes.clear();
        storage
            .upsert_managed_model_v2(&ManagedModelV2Upsert {
                previous_slug: None,
                model: custom,
            })
            .unwrap();
        storage.delete_managed_model_v2("local-model").unwrap();
        assert!(storage
            .get_managed_model_v2("local-model")
            .unwrap()
            .is_none());
    }

    #[test]
    fn state_update_changes_only_enabled_visibility_and_edit_metadata() {
        let storage = storage();
        let baseline = storage
            .get_managed_model_v2("gpt-5.6-sol")
            .unwrap()
            .unwrap();
        for (enabled, visibility) in [
            (false, "list"),
            (true, "hide"),
            (false, "hide"),
            (true, "list"),
        ] {
            let updated = storage
                .update_managed_model_state_v2(&ManagedModelStateV2Update {
                    slug: baseline.slug.clone(),
                    enabled,
                    visibility: visibility.to_string(),
                })
                .unwrap();
            assert_eq!(updated.enabled, enabled);
            assert_eq!(updated.visibility, visibility);
            assert!(updated.user_edited);
            assert_eq!(updated.price, baseline.price);
            assert_eq!(updated.price_tiers, baseline.price_tiers);
            assert_eq!(updated.routes, baseline.routes);
            assert_eq!(updated.permission_group_ids, baseline.permission_group_ids);
        }
        assert!(matches!(
            storage.update_managed_model_state_v2(&ManagedModelStateV2Update {
                slug: baseline.slug.clone(),
                enabled: true,
                visibility: "hidden".to_string(),
            }),
            Err(rusqlite::Error::InvalidParameterName(_))
        ));
        assert!(matches!(
            storage.update_managed_model_state_v2(&ManagedModelStateV2Update {
                slug: "missing-model".to_string(),
                enabled: true,
                visibility: "list".to_string(),
            }),
            Err(rusqlite::Error::QueryReturnedNoRows)
        ));
    }

    #[test]
    fn batch_state_update_is_atomic_deduplicated_and_preserves_model_details() {
        let storage = storage();
        let first = storage
            .get_managed_model_v2("gpt-5.6-sol")
            .unwrap()
            .unwrap();
        let second = storage
            .get_managed_model_v2("gpt-5.6-luna")
            .unwrap()
            .unwrap();

        let updated = storage
            .update_managed_models_state_v2(&ManagedModelBatchStateV2Update {
                slugs: vec![
                    " gpt-5.6-sol ".to_string(),
                    "GPT-5.6-SOL".to_string(),
                    "gpt-5.6-luna".to_string(),
                ],
                enabled: false,
                visibility: "hide".to_string(),
            })
            .unwrap();
        assert_eq!(updated.len(), 2);
        assert!(updated
            .iter()
            .all(|model| !model.enabled && model.visibility == "hide"));
        assert!(updated.iter().all(|model| model.user_edited));
        assert_eq!(updated[0].updated_at, updated[1].updated_at);
        assert_eq!(updated[0].price, first.price);
        assert_eq!(updated[0].routes, first.routes);
        assert_eq!(updated[1].price, second.price);
        assert_eq!(updated[1].routes, second.routes);

        storage
            .update_managed_models_state_v2(&ManagedModelBatchStateV2Update {
                slugs: vec!["gpt-5.6-sol".to_string(), "gpt-5.6-luna".to_string()],
                enabled: true,
                visibility: "list".to_string(),
            })
            .unwrap();
        let before_failed_batch = storage
            .get_managed_model_v2("gpt-5.6-sol")
            .unwrap()
            .unwrap();
        assert!(storage
            .update_managed_models_state_v2(&ManagedModelBatchStateV2Update {
                slugs: vec!["gpt-5.6-sol".to_string(), "missing-model".to_string()],
                enabled: false,
                visibility: "hide".to_string(),
            })
            .is_err());
        let after_failed_batch = storage
            .get_managed_model_v2("gpt-5.6-sol")
            .unwrap()
            .unwrap();
        assert_eq!(after_failed_batch.enabled, before_failed_batch.enabled);
        assert_eq!(
            after_failed_batch.visibility,
            before_failed_batch.visibility
        );
        assert_eq!(
            after_failed_batch.updated_at,
            before_failed_batch.updated_at
        );

        assert!(storage
            .update_managed_models_state_v2(&ManagedModelBatchStateV2Update {
                slugs: vec!["  ".to_string()],
                enabled: true,
                visibility: "list".to_string(),
            })
            .is_err());
        assert!(storage
            .update_managed_models_state_v2(&ManagedModelBatchStateV2Update {
                slugs: vec!["gpt-5.6-sol".to_string()],
                enabled: true,
                visibility: "hidden".to_string(),
            })
            .is_err());
    }

    #[test]
    fn invalid_atomic_upsert_rolls_back_everything() {
        let storage = storage();
        let mut first = storage
            .get_managed_model_v2("gpt-5.6-luna")
            .unwrap()
            .unwrap();
        first.id = String::new();
        first.slug = "ok-custom".into();
        first.origin = "custom".into();
        first.builtin_revision = None;
        let mut invalid = first.clone();
        invalid.slug = "bad-custom".into();
        invalid.price.price_status = "missing".into();
        let _err = storage
            .upsert_managed_models_v2(&[
                ManagedModelV2Upsert {
                    previous_slug: None,
                    model: first,
                },
                ManagedModelV2Upsert {
                    previous_slug: None,
                    model: invalid,
                },
            ])
            .unwrap_err();
        assert!(storage.get_managed_model_v2("ok-custom").unwrap().is_none());
    }

    #[test]
    fn route_ensure_preserves_existing_builtin_data_and_is_idempotent() {
        let storage = storage();
        let original = storage
            .get_managed_model_v2("gpt-5.6-sol")
            .unwrap()
            .unwrap();
        storage
            .conn
            .execute(
                "DELETE FROM model_routes
                 WHERE model_id=?1 AND source_kind='account_pool' AND source_id='default'
                   AND upstream_model='gpt-5.6-sol' COLLATE NOCASE",
                [&original.id],
            )
            .unwrap();
        storage
            .conn
            .execute(
                "INSERT INTO model_routes(
                    id,model_id,source_kind,source_id,upstream_model,
                    enabled,priority,weight,created_at,updated_at
                 ) VALUES('preserved-route',?1,'aggregate_api','aggregate-test',
                    'preserved-upstream',0,7,3,11,12)",
                [&original.id],
            )
            .unwrap();
        let before = storage
            .get_managed_model_v2("gpt-5.6-sol")
            .unwrap()
            .unwrap();
        assert!(!before.user_edited);

        let stale_same_slug_candidate = custom_creation_candidate("GPT-5.6-SOL");
        let result = storage
            .upsert_missing_managed_models_and_ensure_routes_v2(
                &[stale_same_slug_candidate],
                &[account_route_ensure("gpt-5.6-sol", "gpt-5.6-sol")],
            )
            .unwrap();
        assert!(result.created_models.is_empty());
        assert_eq!(result.added_routes, ["gpt-5.6-sol"]);
        assert!(result.unchanged_routes.is_empty());

        let after = storage
            .get_managed_model_v2("gpt-5.6-sol")
            .unwrap()
            .unwrap();
        assert_eq!(after.id, before.id);
        assert_eq!(after.slug, before.slug);
        assert_eq!(after.display_name, before.display_name);
        assert_eq!(after.description, before.description);
        assert_eq!(after.provider, before.provider);
        assert_eq!(after.family, before.family);
        assert_eq!(after.category, before.category);
        assert_eq!(after.tags, before.tags);
        assert_eq!(after.origin, before.origin);
        assert_eq!(after.enabled, before.enabled);
        assert_eq!(after.supported_in_api, before.supported_in_api);
        assert_eq!(after.visibility, before.visibility);
        assert_eq!(after.sort_order, before.sort_order);
        assert_eq!(after.context_window, before.context_window);
        assert_eq!(after.max_context_window, before.max_context_window);
        assert_eq!(
            after.default_reasoning_effort,
            before.default_reasoning_effort
        );
        assert_eq!(after.capabilities, before.capabilities);
        assert_eq!(after.instructions_mode, before.instructions_mode);
        assert_eq!(after.instructions_text, before.instructions_text);
        assert_eq!(after.fast_policy, before.fast_policy);
        assert_eq!(after.builtin_revision, before.builtin_revision);
        assert_eq!(after.user_edited, before.user_edited);
        assert_eq!(after.price, before.price);
        assert_eq!(after.price_tiers, before.price_tiers);
        assert_eq!(after.permission_group_ids, before.permission_group_ids);
        assert_eq!(after.created_at, before.created_at);
        assert_eq!(after.updated_at, before.updated_at);
        assert_eq!(after.routes.len(), before.routes.len() + 1);
        assert_eq!(
            after
                .routes
                .iter()
                .find(|route| route.id == "preserved-route"),
            before
                .routes
                .iter()
                .find(|route| route.id == "preserved-route")
        );

        let repeated = storage
            .upsert_missing_managed_models_and_ensure_routes_v2(
                &[],
                &[account_route_ensure("GPT-5.6-SOL", "GPT-5.6-SOL")],
            )
            .unwrap();
        assert!(repeated.created_models.is_empty());
        assert!(repeated.added_routes.is_empty());
        assert_eq!(repeated.unchanged_routes, ["gpt-5.6-sol"]);
        assert_eq!(
            storage
                .get_managed_model_v2("gpt-5.6-sol")
                .unwrap()
                .unwrap()
                .routes
                .len(),
            after.routes.len()
        );
    }

    #[test]
    fn route_ensure_creates_a_model_and_updates_an_existing_model_in_one_batch() {
        let storage = storage();
        let existing = storage
            .get_managed_model_v2("gpt-5.6-luna")
            .unwrap()
            .unwrap();
        storage
            .conn
            .execute(
                "DELETE FROM model_routes
                 WHERE model_id=?1 AND source_kind='account_pool' AND source_id='default'",
                [&existing.id],
            )
            .unwrap();

        let result = storage
            .upsert_missing_managed_models_and_ensure_routes_v2(
                &[custom_creation_candidate("account-discovered-new")],
                &[
                    account_route_ensure("account-discovered-new", "account-discovered-new"),
                    account_route_ensure("gpt-5.6-luna", "gpt-5.6-luna"),
                ],
            )
            .unwrap();
        assert_eq!(result.created_models, ["account-discovered-new"]);
        assert_eq!(
            result.added_routes,
            ["account-discovered-new", "gpt-5.6-luna"]
        );
        assert!(result.unchanged_routes.is_empty());

        let created = storage
            .get_managed_model_v2("account-discovered-new")
            .unwrap()
            .unwrap();
        assert_eq!(created.origin, "custom");
        assert_eq!(created.routes.len(), 1);
        assert_eq!(created.routes[0].upstream_model, "account-discovered-new");
        assert!(storage
            .get_managed_model_v2("gpt-5.6-luna")
            .unwrap()
            .unwrap()
            .routes
            .iter()
            .any(|route| route.upstream_model == "gpt-5.6-luna"));
    }

    #[test]
    fn route_ensure_rolls_back_created_models_and_routes_on_a_late_error() {
        let storage = storage();
        let existing = storage
            .get_managed_model_v2("gpt-5.6-luna")
            .unwrap()
            .unwrap();
        storage
            .conn
            .execute(
                "DELETE FROM model_routes
                 WHERE model_id=?1 AND source_kind='account_pool' AND source_id='default'",
                [&existing.id],
            )
            .unwrap();
        let mut invalid = account_route_ensure("gpt-5.6-sol", "gpt-5.6-sol");
        invalid.route.source_kind = "invalid-source".to_string();

        let error = storage
            .upsert_missing_managed_models_and_ensure_routes_v2(
                &[custom_creation_candidate("account-discovered-rollback")],
                &[
                    account_route_ensure(
                        "account-discovered-rollback",
                        "account-discovered-rollback",
                    ),
                    account_route_ensure("gpt-5.6-luna", "gpt-5.6-luna"),
                    invalid,
                ],
            )
            .unwrap_err();
        assert!(error.to_string().contains("invalid model route"));
        assert!(storage
            .get_managed_model_v2("account-discovered-rollback")
            .unwrap()
            .is_none());
        assert!(!storage
            .get_managed_model_v2("gpt-5.6-luna")
            .unwrap()
            .unwrap()
            .routes
            .iter()
            .any(|route| {
                route.source_kind == "account_pool"
                    && route.source_id == "default"
                    && route.upstream_model.eq_ignore_ascii_case("gpt-5.6-luna")
            }));
    }

    #[test]
    fn migration_ignores_incomplete_legacy_route_schema() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage
            .conn
            .execute_batch(
                "CREATE TABLE schema_migrations (
                    version TEXT PRIMARY KEY,
                    applied_at INTEGER NOT NULL
                 );
                 CREATE TABLE model_groups (id TEXT PRIMARY KEY);
                 CREATE TABLE request_logs (id INTEGER PRIMARY KEY);
                 CREATE TABLE model_source_mappings (id TEXT PRIMARY KEY);",
            )
            .expect("create partial legacy schema");

        storage
            .apply_model_catalog_v2_migration()
            .expect("migrate partial legacy schema");
        storage
            .conn
            .execute_batch(include_str!(
                "../../migrations/127_model_catalog_cache_write_prices.sql"
            ))
            .expect("add cache-write pricing schema");
        storage
            .conn
            .execute_batch(include_str!("../../migrations/129_model_fast_policy.sql"))
            .expect("add model fast policy schema");

        assert_eq!(
            storage
                .list_managed_models_v2(true)
                .expect("list migrated models")
                .len(),
            10
        );
    }
}
