use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock, RwLock};
use serde::{Serialize, Deserialize};

use crate::storage_helpers::open_storage;
use codexmanager_core::storage::{
    ProxyProfileUrlTestInsertInput, ProxyProfileUpdateInput,
};

use super::download::run_proxy_download_test_with_cancel;
use super::presets::{resolve_download_test_target, upload_endpoint_status};
use super::upload::run_proxy_upload_test_with_cancel;
use super::latency::run_proxy_latency_test;
use super::cloudflare_speedtest::run_cloudflare_speed_test;
use super::cloudflare_style::config::CfStyleConfig;
use super::cloudflare_style::model::CfStyleResult;


#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobScope {
    SystemProxy,
    AccountProxy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobKind {
    Latency,
    Speed,
    CloudflareStyleSpeed,
}


#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl JobStatus {
    #[allow(dead_code)]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeedSample {
    pub payload_bytes: u64,
    pub duration_ms: u64,
    pub mbps: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeedMetricSummary {
    pub median: f64,
    pub average: f64,
    pub p90: f64,
    pub best: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadDiagnosticResult {
    pub provider_id: String,
    pub file_size_id: String,
    pub status: String,
    pub error: Option<String>,
    pub downloaded_bytes: u64,
    pub duration_ms: u64,
    pub mbps: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobPhase {
    Queued,
    Preflight,
    Latency,
    Download,
    Upload,
    Diagnostics,
    Saving,
    Done,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobState {
    pub job_id: String,
    pub scope: JobScope,
    pub proxy_profile_id: Option<String>,
    pub account_id: Option<String>,
    pub kind: JobKind,
    pub status: JobStatus,
    pub phase: JobPhase,
    pub downloaded_bytes: u64,
    pub uploaded_bytes: u64,
    pub download_mbps: Option<f64>,
    pub upload_mbps: Option<f64>,
    pub latency_ms: Option<u64>,
    pub started_at: i64,
    pub updated_at: i64,
    pub error: Option<String>,
    pub observed_ip: Option<String>,
    pub observed_country: Option<String>,
    pub observed_colo: Option<String>,
    pub download_samples: Vec<SpeedSample>,
    pub upload_samples: Vec<SpeedSample>,
    pub download_summary: Option<SpeedMetricSummary>,
    pub upload_summary: Option<SpeedMetricSummary>,
    pub download_diagnostics: Vec<DownloadDiagnosticResult>,
    pub cf_style_result: Option<CfStyleResult>,
}


#[derive(Debug, Clone)]
pub enum JobParams {
    Latency {
        id: String,
        resolved_proxy_url: Option<String>,
    },
    Speed {
        id: String,
        resolved_proxy_url: Option<String>,
        provider_id: Option<String>,
        file_size_id: Option<String>,
        diagnostic_provider_id: Option<String>,
        diagnostic_file_size_id: Option<String>,
    },
    CloudflareStyleSpeed {
        id: String,
        resolved_proxy_url: Option<String>,
        config: CfStyleConfig,
    },
}


#[derive(Clone)]
pub struct ActiveJob {
    pub state: JobState,
    pub params: JobParams,
    pub cancel_flag: Arc<AtomicBool>,
}

pub struct JobRegistry {
    jobs: RwLock<HashMap<String, ActiveJob>>,
}

static REGISTRY: OnceLock<Arc<JobRegistry>> = OnceLock::new();

impl JobRegistry {
    pub fn global() -> Arc<Self> {
        REGISTRY.get_or_init(|| {
            let registry = Arc::new(Self {
                jobs: RwLock::new(HashMap::new()),
            });
            spawn_scheduler_loop(registry.clone());
            registry
        }).clone()
    }

    pub fn create_latency_job(&self, id: &str) -> JobState {
        let job_id = format!("job_lat_{}", rand::random::<u32>());
        let now = codexmanager_core::storage::now_ts();
        let state = JobState {
            job_id: job_id.clone(),
            scope: JobScope::SystemProxy,
            proxy_profile_id: Some(id.to_string()),
            account_id: None,
            kind: JobKind::Latency,
            status: JobStatus::Queued,
            phase: JobPhase::Queued,
            downloaded_bytes: 0,
            uploaded_bytes: 0,
            download_mbps: None,
            upload_mbps: None,
            latency_ms: None,
            started_at: now,
            updated_at: now,
            error: None,
            observed_ip: None,
            observed_country: None,
            observed_colo: None,
            download_samples: Vec::new(),
            upload_samples: Vec::new(),
            download_summary: None,
            upload_summary: None,
            download_diagnostics: Vec::new(),
            cf_style_result: None,
        };
        let active_job = ActiveJob {
            state: state.clone(),
            params: JobParams::Latency {
                id: id.to_string(),
                resolved_proxy_url: None,
            },
            cancel_flag: Arc::new(AtomicBool::new(false)),
        };
        self.jobs.write().unwrap().insert(job_id, active_job);
        state
    }

    pub fn create_speed_job(
        &self,
        id: &str,
        provider_id: Option<&str>,
        file_size_id: Option<&str>,
        diagnostic_provider_id: Option<&str>,
        diagnostic_file_size_id: Option<&str>,
    ) -> JobState {
        let job_id = format!("job_spd_{}", rand::random::<u32>());
        let now = codexmanager_core::storage::now_ts();
        let state = JobState {
            job_id: job_id.clone(),
            scope: JobScope::SystemProxy,
            proxy_profile_id: Some(id.to_string()),
            account_id: None,
            kind: JobKind::Speed,
            status: JobStatus::Queued,
            phase: JobPhase::Queued,
            downloaded_bytes: 0,
            uploaded_bytes: 0,
            download_mbps: None,
            upload_mbps: None,
            latency_ms: None,
            started_at: now,
            updated_at: now,
            error: None,
            observed_ip: None,
            observed_country: None,
            observed_colo: None,
            download_samples: Vec::new(),
            upload_samples: Vec::new(),
            download_summary: None,
            upload_summary: None,
            download_diagnostics: Vec::new(),
            cf_style_result: None,
        };
        let active_job = ActiveJob {
            state: state.clone(),
            params: JobParams::Speed {
                id: id.to_string(),
                resolved_proxy_url: None,
                provider_id: provider_id.map(String::from),
                file_size_id: file_size_id.map(String::from),
                diagnostic_provider_id: diagnostic_provider_id.map(String::from),
                diagnostic_file_size_id: diagnostic_file_size_id.map(String::from),
            },
            cancel_flag: Arc::new(AtomicBool::new(false)),
        };
        self.jobs.write().unwrap().insert(job_id, active_job);
        state
    }

    pub fn create_cloudflare_style_speed_job(
        &self,
        id: &str,
        config: CfStyleConfig,
    ) -> JobState {
        let job_id = format!("job_cf_spd_{}", rand::random::<u32>());
        let now = codexmanager_core::storage::now_ts();
        let state = JobState {
            job_id: job_id.clone(),
            scope: JobScope::SystemProxy,
            proxy_profile_id: Some(id.to_string()),
            account_id: None,
            kind: JobKind::CloudflareStyleSpeed,
            status: JobStatus::Queued,
            phase: JobPhase::Queued,
            downloaded_bytes: 0,
            uploaded_bytes: 0,
            download_mbps: None,
            upload_mbps: None,
            latency_ms: None,
            started_at: now,
            updated_at: now,
            error: None,
            observed_ip: None,
            observed_country: None,
            observed_colo: None,
            download_samples: Vec::new(),
            upload_samples: Vec::new(),
            download_summary: None,
            upload_summary: None,
            download_diagnostics: Vec::new(),
            cf_style_result: None,
        };
        let active_job = ActiveJob {
            state: state.clone(),
            params: JobParams::CloudflareStyleSpeed {
                id: id.to_string(),
                resolved_proxy_url: None,
                config,
            },
            cancel_flag: Arc::new(AtomicBool::new(false)),
        };
        self.jobs.write().unwrap().insert(job_id, active_job);
        state
    }

    pub fn create_account_latency_job(
        &self,
        account_id: &str,
        proxy_profile_id: Option<&str>,
        proxy_url: &str,
    ) -> JobState {
        let job_id = format!("job_lat_{}", rand::random::<u32>());
        let now = codexmanager_core::storage::now_ts();
        let state = JobState {
            job_id: job_id.clone(),
            scope: JobScope::AccountProxy,
            proxy_profile_id: proxy_profile_id.map(str::to_string),
            account_id: Some(account_id.to_string()),
            kind: JobKind::Latency,
            status: JobStatus::Queued,
            phase: JobPhase::Queued,
            downloaded_bytes: 0,
            uploaded_bytes: 0,
            download_mbps: None,
            upload_mbps: None,
            latency_ms: None,
            started_at: now,
            updated_at: now,
            error: None,
            observed_ip: None,
            observed_country: None,
            observed_colo: None,
            download_samples: Vec::new(),
            upload_samples: Vec::new(),
            download_summary: None,
            upload_summary: None,
            download_diagnostics: Vec::new(),
            cf_style_result: None,
        };
        let active_job = ActiveJob {
            state: state.clone(),
            params: JobParams::Latency {
                id: account_id.to_string(),
                resolved_proxy_url: Some(proxy_url.to_string()),
            },
            cancel_flag: Arc::new(AtomicBool::new(false)),
        };
        self.jobs.write().unwrap().insert(job_id, active_job);
        state
    }

    pub fn create_account_speed_job(
        &self,
        account_id: &str,
        proxy_profile_id: Option<&str>,
        proxy_url: &str,
        provider_id: Option<&str>,
        file_size_id: Option<&str>,
        diagnostic_provider_id: Option<&str>,
        diagnostic_file_size_id: Option<&str>,
    ) -> JobState {
        let job_id = format!("job_spd_{}", rand::random::<u32>());
        let now = codexmanager_core::storage::now_ts();
        let state = JobState {
            job_id: job_id.clone(),
            scope: JobScope::AccountProxy,
            proxy_profile_id: proxy_profile_id.map(str::to_string),
            account_id: Some(account_id.to_string()),
            kind: JobKind::Speed,
            status: JobStatus::Queued,
            phase: JobPhase::Queued,
            downloaded_bytes: 0,
            uploaded_bytes: 0,
            download_mbps: None,
            upload_mbps: None,
            latency_ms: None,
            started_at: now,
            updated_at: now,
            error: None,
            observed_ip: None,
            observed_country: None,
            observed_colo: None,
            download_samples: Vec::new(),
            upload_samples: Vec::new(),
            download_summary: None,
            upload_summary: None,
            download_diagnostics: Vec::new(),
            cf_style_result: None,
        };
        let active_job = ActiveJob {
            state: state.clone(),
            params: JobParams::Speed {
                id: account_id.to_string(),
                resolved_proxy_url: Some(proxy_url.to_string()),
                provider_id: provider_id.map(String::from),
                file_size_id: file_size_id.map(String::from),
                diagnostic_provider_id: diagnostic_provider_id.map(String::from),
                diagnostic_file_size_id: diagnostic_file_size_id.map(String::from),
            },
            cancel_flag: Arc::new(AtomicBool::new(false)),
        };
        self.jobs.write().unwrap().insert(job_id, active_job);
        state
    }

    pub fn create_account_cloudflare_style_speed_job(
        &self,
        account_id: &str,
        proxy_profile_id: Option<&str>,
        proxy_url: &str,
        config: CfStyleConfig,
    ) -> JobState {
        let job_id = format!("job_cf_spd_{}", rand::random::<u32>());
        let now = codexmanager_core::storage::now_ts();
        let state = JobState {
            job_id: job_id.clone(),
            scope: JobScope::AccountProxy,
            proxy_profile_id: proxy_profile_id.map(str::to_string),
            account_id: Some(account_id.to_string()),
            kind: JobKind::CloudflareStyleSpeed,
            status: JobStatus::Queued,
            phase: JobPhase::Queued,
            downloaded_bytes: 0,
            uploaded_bytes: 0,
            download_mbps: None,
            upload_mbps: None,
            latency_ms: None,
            started_at: now,
            updated_at: now,
            error: None,
            observed_ip: None,
            observed_country: None,
            observed_colo: None,
            download_samples: Vec::new(),
            upload_samples: Vec::new(),
            download_summary: None,
            upload_summary: None,
            download_diagnostics: Vec::new(),
            cf_style_result: None,
        };
        let active_job = ActiveJob {
            state: state.clone(),
            params: JobParams::CloudflareStyleSpeed {
                id: account_id.to_string(),
                resolved_proxy_url: Some(proxy_url.to_string()),
                config,
            },
            cancel_flag: Arc::new(AtomicBool::new(false)),
        };
        self.jobs.write().unwrap().insert(job_id, active_job);
        state
    }


    pub fn get_job(&self, job_id: &str) -> Option<JobState> {
        self.jobs.read().unwrap().get(job_id).map(|j| j.state.clone())
    }

    pub fn cancel_job(&self, job_id: &str) -> bool {
        let registry = self.jobs.write().unwrap();
        if let Some(job) = registry.get(job_id) {
            job.cancel_flag.store(true, Ordering::SeqCst);
            // Immediately mark the job as Cancelled regardless of current status.
            // For Running jobs, execute_job will check should_cancel() before writing
            // final results; even if it continues briefly in block_in_place, the status
            // is already visible to pollers.
            let is_terminal = matches!(
                job.state.status,
                JobStatus::Completed | JobStatus::Failed | JobStatus::Cancelled
            );
            drop(registry);
            if !is_terminal {
                self.update_job_status(job_id, JobStatus::Cancelled, Some("Cancelled".to_string()));
            }
            true
        } else {
            false
        }
    }

    fn update_job_status(&self, job_id: &str, status: JobStatus, error: Option<String>) {
        if let Some(job) = self.jobs.write().unwrap().get_mut(job_id) {
            // Do not overwrite a Cancelled status: cancel_job sets it optimistically
            // before execute_job finishes, so we must respect that decision.
            if job.state.status == JobStatus::Cancelled && status != JobStatus::Cancelled {
                return;
            }
            job.state.status = status;
            job.state.error = error;
            job.state.updated_at = codexmanager_core::storage::now_ts();
            if status == JobStatus::Completed || status == JobStatus::Failed || status == JobStatus::Cancelled {
                job.state.phase = JobPhase::Done;
            }
        }
    }

    fn update_job_phase(&self, job_id: &str, phase: JobPhase) {
        if let Some(job) = self.jobs.write().unwrap().get_mut(job_id) {
            job.state.phase = phase;
            job.state.updated_at = codexmanager_core::storage::now_ts();
        }
    }

    fn update_download_progress(&self, job_id: &str, bytes: u64, mbps: Option<f64>) {
        if let Some(job) = self.jobs.write().unwrap().get_mut(job_id) {
            job.state.downloaded_bytes = bytes;
            if let Some(v) = mbps {
                job.state.download_mbps = Some(v);
            }
            job.state.updated_at = codexmanager_core::storage::now_ts();
        }
    }

    fn update_upload_progress(&self, job_id: &str, bytes: u64, mbps: Option<f64>) {
        if let Some(job) = self.jobs.write().unwrap().get_mut(job_id) {
            job.state.uploaded_bytes = bytes;
            if let Some(v) = mbps {
                job.state.upload_mbps = Some(v);
            }
            job.state.updated_at = codexmanager_core::storage::now_ts();
        }
    }

    fn update_final_metrics(&self, job_id: &str, latency: Option<u64>, download_mbps: Option<f64>, upload_mbps: Option<f64>) {
        if let Some(job) = self.jobs.write().unwrap().get_mut(job_id) {
            if latency.is_some() {
                job.state.latency_ms = latency;
            }
            if download_mbps.is_some() {
                job.state.download_mbps = download_mbps;
            }
            if upload_mbps.is_some() {
                job.state.upload_mbps = upload_mbps;
            }
            job.state.updated_at = codexmanager_core::storage::now_ts();
        }
    }

    pub fn update_preflight_info(&self, job_id: &str, ip: String, country: String, colo: String) {
        if let Some(job) = self.jobs.write().unwrap().get_mut(job_id) {
            job.state.observed_ip = Some(ip);
            job.state.observed_country = Some(country);
            job.state.observed_colo = Some(colo);
            job.state.updated_at = codexmanager_core::storage::now_ts();
        }
    }

    pub fn add_download_sample(&self, job_id: &str, sample: SpeedSample) {
        if let Some(job) = self.jobs.write().unwrap().get_mut(job_id) {
            job.state.download_samples.push(sample);
            job.state.updated_at = codexmanager_core::storage::now_ts();
        }
    }

    pub fn add_upload_sample(&self, job_id: &str, sample: SpeedSample) {
        if let Some(job) = self.jobs.write().unwrap().get_mut(job_id) {
            job.state.upload_samples.push(sample);
            job.state.updated_at = codexmanager_core::storage::now_ts();
        }
    }

    pub fn set_summaries(&self, job_id: &str, download_summary: Option<SpeedMetricSummary>, upload_summary: Option<SpeedMetricSummary>) {
        if let Some(job) = self.jobs.write().unwrap().get_mut(job_id) {
            job.state.download_summary = download_summary;
            job.state.upload_summary = upload_summary;
            job.state.updated_at = codexmanager_core::storage::now_ts();
        }
    }

    pub fn add_download_diagnostic(&self, job_id: &str, diagnostic: DownloadDiagnosticResult) {
        if let Some(job) = self.jobs.write().unwrap().get_mut(job_id) {
            job.state.download_diagnostics.push(diagnostic);
            job.state.updated_at = codexmanager_core::storage::now_ts();
        }
    }

    pub fn update_cf_style_result(&self, job_id: &str, result: CfStyleResult) {
        if let Some(job) = self.jobs.write().unwrap().get_mut(job_id) {
            job.state.cf_style_result = Some(result);
            job.state.updated_at = codexmanager_core::storage::now_ts();
        }
    }
}


static SCHEDULER_RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();

fn scheduler_runtime() -> &'static tokio::runtime::Runtime {
    SCHEDULER_RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .thread_name("proxy-jobs-scheduler")
            .build()
            .unwrap_or_else(|err| panic!("build scheduler runtime failed: {err}"))
    })
}

fn collect_jobs_to_start(jobs_map: &HashMap<String, ActiveJob>) -> Vec<ActiveJob> {
    let mut jobs_to_start = Vec::new();

    // Считаем активные (Running) джобы для проверки лимитов
    let mut active_latency_count = 0;
    let mut active_speed_count = 0;
    let mut running_profiles = HashMap::new();
    let mut running_accounts = HashMap::new();
    let mut large_speedtest_active = false;

    for job in jobs_map.values() {
        if job.state.status == JobStatus::Running {
            match job.state.kind {
                JobKind::Latency => {
                    active_latency_count += 1;
                }
                JobKind::Speed | JobKind::CloudflareStyleSpeed => {
                    active_speed_count += 1;
                    if let Some(ref pid) = job.state.proxy_profile_id {
                        running_profiles.insert(pid.clone(), true);
                    }
                    if let Some(ref aid) = job.state.account_id {
                        running_accounts.insert(aid.clone(), true);
                    }
                    if let JobParams::Speed { ref file_size_id, .. } = job.params {
                        if let Some(ref sz) = file_size_id {
                            if sz == "size_1gb" || sz == "size_10gb" || sz == "size_100mb" {
                                large_speedtest_active = true;
                            }
                        }
                    }
                }
            }
        }
    }

    let mut queued_jobs: Vec<ActiveJob> = jobs_map
        .values()
        .filter(|j| j.state.status == JobStatus::Queued)
        .cloned()
        .collect();
    queued_jobs.sort_by_key(|j| j.state.started_at);

    for job in queued_jobs {
        match job.state.kind {
            JobKind::Latency => {
                if active_latency_count < 4 {
                    active_latency_count += 1;
                    jobs_to_start.push(job);
                }
            }
            JobKind::Speed | JobKind::CloudflareStyleSpeed => {
                if active_speed_count >= 1 {
                    continue;
                }
                if let Some(ref pid) = job.state.proxy_profile_id {
                    if running_profiles.contains_key(pid) {
                        continue;
                    }
                }
                if let Some(ref aid) = job.state.account_id {
                    if running_accounts.contains_key(aid) {
                        continue;
                    }
                }
                let is_large = if let JobParams::Speed { ref file_size_id, .. } = job.params {
                    file_size_id.as_deref() == Some("size_1gb")
                        || file_size_id.as_deref() == Some("size_10gb")
                        || file_size_id.as_deref() == Some("size_100mb")
                } else {
                    false
                };
                if is_large && large_speedtest_active {
                    continue;
                }

                active_speed_count += 1;
                if let Some(ref pid) = job.state.proxy_profile_id {
                    running_profiles.insert(pid.clone(), true);
                }
                if let Some(ref aid) = job.state.account_id {
                    running_accounts.insert(aid.clone(), true);
                }
                if is_large {
                    large_speedtest_active = true;
                }
                jobs_to_start.push(job);
            }
        }
    }


    jobs_to_start
}

fn spawn_scheduler_loop(registry: Arc<JobRegistry>) {
    scheduler_runtime().spawn(async move {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

            let jobs_to_start = {
                let jobs_map = registry.jobs.read().unwrap();
                collect_jobs_to_start(&jobs_map)
            };

            // Запускаем отобранные джобы
            for job in jobs_to_start {
                registry.update_job_status(&job.state.job_id, JobStatus::Running, None);
                let registry_clone = registry.clone();
                scheduler_runtime().spawn(async move {
                    execute_job(registry_clone, job).await;
                });
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_speed_job(
        job_id: &str,
        status: JobStatus,
        scope: JobScope,
        proxy_profile_id: Option<&str>,
        account_id: Option<&str>,
        file_size_id: Option<&str>,
        started_at: i64,
    ) -> ActiveJob {
        ActiveJob {
            state: JobState {
                job_id: job_id.to_string(),
                scope,
                proxy_profile_id: proxy_profile_id.map(str::to_string),
                account_id: account_id.map(str::to_string),
                kind: JobKind::Speed,
                status,
                phase: if status == JobStatus::Running {
                    JobPhase::Download
                } else {
                    JobPhase::Queued
                },
                downloaded_bytes: 0,
                uploaded_bytes: 0,
                download_mbps: None,
                upload_mbps: None,
                latency_ms: None,
                started_at,
                updated_at: started_at,
                error: None,
                observed_ip: None,
                observed_country: None,
                observed_colo: None,
                download_samples: Vec::new(),
                upload_samples: Vec::new(),
                download_summary: None,
                upload_summary: None,
                download_diagnostics: Vec::new(),
                cf_style_result: None,
            },
            params: JobParams::Speed {

                id: proxy_profile_id
                    .or(account_id)
                    .unwrap_or("job-target")
                    .to_string(),
                resolved_proxy_url: None,
                provider_id: Some("cachefly".to_string()),
                file_size_id: file_size_id.map(str::to_string),
                diagnostic_provider_id: None,
                diagnostic_file_size_id: None,
            },
            cancel_flag: Arc::new(AtomicBool::new(false)),
        }
    }

    #[test]
    fn scheduler_blocks_speed_job_for_running_proxy_profile() {
        let mut jobs = HashMap::new();
        jobs.insert(
            "running".to_string(),
            make_speed_job(
                "running",
                JobStatus::Running,
                JobScope::SystemProxy,
                Some("profile-1"),
                None,
                Some("size_10mb"),
                10,
            ),
        );
        jobs.insert(
            "queued".to_string(),
            make_speed_job(
                "queued",
                JobStatus::Queued,
                JobScope::SystemProxy,
                Some("profile-1"),
                None,
                Some("size_10mb"),
                20,
            ),
        );

        let jobs_to_start = collect_jobs_to_start(&jobs);

        assert!(jobs_to_start.is_empty());
    }

    #[test]
    fn scheduler_blocks_account_proxy_speed_job_for_running_account() {
        let mut jobs = HashMap::new();
        jobs.insert(
            "running".to_string(),
            make_speed_job(
                "running",
                JobStatus::Running,
                JobScope::AccountProxy,
                None,
                Some("acc-1"),
                Some("size_10mb"),
                10,
            ),
        );
        jobs.insert(
            "queued".to_string(),
            make_speed_job(
                "queued",
                JobStatus::Queued,
                JobScope::AccountProxy,
                None,
                Some("acc-1"),
                Some("size_10mb"),
                20,
            ),
        );

        let jobs_to_start = collect_jobs_to_start(&jobs);

        assert!(jobs_to_start.is_empty());
    }

    #[test]
    fn scheduler_blocks_second_large_speed_job_globally() {
        let mut jobs = HashMap::new();
        jobs.insert(
            "running".to_string(),
            make_speed_job(
                "running",
                JobStatus::Running,
                JobScope::SystemProxy,
                Some("profile-1"),
                None,
                Some("size_1gb"),
                10,
            ),
        );
        jobs.insert(
            "queued".to_string(),
            make_speed_job(
                "queued",
                JobStatus::Queued,
                JobScope::SystemProxy,
                Some("profile-2"),
                None,
                Some("size_10gb"),
                20,
            ),
        );

        let jobs_to_start = collect_jobs_to_start(&jobs);

        assert!(jobs_to_start.is_empty());
    }
}

async fn execute_job(registry: Arc<JobRegistry>, job: ActiveJob) {
    let cancel_flag = job.cancel_flag.clone();
    let cancel_flag_for_closure = cancel_flag.clone();
    let should_cancel = move || cancel_flag_for_closure.load(Ordering::SeqCst);

    match job.params {
        JobParams::Latency {
            id,
            resolved_proxy_url,
        } => {
            registry.update_job_phase(&job.state.job_id, JobPhase::Latency);

            let storage = match open_storage() {
                Some(s) => s,
                None => {
                    registry.update_job_status(&job.state.job_id, JobStatus::Failed, Some("storage unavailable".to_string()));
                    return;
                }
            };

            let (proxy_url, system_profile_id) = if job.state.scope == JobScope::SystemProxy {
                let profile = match storage.find_proxy_profile(&id) {
                    Ok(Some(p)) => p,
                    Ok(None) => {
                        registry.update_job_status(&job.state.job_id, JobStatus::Failed, Some("proxy profile not found".to_string()));
                        return;
                    }
                    Err(err) => {
                        registry.update_job_status(&job.state.job_id, JobStatus::Failed, Some(format!("read proxy profile failed: {err}")));
                        return;
                    }
                };

                let proxy_url = match crate::proxy_registry::validation::normalize_proxy_profile_url(&profile.proxy_url) {
                    Ok(url) => url,
                    Err(err) => {
                        registry.update_job_status(&job.state.job_id, JobStatus::Failed, Some(err));
                        return;
                    }
                };
                (proxy_url, Some(profile.id))
            } else {
                let Some(proxy_url) = resolved_proxy_url else {
                    registry.update_job_status(&job.state.job_id, JobStatus::Failed, Some("account proxy test target is missing resolved proxy URL".to_string()));
                    return;
                };
                (proxy_url, None)
            };

            if should_cancel() {
                registry.update_job_status(&job.state.job_id, JobStatus::Cancelled, Some("Cancelled".to_string()));
                return;
            }

            // Запускаем тест. Поскольку он синхронный, мы выполняем его в блокирующем пуле tokio::task::block_in_place
            let outcome = tokio::task::block_in_place(|| {
                run_proxy_latency_test(
                    proxy_url.as_str(),
                    "http://cp.cloudflare.com/generate_204",
                    true,
                )
            });

            if should_cancel() {
                registry.update_job_status(&job.state.job_id, JobStatus::Cancelled, Some("Cancelled".to_string()));
                return;
            }

            registry.update_job_phase(&job.state.job_id, JobPhase::Saving);

            if let Some(profile_id) = system_profile_id {
                let mut ip = None;
                let mut country_code = None;
                let mut country_name = None;
                let mut region_name = None;
                let mut city_name = None;
                let mut asn = None;
                let mut as_org = None;
                let mut flag_img_url = None;
                let mut flag_emoji = None;
                let mut timezone_id = None;
                let mut timezone_offset = None;
                let mut timezone_utc = None;
                let mut isp = None;
                let mut as_domain = None;

                if outcome.status == "ok" {
                    let geo_outcome = tokio::task::block_in_place(|| {
                        crate::account::proxy_health::check_account_proxy(&proxy_url, |country_code| {
                            storage
                                .find_cached_proxy_flag_by_country(country_code)
                                .unwrap_or(None)
                        })
                    });
                    if let Some(geo) = geo_outcome.geo {
                        ip = geo.ip;
                        country_code = geo.country_code;
                        country_name = geo.country_name;
                        region_name = geo.region_name;
                        city_name = geo.city_name;
                        asn = geo.asn;
                        as_org = geo.as_org;
                        isp = geo.isp;
                        as_domain = geo.as_domain;
                        flag_img_url = geo.flag_img_url;
                        flag_emoji = geo.flag_emoji;
                        timezone_id = geo.timezone_id;
                        timezone_offset = geo.timezone_offset;
                        timezone_utc = geo.timezone_utc;
                    }
                }

                let _ = storage.update_proxy_profile(&ProxyProfileUpdateInput {
                    id: profile_id.clone(),
                    name: None,
                    proxy_url: None,
                    enabled: None,
                    status: Some(outcome.status.clone()),
                    last_error: Some(outcome.error.clone().unwrap_or_default()),
                    last_url_latency_ms: outcome.url_latency_ms,
                    last_download_mbps: None,
                    last_upload_mbps: None,
                    last_tested_at: Some(outcome.tested_at),
                    ip,
                    country_code,
                    country_name,
                    region_name,
                    city_name,
                    asn,
                    as_org,
                    isp,
                    as_domain,
                    flag_img_url,
                    flag_emoji,
                    timezone_id,
                    timezone_offset,
                    timezone_utc,
                    tags_json: None,
                    notes: None,
                });

                let _ = storage.insert_proxy_profile_url_test(&ProxyProfileUrlTestInsertInput {
                    proxy_profile_id: profile_id,
                    status: outcome.status.clone(),
                    url_latency_ms: outcome.url_latency_ms,
                    status_code: outcome.status_code,
                    test_url: outcome.test_url,
                    final_url: outcome.final_url,
                    redirected: outcome.redirected,
                    tested_at: outcome.tested_at,
                    error_code: outcome.error_code,
                    error: outcome.error.clone(),
                });
            } else {
                persist_account_latency_outcome(&storage, &id, &outcome);
            }

            registry.update_final_metrics(&job.state.job_id, outcome.url_latency_ms.map(|v| v as u64), None, None);

            if outcome.status == "ok" {
                registry.update_job_status(&job.state.job_id, JobStatus::Completed, None);
            } else {
                registry.update_job_status(&job.state.job_id, JobStatus::Failed, outcome.error);
            }
        }

        JobParams::Speed {
            id,
            resolved_proxy_url,
            provider_id,
            file_size_id,
            diagnostic_provider_id,
            diagnostic_file_size_id,
        } => {
            let storage = match open_storage() {
                Some(s) => s,
                None => {
                    registry.update_job_status(&job.state.job_id, JobStatus::Failed, Some("storage unavailable".to_string()));
                    return;
                }
            };

            let (proxy_url, system_profile_id) = if job.state.scope == JobScope::SystemProxy {
                let profile = match storage.find_proxy_profile(&id) {
                    Ok(Some(p)) => p,
                    Ok(None) => {
                        registry.update_job_status(&job.state.job_id, JobStatus::Failed, Some("proxy profile not found".to_string()));
                        return;
                    }
                    Err(err) => {
                        registry.update_job_status(&job.state.job_id, JobStatus::Failed, Some(format!("read proxy profile failed: {err}")));
                        return;
                    }
                };

                let proxy_url = match crate::proxy_registry::validation::normalize_proxy_profile_url(&profile.proxy_url) {
                    Ok(url) => url,
                    Err(err) => {
                        registry.update_job_status(&job.state.job_id, JobStatus::Failed, Some(err));
                        return;
                    }
                };
                (proxy_url, Some(profile.id))
            } else {
                let Some(proxy_url) = resolved_proxy_url else {
                    registry.update_job_status(&job.state.job_id, JobStatus::Failed, Some("account proxy test target is missing resolved proxy URL".to_string()));
                    return;
                };
                (proxy_url, None)
            };

            let provider = provider_id.as_deref().unwrap_or("cloudflare_http_rust");

            let final_status;
            let final_error;
            let final_download_mbps;
            let final_upload_mbps;
            let final_latency_ms;

            if provider == "cloudflare_http_rust" {
                // Запускаем Cloudflare HTTP speed test
                registry.update_job_phase(&job.state.job_id, JobPhase::Preflight);

                let registry_cf = registry.clone();
                let job_id_cf = job.state.job_id.clone();
                let cancel_flag_cf = cancel_flag.clone();
                let should_cancel_cf = move || cancel_flag_cf.load(Ordering::SeqCst);

                let resolved_file_size = file_size_id.as_deref().unwrap_or("size_25mb");
                let max_payload_bytes = match resolved_file_size {
                    "size_100kb" => 100_000,
                    "size_1mb" => 1_000_000,
                    "size_10mb" => 10_000_000,
                    "size_25mb" => 25_000_000,
                    "size_100mb" => 100_000_000,
                    _ => 25_000_000,
                };

                let on_download_progress = {
                    let r = registry_cf.clone();
                    let jid = job_id_cf.clone();
                    move |bytes, mbps| r.update_download_progress(&jid, bytes, Some(mbps))
                };
                let on_upload_progress = {
                    let r = registry_cf.clone();
                    let jid = job_id_cf.clone();
                    move |bytes, mbps| r.update_upload_progress(&jid, bytes, Some(mbps))
                };
                let on_download_sample = {
                    let r = registry_cf.clone();
                    let jid = job_id_cf.clone();
                    move |sample| r.add_download_sample(&jid, sample)
                };
                let on_upload_sample = {
                    let r = registry_cf.clone();
                    let jid = job_id_cf.clone();
                    move |sample| r.add_upload_sample(&jid, sample)
                };
                let on_preflight = {
                    let r = registry_cf.clone();
                    let jid = job_id_cf.clone();
                    move |ip, country, colo| r.update_preflight_info(&jid, ip, country, colo)
                };
                let on_payload_start = {
                    let r = registry_cf.clone();
                    let jid = job_id_cf.clone();
                    move |_, is_download| {
                        if is_download {
                            r.update_job_phase(&jid, JobPhase::Download);
                        } else {
                            r.update_job_phase(&jid, JobPhase::Upload);
                        }
                    }
                };

                let outcome = run_cloudflare_speed_test(
                    &proxy_url,
                    max_payload_bytes,
                    should_cancel_cf,
                    on_download_progress,
                    on_upload_progress,
                    on_download_sample,
                    on_upload_sample,
                    on_preflight,
                    on_payload_start,
                ).await;

                registry_cf.set_summaries(
                    &job_id_cf,
                    outcome.download_summary.clone(),
                    outcome.upload_summary.clone(),
                );

                final_status = match outcome.status.as_str() {
                    "ok" => "ok",
                    "cancelled" => "cancelled",
                    "partial" => "partial",
                    _ => "failed",
                };
                final_error = outcome.error;
                final_download_mbps = outcome.download_summary.map(|s| s.median);
                final_upload_mbps = outcome.upload_summary.map(|s| s.median);
                final_latency_ms = outcome.latency_ms;
            } else {
                final_latency_ms = None;
                // 1. Проверяем upload endpoint config.
                let endpoint_status = upload_endpoint_status();
                if !endpoint_status.configured {
                    let err = super::errors::upload_endpoint_not_configured_error();
                    let status = super::errors::proxy_test_result_status(&err);

                    if let Some(profile_id) = system_profile_id.as_ref() {
                        let _ = storage.update_proxy_profile(&ProxyProfileUpdateInput {
                            id: profile_id.clone(),
                            name: None,
                            proxy_url: None,
                            enabled: None,
                            status: Some(status.to_string()),
                            last_error: Some(err.message.clone()),
                            last_url_latency_ms: None,
                            last_download_mbps: None,
                            last_upload_mbps: None,
                            last_tested_at: Some(codexmanager_core::storage::now_ts()),
                            ip: None,
                            country_code: None,
                            country_name: None,
                            region_name: None,
                            city_name: None,
                            asn: None,
                            as_org: None,
                            isp: None,
                            as_domain: None,
                            flag_img_url: None,
                            flag_emoji: None,
                            timezone_id: None,
                            timezone_offset: None,
                            timezone_utc: None,
                            tags_json: None,
                            notes: None,
                        });
                    } else {
                        persist_account_speed_result(
                            &storage,
                            &id,
                            status,
                            None,
                            None,
                            None,
                            Some(err.message.as_str()),
                            codexmanager_core::storage::now_ts(),
                        );
                    }

                    registry.update_job_status(&job.state.job_id, JobStatus::Failed, Some(err.message));
                    return;
                }

                // 2. Резолвим target для download
                let download_target = match resolve_download_test_target(provider_id.as_deref(), file_size_id.as_deref()) {
                    Ok(t) => t,
                    Err(err) => {
                        registry.update_job_status(&job.state.job_id, JobStatus::Failed, Some(err));
                        return;
                    }
                };

                if should_cancel() {
                    registry.update_job_status(&job.state.job_id, JobStatus::Cancelled, Some("Cancelled".to_string()));
                    return;
                }

                // 3. Запускаем download test
                registry.update_job_phase(&job.state.job_id, JobPhase::Download);

                let registry_dl = registry.clone();
                let job_id_dl = job.state.job_id.clone();
                let cancel_flag_dl = cancel_flag.clone();
                let should_cancel_dl = move || cancel_flag_dl.load(Ordering::SeqCst);

                let download_outcome = tokio::task::block_in_place(|| {
                    run_proxy_download_test_with_cancel(
                        proxy_url.as_str(),
                        &download_target,
                        should_cancel_dl,
                        |bytes| registry_dl.update_download_progress(&job_id_dl, bytes, None),
                    )
                });

                if download_outcome.cancelled || should_cancel() {
                    registry.update_job_status(&job.state.job_id, JobStatus::Cancelled, Some("Cancelled".to_string()));
                    return;
                }

                if download_outcome.status != "ok" {
                    registry.update_job_phase(&job.state.job_id, JobPhase::Saving);
                    if let Some(profile_id) = system_profile_id.as_ref() {
                        let _ = storage.update_proxy_profile(&ProxyProfileUpdateInput {
                            id: profile_id.clone(),
                            name: None,
                            proxy_url: None,
                            enabled: None,
                            status: Some("failed".to_string()),
                            last_error: Some(download_outcome.error.clone().unwrap_or_default()),
                            last_url_latency_ms: None,
                            last_download_mbps: None,
                            last_upload_mbps: None,
                            last_tested_at: Some(codexmanager_core::storage::now_ts()),
                            ip: None,
                            country_code: None,
                            country_name: None,
                            region_name: None,
                            city_name: None,
                            asn: None,
                            as_org: None,
                            isp: None,
                            as_domain: None,
                            flag_img_url: None,
                            flag_emoji: None,
                            timezone_id: None,
                            timezone_offset: None,
                            timezone_utc: None,
                            tags_json: None,
                            notes: None,
                        });
                    } else {
                        persist_account_speed_result(
                            &storage,
                            &id,
                            "failed",
                            None,
                            None,
                            None,
                            download_outcome.error.as_deref(),
                            codexmanager_core::storage::now_ts(),
                        );
                    }
                    registry.update_job_status(&job.state.job_id, JobStatus::Failed, download_outcome.error);
                    return;
                }

                // 4. Запускаем upload test
                registry.update_job_phase(&job.state.job_id, JobPhase::Upload);

                let upload_bytes = download_target.read_limit_bytes.unwrap_or(download_target.size_bytes) as u64;
                let registry_ul = registry.clone();
                let job_id_ul = job.state.job_id.clone();
                let cancel_flag_ul = cancel_flag.clone();
                let should_cancel_ul = move || cancel_flag_ul.load(Ordering::SeqCst);
                let progress_ul = Arc::new(move |bytes| registry_ul.update_upload_progress(&job_id_ul, bytes, None));

                let upload_outcome = tokio::task::block_in_place(|| {
                    run_proxy_upload_test_with_cancel(
                        proxy_url.as_str(),
                        upload_bytes,
                        should_cancel_ul,
                        move |bytes| progress_ul(bytes),
                    )
                });

                if upload_outcome.cancelled || should_cancel() {
                    registry.update_job_status(&job.state.job_id, JobStatus::Cancelled, Some("Cancelled".to_string()));
                    return;
                }

                final_status = if upload_outcome.status == "ok" {
                    "ok"
                } else {
                    "partial"
                };
                final_error = if final_status != "ok" {
                    upload_outcome.error.clone()
                } else {
                    None
                };
                final_download_mbps = Some(download_outcome.download_mbps.unwrap_or(0.0));
                final_upload_mbps = if upload_outcome.status == "ok" {
                    upload_outcome.upload_mbps
                } else {
                    None
                };
            }

            // Проверяем статус cancel перед диагностикой
            if final_status == "cancelled" || should_cancel() {
                registry.update_job_status(&job.state.job_id, JobStatus::Cancelled, Some("Cancelled".to_string()));
                return;
            }

            // 5. Опциональная диагностика скачивания
            if let (Some(diag_provider), Some(diag_size)) = (diagnostic_provider_id.as_deref(), diagnostic_file_size_id.as_deref()) {
                if !diag_provider.is_empty() && !diag_size.is_empty() {
                    registry.update_job_phase(&job.state.job_id, JobPhase::Diagnostics);

                    let diag_target = resolve_download_test_target(Some(diag_provider), Some(diag_size));
                    match diag_target {
                        Ok(target) => {
                            let job_id_diag = job.state.job_id.clone();
                            let cancel_flag_diag = cancel_flag.clone();
                            let should_cancel_diag = move || cancel_flag_diag.load(Ordering::SeqCst);

                            // Запускаем тест скачивания
                            let diag_outcome = tokio::task::block_in_place(|| {
                                run_proxy_download_test_with_cancel(
                                    proxy_url.as_str(),
                                    &target,
                                    should_cancel_diag,
                                    |_| {}, // Прогресс диагностики отдельно не репортим в общие поля
                                )
                            });

                            let diag_status = if diag_outcome.cancelled {
                                "cancelled".to_string()
                            } else {
                                diag_outcome.status.clone()
                            };

                            let diag_result = DownloadDiagnosticResult {
                                provider_id: diag_provider.to_string(),
                                file_size_id: diag_size.to_string(),
                                status: diag_status.clone(),
                                error: diag_outcome.error.clone(),
                                downloaded_bytes: diag_outcome.bytes_read as u64,
                                duration_ms: diag_outcome.duration_ms as u64,
                                mbps: diag_outcome.download_mbps.unwrap_or(0.0),
                            };

                            registry.add_download_diagnostic(&job_id_diag, diag_result);

                            let scope_str = match job.state.scope {
                                JobScope::SystemProxy => "system_proxy".to_string(),
                                JobScope::AccountProxy => "account_proxy".to_string(),
                            };
                            let redacted_diag_error = diag_outcome.error.as_ref().map(|e| crate::account_proxy::redact_proxy_url_for_log(e));
                            let _ = storage.insert_proxy_diagnostic_test(&codexmanager_core::storage::ProxyDiagnosticTestInsertInput {
                                scope: scope_str,
                                proxy_profile_id: if job.state.scope == JobScope::SystemProxy { Some(id.clone()) } else { None },
                                account_id: if job.state.scope == JobScope::AccountProxy { Some(id.clone()) } else { None },
                                status: diag_status,
                                provider: diag_provider.to_string(),
                                file_size_id: diag_size.to_string(),
                                downloaded_bytes: Some(diag_outcome.bytes_read as i64),
                                duration_ms: Some(diag_outcome.duration_ms),
                                mbps: diag_outcome.download_mbps,
                                tested_at: codexmanager_core::storage::now_ts(),
                                error: redacted_diag_error,
                            });
                        }
                        Err(err) => {
                            let diag_result = DownloadDiagnosticResult {
                                provider_id: diag_provider.to_string(),
                                file_size_id: diag_size.to_string(),
                                status: "failed".to_string(),
                                error: Some(err.clone()),
                                downloaded_bytes: 0,
                                duration_ms: 0,
                                mbps: 0.0,
                            };
                            registry.add_download_diagnostic(&job.state.job_id, diag_result);

                            let scope_str = match job.state.scope {
                                JobScope::SystemProxy => "system_proxy".to_string(),
                                JobScope::AccountProxy => "account_proxy".to_string(),
                            };
                            let _ = storage.insert_proxy_diagnostic_test(&codexmanager_core::storage::ProxyDiagnosticTestInsertInput {
                                scope: scope_str,
                                proxy_profile_id: if job.state.scope == JobScope::SystemProxy { Some(id.clone()) } else { None },
                                account_id: if job.state.scope == JobScope::AccountProxy { Some(id.clone()) } else { None },
                                status: "failed".to_string(),
                                provider: diag_provider.to_string(),
                                file_size_id: diag_size.to_string(),
                                downloaded_bytes: Some(0),
                                duration_ms: Some(0),
                                mbps: Some(0.0),
                                tested_at: codexmanager_core::storage::now_ts(),
                                error: Some(crate::account_proxy::redact_proxy_url_for_log(&err)),
                            });
                        }
                    }
                }
            }

            // 6. Сохранение финальных результатов
            registry.update_job_phase(&job.state.job_id, JobPhase::Saving);

            if final_status == "cancelled" || should_cancel() {
                registry.update_job_status(&job.state.job_id, JobStatus::Cancelled, Some("Cancelled".to_string()));
                return;
            }

            if let Some(profile_id) = system_profile_id {
                let _ = storage.update_proxy_profile(&ProxyProfileUpdateInput {
                    id: profile_id,
                    name: None,
                    proxy_url: None,
                    enabled: None,
                    status: Some(final_status.to_string()),
                    last_error: Some(final_error.clone().unwrap_or_default()),
                    last_url_latency_ms: final_latency_ms.map(|v| v as i64),
                    last_download_mbps: final_download_mbps,
                    last_upload_mbps: final_upload_mbps,
                    last_tested_at: Some(codexmanager_core::storage::now_ts()),
                    ip: None,
                    country_code: None,
                    country_name: None,
                    region_name: None,
                    city_name: None,
                    asn: None,
                    as_org: None,
                    isp: None,
                    as_domain: None,
                    flag_img_url: None,
                    flag_emoji: None,
                    timezone_id: None,
                    timezone_offset: None,
                    timezone_utc: None,
                    tags_json: None,
                    notes: None,
                });
            } else {
                persist_account_speed_result(
                    &storage,
                    &id,
                    final_status,
                    final_latency_ms.map(|v| v as i64),
                    final_download_mbps,
                    final_upload_mbps,
                    final_error.as_deref(),
                    codexmanager_core::storage::now_ts(),
                );
            }

            // Запись в историю proxy_speed_tests
            let finished_at = codexmanager_core::storage::now_ts();
            let current_state = registry.get_job(&job.state.job_id);
            let (samples_json, download_summary_json, upload_summary_json, observed_ip, observed_country, observed_colo) = if let Some(st) = current_state {
                let samples_map = serde_json::json!({
                    "download": st.download_samples,
                    "upload": st.upload_samples,
                });
                (
                    Some(samples_map.to_string()),
                    st.download_summary.as_ref().and_then(|s| serde_json::to_string(s).ok()),
                    st.upload_summary.as_ref().and_then(|s| serde_json::to_string(s).ok()),
                    st.observed_ip.clone(),
                    st.observed_country.clone(),
                    st.observed_colo.clone(),
                )
            } else {
                (None, None, None, None, None, None)
            };

            let scope_str = match job.state.scope {
                JobScope::SystemProxy => "system_proxy".to_string(),
                JobScope::AccountProxy => "account_proxy".to_string(),
            };

            let max_payload_bytes = match file_size_id.as_deref().unwrap_or("size_25mb") {
                "size_100kb" => Some(100_000),
                "size_1mb" => Some(1_000_000),
                "size_10mb" => Some(10_000_000),
                "size_25mb" => Some(25_000_000),
                "size_100mb" => Some(100_000_000),
                _ => Some(25_000_000),
            };

            let redacted_error = final_error.as_ref().map(|e| crate::account_proxy::redact_proxy_url_for_log(e));

            let _ = storage.insert_proxy_speed_test(&codexmanager_core::storage::ProxySpeedTestInsertInput {
                scope: scope_str,
                proxy_profile_id: if job.state.scope == JobScope::SystemProxy { Some(id.clone()) } else { None },
                account_id: if job.state.scope == JobScope::AccountProxy { Some(id.clone()) } else { None },
                status: final_status.to_string(),
                provider: provider.to_string(),
                observed_ip,
                observed_country,
                observed_colo,
                max_payload_bytes,
                samples_json,
                download_summary_json,
                upload_summary_json,
                started_at: job.state.started_at,
                finished_at,
                error_code: None,
                error: redacted_error,
            });

            registry.update_final_metrics(&job.state.job_id, final_latency_ms, final_download_mbps, final_upload_mbps);

            if final_status == "ok" {
                registry.update_job_status(&job.state.job_id, JobStatus::Completed, None);
            } else {
                registry.update_job_status(&job.state.job_id, JobStatus::Failed, final_error);
            }
        }
        JobParams::CloudflareStyleSpeed {
            id,
            resolved_proxy_url,
            config,
        } => {
            let storage = match open_storage() {
                Some(s) => s,
                None => {
                    registry.update_job_status(&job.state.job_id, JobStatus::Failed, Some("storage unavailable".to_string()));
                    return;
                }
            };

            let (proxy_url, system_profile_id) = if job.state.scope == JobScope::SystemProxy {
                let profile = match storage.find_proxy_profile(&id) {
                    Ok(Some(p)) => p,
                    Ok(None) => {
                        registry.update_job_status(&job.state.job_id, JobStatus::Failed, Some("proxy profile not found".to_string()));
                        return;
                    }
                    Err(err) => {
                        registry.update_job_status(&job.state.job_id, JobStatus::Failed, Some(format!("read proxy profile failed: {err}")));
                        return;
                    }
                };

                let proxy_url = match crate::proxy_registry::validation::normalize_proxy_profile_url(&profile.proxy_url) {
                    Ok(url) => url,
                    Err(err) => {
                        registry.update_job_status(&job.state.job_id, JobStatus::Failed, Some(err));
                        return;
                    }
                };
                (proxy_url, Some(profile.id))
            } else {
                let Some(proxy_url) = resolved_proxy_url else {
                    registry.update_job_status(&job.state.job_id, JobStatus::Failed, Some("account proxy test target is missing resolved proxy URL".to_string()));
                    return;
                };
                (proxy_url, None)
            };

            registry.update_job_phase(&job.state.job_id, JobPhase::Preflight);

            let registry_cf = registry.clone();
            let job_id_cf = job.state.job_id.clone();
            let cancel_flag_cf = cancel_flag.clone();
            let should_cancel_cf = move || cancel_flag_cf.load(Ordering::SeqCst);

            let on_phase = {
                let r = registry_cf.clone();
                let jid = job_id_cf.clone();
                move |phase| {
                    let job_phase = match phase {
                        super::cloudflare_style::runner::CfStylePhase::Preflight => JobPhase::Preflight,
                        super::cloudflare_style::runner::CfStylePhase::Latency => JobPhase::Latency,
                        super::cloudflare_style::runner::CfStylePhase::Download => JobPhase::Download,
                        super::cloudflare_style::runner::CfStylePhase::Upload => JobPhase::Upload,
                        super::cloudflare_style::runner::CfStylePhase::Done => JobPhase::Done,
                    };
                    r.update_job_phase(&jid, job_phase);
                }
            };

            let on_download_progress = {
                let r = registry_cf.clone();
                let jid = job_id_cf.clone();
                move |bytes, mbps| r.update_download_progress(&jid, bytes, Some(mbps))
            };

            let on_upload_progress = {
                let r = registry_cf.clone();
                let jid = job_id_cf.clone();
                move |bytes, mbps| r.update_upload_progress(&jid, bytes, Some(mbps))
            };

            let outcome = super::cloudflare_style::runner::run_cf_style_speed_test(
                &proxy_url,
                &config,
                should_cancel_cf,
                on_phase,
                on_download_progress,
                on_upload_progress,
            ).await;

            registry_cf.update_cf_style_result(&job_id_cf, outcome.clone());

            let final_status = match outcome.status {
                super::cloudflare_style::model::CfStyleStatus::Ok => "ok",
                super::cloudflare_style::model::CfStyleStatus::Partial => "partial",
                super::cloudflare_style::model::CfStyleStatus::Failed => "failed",
                super::cloudflare_style::model::CfStyleStatus::Timeout => "timeout",
                super::cloudflare_style::model::CfStyleStatus::Cancelled => "cancelled",
            };

            let final_error = outcome.errors.first().map(|e| e.message.clone());
            let final_download_mbps = outcome.download.as_ref().map(|d| d.final_mbps);
            let final_upload_mbps = outcome.upload.as_ref().map(|u| u.final_mbps);
            let final_latency_ms = outcome.latency.as_ref().map(|l| l.median_ms as i64);

            if final_status == "cancelled" || should_cancel() {
                registry.update_job_status(&job.state.job_id, JobStatus::Cancelled, Some("Cancelled".to_string()));
                return;
            }

            registry.update_job_phase(&job.state.job_id, JobPhase::Saving);

            if let Some(profile_id) = system_profile_id {
                let _ = storage.update_proxy_profile(&ProxyProfileUpdateInput {
                    id: profile_id,
                    name: None,
                    proxy_url: None,
                    enabled: None,
                    status: Some(final_status.to_string()),
                    last_error: Some(final_error.clone().unwrap_or_default()),
                    last_url_latency_ms: final_latency_ms,
                    last_download_mbps: final_download_mbps,
                    last_upload_mbps: final_upload_mbps,
                    last_tested_at: Some(codexmanager_core::storage::now_ts()),
                    ip: outcome.endpoint_info.observed_ip.clone(),
                    country_code: outcome.endpoint_info.observed_country.clone(),
                    country_name: None,
                    region_name: None,
                    city_name: None,
                    asn: None,
                    as_org: None,
                    isp: None,
                    as_domain: None,
                    flag_img_url: None,
                    flag_emoji: None,
                    timezone_id: None,
                    timezone_offset: None,
                    timezone_utc: None,
                    tags_json: None,
                    notes: None,
                });
            } else {
                persist_account_speed_result(
                    &storage,
                    &id,
                    final_status,
                    final_latency_ms,
                    final_download_mbps,
                    final_upload_mbps,
                    final_error.as_deref(),
                    codexmanager_core::storage::now_ts(),
                );
            }

            let finished_at = codexmanager_core::storage::now_ts();
            let scope_str = match job.state.scope {
                JobScope::SystemProxy => "system_proxy".to_string(),
                JobScope::AccountProxy => "account_proxy".to_string(),
            };

            let max_payload_bytes = {
                let dl_bytes = config.get_download_preset().max_bytes(false);
                let ul_bytes = if config.should_run_upload() {
                    config.get_upload_preset().max_bytes(true)
                } else {
                    0
                };
                Some(std::cmp::max(dl_bytes, ul_bytes) as i64)
            };


            let samples_map = serde_json::json!({
                "download": outcome.download.as_ref().map(|d| {
                    d.runs.iter().map(|r| {
                        serde_json::json!({
                            "payloadBytes": r.payload_bytes,
                            "durationMs": r.total_duration_ms,
                            "mbps": r.adjusted_mbps,
                        })
                    }).collect::<Vec<_>>()
                }).unwrap_or_default(),
                "upload": outcome.upload.as_ref().map(|u| {
                    u.runs.iter().map(|r| {
                        serde_json::json!({
                            "payloadBytes": r.payload_bytes,
                            "durationMs": r.total_duration_ms,
                            "mbps": r.adjusted_mbps,
                        })
                    }).collect::<Vec<_>>()
                }).unwrap_or_default(),
            });

            let download_summary_json = outcome.download.as_ref().map(|d| {
                serde_json::json!({
                    "median": d.median_mbps,
                    "average": d.avg_mbps,
                    "p90": d.p90_mbps,
                    "best": d.max_mbps,
                }).to_string()
            });

            let upload_summary_json = outcome.upload.as_ref().map(|u| {
                serde_json::json!({
                    "median": u.median_mbps,
                    "average": u.avg_mbps,
                    "p90": u.p90_mbps,
                    "best": u.max_mbps,
                }).to_string()
            });

            let redacted_error = final_error.as_ref().map(|e| crate::account_proxy::redact_proxy_url_for_log(e));

            let _ = storage.insert_proxy_speed_test(&codexmanager_core::storage::ProxySpeedTestInsertInput {
                scope: scope_str,
                proxy_profile_id: if job.state.scope == JobScope::SystemProxy { Some(id.clone()) } else { None },
                account_id: if job.state.scope == JobScope::AccountProxy { Some(id.clone()) } else { None },
                status: final_status.to_string(),
                provider: "cloudflare_style".to_string(),
                observed_ip: outcome.endpoint_info.observed_ip.clone(),
                observed_country: outcome.endpoint_info.observed_country.clone(),
                observed_colo: outcome.endpoint_info.observed_colo.clone(),
                max_payload_bytes,
                samples_json: Some(samples_map.to_string()),
                download_summary_json,
                upload_summary_json,
                started_at: job.state.started_at,
                finished_at,
                error_code: None,
                error: redacted_error,
            });

            if let Some(ref d) = outcome.download {
                for run in &d.runs {
                    registry_cf.add_download_sample(&job_id_cf, SpeedSample {
                        payload_bytes: run.payload_bytes,
                        duration_ms: run.total_duration_ms,
                        mbps: run.adjusted_mbps,
                    });
                }
                registry_cf.jobs.write().unwrap().get_mut(&job_id_cf).map(|j| {
                    j.state.download_summary = Some(SpeedMetricSummary {
                        median: d.median_mbps,
                        average: d.avg_mbps,
                        p90: d.p90_mbps,
                        best: d.max_mbps,
                    });
                });
            }

            if let Some(ref u) = outcome.upload {
                for run in &u.runs {
                    registry_cf.add_upload_sample(&job_id_cf, SpeedSample {
                        payload_bytes: run.payload_bytes,
                        duration_ms: run.total_duration_ms,
                        mbps: run.adjusted_mbps,
                    });
                }
                registry_cf.jobs.write().unwrap().get_mut(&job_id_cf).map(|j| {
                    j.state.upload_summary = Some(SpeedMetricSummary {
                        median: u.median_mbps,
                        average: u.avg_mbps,
                        p90: u.p90_mbps,
                        best: u.max_mbps,
                    });
                });
            }

            registry.update_final_metrics(
                &job.state.job_id,
                final_latency_ms.map(|v| v as u64),
                final_download_mbps,
                final_upload_mbps,
            );

            if final_status == "ok" {
                registry.update_job_status(&job.state.job_id, JobStatus::Completed, None);
            } else {
                registry.update_job_status(&job.state.job_id, JobStatus::Failed, final_error);
            }
        }
    }
}

fn persist_account_latency_outcome(
    storage: &codexmanager_core::storage::Storage,
    account_id: &str,
    outcome: &super::latency::ProxyLatencyTestOutcome,
) {
    let (last_download_mbps, last_upload_mbps) = storage
        .find_account_proxy_settings(account_id)
        .ok()
        .flatten()
        .map(|settings| (settings.last_download_mbps, settings.last_upload_mbps))
        .unwrap_or((None, None));

    let _ = storage.update_account_proxy_test_result(
        account_id,
        outcome.status.as_str(),
        outcome.url_latency_ms,
        last_download_mbps,
        last_upload_mbps,
        Some(outcome.tested_at),
        outcome.error.as_deref(),
    );

    let redacted_test_url = crate::account_proxy::redact_proxy_url_for_log(&outcome.test_url);
    let redacted_final_url = outcome.final_url.as_ref().map(|u| crate::account_proxy::redact_proxy_url_for_log(u));
    let redacted_error = outcome.error.as_ref().map(|e| crate::account_proxy::redact_proxy_url_for_log(e));

    let _ = storage.insert_account_proxy_url_test(&codexmanager_core::storage::AccountProxyUrlTestInsertInput {
        account_id: account_id.to_string(),
        status: outcome.status.clone(),
        url_latency_ms: outcome.url_latency_ms,
        status_code: outcome.status_code,
        test_url: redacted_test_url,
        final_url: redacted_final_url,
        redirected: outcome.redirected,
        tested_at: outcome.tested_at,
        error_code: outcome.error_code.clone(),
        error: redacted_error,
    });
}

fn persist_account_speed_result(
    storage: &codexmanager_core::storage::Storage,
    account_id: &str,
    status: &str,
    latency_ms: Option<i64>,
    last_download_mbps: Option<f64>,
    last_upload_mbps: Option<f64>,
    last_error: Option<&str>,
    tested_at: i64,
) {
    let _ = storage.update_account_proxy_test_result(
        account_id,
        status,
        latency_ms,
        last_download_mbps,
        last_upload_mbps,
        Some(tested_at),
        last_error,
    );
}
