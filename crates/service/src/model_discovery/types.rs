use base64::Engine;
use serde::{Deserialize, Serialize};

pub(super) const DEFAULT_INTERVAL: u64 = 60;

pub(super) fn validate_interval(value: u64) -> Result<(), String> {
    if !(5..=10080).contains(&value) {
        return Err("intervalMinutes must be between 5 and 10080".into());
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Source {
    pub source_kind: String,
    pub source_id: String,
}

impl Source {
    pub fn parse(kind: &str, id: &str) -> Result<Self, String> {
        if !matches!(kind, "openai_account" | "aggregate_api") {
            return Err("unsupported model discovery sourceKind".into());
        }
        let id = id.trim();
        if id.is_empty() || id.len() > 512 || id.chars().any(char::is_control) {
            return Err("invalid model discovery sourceId".into());
        }
        Ok(Self {
            source_kind: kind.into(),
            source_id: id.into(),
        })
    }

    pub fn key(&self) -> String {
        format!(
            "{}.{}",
            self.source_kind,
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(self.source_id.as_bytes())
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Config {
    pub schema_version: u32,
    #[serde(flatten)]
    pub source: Source,
    pub enabled: bool,
    pub interval_minutes: u64,
    pub generation: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(super) struct Status {
    pub running: bool,
    pub last_started_at: Option<i64>,
    pub last_finished_at: Option<i64>,
    pub last_success_at: Option<i64>,
    pub next_run_at: Option<i64>,
    pub last_error: Option<String>,
    pub last_warning: Option<String>,
    pub discovered_count: usize,
    pub created_model_count: usize,
    pub added_route_count: usize,
    pub consecutive_failures: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ModelDiscoveryTask {
    pub source_kind: String,
    pub source_id: String,
    pub enabled: bool,
    pub interval_minutes: u64,
    pub running: bool,
    pub last_started_at: Option<i64>,
    pub last_finished_at: Option<i64>,
    pub last_success_at: Option<i64>,
    pub next_run_at: Option<i64>,
    pub last_error: Option<String>,
    pub last_warning: Option<String>,
    pub discovered_count: usize,
    pub created_model_count: usize,
    pub added_route_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ModelDiscoveryList {
    pub items: Vec<ModelDiscoveryTask>,
}

#[derive(Debug, Clone)]
pub(super) struct Record {
    pub config: Config,
    pub status: Status,
}

impl Record {
    pub fn new(source: Source) -> Self {
        Self {
            config: Config {
                schema_version: 1,
                source,
                enabled: false,
                interval_minutes: DEFAULT_INTERVAL,
                generation: 0,
            },
            status: Status::default(),
        }
    }

    pub fn task(&self) -> ModelDiscoveryTask {
        ModelDiscoveryTask {
            source_kind: self.config.source.source_kind.clone(),
            source_id: self.config.source.source_id.clone(),
            enabled: self.config.enabled,
            interval_minutes: self.config.interval_minutes,
            running: self.status.running,
            last_started_at: self.status.last_started_at,
            last_finished_at: self.status.last_finished_at,
            last_success_at: self.status.last_success_at,
            next_run_at: self.status.next_run_at,
            last_error: self.status.last_error.clone(),
            last_warning: self.status.last_warning.clone(),
            discovered_count: self.status.discovered_count,
            created_model_count: self.status.created_model_count,
            added_route_count: self.status.added_route_count,
        }
    }

    pub fn update(
        &mut self,
        enabled: bool,
        interval: Option<u64>,
        now: i64,
    ) -> Result<bool, String> {
        let interval = interval.unwrap_or(self.config.interval_minutes);
        validate_interval(interval)?;
        if self.config.enabled == enabled && self.config.interval_minutes == interval {
            return Ok(false);
        }
        self.config.generation = self
            .config
            .generation
            .checked_add(1)
            .ok_or_else(|| "model discovery generation exhausted".to_owned())?;
        self.config.enabled = enabled;
        self.config.interval_minutes = interval;
        if self.status.running {
            self.status.last_finished_at = Some(now);
        }
        self.status.running = false;
        self.status.next_run_at = enabled.then_some(now);
        self.status.last_error = None;
        self.status.last_warning = None;
        self.status.consecutive_failures = 0;
        Ok(true)
    }

    pub fn finish_error(&mut self, message: &str, now: i64) {
        self.status.running = false;
        self.status.last_finished_at = Some(now);
        self.status.last_error = Some(message.into());
        self.status.last_warning = None;
        self.status.created_model_count = 0;
        self.status.added_route_count = 0;
        self.status.consecutive_failures = self.status.consecutive_failures.saturating_add(1);
        // At least the configured interval; exponential retries grow to six hours.
        let delay = (300_i64
            * (1_i64 << self.status.consecutive_failures.saturating_sub(1).min(7)))
        .min(6 * 60 * 60)
        .max((self.config.interval_minutes * 60) as i64);
        self.status.next_run_at = self.config.enabled.then_some(now.saturating_add(delay));
    }
}
