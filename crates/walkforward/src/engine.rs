use anyhow::{anyhow, Context, Result};
use backtest::engine::build_indicator_set_from_defs;
use backtest::simulator::{simulate, SimParams};
use backtest::Metrics;
use backtest::{BacktestConfig, GroupSpec};
use data::IndicatorCache;
use indicators::IndicatorConfig;
use rayon::prelude::*;
use risk::{build_exit, build_volume, StopManagerConfig, VolumeManagerConfig};
use std::collections::{HashMap, HashSet};
use ts_core::{Bar, IndicatorSet, Params, SymbolInfo, Timeframe};

use crate::config::WalkforwardConfig;
use crate::report::{WfReport, WfRound};

/// Score a metric by name (mirrors the backtest engine's metric selection).
fn score_metric(m: &Metrics, metric: &str) -> f64 {
    let v = match metric {
        "sortino" => m.sortino_ratio,
        "profit_factor" => m.profit_factor,
        "net_profit" => m.net_profit,
        "win_rate" => m.win_rate,
        "recovery_factor" => m.recovery_factor,
        "expectancy" => m.expectancy,
        "performance_score" => m.performance_score,
        "enhanced_score" => m.enhanced_score,
        "calmar" => m.calmar_ratio,
        "total_r" => m.total_r,
        _ => m.sharpe_ratio,
    };
    // Treat degenerate combos (no trades, non-finite scores) as worst.
    if m.total_trades == 0 || v.is_nan() {
        f64::NEG_INFINITY
    } else if v.is_infinite() {
        if v.is_sign_positive() {
            999.0 // Cap positive infinity (e.g. profit_factor with 0 losses) to a high finite score
        } else {
            f64::NEG_INFINITY
        }
    } else {
        v
    }
}

/// Run a single combo over a bar slice and return its metrics.
fn eval_combo_wfo(
    group: &GroupSpec,
    sm: &StopManagerConfig,
    bars: &[Bar],
    cols: &IndicatorSet,
    signals: &[Option<ts_core::Signal>],
    symbol_info: &SymbolInfo,
) -> Result<Metrics> {
    let vol = build_volume(&VolumeManagerConfig::FixedPercent {
        pct: group.risk_percentage,
        initial_balance: group.initial_balance,
    })?;

    let exit_mgrs = build_exit(&group.exit_managers);

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
        collect_trades: true,
    };
    let sim = simulate(
        &sim_params,
        bars,
        bars,
        cols,
        signals,
        symbol_info,
        vol.as_ref(),
        &exit_mgrs,
    );
    Ok(sim.metrics)
}

/// Execute a rolling in-sample/out-of-sample walk-forward analysis.
///
/// For each window the combo grid (grouped by signal configurations as GroupSpecs)
/// is optimised on the in-sample slice (ranked by `wf.metric`); the single best combo
/// (specific signal group + stop manager) is then evaluated, untouched, on the
/// following out-of-sample slice. Aggregates walk-forward efficiency and
/// out-of-sample consistency. Reuses the same (look-ahead-free) indicator and
/// simulation pipeline as the backtest engine.
pub fn run(
    base: &BacktestConfig,
    wf: &WalkforwardConfig,
    symbol: &str,
    timeframe: Timeframe,
    bars: &[Bar],
    symbol_info: &SymbolInfo,
) -> Result<WfReport> {
    if wf.is_bars == 0 || wf.oos_bars == 0 || wf.step_bars == 0 {
        return Err(anyhow!("is_bars, oos_bars and step_bars must all be > 0"));
    }

    // Only group specs for the loaded symbol/timeframe are valid against these bars.
    let groups: Vec<GroupSpec> = backtest::grid::generate_group_specs(base)?
        .into_iter()
        .filter(|g| g.symbol == symbol && g.timeframe == timeframe)
        .collect();
    if groups.is_empty() {
        return Err(anyhow!("no combos generated for {symbol}/{timeframe}"));
    }

    let n = bars.len();
    let window = wf.is_bars + wf.oos_bars;
    if n < window {
        return Err(anyhow!(
            "not enough bars: have {n}, need at least is_bars+oos_bars = {window}"
        ));
    }

    let global_cache = IndicatorCache::new(base.data_dir.join("ind_cache"));

    // ── Precompute all indicators and save them in the global cache ───────────
    tracing::info!("Pre-computing indicators for global cache...");
    let mut ind_cfgs_by_tf: HashMap<Timeframe, Vec<IndicatorConfig>> = HashMap::new();
    let mut seen: HashMap<Timeframe, HashSet<String>> = HashMap::new();

    for group in &groups {
        for d in &group.indicators {
            let tf = if let Some(ref tf_str) = d.timeframe {
                match ts_core::parse_timeframe(tf_str) {
                    Ok(t) => t,
                    Err(e) => {
                        tracing::warn!("invalid htf timeframe {tf_str}: {e}");
                        continue;
                    }
                }
            } else {
                timeframe
            };
            let cfg = IndicatorConfig {
                kind: d.ind_type.clone(),
                params: d.params.clone(),
            };
            let cache_key = cfg.canonical_json();
            let entry = seen.entry(tf).or_default();
            if entry.insert(cache_key) {
                ind_cfgs_by_tf.entry(tf).or_default().push(cfg);
            }
        }
    }

    for (tf, cfgs) in ind_cfgs_by_tf {
        if cfgs.is_empty() {
            continue;
        }
        if tf == timeframe {
            indicators::compute_all(&cfgs, bars, Some(&global_cache))
                .context("precompute LTF indicators")?;
        } else {
            let htf_bars = data::resample(bars, tf, false)
                .with_context(|| format!("resample bars for precompute to {tf:?}"))?;
            if !htf_bars.is_empty() {
                indicators::compute_all(&cfgs, &htf_bars, Some(&global_cache))
                    .context("precompute HTF indicators")?;
            }
        }
    }

    // Precompute indicators and signals on the full series for all group specs!
    tracing::info!(
        "Pre-computing indicators and signals for {} groups on the full series ({} bars)...",
        groups.len(),
        n
    );
    let precomputed: Vec<(IndicatorSet, Vec<Option<ts_core::Signal>>)> = groups
        .par_iter()
        .map(
            |group| -> Result<(IndicatorSet, Vec<Option<ts_core::Signal>>)> {
                let cols =
                    build_indicator_set_from_defs(&group.indicators, bars, Some(&global_cache))?;
                let strat = strategy::build_strategy(&group.strategy_name)?;
                let params = Params(group.strategy_params.clone());
                let ind_params: HashMap<String, Params> = group
                    .indicators
                    .iter()
                    .map(|d| (d.name.clone(), Params(d.params.clone())))
                    .collect();
                let signals = strat.generate_signals(bars, &cols, &params, &ind_params);
                Ok((cols, signals))
            },
        )
        .collect::<Result<Vec<_>>>()?;

    let total_rounds = (n - window) / wf.step_bars + 1;
    let pb = indicatif::ProgressBar::new(total_rounds as u64);
    pb.set_style(
        indicatif::ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({eta}) {msg}")
            .unwrap()
            .progress_chars("#>-"),
    );
    pb.set_message("Optimising walk-forward rounds");

    let mut rounds: Vec<WfRound> = Vec::new();
    let mut start = 0usize;
    let mut idx = 0usize;
    while start + window <= n {
        let is_slice = &bars[start..start + wf.is_bars];
        let oos_slice = &bars[start + wf.is_bars..start + window];

        // ── In-sample optimisation (parallel over signal groups) ────────────────
        let scored: Vec<(usize, usize, f64)> = groups
            .par_iter()
            .enumerate()
            .map(|(g_idx, group)| {
                let (cols_full, signals_full) = &precomputed[g_idx];
                let signals = &signals_full[start..start + wf.is_bars];

                // Optimisation: if no entry signals exist in this window, skip simulations completely!
                if !signals.iter().any(|s| s.is_some()) {
                    return (g_idx, 0, f64::NEG_INFINITY);
                }

                let cols = cols_full.slice(start..start + wf.is_bars);
                let res = (|| -> Result<Vec<(usize, f64)>> {
                    let vol = build_volume(&VolumeManagerConfig::FixedPercent {
                        pct: group.risk_percentage,
                        initial_balance: group.initial_balance,
                    })?;

                    let exit_mgrs = build_exit(&group.exit_managers);

                    let mut group_scores = Vec::with_capacity(group.stop_managers.len());
                    for (sm_idx, sm) in group.stop_managers.iter().enumerate() {
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
                            is_slice,
                            is_slice,
                            &cols,
                            signals,
                            symbol_info,
                            vol.as_ref(),
                            &exit_mgrs,
                        );
                        let score = score_metric(&sim.metrics, &wf.metric);
                        group_scores.push((sm_idx, score));
                    }
                    Ok(group_scores)
                })();

                match res {
                    Ok(scores) => {
                        if let Some((sm_idx, score)) = scores
                            .iter()
                            .filter(|(_, s)| s.is_finite())
                            .max_by(|a, b| a.1.total_cmp(&b.1))
                        {
                            (g_idx, *sm_idx, *score)
                        } else {
                            (g_idx, 0, f64::NEG_INFINITY)
                        }
                    }
                    Err(_) => (g_idx, 0, f64::NEG_INFINITY),
                }
            })
            .collect();

        let best = scored
            .iter()
            .filter(|(_, _, s)| s.is_finite())
            .max_by(|a, b| a.2.total_cmp(&b.2))
            .copied();

        let Some((best_g, best_sm, is_score)) = best else {
            pb.inc(1);
            idx += 1;
            start += wf.step_bars;
            continue;
        };

        // ── Out-of-sample evaluation of the IS-optimal combo ───────────────────
        let (cols_full, signals_full) = &precomputed[best_g];
        let oos_cols = cols_full.slice(start + wf.is_bars..start + window);
        let oos_signals = &signals_full[start + wf.is_bars..start + window];
        let oos_metrics = eval_combo_wfo(
            &groups[best_g],
            &groups[best_g].stop_managers[best_sm],
            oos_slice,
            &oos_cols,
            oos_signals,
            symbol_info,
        )?;
        let oos_score = score_metric(&oos_metrics, &wf.metric);
        let combo_desc =
            format_combo_params(&groups[best_g], &groups[best_g].stop_managers[best_sm]);
        let resolved_json =
            resolved_params_json(&groups[best_g], &groups[best_g].stop_managers[best_sm]);

        rounds.push(WfRound {
            window: idx,
            is_score,
            oos_score,
            oos_total_r: oos_metrics.total_r,
            oos_trades: oos_metrics.total_trades,
            combo: combo_desc,
            params: resolved_json,
        });

        pb.inc(1);
        idx += 1;
        start += wf.step_bars;
    }
    pb.finish_and_clear();

    Ok(WfReport::build(rounds, wf))
}

fn resolved_params_json(group: &GroupSpec, sm: &StopManagerConfig) -> serde_json::Value {
    let mut sm_map = serde_json::Map::new();
    sm_map.insert("type".to_string(), serde_json::json!(sm.sm_type));
    sm_map.insert(
        "stop_distance".to_string(),
        serde_json::json!(sm.stop_distance),
    );
    if sm.sm_type.contains("variant3") || sm.sm_type.contains("variant4") {
        sm_map.insert("start_rr".to_string(), serde_json::json!(sm.start_rr));
    }

    let mut sp_map = serde_json::Map::new();
    let mut sp_keys: Vec<_> = group.strategy_params.keys().collect();
    sp_keys.sort();
    for k in sp_keys {
        sp_map.insert(k.clone(), group.strategy_params.get(k).unwrap().clone());
    }

    let mut ind_map = serde_json::Map::new();
    let mut indicators = group.indicators.clone();
    indicators.sort_by(|a, b| a.name.cmp(&b.name));
    for d in indicators {
        let mut def_map = serde_json::Map::new();
        def_map.insert("type".to_string(), serde_json::json!(d.ind_type));
        if let Some(ref tf) = d.timeframe {
            def_map.insert("timeframe".to_string(), serde_json::json!(tf));
        }
        let mut param_keys: Vec<_> = d.params.keys().collect();
        param_keys.sort();
        for pk in param_keys {
            if pk != "type" && pk != "timeframe" {
                def_map.insert(pk.clone(), d.params.get(pk).unwrap().clone());
            }
        }
        ind_map.insert(d.name.clone(), serde_json::json!(def_map));
    }

    serde_json::json!({
        "stop_manager": serde_json::Value::Object(sm_map),
        "strategy_parameters": serde_json::Value::Object(sp_map),
        "indicators": serde_json::Value::Object(ind_map),
    })
}

fn format_combo_params(group: &GroupSpec, sm: &StopManagerConfig) -> String {
    let mut parts = Vec::new();

    // Stop manager type & stop_distance & start_rr
    parts.push(format!("{}(dist={}", sm.sm_type, sm.stop_distance));
    if sm.sm_type.contains("variant3") || sm.sm_type.contains("variant4") {
        parts.push(format!(",rr={}", sm.start_rr));
    }
    parts.push(")".to_string());

    // Strategy parameters
    let mut sp: Vec<_> = group.strategy_params.iter().collect();
    sp.sort_by_key(|(k, _)| k.as_str());
    for (k, v) in sp {
        parts.push(format!("|{}={}", k, json_val_to_str(v)));
    }

    // Indicators parameters
    for d in &group.indicators {
        let mut params: Vec<_> = d
            .params
            .iter()
            .filter(|(k, _)| k.as_str() != "type")
            .collect();
        if !params.is_empty() {
            params.sort_by_key(|(k, _)| k.as_str());
            parts.push(format!("|{}:", d.name));
            for (i, (k, v)) in params.into_iter().enumerate() {
                if i > 0 {
                    parts.push(",".to_string());
                }
                parts.push(format!("{}={}", k, json_val_to_str(v)));
            }
        }
    }

    parts.concat()
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
