use serde::{Deserialize, Serialize};

/// Overall test outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CfStyleStatus {
    Ok,
    Partial,
    Failed,
    Timeout,
    Cancelled,
}

/// Direction of a throughput measurement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThroughputDirection {
    Download,
    Upload,
}

/// Outcome of a single throughput run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CfStyleRunStatus {
    Ok,
    Failed,
    Timeout,
    Cancelled,
}

/// Latency measurement results.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CfStyleLatencyResult {
    pub raw_samples_ms: Vec<f64>,
    pub min_ms: f64,
    pub avg_ms: f64,
    pub median_ms: f64,
    pub p90_ms: f64,
    pub p95_ms: f64,
    pub jitter_ms: f64,
}

/// A single throughput measurement run.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CfStyleThroughputRun {
    pub payload_bytes: u64,
    pub transferred_bytes: u64,
    pub total_duration_ms: u64,
    pub ttfb_ms: Option<u64>,
    pub transfer_duration_ms: u64,
    pub raw_mbps: f64,
    pub adjusted_mbps: f64,
    pub status: CfStyleRunStatus,
    pub error: Option<String>,
}

/// Aggregated throughput results for one direction (download or upload).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CfStyleThroughputResult {
    pub direction: ThroughputDirection,
    pub runs: Vec<CfStyleThroughputRun>,
    pub final_mbps: f64,
    pub raw_final_mbps: f64,
    pub adjusted_final_mbps: f64,
    pub avg_mbps: f64,
    pub median_mbps: f64,
    pub p90_mbps: f64,
    pub max_mbps: f64,
    pub total_bytes: u64,
    pub total_duration_ms: u64,
}

/// Information observed from the target endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CfStyleEndpointInfo {
    pub observed_ip: Option<String>,
    pub observed_country: Option<String>,
    pub observed_colo: Option<String>,
}

/// Redacted proxy information that was used during the test.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CfStyleUsedProxy {
    pub proxy_url_redacted: String,
    pub proxy_scheme: String,
    pub dns_note: String,
}

/// An error captured during a specific phase of the test.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CfStyleSpeedTestError {
    pub phase: String,
    pub message: String,
}

/// Complete speed-test result.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CfStyleResult {
    pub status: CfStyleStatus,
    pub latency: Option<CfStyleLatencyResult>,
    pub download: Option<CfStyleThroughputResult>,
    pub upload: Option<CfStyleThroughputResult>,
    pub used_proxy: Option<CfStyleUsedProxy>,
    pub endpoint_info: CfStyleEndpointInfo,
    pub started_at: String,
    pub finished_at: String,
    pub duration_ms: u64,
    pub errors: Vec<CfStyleSpeedTestError>,
}

impl CfStyleResult {
    /// Create a failed result with a single error message.
    #[allow(dead_code)]
    pub fn new_failed(error: String) -> Self {
        let now = chrono::Utc::now().to_rfc3339();
        Self {
            status: CfStyleStatus::Failed,
            latency: None,
            download: None,
            upload: None,
            used_proxy: None,
            endpoint_info: CfStyleEndpointInfo {
                observed_ip: None,
                observed_country: None,
                observed_colo: None,
            },
            started_at: now.clone(),
            finished_at: now,
            duration_ms: 0,
            errors: vec![CfStyleSpeedTestError {
                phase: "init".to_string(),
                message: error,
            }],
        }
    }
}
