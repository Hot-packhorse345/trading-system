//! Canonical multi-timeframe indicator builder shared by the backtest engine and
//! the live worker. Keeping a single implementation guarantees that live trading
//! sees exactly the same indicator columns (including higher-timeframe ones) as
//! the backtest — previously the live worker dropped the `timeframe` override and
//! never produced `*_htf` columns, so multi-timeframe strategies silently never
//! traded live.

use anyhow::{Context, Result};
use data::{resample, IndicatorCache};
use serde_json::Value;
use std::collections::HashMap;
use ts_core::{parse_timeframe, Bar, IndicatorSet};

use crate::{compute_all, IndicatorConfig};

/// One indicator definition with an optional higher-timeframe override.
#[derive(Debug, Clone)]
pub struct TfIndicator {
    pub name: String,
    pub ind_type: String,
    pub timeframe: Option<String>,
    pub params: HashMap<String, Value>,
}

/// Build an [`IndicatorSet`] from a list of indicator definitions.
///
/// Low-timeframe indicators (`timeframe == None`) are computed directly on `bars`
/// and stored under their natural column names. Higher-timeframe indicators are
/// computed on a resampled series and aligned back onto `bars` under
/// `"{name}.{col}"` keys.
///
/// HTF alignment is **look-ahead free**: an LTF bar at time `t` is only assigned a
/// higher-timeframe bucket that has already *closed* by `t` (`open + tf_secs <= t`).
pub fn build_indicator_set(
    defs: &[TfIndicator],
    bars: &[Bar],
    ind_cache: Option<&IndicatorCache>,
) -> Result<IndicatorSet> {
    let ltf_cfgs: Vec<IndicatorConfig> = defs
        .iter()
        .filter(|d| d.timeframe.is_none())
        .map(|d| IndicatorConfig {
            kind: d.ind_type.clone(),
            params: d.params.clone(),
        })
        .collect();

    let mut cols = compute_all(&ltf_cfgs, bars, ind_cache).context("compute LTF indicators")?;

    for htf_def in defs.iter().filter(|d| d.timeframe.is_some()) {
        let tf_str = htf_def.timeframe.as_deref().unwrap();
        let htf_tf = parse_timeframe(tf_str).with_context(|| {
            format!(
                "indicator '{}' has unknown timeframe '{tf_str}'",
                htf_def.name
            )
        })?;

        let htf_bars = resample(bars, htf_tf, false)
            .with_context(|| format!("resample bars for HTF indicator '{}'", htf_def.name))?;
        if htf_bars.is_empty() {
            continue;
        }

        let htf_cfg = IndicatorConfig {
            kind: htf_def.ind_type.clone(),
            params: htf_def.params.clone(),
        };
        let htf_cols = compute_all(&[htf_cfg], &htf_bars, ind_cache)
            .with_context(|| format!("compute HTF indicator '{}'", htf_def.name))?
            .into_columns();
        let htf_times: Vec<i64> = htf_bars.iter().map(|b| b.time).collect();

        // HTF bars are stamped at their OPEN time but their value is only known
        // once the bar has CLOSED (open + tf_secs). An LTF bar at time `t` may only
        // use buckets that closed by `t`; otherwise the strategy would see a
        // higher-timeframe candle still forming — look-ahead/repaint.
        let dst_secs = htf_tf.seconds();
        let mut fwd_idx: Vec<Option<usize>> = vec![None; bars.len()];
        let mut ptr = 0usize;
        for (ltf_i, ltf_bar) in bars.iter().enumerate() {
            while ptr + 1 < htf_times.len() && htf_times[ptr + 1] + dst_secs <= ltf_bar.time {
                ptr += 1;
            }
            if htf_times[ptr] + dst_secs <= ltf_bar.time {
                fwd_idx[ltf_i] = Some(ptr);
            }
        }

        for (col_name, htf_vals) in htf_cols {
            let full_key = format!("{}.{}", htf_def.name, col_name);
            let aligned: Vec<f64> = fwd_idx
                .iter()
                .map(|opt| opt.map(|i| htf_vals[i]).unwrap_or(f64::NAN))
                .collect();
            cols.insert(full_key, aligned);
        }
    }

    Ok(cols)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn htf_columns_are_only_visible_after_the_bucket_closes() {
        // 3 hours of 1-minute bars (180 bars), close increasing by 1 each bar.
        let bars: Vec<Bar> = (0..180)
            .map(|i| {
                let c = 100.0 + i as f64;
                Bar::new(i as i64 * 60, c, c + 0.5, c - 0.5, c, 1.0)
            })
            .collect();

        let defs = vec![TfIndicator {
            name: "trend".into(),
            ind_type: "sma".into(),
            timeframe: Some("1h".into()),
            params: [("period".to_string(), json!(1))].into_iter().collect(),
        }];

        let cols = build_indicator_set(&defs, &bars, None).unwrap();
        let col = cols.get("trend.sma_1");

        // First hour (e.g. minute 30): no HTF bucket has closed yet → NaN.
        assert!(
            col[30].is_nan(),
            "HTF value leaked before the bucket closed (look-ahead)"
        );
        // Third hour (e.g. minute 150): a closed HTF bucket exists → finite.
        assert!(
            col[150].is_finite(),
            "HTF value should be available after a bucket closes"
        );
    }
}
