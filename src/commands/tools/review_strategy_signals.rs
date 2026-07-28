use super::utils::{build_data_provider, ensure_bars_range, fmt_ts, print_table, tail_bars};
use anyhow::Result;
use backtest::config::BacktestConfig;
use backtest::engine::build_indicator_set;
use backtest::grid::generate_combos;
use backtest::simulator::{simulate, SimParams};
use backtest::write_trades_csv;
use data::IndicatorCache;
use risk::{build_volume, config::VolumeManagerConfig};
use std::collections::HashMap;
use std::path::PathBuf;
use strategy::build_strategy;
use tracing::info;
use ts_core::{Direction, Params, Signal};

pub async fn run(
    config: PathBuf,
    combo_idx: usize,
    bars: usize,
    tail: usize,
    data_dir: Option<PathBuf>,
    export: Option<PathBuf>,
    export_trades: Option<PathBuf>,
) -> Result<()> {
    let text = std::fs::read_to_string(&config)?;
    let cfg: BacktestConfig = serde_json::from_str(&text)?;
    let combos = generate_combos(&cfg)?;
    if combos.is_empty() {
        anyhow::bail!("no combos generated from {}", config.display());
    }
    if combo_idx >= combos.len() {
        anyhow::bail!(
            "combo index {combo_idx} out of range (max {})",
            combos.len() - 1
        );
    }
    let combo = &combos[combo_idx];
    let data_dir = data_dir.unwrap_or_else(|| cfg.data_dir.clone());
    let broker = cfg.data_provider.clone();

    let all_bars = ensure_bars_range(
        &combo.symbol,
        combo.timeframe,
        &data_dir,
        &broker,
        combo.start,
        combo.end,
    )
    .await?;
    if all_bars.is_empty() {
        anyhow::bail!("no bars for {} in {}", combo.symbol, data_dir.display());
    }
    let all_bars = tail_bars(all_bars, bars);

    let ind_cache = IndicatorCache::new(data_dir.join("ind_cache"));
    let cols = build_indicator_set(combo, &all_bars, &ind_cache)?;
    let strat = build_strategy(&combo.strategy_name)?;
    let strategy_params = Params(combo.strategy_params.clone());
    let ind_params: HashMap<String, Params> = combo
        .indicators
        .iter()
        .map(|d| (d.name.clone(), Params(d.params.clone())))
        .collect();
    let hold = Signal::new(Direction::Hold, 0.0, 0.0, 0.0);
    let all_signals = strat.generate_signals(&all_bars, &cols, &strategy_params, &ind_params);

    let signals: Vec<(i64, Signal)> = (0..all_bars.len())
        .filter_map(|idx| {
            let sig = all_signals[idx].as_ref().unwrap_or(&hold);
            if sig.is_valid() {
                Some((all_bars[idx].time, *sig))
            } else {
                None
            }
        })
        .collect();

    if let Some(path) = export {
        let mut wtr = csv::Writer::from_path(&path)?;
        wtr.write_record([
            "timestamp",
            "direction",
            "entry_price",
            "stop_loss",
            "take_profit",
        ])?;
        for (ts, sig) in &signals {
            wtr.write_record(&[
                fmt_ts(*ts),
                format!("{:?}", sig.direction),
                format!("{:.6}", sig.entry_price),
                format!("{:.6}", sig.stop_loss),
                format!("{:.6}", sig.take_profit),
            ])?;
        }
        wtr.flush()?;
        info!(path=%path.display(), signals=%signals.len(), "signals exported");
    }

    if let Some(path) = export_trades {
        let provider = build_data_provider(&cfg.data_provider)?;
        let symbol_info = provider.symbol_info(&combo.symbol).await?;
        let vol_cfg = VolumeManagerConfig::FixedPercent {
            pct: combo.risk_percentage,
            initial_balance: combo.initial_balance,
        };
        let vol_mgr = build_volume(&vol_cfg)?;
        let sim = simulate(
            &SimParams::from(combo),
            &all_bars,
            &all_bars,
            &cols,
            &all_signals,
            &symbol_info,
            vol_mgr.as_ref(),
            &[],
        );
        write_trades_csv(&sim.trades, &path)?;
        info!(path=%path.display(), trades=%sim.trades.len(), "trades exported");
    }

    println!("Strategy:  {}", combo.strategy_name);
    println!("Symbol:    {}", combo.symbol);
    println!("Timeframe: {:?}", combo.timeframe);
    println!("Bars:      {}", all_bars.len());
    println!("Signals:   {}", signals.len());
    println!();

    if tail == 0 || signals.is_empty() {
        return Ok(());
    }

    let start = signals.len().saturating_sub(tail);
    let headers = vec![
        "timestamp".to_string(),
        "direction".to_string(),
        "entry".to_string(),
        "stop".to_string(),
        "take".to_string(),
    ];
    let rows: Vec<Vec<String>> = signals[start..]
        .iter()
        .map(|(ts, sig)| {
            vec![
                fmt_ts(*ts),
                format!("{:?}", sig.direction),
                format!("{:.6}", sig.entry_price),
                format!("{:.6}", sig.stop_loss),
                format!("{:.6}", sig.take_profit),
            ]
        })
        .collect();
    print_table(&headers, &rows, &[false, false, true, true, true]);
    Ok(())
}
