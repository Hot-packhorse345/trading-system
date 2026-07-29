use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use ts_core::Bar;

const EVENT_COUNT: usize = 3;
const CRASH_BARS: usize = 3;
const REVERSION_BARS: usize = 2;
const EVENT_LEN: usize = CRASH_BARS + REVERSION_BARS;
/// Cumulative crash magnitude, expressed in multiples of the series' rolling
/// log-return standard deviation.
const CRASH_STD_DEVS: f64 = 5.0;
/// Weights (fraction of CRASH_STD_DEVS) applied per crash bar; sums to -1.0.
const CRASH_WEIGHTS: [f64; CRASH_BARS] = [-0.6, -0.3, -0.1];
/// Weights applied per reversion bar — a partial bounce, not a full recovery,
/// since real flash crashes rarely fully mean-revert within 2 bars.
const REVERSION_WEIGHTS: [f64; REVERSION_BARS] = [0.4, 0.2];
/// Fraction of local average volume assigned to shocked bars, simulating the
/// liquidity vacuum that typically accompanies a flash crash.
const SHOCK_VOLUME_FRACTION: f64 = 0.1;
/// Shortest series length we'll inject shocks into — below this there isn't
/// enough room to keep warmup/tail bars untouched.
const MIN_BARS_FOR_INJECTION: usize = 200;

/// Deterministic seed derived from a string key (e.g. `"{symbol}_{tf}"`) so
/// repeated stress-test runs against the same symbol/timeframe are
/// reproducible.
pub fn seed_from_key(key: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    key.hash(&mut hasher);
    hasher.finish()
}

/// Injects synthetic "black swan" events (flash-crash + partial reversion,
/// with a volume collapse) into a copy of `bars`, for Phase 6 tail-risk
/// stress testing. Never touches the first/last 5% of the series (keeps
/// indicator warmup and the tail intact). Deterministic given the same
/// `bars` and `seed`.
pub fn inject_synthetic_shocks(bars: &[Bar], seed: u64) -> Vec<Bar> {
    let n = bars.len();
    let mut out = bars.to_vec();
    if n < MIN_BARS_FOR_INJECTION {
        return out;
    }

    let sigma = rolling_log_return_std(bars);
    if sigma <= 0.0 {
        return out;
    }

    let lo = (n as f64 * 0.05) as usize;
    let hi = (n as f64 * 0.95) as usize;
    let span = hi.saturating_sub(lo);
    if span <= EVENT_LEN {
        return out;
    }

    for i in 0..EVENT_COUNT {
        let base = lo + (span * (i + 1)) / (EVENT_COUNT + 1);
        let jitter = (seed.wrapping_add(i as u64 * 2_654_435_761)) % 11; // 0..=10
        let start = (base + jitter as usize)
            .saturating_sub(5)
            .clamp(lo, hi - EVENT_LEN);
        apply_event(&mut out, start, sigma);
    }

    out
}

fn rolling_log_return_std(bars: &[Bar]) -> f64 {
    let returns: Vec<f64> = bars
        .windows(2)
        .filter_map(|w| {
            if w[0].close > 0.0 && w[1].close > 0.0 {
                Some((w[1].close / w[0].close).ln())
            } else {
                None
            }
        })
        .collect();
    if returns.is_empty() {
        return 0.0;
    }
    let mean = returns.iter().sum::<f64>() / returns.len() as f64;
    let var = returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / returns.len() as f64;
    var.sqrt()
}

fn apply_event(bars: &mut [Bar], start: usize, sigma: f64) {
    let lookback_start = start.saturating_sub(20);
    let local_avg_volume = if start > lookback_start {
        let window = &bars[lookback_start..start];
        window.iter().map(|b| b.volume).sum::<f64>() / window.len() as f64
    } else {
        bars[start].volume
    };
    let shock_volume = local_avg_volume * SHOCK_VOLUME_FRACTION;

    for offset in 0..EVENT_LEN {
        let idx = start + offset;
        let weight = if offset < CRASH_BARS {
            CRASH_WEIGHTS[offset]
        } else {
            REVERSION_WEIGHTS[offset - CRASH_BARS]
        };
        let log_move = weight * CRASH_STD_DEVS * sigma;
        let prev_close = bars[idx - 1].close;
        let new_close = prev_close * log_move.exp();

        bars[idx].open = prev_close;
        bars[idx].close = new_close;
        bars[idx].high = prev_close.max(new_close) * 1.0005;
        bars[idx].low = prev_close.min(new_close) * 0.9995;
        bars[idx].volume = shock_volume;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_bars(n: usize) -> Vec<Bar> {
        (0..n)
            .map(|i| {
                let price = 100.0 + (i as f64 * 0.01).sin() * 2.0;
                Bar::new(
                    i as i64 * 60,
                    price,
                    price + 0.1,
                    price - 0.1,
                    price,
                    1000.0,
                )
            })
            .collect()
    }

    #[test]
    fn test_injects_a_large_drop_in_the_middle() {
        let bars = synthetic_bars(500);
        let seed = seed_from_key("BTCUSDT_1h");
        let shocked = inject_synthetic_shocks(&bars, seed);

        let sigma = rolling_log_return_std(&bars);
        let mut max_drop = 0.0_f64;
        for w in shocked.windows(2) {
            if w[0].close > 0.0 && w[1].close > 0.0 {
                let ret = (w[1].close / w[0].close).ln();
                max_drop = max_drop.min(ret);
            }
        }
        assert!(
            max_drop.abs() >= 3.0 * sigma,
            "expected at least a 3-sigma single-bar move, got {max_drop} vs sigma {sigma}"
        );
    }

    #[test]
    fn test_leaves_warmup_and_tail_untouched() {
        let bars = synthetic_bars(500);
        let seed = seed_from_key("BTCUSDT_1h");
        let shocked = inject_synthetic_shocks(&bars, seed);

        let n = bars.len();
        let lo = (n as f64 * 0.05) as usize;
        let hi = (n as f64 * 0.95) as usize;
        for i in 0..lo {
            assert_eq!(shocked[i].close, bars[i].close);
        }
        for i in hi..n {
            assert_eq!(shocked[i].close, bars[i].close);
        }
    }

    #[test]
    fn test_short_series_returned_unchanged() {
        let bars = synthetic_bars(50);
        let shocked = inject_synthetic_shocks(&bars, 42);
        assert_eq!(shocked.len(), bars.len());
        for (a, b) in bars.iter().zip(shocked.iter()) {
            assert_eq!(a.close, b.close);
        }
    }

    #[test]
    fn test_deterministic_given_same_seed() {
        let bars = synthetic_bars(500);
        let a = inject_synthetic_shocks(&bars, 7);
        let b = inject_synthetic_shocks(&bars, 7);
        assert_eq!(
            a.iter().map(|x| x.close).collect::<Vec<_>>(),
            b.iter().map(|x| x.close).collect::<Vec<_>>()
        );
    }
}
