mod aggregate;
#[cfg(test)]
mod integration_tests;
mod reports;
mod store;
mod types;

use crate::RpcActor;
use serde::de::DeserializeOwned;
use serde_json::Value;
use sha2::{Digest, Sha256};
use types::*;

fn digest(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}
fn parse<T: DeserializeOwned>(params: Option<&Value>) -> Result<T, String> {
    serde_json::from_value(params.cloned().unwrap_or_else(|| serde_json::json!({})))
        .map_err(|_| "health_parameters_invalid".into())
}

pub(crate) fn handle(
    method: &str,
    params: Option<&Value>,
    actor: &RpcActor,
) -> Result<Value, String> {
    let now = codexmanager_core::storage::now_ts();
    match method {
        "serviceHealth/summary" => {
            #[derive(serde::Deserialize, Default)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct Input {
                #[serde(default)]
                source: Source,
            }
            let input: Input = parse(params)?;
            let _ = input.source;
            let mut summary = aggregate::summary(now)?;
            summary.my_reports = reports::mine(actor, now)?;
            serde_json::to_value(summary).map_err(|_| "health_response_invalid".into())
        }
        "serviceHealth/report" => {
            store::prune(now)?;
            Ok(serde_json::json!({"report": reports::report(actor, parse(params)?, now)?}))
        }
        "serviceHealth/resolve" => {
            #[derive(serde::Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Input {
                id: String,
            }
            let input: Input = parse(params)?;
            reports::resolve(actor, &input.id, now)?;
            Ok(serde_json::json!({"ok": true}))
        }
        _ => Err("health_method_unknown".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn report_input_rejects_identity_and_arbitrary_strings() {
        assert!(serde_json::from_value::<ReportInput>(serde_json::json!({"provider":"codex","symptom":"errors","region":"unknown","userId":"victim"})).is_err());
        assert!(serde_json::from_value::<ReportInput>(
            serde_json::json!({"provider":"secret model","symptom":"errors","region":"unknown"})
        )
        .is_err());
        assert!(serde_json::from_value::<Source>(serde_json::json!("shared")).is_err());
    }
}
