use anyhow::{anyhow, Result};
use infra::news::NewsConfig;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;

fn default_true() -> bool {
    true
}
fn default_false() -> bool {
    false
}
fn default_binance() -> String {
    "binance".to_string()
}
fn default_1000() -> usize {
    1000
}
fn default_data() -> PathBuf {
    PathBuf::from("data")
}
fn default_channels() -> Vec<String> {
    vec!["telegram".to_string()]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiveWorkerConfig {
    pub strategy: String,
    pub symbol: String,
    pub timeframe: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_timeframe: Option<Value>,

    #[serde(default = "default_true")]
    pub pyramiding: bool,

    /// Hard cap on simultaneously open positions for this worker (across both
    /// directions). `None` = unlimited. A safety bound against pyramiding opening
    /// an unbounded number of positions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_open_positions: Option<usize>,

    /// How many historical bars to seed on startup. Mutually exclusive with start_date.
    #[serde(default = "default_1000")]
    pub max_historical_bars: usize,

    /// Seed bars from this date onwards (ISO 8601). If set, overrides max_historical_bars.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_date: Option<String>,

    /// If true, generate signals on the current (incomplete) bar as well.
    #[serde(default = "default_false")]
    pub process_incomplete_bars: bool,

    // ── Broker adapters ──────────────────────────────────────────────────
    #[serde(default = "default_binance")]
    pub data_provider: String,

    #[serde(default = "default_binance")]
    pub trade_executor: String,

    #[serde(default = "default_binance")]
    pub bar_streamer: String,

    /// Defaults to bar_streamer value when omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tick_streamer: Option<String>,

    // ── Risk ─────────────────────────────────────────────────────────────
    pub risk_manager: Value,
    pub stop_manager: Value,

    #[serde(default)]
    pub exit_rules: Vec<Value>,

    #[serde(default = "default_false")]
    pub exit_before_entry: bool,

    // ── Strategy ─────────────────────────────────────────────────────────
    #[serde(default)]
    pub strategy_parameters: HashMap<String, Value>,

    #[serde(default)]
    pub indicators: HashMap<String, Value>,

    // ── Infra ────────────────────────────────────────────────────────────
    #[serde(default = "default_channels")]
    pub alert_channels: Vec<String>,

    #[serde(default)]
    pub news_blackout: NewsConfig,

    #[serde(default = "default_data")]
    pub data_dir: PathBuf,

    /// Per-round OOS mean-R-per-trade distribution, produced by the
    /// edge-discovery pipeline (Phase 3) and embedded directly here (see
    /// `tools generate-live-config`, which copies it in from
    /// `oos_distributions/{strategy}_{symbol}_{tf}.json`). When set, a
    /// background task compares this worker's trailing 30-day live
    /// performance against it and halts new entries on concept drift.
    /// Opt-in — omit for live configs not tied to a discovery run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oos_distribution: Option<Vec<f64>>,
}

impl LiveWorkerConfig {
    pub fn tick_streamer_name(&self) -> &str {
        self.tick_streamer.as_deref().unwrap_or(&self.bar_streamer)
    }

    pub fn effective_stop_timeframe(&self) -> String {
        fn resolve(val: &Value, fallback: &str) -> String {
            match val {
                Value::String(s) => {
                    if s == "timeframe" {
                        fallback.to_string()
                    } else {
                        s.clone()
                    }
                }
                Value::Array(arr) => {
                    if let Some(first) = arr.first() {
                        resolve(first, fallback)
                    } else {
                        fallback.to_string()
                    }
                }
                _ => fallback.to_string(),
            }
        }

        match &self.stop_timeframe {
            Some(val) => resolve(val, &self.timeframe),
            None => self.timeframe.clone(),
        }
    }
}

// ── LiveConfig ────────────────────────────────────────────────────────────────

pub struct LiveConfig {
    workers: Vec<LiveWorkerConfig>,
}

impl LiveConfig {
    /// Parse from JSON — accepts either a single object or an array of objects.
    pub fn from_json(s: &str) -> Result<Self> {
        let val: Value = serde_json::from_str(s).map_err(|e| anyhow!("invalid JSON: {e}"))?;

        let workers: Vec<LiveWorkerConfig> = if val.is_array() {
            serde_json::from_value(val)
                .map_err(|e| anyhow!("failed to parse live config array: {e}"))?
        } else {
            let single: LiveWorkerConfig = serde_json::from_value(val)
                .map_err(|e| anyhow!("failed to parse live config object: {e}"))?;
            vec![single]
        };

        if workers.is_empty() {
            return Err(anyhow!("live config has no worker configs"));
        }

        Ok(Self { workers })
    }

    pub fn workers(&self) -> &[LiveWorkerConfig] {
        &self.workers
    }

    pub fn into_workers(self) -> Vec<LiveWorkerConfig> {
        self.workers
    }
}
