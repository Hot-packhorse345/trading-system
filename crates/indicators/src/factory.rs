use crate::{
    ema::{Ema, Sma, Wma},
    macd::Macd,
    rsi::Rsi,
    Indicator,
};
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndicatorConfig {
    pub kind: String,
    #[serde(default)]
    pub params: HashMap<String, Value>,
}

pub fn split_indicator_def(
    obj: &serde_json::Map<String, Value>,
) -> (Option<String>, Option<String>, HashMap<String, Value>) {
    let ind_type = obj
        .get("type")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let timeframe = obj
        .get("timeframe")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let params = obj
        .iter()
        .filter(|(k, _)| *k != "type" && *k != "timeframe")
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    (ind_type, timeframe, params)
}

impl IndicatorConfig {
    pub fn canonical_json(&self) -> String {
        use std::collections::BTreeMap;
        let sorted: BTreeMap<&str, &Value> =
            self.params.iter().map(|(k, v)| (k.as_str(), v)).collect();
        let obj = serde_json::json!({ "kind": self.kind, "params": sorted });
        serde_json::to_string(&obj).unwrap_or_default()
    }
    fn p_usize(&self, k: &str, d: usize) -> usize {
        for key in &[k, "timeperiod", "length", "period"] {
            if let Some(v) = self.params.get(*key).and_then(|v| v.as_u64()) {
                return v as usize;
            }
        }
        d
    }
    fn p_period(&self, primary: &str, d: usize) -> usize {
        self.params
            .get(primary)
            .or_else(|| self.params.get("timeperiod"))
            .or_else(|| self.params.get("length"))
            .or_else(|| self.params.get("period"))
            .and_then(|v| v.as_u64())
            .map(|v| v as usize)
            .unwrap_or(d)
    }
    fn p_str<'a>(&'a self, k: &str, d: &'a str) -> &'a str {
        self.params.get(k).and_then(|v| v.as_str()).unwrap_or(d)
    }
}

pub fn build(cfg: &IndicatorConfig) -> Result<Box<dyn Indicator>> {
    match cfg.kind.as_str() {
        "ema" | "ma" => {
            let period = cfg.p_period("period", 20);
            let mav = cfg.p_str("mav", "EMA").to_uppercase();
            match mav.as_str() {
                "SMA" | "DEMA" => Ok(Box::new(Sma { period })),
                "WMA" | "WWMA" => Ok(Box::new(Wma { period })),
                _ => Ok(Box::new(Ema { period })),
            }
        }
        "sma" => Ok(Box::new(Sma {
            period: cfg.p_period("period", 20),
        })),
        "wma" => Ok(Box::new(Wma {
            period: cfg.p_period("period", 20),
        })),
        "macd" => Ok(Box::new(Macd::new(
            cfg.p_usize("fast", 12),
            cfg.p_usize("slow", 26),
            cfg.p_usize("signal", 9),
        ))),
        "rsi" => Ok(Box::new(Rsi::new(cfg.p_period("period", 14)))),
        other => Err(anyhow!("unknown indicator: '{other}'")),
    }
}
