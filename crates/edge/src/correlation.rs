use std::collections::HashMap;
use ts_core::Bar;

/// Minimum number of aligned return observations required before a
/// correlation estimate is considered meaningful.
const MIN_OBSERVATIONS: usize = 5;
const SECONDS_PER_DAY: f64 = 86_400.0;

/// Computes the Pearson correlation of log returns between two bar series,
/// restricted to their trailing `window_days` (ending at the earlier of the
/// two series' last timestamp). Bars are aligned by exact timestamp match, so
/// both series should share the same timeframe/grid.
///
/// Returns `None` if there isn't enough overlapping data to form a reliable
/// estimate.
pub fn rolling_pearson_correlation(a: &[Bar], b: &[Bar], window_days: f64) -> Option<f64> {
    let a_last = a.last()?.time;
    let b_last = b.last()?.time;
    let end = a_last.min(b_last);
    let start = end - (window_days * SECONDS_PER_DAY) as i64;

    let b_map: HashMap<i64, f64> = b
        .iter()
        .filter(|bar| bar.time >= start && bar.time <= end)
        .map(|bar| (bar.time, bar.close))
        .collect();

    let mut aligned: Vec<(f64, f64)> = Vec::new();
    for bar in a.iter().filter(|bar| bar.time >= start && bar.time <= end) {
        if let Some(&b_close) = b_map.get(&bar.time) {
            aligned.push((bar.close, b_close));
        }
    }

    let mut ra = Vec::with_capacity(aligned.len());
    let mut rb = Vec::with_capacity(aligned.len());
    for w in aligned.windows(2) {
        let (a0, b0) = w[0];
        let (a1, b1) = w[1];
        if a0 > 0.0 && a1 > 0.0 && b0 > 0.0 && b1 > 0.0 {
            ra.push((a1 / a0).ln());
            rb.push((b1 / b0).ln());
        }
    }

    if ra.len() < MIN_OBSERVATIONS {
        return None;
    }
    Some(pearson(&ra, &rb))
}

fn pearson(x: &[f64], y: &[f64]) -> f64 {
    let n = x.len() as f64;
    let mx = x.iter().sum::<f64>() / n;
    let my = y.iter().sum::<f64>() / n;
    let mut cov = 0.0;
    let mut vx = 0.0;
    let mut vy = 0.0;
    for i in 0..x.len() {
        let dx = x[i] - mx;
        let dy = y[i] - my;
        cov += dx * dy;
        vx += dx * dx;
        vy += dy * dy;
    }
    if vx <= 0.0 || vy <= 0.0 {
        return 0.0;
    }
    cov / (vx.sqrt() * vy.sqrt())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_bars(times: &[i64], closes: &[f64]) -> Vec<Bar> {
        times
            .iter()
            .zip(closes.iter())
            .map(|(&t, &c)| Bar::new(t, c, c, c, c, 100.0))
            .collect()
    }

    #[test]
    fn test_identical_series_correlate_perfectly() {
        let times: Vec<i64> = (0..60).map(|i| i * 86_400).collect();
        let closes: Vec<f64> = (0..60).map(|i| 100.0 + i as f64).collect();
        let a = make_bars(&times, &closes);
        let b = a.clone();

        let corr = rolling_pearson_correlation(&a, &b, 90.0).unwrap();
        assert!((corr - 1.0).abs() < 1e-9);
    }

    #[test]
    fn test_inverse_series_correlate_negatively() {
        // Build a's log-returns directly (with variation, so std-dev isn't
        // zero), then derive b as the exact negative of each log-return —
        // this guarantees the two return series correlate at exactly -1,
        // unlike two arithmetic price series (whose *log* returns don't mirror
        // each other even when the raw prices are exact mirror images).
        let times: Vec<i64> = (0..60).map(|i| i * 86_400).collect();
        let mut closes_a = vec![100.0];
        let mut closes_b = vec![100.0];
        for i in 0..59 {
            let r = 0.01 + 0.005 * (i as f64 * 0.3).sin();
            closes_a.push(closes_a[i] * r.exp());
            closes_b.push(closes_b[i] * (-r).exp());
        }
        let a = make_bars(&times, &closes_a);
        let b = make_bars(&times, &closes_b);

        let corr = rolling_pearson_correlation(&a, &b, 90.0).unwrap();
        assert!(
            corr < -0.9,
            "expected strong negative correlation, got {corr}"
        );
    }

    #[test]
    fn test_insufficient_overlap_returns_none() {
        let a = make_bars(&[0, 86_400], &[100.0, 101.0]);
        let b = make_bars(&[0, 86_400], &[100.0, 101.0]);
        assert!(rolling_pearson_correlation(&a, &b, 90.0).is_none());
    }
}
