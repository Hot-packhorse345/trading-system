use crate::gates::Gates;
use crate::summary::DiscoveryEntry;
use anyhow::{Context, Result};
use chrono::Local;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};

pub fn append_finding(strategy: &str, entry: &DiscoveryEntry, gates: &Gates) -> Result<()> {
    let doc_dir = "docs/strategy";
    fs::create_dir_all(doc_dir)?;
    let doc_path = format!("{doc_dir}/{strategy}.md");

    // If strategy file doesn't exist, create it from skeleton
    if fs::metadata(&doc_path).is_err() {
        let skeleton = format!(
            "# Strategy: {}\n\n\
             ## Overview\n\n\
             ## Symbol & TF Guidelines\n\n\
             ## Asset Class Profiles\n\n\
             ## Walkforward Parameter Discoveries\n\n",
            strategy
        );
        fs::write(&doc_path, skeleton)
            .with_context(|| format!("Cannot create strategy doc: {doc_path}"))?;
    }

    let mut current = String::new();
    File::open(&doc_path)?.read_to_string(&mut current)?;
    let sec_num = current.matches("### 3.").count() + 1;
    let dt = Local::now().format("%Y-%m-%d").to_string();

    let verdict_str = match &entry.verdict {
        crate::summary::Verdict::Passed => "Passed",
        crate::summary::Verdict::FailedGate => "Failed Gate",
        crate::summary::Verdict::Generated => "Generated",
        crate::summary::Verdict::Error(e) => e.as_str(),
    };

    let consensus_json = serde_json::to_string_pretty(&entry.consensus).unwrap_or_default();

    let md = format!(
        "\n\n---\n\n\
         ### 3.{sec_num} {symbol} {tf} Walk-Forward edge (Discovered on {dt})\n\
         Verdict: **{verdict}**\n\n\
         - **WFO Performance (Walk-Forward Analysis)**:\n\
           - Walk-Forward Efficiency (WFE): **{wfe:.4}** (Target: $\\ge {min_wfe:.2}$)\n\
           - OOS Consistency: **{consistency:.4}** (Target: $\\ge {min_consistency:.2}$)\n\
           - OOS Consistency LCB (Wilson): **{consistency_lcb:.4}** (Target: $\\ge {min_consistency_lcb:.2}$)\n\
         - **Consensus Parameters (Full Set)**:\n\
           ```json\n\
           {consensus_json}\n\
           ```\n\
         - **Full-Range Backtest Results (includes holdout)**:\n\
           - **Profit Factor (R)**: **{pf:.4}** (Target: $\\ge {pf_threshold_used:.2}$, FDR-adjusted from base {min_pf:.2})\n\
           - **Annualized Sharpe Ratio**: **{sharpe:.4}** (Target: $\\ge {sharpe_threshold_used:.2}$, FDR-adjusted from base {min_sharpe:.2})\n\
           - **Win Rate**: **{wr:.4}** (Target: $\\ge {min_wr:.2}$)\n\
           - **Max Drawdown %**: **{dd:.4}%** (Target: $\\le {max_dd_pct:.2}$)\n\
           - **Total Trades**: **{trades}** (Target: $\\ge {min_trades}$)\n\
           - **Total Net R**: **{net_r:.2} R** (Expectancy: **{density:.2} R/year**, Target: $\\ge {min_density:.2}$)\n\
           - **Average Open Time**: **{avg_open_time_days:.1} days** (Target: $\\le {max_avg_open_time_days:.1}$ days)\n\
         - **Plateau Test (Robustness)**:\n\
           - **Pass Rate**: **{plateau_pass_rate:.2}%** (Target: $\\ge {plateau_pass_rate_target:.2}$)\n\
           - **Profitable Immediate Neighbors**: **{plateau_neighbors}** (Target: $\\ge {plateau_min_neighbors}$)\n\
         - **Synthetic Stress Test (Black-Swan Injection)**:\n\
           - Status: **{stress_test_status}**, Max Drawdown under stress: **{stress_test_dd_pct:.2}%** (Target: $\\le {max_stress_dd_pct:.2}$)\n\
         - **Cross-Asset Validation**:\n\
           - Status: **{cross_asset_status}** (required: **{cross_asset_required}**)\n\
           - Rolling correlation with **{cross_symbol}**: **{cross_asset_correlation}** (Target: $\\ge {min_cross_asset_correlation:.2}$)\n\
           - Validation on **{cross_symbol} {tf}** produced a profit factor of **{cross_pf:.4}** ({cross_net_r:.2} R over {cross_trades} trades). Target: $\\ge {cross_min_pf:.2}$\n\
         - **Search Context**:\n\
           - Holdout Period: **{holdout_start} to {holdout_end}**\n\
           - Trials for strategy in this run: **{trials_session_strategy}**\n\
           - Historical trials for strategy: **{trials_historical_strategy}**\n\n\
         > [!NOTE]\n\
         > Discovery entry registered in {csv_path}\n",
         symbol = entry.symbol,
         tf = entry.tf,
         verdict = verdict_str,
         wfe = entry.wfe,
         min_wfe = gates.min_wfe,
         consistency = entry.consistency,
         min_consistency = gates.min_consistency,
         consistency_lcb = entry.consistency_lcb,
         min_consistency_lcb = gates.min_oos_consistency_lcb,
         consensus_json = consensus_json,
         pf = entry.pf,
         min_pf = gates.min_pf,
         pf_threshold_used = entry.pf_threshold_used,
         sharpe = entry.sharpe,
         min_sharpe = gates.min_sharpe,
         sharpe_threshold_used = entry.sharpe_threshold_used,
         wr = entry.wr,
         min_wr = gates.min_wr,
         dd = entry.dd,
         max_dd_pct = gates.max_dd_pct,
         trades = entry.trades,
         min_trades = gates.min_trades,
         net_r = entry.net_r,
         density = entry.density,
         avg_open_time_days = entry.avg_open_time_days,
         min_density = gates.min_density,
         plateau_pass_rate = entry.plateau_pass_rate * 100.0,
         plateau_pass_rate_target = gates.plateau_pass_rate * 100.0,
         plateau_neighbors = entry.plateau_neighbor_profitable_count,
         plateau_min_neighbors = gates.plateau_min_neighbors,
         stress_test_status = entry.stress_test_status,
         stress_test_dd_pct = entry.stress_test_dd_pct,
         max_stress_dd_pct = gates.max_stress_dd_pct,
         cross_symbol = entry.cross_symbol,
         cross_asset_correlation = entry.cross_asset_correlation.map(|c| format!("{c:.2}")).unwrap_or_else(|| "N/A".to_string()),
         min_cross_asset_correlation = gates.min_cross_asset_correlation,
         cross_pf = entry.cross_pf,
         cross_net_r = entry.cross_net_r,
         cross_trades = entry.cross_trades,
         cross_asset_status = entry.cross_asset_status,
         cross_asset_required = entry.cross_asset_required,
         cross_min_pf = gates.cross_min_pf,
         trials_session_strategy = entry.trials_session_strategy,
         trials_historical_strategy = entry.trials_historical_strategy,
         holdout_start = entry.holdout_start,
         holdout_end = entry.holdout_end,
         csv_path = entry.csv_path,
         max_avg_open_time_days = gates.max_avg_open_time_days,
    );

    OpenOptions::new()
        .append(true)
        .open(&doc_path)?
        .write_all(md.as_bytes())?;
    Ok(())
}
