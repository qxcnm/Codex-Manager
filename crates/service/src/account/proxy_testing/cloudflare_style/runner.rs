use std::time::Instant;

use chrono::Utc;

use super::config::CfStyleConfig;
use super::download::run_download;
use super::latency::run_latency;
use super::model::*;
use super::upload::run_upload;

use crate::account::proxy_testing::client::{build_proxy_test_client, ProxyTestRedirectPolicy};


/// Progress callback phases for UI integration.
#[derive(Debug, Clone, Copy)]
pub(crate) enum CfStylePhase {
    Preflight,
    Latency,
    Download,
    Upload,
    Done,
}

/// Main entry point for the Cloudflare-style proxy speed test.
///
/// Phases: Preflight → Latency → Download → Upload → Done.
/// Uses the real Codex-Manager proxy transport via `build_proxy_test_client`.
pub(crate) async fn run_cf_style_speed_test(
    proxy_url: &str,
    config: &CfStyleConfig,
    should_cancel: impl Fn() -> bool + Send + Sync,
    on_phase: impl Fn(CfStylePhase) + Send + Sync,
    on_download_progress: impl Fn(u64, f64) + Send + Sync,
    on_upload_progress: impl Fn(u64, f64) + Send + Sync,
) -> CfStyleResult {
    let overall_start = Instant::now();
    let started_at = Utc::now().to_rfc3339();

    let mut errors = Vec::new();
    let mut endpoint_info = CfStyleEndpointInfo {
        observed_ip: None,
        observed_country: None,
        observed_colo: None,
    };

    log::info!(
        "cf_style speed test: starting for proxy={}, download_preset={:?}, upload_preset={:?}",
        crate::account_proxy::redact_proxy_url_for_log(proxy_url),
        config.download_preset,
        config.upload_preset
    );

    // Build proxy-aware HTTP client (reuses existing infrastructure)
    let (client, context) = match build_proxy_test_client(
        proxy_url,
        ProxyTestRedirectPolicy::Limited(5),
        false,
    ) {
        Ok(res) => res,
        Err(err) => {
            log::error!("cf_style speed test: failed to build proxy client: {}", err);
            return CfStyleResult {
                status: CfStyleStatus::Failed,
                latency: None,
                download: None,
                upload: None,
                used_proxy: None,
                endpoint_info,
                started_at,
                finished_at: Utc::now().to_rfc3339(),
                duration_ms: overall_start.elapsed().as_millis() as u64,
                errors: vec![CfStyleSpeedTestError {
                    phase: "init".to_string(),
                    message: format!("failed to build proxy client: {}", err),
                }],
            };
        }
    };

    // Document DNS behavior based on proxy scheme
    let dns_note = match context.parsed_proxy_url.scheme() {
        "socks5h" => "proxy-side DNS (socks5h)",
        "socks5" => "local DNS (socks5)",
        _ => "local DNS (HTTP CONNECT)",
    };

    let used_proxy = Some(CfStyleUsedProxy {
        proxy_url_redacted: context.proxy_url_redacted.clone(),
        proxy_scheme: context.parsed_proxy_url.scheme().to_string(),
        dns_note: dns_note.to_string(),
    });

    // ── Preflight ──────────────────────────────────────────────────

    on_phase(CfStylePhase::Preflight);
    if !should_cancel() {
        log::info!("cf_style speed test: running preflight...");
        match do_preflight(&client, &config.base_url).await {
            Ok(info) => {
                log::info!(
                    "cf_style speed test: preflight ok. IP={}, country={}, colo={}",
                    info.observed_ip.as_deref().unwrap_or("?"),
                    info.observed_country.as_deref().unwrap_or("?"),
                    info.observed_colo.as_deref().unwrap_or("?")
                );
                endpoint_info = info;
            }
            Err(err) => {
                log::warn!("cf_style speed test: preflight failed: {}", err);
                errors.push(CfStyleSpeedTestError {
                    phase: "preflight".to_string(),
                    message: err,
                });
            }
        }
    }

    if should_cancel() {
        return build_cancelled_result(started_at, overall_start, endpoint_info, used_proxy, errors);
    }

    // ── Latency ────────────────────────────────────────────────────

    let dl_preset = config.get_download_preset();
    let ul_preset = config.get_upload_preset();
    let latency_samples = std::cmp::max(
        dl_preset.latency_samples(),
        ul_preset.latency_samples(),
    );

    on_phase(CfStylePhase::Latency);
    log::info!(
        "cf_style speed test: running latency ({} samples)...",
        latency_samples
    );
    let latency = match run_latency(&client, &config.base_url, latency_samples, &should_cancel).await {
        Ok(result) => {
            log::info!(
                "cf_style speed test: latency done. median={:.1}ms, jitter={:.1}ms",
                result.median_ms,
                result.jitter_ms
            );
            Some(result)
        }
        Err(err) => {
            log::warn!("cf_style speed test: latency failed: {}", err);
            errors.push(CfStyleSpeedTestError {
                phase: "latency".to_string(),
                message: err,
            });
            None
        }
    };

    if should_cancel() {
        return build_cancelled_result(started_at, overall_start, endpoint_info, used_proxy, errors);
    }

    // ── Download ───────────────────────────────────────────────────

    let dl_max_bytes = dl_preset.max_bytes(false);
    on_phase(CfStylePhase::Download);
    log::info!(
        "cf_style speed test: running download (max_payload={}, tests_per_payload=1)...",
        dl_max_bytes
    );
    let dl_result = run_download(
        &client,
        &config.base_url,
        dl_preset,
        dl_max_bytes,
        1,
        &should_cancel,
        &on_download_progress,
    )
    .await;

    let download = build_throughput_result(
        ThroughputDirection::Download,
        dl_result.runs,
        dl_result.final_mbps,
    );

    if should_cancel() {
        return build_cancelled_result(started_at, overall_start, endpoint_info, used_proxy, errors);
    }

    // ── Upload ─────────────────────────────────────────────────────

    let ul_max_bytes = ul_preset.max_bytes(true);
    let upload = if config.should_run_upload() {
        on_phase(CfStylePhase::Upload);
        log::info!(
            "cf_style speed test: running upload (max_payload={}, tests_per_payload=1)...",
            ul_max_bytes
        );
        let ul_result = run_upload(
            &client,
            &config.base_url,
            ul_preset,
            ul_max_bytes,
            1,
            &should_cancel,
            &on_upload_progress,
        )
        .await;

        build_throughput_result(
            ThroughputDirection::Upload,
            ul_result.runs,
            ul_result.final_mbps,
        )
    } else {
        None
    };

    // ── Done ───────────────────────────────────────────────────────

    on_phase(CfStylePhase::Done);

    let status = if errors.is_empty() {
        CfStyleStatus::Ok
    } else if download.is_some() {
        CfStyleStatus::Partial
    } else {
        CfStyleStatus::Failed
    };

    let result = CfStyleResult {
        status,
        latency,
        download,
        upload,
        used_proxy,
        endpoint_info,
        started_at,
        finished_at: Utc::now().to_rfc3339(),
        duration_ms: overall_start.elapsed().as_millis() as u64,
        errors,
    };

    log::info!(
        "cf_style speed test: completed. status={:?}, duration={}ms",
        result.status,
        result.duration_ms
    );

    result
}

/// Build aggregated throughput result from individual runs.
fn build_throughput_result(
    direction: ThroughputDirection,
    runs: Vec<CfStyleThroughputRun>,
    final_mbps: f64,
) -> Option<CfStyleThroughputResult> {
    if runs.is_empty() {
        return None;
    }

    let ok_runs: Vec<&CfStyleThroughputRun> = runs
        .iter()
        .filter(|r| matches!(r.status, CfStyleRunStatus::Ok))
        .collect();

    if ok_runs.is_empty() {
        return None;
    }

    let raw_values: Vec<f64> = ok_runs.iter().map(|r| r.raw_mbps).collect();
    let adj_values: Vec<f64> = ok_runs.iter().map(|r| r.adjusted_mbps).collect();

    let mut raw_sorted = raw_values.clone();
    raw_sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let raw_final = super::stats::percentile(&raw_sorted, 0.9).unwrap_or(0.0);

    let mut adj_sorted = adj_values.clone();
    adj_sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let adj_final = super::stats::percentile(&adj_sorted, 0.9).unwrap_or(0.0);

    let avg = super::stats::average(&adj_values).unwrap_or(0.0);
    let median_val = {
        let mut s = adj_values.clone();
        s.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        super::stats::median(&s).unwrap_or(0.0)
    };
    let p90 = adj_final;
    let max = adj_values
        .iter()
        .cloned()
        .fold(0.0_f64, f64::max);

    let total_bytes: u64 = ok_runs.iter().map(|r| r.transferred_bytes).sum();
    let total_duration_ms: u64 = ok_runs.iter().map(|r| r.total_duration_ms).sum();

    Some(CfStyleThroughputResult {
        direction,
        runs,
        final_mbps,
        raw_final_mbps: raw_final,
        adjusted_final_mbps: adj_final,
        avg_mbps: avg,
        median_mbps: median_val,
        p90_mbps: p90,
        max_mbps: max,
        total_bytes,
        total_duration_ms,
    })
}

fn build_cancelled_result(
    started_at: String,
    overall_start: Instant,
    endpoint_info: CfStyleEndpointInfo,
    used_proxy: Option<CfStyleUsedProxy>,
    errors: Vec<CfStyleSpeedTestError>,
) -> CfStyleResult {
    CfStyleResult {
        status: CfStyleStatus::Cancelled,
        latency: None,
        download: None,
        upload: None,
        used_proxy,
        endpoint_info,
        started_at,
        finished_at: Utc::now().to_rfc3339(),
        duration_ms: overall_start.elapsed().as_millis() as u64,
        errors,
    }
}

async fn do_preflight(client: &reqwest::Client, base_url: &str) -> Result<CfStyleEndpointInfo, String> {
    let meta_url = format!("{}/meta", base_url);
    let trace_url = format!("{}/cdn-cgi/trace", base_url);

    // Try /meta first (returns JSON with IP, country, colo)
    match client
        .get(&meta_url)
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => {
            if let Ok(info) = resp.json::<serde_json::Value>().await {
                return Ok(CfStyleEndpointInfo {
                    observed_ip: info["clientIp"].as_str().map(String::from),
                    observed_country: info["country"].as_str().map(String::from),
                    observed_colo: info["colo"].as_str().map(String::from),
                });
            }
        }
        _ => {}
    }

    // Fallback to /cdn-cgi/trace (returns key=value text)
    log::info!("cf_style preflight: /meta failed, trying /cdn-cgi/trace fallback...");
    match client.get(&trace_url).send().await {
        Ok(resp) if resp.status().is_success() => {
            let text = resp
                .text()
                .await
                .map_err(|e| format!("read trace body: {e}"))?;
            let mut ip = None;
            let mut country = None;
            let mut colo = None;
            for line in text.lines() {
                if let Some((k, v)) = line.split_once('=') {
                    match k {
                        "ip" => ip = Some(v.to_string()),
                        "loc" => country = Some(v.to_string()),
                        "colo" => colo = Some(v.to_string()),
                        _ => {}
                    }
                }
            }
            Ok(CfStyleEndpointInfo {
                observed_ip: ip,
                observed_country: country,
                observed_colo: colo,
            })
        }
        Ok(resp) => Err(format!("trace HTTP {}", resp.status())),
        Err(err) => Err(format!("trace request failed: {err}")),
    }
}
