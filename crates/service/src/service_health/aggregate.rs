//! Aggregate this deployment's bounded metadata; results are not an upstream SLA.
use super::{reports, store, types::*};
use codexmanager_core::storage::ServiceHealthLogSample;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
struct Counters {
    samples: u64,
    successes: u64,
    rate_limited: u64,
    upstream_errors: u64,
    local_errors: u64,
    network_errors: u64,
    unknown_errors: u64,
    latencies: Vec<u64>,
    reporters: u64,
    regions: BTreeSet<Region>,
}

fn add_sample(counts: &mut Counters, sample: &ServiceHealthLogSample) {
    counts.samples += 1;
    let status = sample.status_code.unwrap_or(0);
    if (200..300).contains(&status) && !sample.has_error {
        counts.successes += 1;
    } else if status == 499 || matches!(status, 400..=428 | 430..=498) {
        counts.local_errors += 1;
    } else if sample.network_error {
        counts.network_errors += 1;
    } else if !sample.upstream_present && status >= 400 {
        counts.local_errors += 1;
    } else if status == 429 && sample.upstream_present {
        counts.rate_limited += 1;
    } else if (500..600).contains(&status)
        && sample.confirmed_upstream_error
        && sample.upstream_present
    {
        counts.upstream_errors += 1;
    } else {
        counts.unknown_errors += 1;
    }
    if (200..300).contains(&status) && !sample.has_error {
        if let Some(ms) = sample.first_response_ms.filter(|ms| *ms >= 0) {
            counts.latencies.push(ms as u64);
        }
    }
}

fn provider_summary(provider: Provider, mut counts: Counters) -> ProviderSummary {
    let status = if provider == Provider::Unknown {
        Status::Unknown
    } else if counts.reporters > 0
        || (counts.samples >= 5 && (counts.samples - counts.successes) * 5 >= counts.samples)
    {
        Status::Degraded
    } else if counts.samples >= 5 {
        Status::Healthy
    } else {
        Status::Unknown
    };
    counts.latencies.sort_unstable();
    let n = counts.latencies.len();
    let average = (n > 0).then(|| {
        counts
            .latencies
            .iter()
            .map(|v| u128::from(*v))
            .sum::<u128>() as f64
            / n as f64
    });
    let p95 = (n > 0).then(|| counts.latencies[(n * 95).div_ceil(100) - 1]);
    ProviderSummary {
        provider,
        status,
        sample_count: counts.samples,
        success_count: counts.successes,
        rate_limited_count: counts.rate_limited,
        upstream_error_count: counts.upstream_errors,
        local_error_count: counts.local_errors,
        network_error_count: counts.network_errors,
        unknown_error_count: counts.unknown_errors,
        availability_percent: (counts.samples > 0)
            .then(|| counts.successes as f64 * 100.0 / counts.samples as f64),
        avg_first_response_ms: average,
        p95_first_response_ms: p95,
        active_report_count: counts.reporters,
        reporter_count: counts.reporters,
        source_count: usize::from(counts.samples > 0 || counts.reporters > 0),
        regions: counts.regions.into_iter().collect(),
    }
}

pub fn summary(now: i64) -> Result<Summary, String> {
    store::prune(now)?;
    let mut samples = store::samples(now)?;
    let sampled = samples.len() > SAMPLE_LIMIT;
    samples.truncate(SAMPLE_LIMIT);
    let mut counters: BTreeMap<Provider, Counters> = BTreeMap::new();
    for sample in samples {
        let provider = match sample.provider.as_str() {
            "codex" => Provider::Codex,
            "claude" => Provider::Claude,
            _ => Provider::Unknown,
        };
        add_sample(counters.entry(provider).or_default(), &sample);
    }
    // Only the latest active vote per user/provider counts, even across regions.
    let mut seen = BTreeSet::new();
    let mut regions = BTreeSet::new();
    for (owner, report) in reports::active(now)? {
        if !seen.insert((owner, report.provider)) {
            continue;
        }
        let counts = counters.entry(report.provider).or_default();
        counts.reporters += 1;
        if report.region != Region::Unknown {
            counts.regions.insert(report.region);
            regions.insert(report.region);
        }
    }
    let providers: Vec<_> = [Provider::Codex, Provider::Claude, Provider::Unknown]
        .into_iter()
        .map(|provider| provider_summary(provider, counters.remove(&provider).unwrap_or_default()))
        .collect();
    let sample_count = providers.iter().map(|p| p.sample_count).sum();
    let source_count = usize::from(providers.iter().any(|p| p.source_count > 0));
    Ok(Summary {
        source: Source::Local,
        coverage: Coverage::ThisDeployment,
        window_start: now - WINDOW_SECS,
        window_end: now,
        updated_at: now,
        sampled,
        sample_limit: SAMPLE_LIMIT,
        sample_count,
        source_count,
        region_count: regions.len(),
        scope: Scope::InsufficientData,
        providers,
        my_reports: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample(code: i64) -> ServiceHealthLogSample {
        ServiceHealthLogSample {
            provider: "codex".into(),
            status_code: Some(code),
            first_response_ms: None,
            has_error: code >= 400,
            upstream_present: true,
            confirmed_upstream_error: false,
            network_error: false,
            created_at: 100,
        }
    }
    #[test]
    fn ambiguous_and_local_errors_are_not_attributed_to_provider() {
        let mut counts = Counters::default();
        add_sample(&mut counts, &sample(502));
        assert_eq!(counts.unknown_errors, 1);
        assert_eq!(counts.upstream_errors, 0);
        for code in [400, 401, 403, 404, 422, 499] {
            add_sample(&mut counts, &sample(code));
        }
        assert_eq!(counts.local_errors, 6);
        add_sample(&mut counts, &sample(429));
        assert_eq!(counts.rate_limited, 1);
    }
    #[test]
    fn unknown_failures_are_in_sample_success_rate() {
        let mut counts = Counters::default();
        for _ in 0..5 {
            add_sample(&mut counts, &sample(200));
        }
        for _ in 0..95 {
            add_sample(&mut counts, &sample(502));
        }
        let result = provider_summary(Provider::Codex, counts);
        assert_eq!(result.availability_percent, Some(5.0));
        assert!(matches!(result.status, Status::Degraded));
        assert_eq!(result.upstream_error_count, 0);
    }
    #[test]
    fn no_samples_is_unknown_and_latency_uses_exact_percentile() {
        let empty = provider_summary(Provider::Codex, Counters::default());
        assert!(matches!(empty.status, Status::Unknown));
        assert_eq!(empty.availability_percent, None);
        let counts = Counters {
            samples: 20,
            successes: 20,
            latencies: (1..=20).collect(),
            ..Default::default()
        };
        let result = provider_summary(Provider::Codex, counts);
        assert_eq!(result.p95_first_response_ms, Some(19));
        assert_eq!(result.avg_first_response_ms, Some(10.5));
    }
}
