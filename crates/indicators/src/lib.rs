pub mod ema;
pub mod factory;
pub mod macd;
pub mod math;
pub mod multi_tf;
pub mod rsi;

pub use factory::{build, split_indicator_def, IndicatorConfig};
pub use multi_tf::{build_indicator_set, TfIndicator};

use anyhow::Result;
use data::IndicatorCache;
use sha2::{Digest, Sha256};
use ts_core::{Bar, IndicatorSet};

pub trait Indicator: Send + Sync {
    fn compute(&self, bars: &[Bar]) -> Vec<(String, Vec<f64>)>;
}

pub fn compute_all(
    cfgs: &[IndicatorConfig],
    bars: &[Bar],
    cache: Option<&IndicatorCache>,
) -> Result<IndicatorSet> {
    let mut set = IndicatorSet::default();
    let bars_hash = cache.map(|_| hash_bars(bars));

    for cfg in cfgs {
        if let (Some(c), Some(ref h)) = (cache, &bars_hash) {
            let config_json = cfg.canonical_json();
            if let Ok(Some(cached_cols)) = c.get(h, &config_json) {
                let cached_ok =
                    !cached_cols.is_empty() && cached_cols.values().all(|v| v.len() == bars.len());
                if cached_ok {
                    for (col, values) in cached_cols {
                        set.insert(col, values);
                    }
                    continue;
                }
            }
            let ind = build(cfg)?;
            let computed = ind.compute(bars);
            c.put(h, &config_json, &computed).ok();
            for (col, values) in computed {
                set.insert(col, values);
            }
        } else {
            let ind = build(cfg)?;
            let computed = ind.compute(bars);
            for (col, values) in computed {
                set.insert(col, values);
            }
        }
    }
    Ok(set)
}

pub fn hash_bars(bars: &[Bar]) -> String {
    let mut h = Sha256::new();
    h.update((bars.len() as u64).to_le_bytes());
    for b in bars {
        h.update(b.time.to_le_bytes());
        h.update(b.open.to_le_bytes());
        h.update(b.high.to_le_bytes());
        h.update(b.low.to_le_bytes());
        h.update(b.close.to_le_bytes());
        h.update(b.volume.to_le_bytes());
    }
    format!("{:x}", h.finalize())
}
