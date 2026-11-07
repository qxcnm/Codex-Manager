use crate::commands::shared::rpc_call_in_background;

#[tauri::command]
pub async fn service_health_summary(
    addr: Option<String>,
    source: Option<String>,
) -> Result<serde_json::Value, String> {
    rpc_call_in_background(
        "serviceHealth/summary",
        addr,
        Some(serde_json::json!({"source": source.unwrap_or_else(|| "local".into())})),
    )
    .await
}

#[tauri::command]
pub async fn service_health_report(
    addr: Option<String>,
    provider: String,
    symptom: String,
    region: String,
) -> Result<serde_json::Value, String> {
    rpc_call_in_background(
        "serviceHealth/report",
        addr,
        Some(serde_json::json!({"provider": provider, "symptom": symptom, "region": region})),
    )
    .await
}

#[tauri::command]
pub async fn service_health_resolve(
    addr: Option<String>,
    id: String,
) -> Result<serde_json::Value, String> {
    rpc_call_in_background(
        "serviceHealth/resolve",
        addr,
        Some(serde_json::json!({"id": id})),
    )
    .await
}
