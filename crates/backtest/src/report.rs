use crate::result::BacktestResult;
use anyhow::Result;
use chrono::DateTime;
use csv::Writer;
use std::collections::HashSet;
use std::path::Path;
use ts_core::TradeRecord;

const METRIC_COLS: &[&str] = &[
    "total_trades",
    "winning_trades",
    "losing_trades",
    "win_rate",
    "profit_factor_r",
    "sharpe_ratio",
    "sharpe_annualized",
    "sortino_ratio",
    "long_sharpe_ratio",
    "short_sharpe_ratio",
    "total_net_r",
    "avg_r",
    "gross_profit_r",
    "gross_loss_r",
    "avg_win",
    "avg_loss",
    "expectancy_r",
    "max_win_r",
    "max_loss_r",
    "common_win_r",
    "common_loss_r",
    "net_profit",
    "final_balance",
    "total_swap_cost",
    "max_dd",
    "max_drawdown_pct",
    "worst_segment_dd_pct",
    "longest_winning_streak",
    "longest_losing_streak",
    "volatility_r",
    "profit_bias",
    "recovery_factor",
    "performance_score",
    "calmar_ratio",
    "mar_ratio",
    "ulcer_index",
    "enhanced_score",
    "trades_significance",
];

fn fmt_ts_secs(ts: i64) -> String {
    let dt = DateTime::from_timestamp(ts, 0).unwrap();
    dt.to_rfc3339()
}

pub fn write_csv(results: &[BacktestResult], path: impl AsRef<Path>) -> Result<()> {
    // Build ordered config column list: preserve insertion order from first result,
    // append any extra keys seen in subsequent results (handles multi-config workflows).
    let mut config_cols: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for r in results {
        for (k, _) in &r.config {
            if seen.insert(k.clone()) {
                config_cols.push(k.clone());
            }
        }
    }

    // Header: rank | config cols | metric cols
    let mut header = vec!["rank".to_string()];
    header.extend(config_cols.iter().cloned());
    header.extend(METRIC_COLS.iter().map(|s| s.to_string()));

    let mut wtr = Writer::from_path(path)?;
    wtr.write_record(&header)?;

    // Results arrive pre-sorted by the engine; assign sequential ranks.
    for (rank, r) in results.iter().enumerate() {
        let cfg_map: std::collections::HashMap<&str, &str> = r
            .config
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();

        let mut row = vec![(rank + 1).to_string()];

        for col in &config_cols {
            row.push(cfg_map.get(col.as_str()).copied().unwrap_or("").to_string());
        }

        let m = &r.metrics;
        row.extend([
            m.total_trades.to_string(),
            m.winning_trades.to_string(),
            m.losing_trades.to_string(),
            format!("{:.4}", m.win_rate),
            format!("{:.4}", m.profit_factor),
            format!("{:.4}", m.sharpe_ratio),
            format!("{:.4}", m.sharpe_annualized),
            format!("{:.4}", m.sortino_ratio),
            format!("{:.4}", m.long_sharpe_ratio),
            format!("{:.4}", m.short_sharpe_ratio),
            format!("{:.4}", m.total_r),
            format!("{:.4}", m.avg_r),
            format!("{:.4}", m.gross_profit),
            format!("{:.4}", m.gross_loss),
            format!("{:.4}", m.avg_win),
            format!("{:.4}", m.avg_loss),
            format!("{:.4}", m.expectancy),
            format!("{:.4}", m.max_win_r),
            format!("{:.4}", m.max_loss_r),
            format!("{:.4}", m.median_win_r),
            format!("{:.4}", m.median_loss_r),
            format!("{:.2}", m.net_profit),
            format!("{:.2}", m.final_balance),
            format!("{:.2}", m.total_swap_cost),
            format!("{:.2}", m.max_drawdown),
            format!("{:.4}", m.max_drawdown_pct),
            format!("{:.4}", m.worst_segment_dd_pct),
            m.longest_win_streak.to_string(),
            m.longest_loss_streak.to_string(),
            format!("{:.4}", m.volatility_r),
            format!("{:.4}", m.profit_bias),
            format!("{:.4}", m.recovery_factor),
            format!("{:.4}", m.performance_score),
            format!("{:.4}", m.calmar_ratio),
            format!("{:.4}", m.mar_ratio),
            format!("{:.4}", m.ulcer_index),
            format!("{:.4}", m.enhanced_score),
            format!("{:.4}", m.trades_significance),
        ]);

        wtr.write_record(&row)?;
    }
    wtr.flush()?;
    Ok(())
}

pub fn write_trades_csv(trades: &[TradeRecord], path: impl AsRef<Path>) -> Result<()> {
    let mut wtr = Writer::from_path(path)?;
    wtr.write_record([
        "strategy_id",
        "trade_id",
        "symbol",
        "direction",
        "entry_price",
        "exit_price",
        "stop_loss",
        "initial_stop_loss",
        "take_profit",
        "volume",
        "open_risk",
        "entry_time",
        "exit_time",
        "exit_reason",
        "r_multiple",
        "currency_pnl",
        "group_id",
    ])?;
    for t in trades {
        wtr.write_record(&[
            t.strategy_id.to_string(),
            t.trade_id.to_string(),
            t.symbol.clone(),
            format!("{:?}", t.direction),
            format!("{:.5}", t.entry_price),
            format!("{:.5}", t.exit_price),
            format!("{:.5}", t.current_stop_loss),
            format!("{:.5}", t.initial_stop_loss),
            format!("{:.5}", t.take_profit),
            format!("{:.4}", t.volume),
            format!("{:.6}", t.open_risk),
            fmt_ts_secs(t.entry_time),
            fmt_ts_secs(t.exit_time),
            format!("{:?}", t.exit_reason),
            format!("{:.4}", t.r_multiple()),
            format!("{:.2}", t.currency_pnl),
            t.group_id.to_string(),
        ])?;
    }
    wtr.flush()?;
    Ok(())
}
