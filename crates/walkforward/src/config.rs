use serde::{Deserialize, Serialize};

fn default_is_bars() -> usize {
    2000
}
fn default_oos_bars() -> usize {
    500
}
fn default_step_bars() -> usize {
    250
}
fn default_wfe() -> f64 {
    0.5
}
fn default_consistency() -> f64 {
    0.6
}
fn default_metric() -> String {
    "enhanced_score".to_string()
}
fn default_min_oos_trades_per_round() -> usize {
    0
}
fn default_min_oos_consistency_lcb() -> f64 {
    0.0
}
fn default_consistency_confidence_z() -> f64 {
    1.96
}

/// Walk-forward window parameters. Deserialized from the same JSON file as the
/// embedded `BacktestConfig` (extra backtest fields are ignored here).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalkforwardConfig {
    #[serde(default = "default_is_bars")]
    pub is_bars: usize,
    #[serde(default = "default_oos_bars")]
    pub oos_bars: usize,
    #[serde(default = "default_step_bars")]
    pub step_bars: usize,

    #[serde(default = "default_wfe")]
    pub min_wf_efficiency: f64,
    #[serde(default = "default_consistency")]
    pub min_oos_consistency: f64,
    #[serde(default = "default_min_oos_trades_per_round")]
    pub min_oos_trades_per_round: usize,
    #[serde(default = "default_min_oos_consistency_lcb")]
    pub min_oos_consistency_lcb: f64,
    #[serde(default = "default_consistency_confidence_z")]
    pub consistency_confidence_z: f64,

    /// Metric used to rank in-sample combos and to score OOS performance.
    #[serde(default = "default_metric")]
    pub metric: String,
}
