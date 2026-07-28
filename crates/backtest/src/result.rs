use crate::config::ComboKind;
use crate::metrics::Metrics;

#[derive(Debug)]
pub struct BacktestResult {
    pub combo_hash: String,
    pub signal_hash: String,
    pub combo_kind: ComboKind,
    pub strategy_name: String,
    pub symbol: String,
    pub timeframe: String,
    pub stop_manager: String,
    pub config: Vec<(String, String)>, // ordered (col, value) pairs
    pub metrics: Metrics,
}
