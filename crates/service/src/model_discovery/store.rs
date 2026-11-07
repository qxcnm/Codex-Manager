use super::types::{validate_interval, Config, Record, Source, Status};
use std::collections::HashMap;

const CONFIG_PREFIX: &str = "model_discovery.config.";
const STATUS_PREFIX: &str = "model_discovery.status.";

pub(super) fn load() -> Result<Vec<Record>, String> {
    decode(&crate::app_settings::list_app_settings_map_result()?)
}

pub(super) fn decode(settings: &HashMap<String, String>) -> Result<Vec<Record>, String> {
    let mut records = Vec::new();
    for (key, value) in settings {
        let Some(suffix) = key.strip_prefix(CONFIG_PREFIX) else {
            continue;
        };
        let config: Config = serde_json::from_str(value)
            .map_err(|_| "invalid persisted model discovery configuration".to_owned())?;
        let source = Source::parse(&config.source.source_kind, &config.source.source_id)?;
        validate_interval(config.interval_minutes)?;
        if config.schema_version != 1 || source != config.source || source.key() != suffix {
            return Err("unsupported persisted model discovery configuration".into());
        }
        let status = settings
            .get(&format!("{STATUS_PREFIX}{suffix}"))
            .map(|value| {
                serde_json::from_str::<Status>(value)
                    .map_err(|_| "invalid persisted model discovery status".to_owned())
            })
            .transpose()?
            .unwrap_or_default();
        records.push(Record { config, status });
    }
    records.sort_by(|a, b| a.config.source.key().cmp(&b.config.source.key()));
    Ok(records)
}

pub(super) fn changes(
    record: &Record,
    include_config: bool,
) -> Result<Vec<(String, Option<String>)>, String> {
    let suffix = record.config.source.key();
    let mut changes = vec![(
        format!("{STATUS_PREFIX}{suffix}"),
        Some(
            serde_json::to_string(&record.status)
                .map_err(|_| "encode model discovery status failed".to_owned())?,
        ),
    )];
    if include_config {
        changes.push((
            format!("{CONFIG_PREFIX}{suffix}"),
            Some(
                serde_json::to_string(&record.config)
                    .map_err(|_| "encode model discovery configuration failed".to_owned())?,
            ),
        ));
    }
    Ok(changes)
}

pub(super) fn save(record: &Record, include_config: bool) -> Result<(), String> {
    crate::app_settings::save_persisted_app_settings(&changes(record, include_config)?)
}
