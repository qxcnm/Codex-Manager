use std::time::Duration;

/// Summary statistics computed from a set of raw samples.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct StatsSummary {
    pub min: f64,
    pub max: f64,
    pub avg: f64,
    pub median: f64,
    pub p90: f64,
    pub p95: f64,
}

/// Calculate throughput in Mbps (decimal: 1 Mbps = 1_000_000 bits/s).
///
/// Formula: `bytes * 8 / seconds / 1_000_000`.
/// Returns 0.0 if duration is zero.
pub(crate) fn calculate_mbps(bytes: u64, duration: Duration) -> f64 {
    let secs = duration.as_secs_f64();
    if secs == 0.0 {
        return 0.0;
    }
    (bytes as f64) * 8.0 / secs / 1_000_000.0
}

/// Calculate the median of a **sorted** slice.
///
/// Returns `None` for an empty slice.
/// For even-length slices the median is the average of the two middle values.
pub(crate) fn median(sorted: &[f64]) -> Option<f64> {
    let n = sorted.len();
    if n == 0 {
        return None;
    }
    if n % 2 == 1 {
        Some(sorted[n / 2])
    } else {
        Some((sorted[n / 2 - 1] + sorted[n / 2]) / 2.0)
    }
}

/// Calculate the `p`-th percentile (0.0–1.0) from a **sorted** slice.
///
/// Uses ceiling interpolation: `index = ceil(p * n) - 1`, clamped to valid
/// range.  Returns `None` for an empty slice.
pub(crate) fn percentile(sorted: &[f64], p: f64) -> Option<f64> {
    let n = sorted.len();
    if n == 0 {
        return None;
    }
    let idx = ((p * n as f64).ceil() as usize).saturating_sub(1).min(n - 1);
    Some(sorted[idx])
}

/// Calculate the arithmetic mean.  Returns `None` for an empty slice.
pub(crate) fn average(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    Some(values.iter().sum::<f64>() / values.len() as f64)
}

/// Calculate jitter: mean absolute difference between consecutive samples.
///
/// Input is in the **original** (unsorted) order.
/// Returns `0.0` when there are fewer than two samples.
pub(crate) fn jitter(samples: &[f64]) -> f64 {
    if samples.len() < 2 {
        return 0.0;
    }
    let sum: f64 = samples
        .windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .sum();
    sum / (samples.len() - 1) as f64
}

/// Sort the raw samples and compute summary statistics.
///
/// Returns `None` if the input is empty.
pub(crate) fn compute_summary(raw: &[f64]) -> Option<StatsSummary> {
    if raw.is_empty() {
        return None;
    }
    let mut sorted = raw.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    Some(StatsSummary {
        min: sorted[0],
        max: sorted[sorted.len() - 1],
        avg: average(&sorted).unwrap_or(0.0),
        median: median(&sorted).unwrap_or(0.0),
        p90: percentile(&sorted, 0.90).unwrap_or(0.0),
        p95: percentile(&sorted, 0.95).unwrap_or(0.0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    // ── calculate_mbps ──────────────────────────────────────────────

    #[test]
    fn mbps_zero_bytes() {
        assert_eq!(calculate_mbps(0, Duration::from_secs(1)), 0.0);
    }

    #[test]
    fn mbps_zero_duration() {
        assert_eq!(calculate_mbps(1_000_000, Duration::ZERO), 0.0);
    }

    #[test]
    fn mbps_normal_case() {
        // 1 MB in 1 second => 8 Mbps
        let result = calculate_mbps(1_000_000, Duration::from_secs(1));
        assert!((result - 8.0).abs() < 1e-9, "expected 8.0, got {result}");
    }

    #[test]
    fn mbps_large_values() {
        // 1 GB in 10 seconds => 800 Mbps
        let result = calculate_mbps(1_000_000_000, Duration::from_secs(10));
        assert!((result - 800.0).abs() < 1e-6, "expected 800.0, got {result}");
    }

    // ── median ──────────────────────────────────────────────────────

    #[test]
    fn median_empty() {
        assert!(median(&[]).is_none());
    }

    #[test]
    fn median_one_element() {
        assert_eq!(median(&[5.0]), Some(5.0));
    }

    #[test]
    fn median_two_elements() {
        assert_eq!(median(&[1.0, 3.0]), Some(2.0));
    }

    #[test]
    fn median_odd_count() {
        assert_eq!(median(&[1.0, 2.0, 3.0]), Some(2.0));
    }

    #[test]
    fn median_even_count() {
        assert_eq!(median(&[1.0, 2.0, 3.0, 4.0]), Some(2.5));
    }

    // ── percentile ──────────────────────────────────────────────────

    #[test]
    fn percentile_empty() {
        assert!(percentile(&[], 0.5).is_none());
    }

    #[test]
    fn percentile_single_element() {
        assert_eq!(percentile(&[42.0], 0.5), Some(42.0));
        assert_eq!(percentile(&[42.0], 0.95), Some(42.0));
    }

    #[test]
    fn percentile_p50_matches_median() {
        let data: Vec<f64> = (1..=10).map(|x| x as f64).collect();
        let p50 = percentile(&data, 0.5).unwrap();
        let med = median(&data).unwrap();
        // ceiling interpolation p50 may differ slightly from standard median
        // for even-length arrays; ensure they are close.
        assert!(
            (p50 - med).abs() <= 1.0,
            "p50={p50}, median={med} should be close"
        );
    }

    #[test]
    fn percentile_p90() {
        let data: Vec<f64> = (1..=10).map(|x| x as f64).collect();
        let p90 = percentile(&data, 0.90).unwrap();
        assert_eq!(p90, 9.0);
    }

    #[test]
    fn percentile_p95() {
        let data: Vec<f64> = (1..=20).map(|x| x as f64).collect();
        let p95 = percentile(&data, 0.95).unwrap();
        assert_eq!(p95, 19.0);
    }

    // ── average ─────────────────────────────────────────────────────

    #[test]
    fn average_empty() {
        assert!(average(&[]).is_none());
    }

    #[test]
    fn average_single() {
        assert_eq!(average(&[7.0]), Some(7.0));
    }

    #[test]
    fn average_multiple() {
        let result = average(&[2.0, 4.0, 6.0]).unwrap();
        assert!((result - 4.0).abs() < 1e-9);
    }

    // ── jitter ──────────────────────────────────────────────────────

    #[test]
    fn jitter_empty() {
        assert_eq!(jitter(&[]), 0.0);
    }

    #[test]
    fn jitter_single() {
        assert_eq!(jitter(&[5.0]), 0.0);
    }

    #[test]
    fn jitter_two_samples() {
        assert!((jitter(&[1.0, 4.0]) - 3.0).abs() < 1e-9);
    }

    #[test]
    fn jitter_ascending() {
        // [1, 2, 3, 4] => diffs [1,1,1] => mean 1.0
        assert!((jitter(&[1.0, 2.0, 3.0, 4.0]) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn jitter_descending() {
        // [4, 3, 2, 1] => diffs [1,1,1] => mean 1.0
        assert!((jitter(&[4.0, 3.0, 2.0, 1.0]) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn jitter_alternating() {
        // [0, 10, 0, 10] => diffs [10, 10, 10] => mean 10.0
        assert!((jitter(&[0.0, 10.0, 0.0, 10.0]) - 10.0).abs() < 1e-9);
    }

    // ── compute_summary ─────────────────────────────────────────────

    #[test]
    fn compute_summary_empty() {
        assert!(compute_summary(&[]).is_none());
    }

    #[test]
    fn compute_summary_single_element() {
        let s = compute_summary(&[42.0]).unwrap();
        assert_eq!(s.min, 42.0);
        assert_eq!(s.max, 42.0);
        assert_eq!(s.avg, 42.0);
        assert_eq!(s.median, 42.0);
        assert_eq!(s.p90, 42.0);
        assert_eq!(s.p95, 42.0);
    }

    #[test]
    fn compute_summary_normal_case() {
        let raw = vec![5.0, 1.0, 3.0, 9.0, 7.0, 2.0, 8.0, 4.0, 6.0, 10.0];
        let s = compute_summary(&raw).unwrap();
        assert_eq!(s.min, 1.0);
        assert_eq!(s.max, 10.0);
        assert!((s.avg - 5.5).abs() < 1e-9);
        assert!((s.median - 5.5).abs() < 1e-9);
        assert_eq!(s.p90, 9.0);
        assert_eq!(s.p95, 10.0);
    }
}
