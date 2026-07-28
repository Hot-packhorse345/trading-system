use anyhow::{anyhow, Context, Result};
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::IsTerminal;
use std::sync::Arc;
use std::thread;
use tracing::{info, warn};

use broker::traits::DataProvider;
use data::{IndicatorCache, OhlcvCache};
use indicators::{compute_all, IndicatorConfig};
use risk::config::VolumeManagerConfig;
use risk::{build_exit, build_volume};
use strategy::build_strategy;
use ts_core::{parse_timeframe, Bar, IndicatorSet, Params, SymbolInfo, Timeframe};

use crate::config::{GroupSpec, IndicatorDef, ResolvedCombo};
use crate::grid::{combo_hash_for, generate_group_specs};
use crate::result::BacktestResult;
use crate::simulator::{simulate, SimParams};
use crate::top_heap::TopNHeap;

pub fn run(
    cfg: &crate::config::BacktestConfig,
    top: usize,
    metric: Option<&str>,
) -> Result<Vec<BacktestResult>> {
    fs::create_dir_all(&cfg.output_dir).context("create output dir")?;

    let ohlcv_cache = OhlcvCache::new(&cfg.data_dir);
    let ind_cache = IndicatorCache::new(cfg.data_dir.join("ind_cache"));

    let groups = generate_group_specs(cfg).context("generate groups")?;
    if groups.is_empty() {
        warn!("no combos generated");
        return Ok(vec![]);
    }

    // ── Collect bar ranges and indicator configs needed across all groups ──────
    let mut ranges: HashMap<(String, Timeframe), (i64, i64)> = HashMap::new();
    let mut ind_cfgs_by_pair: HashMap<(String, Timeframe), Vec<IndicatorConfig>> = HashMap::new();
    let mut ind_seen: HashMap<(String, Timeframe), HashSet<String>> = HashMap::new();

    for group in &groups {
        let key = (group.symbol.clone(), group.timeframe);
        let range = ranges
            .entry(key.clone())
            .or_insert((group.start, group.end));
        range.0 = range.0.min(group.start);
        range.1 = range.1.max(group.end);

        let stop_key = (group.symbol.clone(), group.stop_timeframe);
        let stop_range = ranges
            .entry(stop_key.clone())
            .or_insert((group.start, group.end));
        stop_range.0 = stop_range.0.min(group.start);
        stop_range.1 = stop_range.1.max(group.end);

        for d in &group.indicators {
            let tf = if let Some(ref tf_str) = d.timeframe {
                match parse_timeframe(tf_str) {
                    Ok(t) => t,
                    Err(e) => {
                        warn!("invalid htf timeframe {tf_str}: {e}");
                        continue;
                    }
                }
            } else {
                group.timeframe
            };
            let key = (group.symbol.clone(), tf);
            let seen = ind_seen.entry(key.clone()).or_default();
            let cfgs = ind_cfgs_by_pair.entry(key.clone()).or_default();
            let cfg = IndicatorConfig {
                kind: d.ind_type.clone(),
                params: d.params.clone(),
            };
            let cache_key = cfg.canonical_json();
            if seen.insert(cache_key) {
                cfgs.push(cfg);
            }
        }
    }

    // ── Load / download bars ──────────────────────────────────────────────────
    let mut bars_by_symbol: HashMap<String, HashMap<Timeframe, Arc<Vec<Bar>>>> = HashMap::new();
    let mut missing: Vec<(String, Timeframe, i64, i64)> = Vec::new();
    let provider: Arc<dyn DataProvider> =
        Arc::from(broker::build_data_provider(&cfg.data_provider)?);

    for ((symbol, tf), (start, end)) in &ranges {
        let bars = ohlcv_cache
            .load(symbol, *tf, *start, *end)
            .with_context(|| format!("load OHLCV {symbol} {tf}"))?;
        if bars_need_download(&bars, *start, *end) {
            missing.push((symbol.clone(), *tf, *start, *end));
        } else {
            bars_by_symbol
                .entry(symbol.clone())
                .or_default()
                .insert(*tf, Arc::new(bars));
        }
    }

    if !missing.is_empty() {
        for (symbol, tf, start, end) in missing {
            info!(symbol=%symbol, tf=?tf, start, end, "downloading OHLCV");
            let bars = download_bars(provider.clone(), symbol.clone(), tf, start, end)
                .with_context(|| format!("download {symbol} {tf}"))?;
            if bars.is_empty() {
                warn!(symbol=%symbol, tf=?tf, "no bars downloaded");
                continue;
            }
            ohlcv_cache
                .save(&symbol, tf, &bars)
                .with_context(|| format!("save OHLCV {symbol} {tf}"))?;
            bars_by_symbol
                .entry(symbol.clone())
                .or_default()
                .insert(tf, Arc::new(bars));
        }
    }

    if bars_by_symbol.is_empty() {
        warn!("no bars loaded — run `download` first");
        return Ok(vec![]);
    }

    // ── Resample bars for HTF timeframes ──────────────────────────────────────
    let mut htf_bars_to_insert = Vec::new();
    for ((symbol, tf), _) in &ind_cfgs_by_pair {
        let has_bars = bars_by_symbol
            .get(symbol)
            .map_or(false, |m| m.contains_key(tf));
        if !has_bars {
            if let Some(m) = bars_by_symbol.get(symbol) {
                if let Some((&src_tf, src_bars)) = m
                    .iter()
                    .filter(|(&src_tf, _)| src_tf.seconds() < tf.seconds())
                    .min_by_key(|(&src_tf, _)| src_tf.seconds())
                {
                    match data::resample(src_bars.as_ref(), *tf, false) {
                        Ok(htf_bars) => {
                            htf_bars_to_insert.push((symbol.clone(), *tf, Arc::new(htf_bars)));
                        }
                        Err(e) => {
                            warn!("failed to resample bars for {symbol} from {src_tf:?} to {tf:?}: {e}");
                        }
                    }
                } else if let Some((&src_tf, src_bars)) = m.iter().next() {
                    match data::resample(src_bars.as_ref(), *tf, false) {
                        Ok(htf_bars) => {
                            htf_bars_to_insert.push((symbol.clone(), *tf, Arc::new(htf_bars)));
                        }
                        Err(e) => {
                            warn!("failed to resample bars for {symbol} from {src_tf:?} to {tf:?}: {e}");
                        }
                    }
                }
            }
        }
    }
    for (symbol, tf, htf_bars) in htf_bars_to_insert {
        bars_by_symbol
            .entry(symbol)
            .or_default()
            .insert(tf, htf_bars);
    }

    for (symbol, tfs) in &bars_by_symbol {
        for (tf, bars) in tfs {
            info!(symbol=%symbol, tf=?tf, bars=%bars.len(), "bars ready");
        }
    }

    // ── Pre-compute indicators ────────────────────────────────────────────────
    for ((symbol, tf), cfgs) in ind_cfgs_by_pair {
        if cfgs.is_empty() {
            continue;
        }
        let bars = match bars_by_symbol.get(&symbol).and_then(|m| m.get(&tf)) {
            Some(b) => b,
            None => {
                warn!(symbol=%symbol, tf=?tf, "no bars for indicator precompute");
                continue;
            }
        };
        info!(symbol=%symbol, tf=?tf, indicators=%cfgs.len(), "precomputing indicators");
        compute_all(&cfgs, bars.as_ref(), Some(&ind_cache)).context("compute indicators")?;
    }

    // ── Symbol info ───────────────────────────────────────────────────────────
    let mut symbol_infos: HashMap<String, SymbolInfo> = HashMap::new();
    let mut symbols: HashSet<String> = HashSet::new();
    for (symbol, _) in ranges.keys() {
        symbols.insert(symbol.clone());
    }
    for symbol in symbols {
        info!(symbol=%symbol, "fetching symbol info");
        let info = fetch_symbol_info(provider.clone(), symbol.clone())
            .with_context(|| format!("symbol info {symbol}"))?;
        symbol_infos.insert(symbol, info);
    }

    let total_combos: usize = groups.iter().map(|g| g.stop_managers.len()).sum();
    info!(combos=%total_combos, groups=%groups.len(), "backtest starting");

    let top_n = top.max(1);
    let score_fn = score_fn_for_metric(metric.unwrap_or("enhanced_score"));
    let pb = backtest_progress(total_combos);
    let bars_by_symbol = Arc::new(bars_by_symbol);
    let symbol_infos = Arc::new(symbol_infos);
    let ind_cache = Arc::new(ind_cache);

    // ── Parallel execution ────────────────────────────────────────────────────
    // Each Rayon thread accumulates its own bounded TopNHeap; results are merged
    // at the end.  Groups are the unit of parallelism: signals are computed once
    // per group, then re-used across all stop-manager variants within that group.
    let results: Vec<BacktestResult> = {
        let heap: TopNHeap = groups
            .par_iter()
            .fold(
                || TopNHeap::new(top_n, score_fn),
                |mut h, group| {
                    run_group(
                        group,
                        &bars_by_symbol,
                        &symbol_infos,
                        &ind_cache,
                        &mut h,
                        &pb,
                    );
                    h
                },
            )
            .reduce(
                || TopNHeap::new(top_n, score_fn),
                |mut a, b| {
                    a.merge(b);
                    a
                },
            );
        heap.into_sorted_vec()
    };

    pb.finish_and_clear();
    info!(completed=%results.len(), "backtest complete");
    Ok(results)
}

fn run_group(
    group: &GroupSpec,
    bars_by_symbol: &HashMap<String, HashMap<Timeframe, Arc<Vec<Bar>>>>,
    symbol_infos: &HashMap<String, SymbolInfo>,
    ind_cache: &IndicatorCache,
    heap: &mut TopNHeap,
    pb: &ProgressBar,
) {
    let n = group.stop_managers.len();

    let bars = match bars_by_symbol
        .get(group.symbol.as_str())
        .and_then(|m| m.get(&group.timeframe))
    {
        Some(b) => b.as_ref(),
        None => {
            warn!(symbol=%group.symbol, tf=?group.timeframe, "no bars loaded for group");
            pb.inc(n as u64);
            return;
        }
    };

    let stop_bars = if group.stop_timeframe == group.timeframe {
        bars
    } else {
        match bars_by_symbol
            .get(group.symbol.as_str())
            .and_then(|m| m.get(&group.stop_timeframe))
        {
            Some(b) => b.as_ref(),
            None => {
                warn!(symbol=%group.symbol, tf=?group.stop_timeframe, "no stop bars loaded for group");
                pb.inc(n as u64);
                return;
            }
        }
    };

    let symbol_info = match symbol_infos.get(group.symbol.as_str()) {
        Some(i) => i,
        None => {
            warn!(symbol=%group.symbol, "no symbol info for group");
            pb.inc(n as u64);
            return;
        }
    };

    let cols = match build_indicator_set_from_defs(&group.indicators, bars, Some(ind_cache)) {
        Ok(c) => c,
        Err(e) => {
            warn!(hash=%group.signal_hash, err=%e, "indicator build failed for group");
            pb.inc(n as u64);
            return;
        }
    };

    let strat = match build_strategy(&group.strategy_name) {
        Ok(s) => s,
        Err(e) => {
            warn!(err=%e, "build_strategy failed for group");
            pb.inc(n as u64);
            return;
        }
    };

    let params = Params(group.strategy_params.clone());
    let ind_params: HashMap<String, Params> = group
        .indicators
        .iter()
        .map(|d| (d.name.clone(), Params(d.params.clone())))
        .collect();
    let signals = strat.generate_signals(bars, &cols, &params, &ind_params);

    let vol_cfg = VolumeManagerConfig::FixedPercent {
        pct: group.risk_percentage,
        initial_balance: group.initial_balance,
    };
    let vol_mgr = match build_volume(&vol_cfg) {
        Ok(v) => v,
        Err(e) => {
            warn!(hash=%group.signal_hash, err=%e, "build_volume failed");
            pb.inc(n as u64);
            return;
        }
    };

    let exit_mgrs = build_exit(&group.exit_managers);

    for sm in &group.stop_managers {
        let sim_params = SimParams {
            symbol: &group.symbol,
            timeframe: group.timeframe,
            stop_timeframe: group.stop_timeframe,
            pyramiding: group.pyramiding,
            initial_balance: group.initial_balance,
            risk_pct: group.risk_percentage,
            commission_pct: group.commission_pct,
            commission_per_lot: group.commission_per_lot,
            swap: group.swap.clone(),
            trading_hours: group.trading_hours.clone(),
            stop_manager: sm,
            strategy_params: &group.strategy_params,
            collect_trades: false,
        };
        let sim = simulate(
            &sim_params,
            bars,
            stop_bars,
            &cols,
            &signals,
            symbol_info,
            vol_mgr.as_ref(),
            &exit_mgrs,
        );
        let ch = combo_hash_for(&group.signal_hash, sm);
        heap.push(BacktestResult {
            combo_hash: ch,
            signal_hash: group.signal_hash.clone(),
            combo_kind: group.combo_kind.clone(),
            strategy_name: group.strategy_name.clone(),
            symbol: group.symbol.clone(),
            timeframe: group.timeframe.to_binance().to_string(),
            stop_manager: format!(
                "{}(d={:.2},rr={:.2})",
                sm.sm_type, sm.stop_distance, sm.start_rr
            ),
            config: build_result_config_from_group(group, sm),
            metrics: sim.metrics,
        });
        pb.inc(1);
    }
}

// ── Indicator set construction ────────────────────────────────────────────────

/// Build an `IndicatorSet` from a slice of `IndicatorDef`s.  Used by both the
/// engine hot path (`GroupSpec`) and the legacy `ResolvedCombo` path.
///
/// Delegates to the canonical, look-ahead-free multi-timeframe builder in the
/// `indicators` crate so backtest and live trading share one implementation.
pub fn build_indicator_set_from_defs(
    defs: &[IndicatorDef],
    bars: &[Bar],
    ind_cache: Option<&IndicatorCache>,
) -> Result<IndicatorSet> {
    let tf_defs: Vec<indicators::TfIndicator> = defs
        .iter()
        .map(|d| indicators::TfIndicator {
            name: d.name.clone(),
            ind_type: d.ind_type.clone(),
            timeframe: d.timeframe.clone(),
            params: d.params.clone(),
        })
        .collect();
    indicators::build_indicator_set(&tf_defs, bars, ind_cache)
}

/// Backward-compatible wrapper for callers that have a `ResolvedCombo`.
pub fn build_indicator_set(
    combo: &ResolvedCombo,
    bars: &[Bar],
    ind_cache: &IndicatorCache,
) -> Result<IndicatorSet> {
    build_indicator_set_from_defs(&combo.indicators, bars, Some(ind_cache))
}

// ── Result config builders ────────────────────────────────────────────────────

fn build_result_config_from_group(
    group: &GroupSpec,
    sm: &risk::config::StopManagerConfig,
) -> Vec<(String, String)> {
    let mut cols = vec![
        ("strategy".to_string(), group.strategy_name.clone()),
        ("symbol".to_string(), group.symbol.clone()),
        (
            "timeframe".to_string(),
            group.timeframe.to_binance().to_string(),
        ),
        (
            "stop_timeframe".to_string(),
            group.stop_timeframe.to_binance().to_string(),
        ),
        ("pyramiding".to_string(), group.pyramiding.to_string()),
        (
            "initial_balance".to_string(),
            group.initial_balance.to_string(),
        ),
        (
            "risk_percentage".to_string(),
            group.risk_percentage.to_string(),
        ),
        (
            "commission_percent".to_string(),
            group.commission_pct.to_string(),
        ),
        (
            "commission_per_lot".to_string(),
            group.commission_per_lot.to_string(),
        ),
        (
            "swap_long_per_lot".to_string(),
            group.swap.long_per_lot.to_string(),
        ),
        (
            "swap_short_per_lot".to_string(),
            group.swap.short_per_lot.to_string(),
        ),
        (
            "swap_long_points".to_string(),
            group.swap.long_points.to_string(),
        ),
        (
            "swap_short_points".to_string(),
            group.swap.short_points.to_string(),
        ),
        (
            "swap_rollover_mode".to_string(),
            format!("{:?}", group.swap.rollover_mode),
        ),
        ("stop_manager.type".to_string(), sm.sm_type.clone()),
        (
            "stop_manager.stop_distance".to_string(),
            sm.stop_distance.to_string(),
        ),
        ("stop_manager.start_rr".to_string(), sm.start_rr.to_string()),
    ];

    let mut sp: Vec<_> = group.strategy_params.iter().collect();
    sp.sort_by_key(|(k, _)| k.as_str());
    for (k, v) in sp {
        cols.push((format!("strategy_parameters.{k}"), json_val_to_str(v)));
    }

    for d in &group.indicators {
        let pfx = format!("indicators.{}", d.name);
        cols.push((format!("{pfx}.type"), d.ind_type.clone()));
        let mut params: Vec<_> = d
            .params
            .iter()
            .filter(|(k, _)| k.as_str() != "type")
            .collect();
        params.sort_by_key(|(k, _)| k.as_str());
        for (k, v) in params {
            cols.push((format!("{pfx}.{k}"), json_val_to_str(v)));
        }
        if let Some(tf) = &d.timeframe {
            cols.push((format!("{pfx}.timeframe"), tf.clone()));
        }
    }

    cols
}

fn json_val_to_str(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Null => String::new(),
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}

// ── Scoring ───────────────────────────────────────────────────────────────────

fn score_fn_for_metric(metric: &str) -> fn(&BacktestResult) -> f64 {
    match metric {
        "sortino" => score_sortino,
        "profit_factor" => score_profit_factor,
        "net_profit" => score_net_profit,
        "win_rate" => score_win_rate,
        "recovery_factor" => score_recovery_factor,
        "expectancy" => score_expectancy,
        "performance_score" => score_performance,
        "enhanced_score" => score_enhanced,
        "calmar" => score_calmar,
        "ulcer_index" => score_ulcer_index,
        _ => score_sharpe,
    }
}

fn score_sharpe(r: &BacktestResult) -> f64 {
    r.metrics.sharpe_ratio
}
fn score_sortino(r: &BacktestResult) -> f64 {
    r.metrics.sortino_ratio
}
fn score_profit_factor(r: &BacktestResult) -> f64 {
    r.metrics.profit_factor
}
fn score_net_profit(r: &BacktestResult) -> f64 {
    r.metrics.net_profit
}
fn score_win_rate(r: &BacktestResult) -> f64 {
    r.metrics.win_rate
}
fn score_recovery_factor(r: &BacktestResult) -> f64 {
    r.metrics.recovery_factor
}
fn score_expectancy(r: &BacktestResult) -> f64 {
    r.metrics.expectancy
}
fn score_performance(r: &BacktestResult) -> f64 {
    r.metrics.performance_score
}
fn score_enhanced(r: &BacktestResult) -> f64 {
    r.metrics.enhanced_score
}
fn score_calmar(r: &BacktestResult) -> f64 {
    r.metrics.calmar_ratio
}
fn score_ulcer_index(r: &BacktestResult) -> f64 {
    -r.metrics.ulcer_index
} // lower is better, so negate

// ── Provider helpers ──────────────────────────────────────────────────────────

fn bars_need_download(bars: &[Bar], start: i64, end: i64) -> bool {
    if bars.is_empty() {
        return true;
    }
    let first = bars.first().map(|b| b.time).unwrap_or(i64::MAX);
    let last = bars.last().map(|b| b.time).unwrap_or(i64::MIN);
    first > start || last < end
}

fn download_bars(
    provider: Arc<dyn DataProvider>,
    symbol: String,
    tf: Timeframe,
    start: i64,
    end: i64,
) -> Result<Vec<Bar>> {
    let handle = thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        rt.block_on(provider.ohlcv(&symbol, tf, start, end))
    });
    handle
        .join()
        .map_err(|_| anyhow!("download thread panicked"))?
}

fn fetch_symbol_info(provider: Arc<dyn DataProvider>, symbol: String) -> Result<SymbolInfo> {
    let handle = thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        rt.block_on(provider.symbol_info(&symbol))
    });
    handle
        .join()
        .map_err(|_| anyhow!("symbol info thread panicked"))?
}

fn backtest_progress(total: usize) -> ProgressBar {
    if total == 0 || !std::io::stderr().is_terminal() {
        return ProgressBar::hidden();
    }

    let pb = ProgressBar::new(total as u64);
    pb.set_draw_target(ProgressDrawTarget::stderr());
    if let Ok(style) = ProgressStyle::with_template(
        "{spinner:.green} {pos}/{len} [{elapsed_precise}] {wide_bar:.cyan/blue}",
    ) {
        pb.set_style(style.progress_chars("=>-"));
    }
    pb
}
