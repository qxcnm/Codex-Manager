use serde::{Deserialize, Serialize};

pub const SAMPLE_LIMIT: usize = 2000;
pub const WINDOW_SECS: i64 = 3600;
pub const REPORT_TTL: i64 = 1800;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    Codex,
    Claude,
    #[default]
    Unknown,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Region {
    #[default]
    Unknown,
    Apac,
    Europe,
    NorthAmerica,
    SouthAmerica,
    MiddleEast,
    Africa,
    Oceania,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Symptom {
    Unavailable,
    Slow,
    RateLimited,
    Errors,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    #[default]
    Local,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Coverage {
    ThisDeployment,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Unknown,
    Healthy,
    Degraded,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    InsufficientData,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReportInput {
    pub provider: Provider,
    pub symptom: Symptom,
    pub region: Region,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub id: String,
    pub provider: Provider,
    pub symptom: Symptom,
    pub region: Region,
    pub created_at: i64,
    pub updated_at: i64,
    pub expires_at: i64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSummary {
    pub provider: Provider,
    pub status: Status,
    pub sample_count: u64,
    pub success_count: u64,
    pub rate_limited_count: u64,
    pub upstream_error_count: u64,
    pub local_error_count: u64,
    pub network_error_count: u64,
    pub unknown_error_count: u64,
    pub availability_percent: Option<f64>,
    pub avg_first_response_ms: Option<f64>,
    pub p95_first_response_ms: Option<u64>,
    pub active_report_count: u64,
    pub reporter_count: u64,
    pub source_count: usize,
    pub regions: Vec<Region>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub source: Source,
    pub coverage: Coverage,
    pub window_start: i64,
    pub window_end: i64,
    pub updated_at: i64,
    pub sampled: bool,
    pub sample_limit: usize,
    pub sample_count: u64,
    pub source_count: usize,
    pub region_count: usize,
    pub scope: Scope,
    pub providers: Vec<ProviderSummary>,
    pub my_reports: Vec<Report>,
}
