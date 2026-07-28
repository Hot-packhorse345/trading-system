use crate::gates::Gates;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Verdict {
    Passed,
    FailedGate,
    Generated,
    Error(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryEntry {
    pub symbol: String,
    pub tf: String,
    pub asset_class: String,
    pub phase_reached: usize,
    pub verdict: Verdict,
    // metrics
    pub wfe: f64,
    pub consistency: f64,
    pub pf: f64,
    pub sharpe: f64,
    pub wr: f64,
    pub dd: f64,
    pub trades: usize,
    pub net_r: f64,
    pub density: f64,
    pub avg_open_time_days: f64,
    pub plateau_pass_rate: f64,
    pub cross_symbol: String,
    pub cross_pf: f64,
    pub cross_net_r: f64,
    pub cross_trades: usize,
    pub cross_asset_status: String,
    pub cross_asset_required: bool,
    pub cross_asset_correlation: Option<f64>,
    pub trials_session_strategy: usize,
    pub trials_historical_strategy: usize,
    pub consistency_lcb: f64,
    pub consensus_share: f64,
    pub consensus_margin: f64,
    pub regime_segments: usize,
    pub holdout_start: String,
    pub holdout_end: String,
    pub consensus: serde_json::Value,
    /// FDR-adjusted thresholds actually applied at Phase 4 (scaled from
    /// `gates.min_pf`/`min_sharpe` by historical trial count).
    pub pf_threshold_used: f64,
    pub sharpe_threshold_used: f64,
    /// Count of immediate one-step-neighbor plateau combos that were also
    /// profitable (topological connectivity check, Phase 5).
    pub plateau_neighbor_profitable_count: usize,
    /// Phase 6: synthetic black-swan stress test.
    pub stress_test_status: String,
    pub stress_test_dd_pct: f64,
    // paths
    pub wfa_config_path: String,
    pub full_backtest_config_path: String,
    pub plateau_config_path: String,
    pub csv_path: String,
    pub oos_distribution_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoverySummary {
    pub created: String,
    pub template: String,
    pub broker: String,
    pub gates: Gates,
    pub entries: Vec<DiscoveryEntry>,
}

impl DiscoverySummary {
    pub fn load_from_file(path: &std::path::Path) -> Result<Self> {
        let text = fs::read_to_string(path)
            .with_context(|| format!("Cannot read summary: {}", path.display()))?;
        serde_json::from_str(&text)
            .with_context(|| format!("Invalid summary JSON: {}", path.display()))
    }

    pub fn save_to_file(&self, path: &std::path::Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(self)?;
        fs::write(path, text)
            .with_context(|| format!("Failed to write summary: {}", path.display()))
    }

    pub fn print_table(&self) {
        println!("\n╔═════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════╗");
        println!("║                                              EDGE DISCOVERY SUMMARY                                                     ║");
        println!("╠═════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════╣");
        println!("║ {:<10} | {:<5} | {:<10} | {:<18} | {:<5} | {:<6} | {:<6} | {:<6} | {:<6} | {:<6} | {:<6} | {:<6} | {:<12} ║",
                 "Symbol", "TF", "Asset Class", "Verdict", "Phase", "Trades", "WFE", "Cons", "PF", "Sharpe", "WR", "Plat%", "Density"
        );
        println!("╠═════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════╣");
        for e in &self.entries {
            let verdict_str = match &e.verdict {
                Verdict::Passed => {
                    if e.cross_asset_status == "confirmed" {
                        "Passed+X".to_string()
                    } else {
                        "Passed".to_string()
                    }
                }
                Verdict::FailedGate => "Failed Gate".to_string(),
                Verdict::Generated => "Generated".to_string(),
                Verdict::Error(err) => format!("Err: {}", err),
            };
            let short_verdict = if verdict_str.len() > 18 {
                format!("{}...", &verdict_str[..15])
            } else {
                verdict_str
            };
            println!("║ {:<10} | {:<5} | {:<10} | {:<18} | {:<5} | {:<6} | {:<6.2} | {:<6.2} | {:<6.2} | {:<6.2} | {:<6.2} | {:<6.1}% | {:<12.2} ║",
                     e.symbol, e.tf, e.asset_class, short_verdict, e.phase_reached, e.trades, e.wfe, e.consistency, e.pf, e.sharpe, e.wr, e.plateau_pass_rate * 100.0, e.density
            );
            if matches!(e.verdict, Verdict::Passed) {
                println!(
                    "  ↳ trials: {}/session, {}/historical | cross-asset: {} | consensus-share: {:.2} (margin {:.2}) | regimes: {}",
                    e.trials_session_strategy,
                    e.trials_historical_strategy,
                    e.cross_asset_status,
                    e.consensus_share,
                    e.consensus_margin,
                    e.regime_segments
                );
            }
        }
        println!("╚═════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════╝\n");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_summary_json_roundtrip() {
        let gates = Gates::default();
        let summary = DiscoverySummary {
            created: "2026-07-02".to_string(),
            template: "temp.json".to_string(),
            broker: "ftmo".to_string(),
            gates,
            entries: vec![DiscoveryEntry {
                symbol: "EURUSD".to_string(),
                tf: "1h".to_string(),
                asset_class: "Forex".to_string(),
                phase_reached: 4,
                verdict: Verdict::Passed,
                wfe: 0.6,
                consistency: 0.7,
                pf: 1.6,
                sharpe: 0.9,
                wr: 0.4,
                dd: 12.0,
                trades: 250,
                net_r: 45.0,
                density: 9.0,
                avg_open_time_days: 0.0,
                plateau_pass_rate: 0.8,
                cross_symbol: "GBPUSD".to_string(),
                cross_pf: 1.3,
                cross_net_r: 20.0,
                cross_trades: 150,
                cross_asset_status: "confirmed".to_string(),
                cross_asset_required: false,
                cross_asset_correlation: Some(0.82),
                trials_session_strategy: 1,
                trials_historical_strategy: 10,
                consistency_lcb: 0.52,
                consensus_share: 0.75,
                consensus_margin: 0.25,
                regime_segments: 3,
                holdout_start: "2025-01-01".to_string(),
                holdout_end: "2025-12-31".to_string(),
                consensus: serde_json::json!({}),
                pf_threshold_used: 1.6,
                sharpe_threshold_used: 0.9,
                plateau_neighbor_profitable_count: 3,
                stress_test_status: "passed".to_string(),
                stress_test_dd_pct: 8.0,
                wfa_config_path: "".to_string(),
                full_backtest_config_path: "".to_string(),
                plateau_config_path: "".to_string(),
                csv_path: "".to_string(),
                oos_distribution_path: "".to_string(),
            }],
        };

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp_path = std::env::temp_dir().join(format!("test_summary_{}.json", timestamp));
        summary.save_to_file(&temp_path).unwrap();

        let loaded = DiscoverySummary::load_from_file(&temp_path).unwrap();
        // cleanup
        let _ = std::fs::remove_file(&temp_path);

        assert_eq!(loaded.entries.len(), 1);
        assert_eq!(loaded.entries[0].symbol, "EURUSD");
        assert_eq!(loaded.entries[0].verdict, Verdict::Passed);
    }
}
