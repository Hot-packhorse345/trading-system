use crate::catalog::SymbolMeta;
use crate::gates::Gates;
use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use std::fs;

#[derive(Debug, Deserialize)]
pub struct DiscoveryTemplate {
    pub strategy: String,
    pub stop_manager: Vec<Value>,
    pub indicators: Value,
    pub strategy_parameters: Value,
    #[serde(default)]
    pub metric: Option<String>,
    #[serde(default)]
    pub data_provider: Option<String>,
    #[serde(default)]
    pub initial_balance: Option<f64>,
    #[serde(default)]
    pub risk_percentage: Option<f64>,
    #[serde(default)]
    pub pyramiding: Option<bool>,
    #[serde(default)]
    pub exit_rules: Option<Vec<Value>>,
    #[serde(default)]
    pub gates: Option<Gates>,
}

impl DiscoveryTemplate {
    pub fn load(path: &str) -> Result<Self> {
        let text =
            fs::read_to_string(path).with_context(|| format!("Cannot read template: {path}"))?;
        serde_json::from_str(&text).with_context(|| format!("Invalid template JSON: {path}"))
    }

    pub fn data_provider(&self) -> &str {
        self.data_provider.as_deref().unwrap_or("mt5")
    }
    pub fn initial_balance(&self) -> f64 {
        self.initial_balance.unwrap_or(100_000.0)
    }
    pub fn risk_pct(&self) -> f64 {
        self.risk_percentage.unwrap_or(0.001)
    }
    pub fn metric(&self) -> &str {
        self.metric.as_deref().unwrap_or("profit_factor")
    }
    pub fn pyramiding(&self) -> bool {
        self.pyramiding.unwrap_or(false)
    }
    pub fn exit_rules(&self) -> Value {
        self.exit_rules
            .clone()
            .map(Value::Array)
            .unwrap_or_else(|| json!([]))
    }

    pub fn build_wfa_config(
        &self,
        symbol: &SymbolMeta,
        tf: &str,
        htf: &[String],
        start_str: &str,
        end_str: &str,
        is_bars: usize,
        oos_bars: usize,
        step_bars: usize,
        gates: &Gates,
    ) -> Value {
        let mut sp = self.strategy_parameters.clone();
        if let Value::Object(ref mut map) = sp {
            map.insert(
                "trade_direction".to_string(),
                Value::String(symbol.trade_direction.clone()),
            );
        }

        let indicators = expand_htf_timeframes(self.indicators.clone(), htf);

        json!({
            "strategy":            self.strategy,
            "symbol":              symbol.symbol,
            "timeframe":           vec![tf],
            "stop_timeframe":      vec![tf],
            "pyramiding":          self.pyramiding(),
            "backtest_start":      start_str,
            "backtest_end":        end_str,
            "data_provider":       self.data_provider(),
            "initial_balance":     self.initial_balance(),
            "risk_percentage":     self.risk_pct(),
            "commission_per_lot":  symbol.commission_per_lot,
            "commission_percent":  symbol.commission_percent,
            "is_bars":             is_bars,
            "oos_bars":            oos_bars,
            "step_bars":           step_bars,
            "min_wf_efficiency":   gates.min_wfe,
            "min_oos_consistency": gates.min_consistency,
            "metric":              self.metric(),
            "exit_rules":          self.exit_rules(),
            "stop_manager":        self.stop_manager,
            "strategy_parameters": sp,
            "indicators":          indicators,
        })
    }

    pub fn build_fixed_config(
        &self,
        symbol: &SymbolMeta,
        tf: &str,
        htf: &[String],
        start_str: &str,
        end_str: &str,
        stop_mgr: Value,
        multiplier_override: Option<&Value>,
    ) -> Value {
        let mut sp = self.strategy_parameters.clone();
        if let Value::Object(ref mut map) = sp {
            map.insert(
                "trade_direction".to_string(),
                Value::String(symbol.trade_direction.clone()),
            );
        }

        let mut indicators = expand_htf_timeframes(self.indicators.clone(), htf);

        resolve_samples(&mut indicators, multiplier_override);
        resolve_samples(&mut sp, multiplier_override);

        json!({
            "strategy":            self.strategy,
            "symbol":              symbol.symbol,
            "timeframe":           tf,
            "stop_timeframe":      tf,
            "pyramiding":          self.pyramiding(),
            "backtest_start":      start_str,
            "backtest_end":        end_str,
            "data_provider":       self.data_provider(),
            "initial_balance":     self.initial_balance(),
            "risk_percentage":     self.risk_pct(),
            "commission_per_lot":  symbol.commission_per_lot,
            "commission_percent":  symbol.commission_percent,
            "stop_manager":        vec![stop_mgr],
            "strategy_parameters": sp,
            "indicators":          indicators,
        })
    }
}

pub fn expand_htf_timeframes(mut indicators: Value, htf: &[String]) -> Value {
    if let Value::Object(ref mut map) = indicators {
        for v in map.values_mut() {
            if let Value::Object(ref mut ind) = v {
                if ind.contains_key("[g]timeframe") {
                    ind.insert("[g]timeframe".to_string(), json!(htf));
                }
            }
        }
    }
    indicators
}

pub fn resolve_samples(v: &mut Value, multiplier_override: Option<&Value>) {
    match v {
        Value::Object(map) => {
            let is_multiplier = map.contains_key("multiplier");
            if map.contains_key("$sample") {
                if !is_multiplier {
                    let sample_type = map.get("$sample").and_then(|x| x.as_str()).unwrap_or("");
                    let pinned_val = match sample_type {
                        "values" => {
                            if let Some(Value::Array(items)) = map.get("items") {
                                items.first().cloned().unwrap_or(Value::Null)
                            } else {
                                Value::Null
                            }
                        }
                        "range" | "linspace" | "log" => {
                            map.get("start").cloned().unwrap_or(Value::Null)
                        }
                        _ => Value::Null,
                    };
                    *v = pinned_val;
                }
            } else {
                for (key, val) in map.iter_mut() {
                    if key == "multiplier" {
                        if let Some(m_over) = multiplier_override {
                            *val = m_over.clone();
                        } else {
                            resolve_samples(val, None);
                        }
                    } else {
                        resolve_samples(val, multiplier_override);
                    }
                }
            }
        }
        Value::Array(arr) => {
            for val in arr.iter_mut() {
                resolve_samples(val, multiplier_override);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_samples_pinning() {
        let mut val = json!({
            "param_a": { "$sample": "range", "start": 10.0, "stop": 20.0, "step": 1.0 },
            "multiplier": { "$sample": "values", "items": [1.5, 2.0] }
        });

        // resolving without multiplier override pins multiplier too (to first value)
        resolve_samples(&mut val, None);
        assert_eq!(val["param_a"], json!(10.0));
        assert_eq!(val["multiplier"], json!(1.5));

        // resolving with multiplier override pins multiplier to override value
        let mut val2 = json!({
            "param_a": { "$sample": "range", "start": 10.0, "stop": 20.0, "step": 1.0 },
            "multiplier": { "$sample": "values", "items": [1.5, 2.0] }
        });
        let mult_override = json!(2.0);
        resolve_samples(&mut val2, Some(&mult_override));
        assert_eq!(val2["multiplier"], json!(2.0));
    }
}
