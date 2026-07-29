use crate::{schema::{ParamKind, ParamSpec}, Strategy};
use std::collections::HashMap;
use ts_core::{Bar, Direction, IndicatorSet, Params, Signal};

/// Mean-reversion strategy: buys when RSI crosses back up out of oversold
/// territory, sells when RSI crosses back down out of overbought territory.
pub struct RsiReversion;

impl Strategy for RsiReversion {
    fn generate_signals(
        &self,
        bars: &[Bar],
        cols: &IndicatorSet,
        params: &Params,
        ind_params: &HashMap<String, Params>,
    ) -> Vec<Option<Signal>> {
        let mut signals = vec![None; bars.len()];

        // Allow the period to be defined in strategy params or indicator params
        let rsi_period = ind_params.get("rsi")
            .and_then(|p| p.u64("period").or_else(|| p.u64("timeperiod")))
            .unwrap_or(params.u64_or("rsi_period", 14)) as usize;

        let rsi_key = format!("rsi_{}", rsi_period);

        if !cols.contains(&rsi_key) {
            return signals;
        }

        let rsi = cols.get(&rsi_key);

        let oversold = params.f64_or("oversold", 30.0);
        let overbought = params.f64_or("overbought", 70.0);
        let stop_pct = params.f64_or("stop_pct", 0.02);
        let tp_pct = params.f64_or("tp_pct", 0.04);

        for i in 1..bars.len() {
            let prev_rsi = rsi[i - 1];
            let curr_rsi = rsi[i];

            if prev_rsi.is_nan() || curr_rsi.is_nan() {
                continue;
            }

            let close = bars[i].close;

            // RSI crosses back up through the oversold line (Buy — bounce)
            if prev_rsi <= oversold && curr_rsi > oversold {
                let sl = close * (1.0 - stop_pct);
                let tp = close * (1.0 + tp_pct);
                signals[i] = Some(Signal::new(Direction::Buy, close, sl, tp));
            }
            // RSI crosses back down through the overbought line (Sell — fade)
            else if prev_rsi >= overbought && curr_rsi < overbought {
                let sl = close * (1.0 + stop_pct);
                let tp = close * (1.0 - tp_pct);
                signals[i] = Some(Signal::new(Direction::Sell, close, sl, tp));
            }
        }
        signals
    }

    fn param_schema(&self) -> Vec<ParamSpec> {
        vec![
            ParamSpec { key: "rsi_period".to_string(), kind: ParamKind::Structural, default: serde_json::json!(14), safe_range: None },
            ParamSpec { key: "oversold".to_string(), kind: ParamKind::Tunable, default: serde_json::json!(30.0), safe_range: Some((10.0, 40.0)) },
            ParamSpec { key: "overbought".to_string(), kind: ParamKind::Tunable, default: serde_json::json!(70.0), safe_range: Some((60.0, 90.0)) },
            ParamSpec { key: "stop_pct".to_string(), kind: ParamKind::Tunable, default: serde_json::json!(0.02), safe_range: Some((0.005, 0.1)) },
            ParamSpec { key: "tp_pct".to_string(), kind: ParamKind::Tunable, default: serde_json::json!(0.04), safe_range: Some((0.01, 0.2)) },
        ]
    }
}
