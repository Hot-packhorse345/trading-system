use crate::{Strategy, schema::{ParamSpec, ParamKind}};
use std::collections::HashMap;
use ts_core::{Bar, Direction, IndicatorSet, Params, Signal};

pub struct EmaCross;

impl Strategy for EmaCross {
    fn generate_signals(
        &self,
        bars: &[Bar],
        cols: &IndicatorSet,
        params: &Params,
        ind_params: &HashMap<String, Params>,
    ) -> Vec<Option<Signal>> {
        let mut signals = vec![None; bars.len()];

        // Allow periods to be defined in strategy params or indicator params
        let fast_period = ind_params.get("ema_fast")
            .and_then(|p| p.u64("period").or_else(|| p.u64("timeperiod")))
            .unwrap_or(params.u64_or("fast_period", 9)) as usize;

        let slow_period = ind_params.get("ema_slow")
            .and_then(|p| p.u64("period").or_else(|| p.u64("timeperiod")))
            .unwrap_or(params.u64_or("slow_period", 21)) as usize;

        let fast_key = format!("ema_{}", fast_period);
        let slow_key = format!("ema_{}", slow_period);

        if !cols.contains(&fast_key) || !cols.contains(&slow_key) {
            return signals;
        }

        let fast_ema = cols.get(&fast_key);
        let slow_ema = cols.get(&slow_key);

        let stop_pct = params.f64_or("stop_pct", 0.02);
        let tp_pct = params.f64_or("tp_pct", 0.04);

        for i in 1..bars.len() {
            let prev_fast = fast_ema[i - 1];
            let prev_slow = slow_ema[i - 1];
            let curr_fast = fast_ema[i];
            let curr_slow = slow_ema[i];

            if prev_fast.is_nan() || prev_slow.is_nan() || curr_fast.is_nan() || curr_slow.is_nan() {
                continue;
            }

            let close = bars[i].close;

            // Golden Cross (Buy)
            if prev_fast <= prev_slow && curr_fast > curr_slow {
                let sl = close * (1.0 - stop_pct);
                let tp = close * (1.0 + tp_pct);
                signals[i] = Some(Signal::new(Direction::Buy, close, sl, tp));
            }
            // Death Cross (Sell)
            else if prev_fast >= prev_slow && curr_fast < curr_slow {
                let sl = close * (1.0 + stop_pct);
                let tp = close * (1.0 - tp_pct);
                signals[i] = Some(Signal::new(Direction::Sell, close, sl, tp));
            }
        }
        signals
    }

    fn param_schema(&self) -> Vec<ParamSpec> {
        vec![
            ParamSpec { key: "fast_period".to_string(), kind: ParamKind::Structural, default: serde_json::json!(9), safe_range: None },
            ParamSpec { key: "slow_period".to_string(), kind: ParamKind::Structural, default: serde_json::json!(21), safe_range: None },
            ParamSpec { key: "stop_pct".to_string(), kind: ParamKind::Tunable, default: serde_json::json!(0.02), safe_range: Some((0.005, 0.1)) },
            ParamSpec { key: "tp_pct".to_string(), kind: ParamKind::Tunable, default: serde_json::json!(0.04), safe_range: Some((0.01, 0.2)) },
        ]
    }
}
