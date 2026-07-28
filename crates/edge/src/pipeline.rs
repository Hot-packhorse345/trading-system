use anyhow::{anyhow, Context, Result};
use backtest::engine::build_indicator_set_from_defs;
use backtest::grid::generate_combos;
use backtest::{canonical_value, BacktestConfig};
use broker::build_data_provider;
use chrono::{DateTime, Utc};
use data::OhlcvCache;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use strategy::build_strategy;
use ts_core::{parse_timeframe, Params};
use walkforward::WalkforwardConfig;

use crate::calibrate::{calibrate_wfa_windows, htf_timeframes, parse_range_years};
use crate::catalog::SymbolMeta;
use crate::consensus::{build_plateau_config, config_from_consensus, PlateauDimension};
use crate::correlation::rolling_pearson_correlation;
use crate::findings::append_finding;
use crate::gates::Gates;
use crate::ledger::{
    append_ledger, count_session_strategy_trials, count_strategy_trials, read_ledger,
    unique_holdout_phase2_hashes, TrialRecord,
};
use crate::stress_test::{inject_synthetic_shocks, seed_from_key};
use crate::summary::{DiscoveryEntry, Verdict};
use crate::template::{resolve_samples, DiscoveryTemplate};

const DAYS_PER_MONTH: f64 = 30.44;

pub struct BacktestRun {
    pub results: Vec<backtest::BacktestResult>,
    pub csv_path: Option<PathBuf>,
}

pub async fn run_pipeline(
    tpl: &DiscoveryTemplate,
    symbol: &SymbolMeta,
    tf: &str,
    gates: &Gates,
    full_catalog: &[SymbolMeta],
    data_dir: PathBuf,
    session_id: &str,
) -> Result<DiscoveryEntry> {
    let autogen_dir = "configs/walkforward/autogen";
    fs::create_dir_all(autogen_dir)?;

    let ledger_path = data_dir.join("results").join("trials_ledger.jsonl");
    let historical = read_ledger(&ledger_path).unwrap_or_default();
    let trials_historical_strategy = count_strategy_trials(&historical, &tpl.strategy) + 1;
    let trials_session_strategy =
        count_session_strategy_trials(&historical, &tpl.strategy, session_id) + 1;

    // Multiple-comparisons correction: the more times this strategy has been
    // tested historically, the higher the bar Phase 4 demands (see
    // Gates::fdr_adjusted_thresholds).
    let (pf_threshold, sharpe_threshold) =
        gates.fdr_adjusted_thresholds(trials_historical_strategy);

    let gates_hash = canonical_value(&serde_json::to_value(gates)?);
    let safe_sym = symbol.symbol.replace('.', "_");
    let htf = htf_timeframes(tf);

    let entry = match async {
        // 0. Ensure historical data is cached
        let (first_ts, last_ts, total_bars) = ensure_data(
            &symbol.symbol,
            tf,
            &symbol.backtest_range,
            tpl.data_provider(),
            &data_dir,
        )
        .await?;

        for htf_tf in &htf {
            let _ = ensure_data(
                &symbol.symbol,
                htf_tf,
                &symbol.backtest_range,
                tpl.data_provider(),
                &data_dir,
            )
            .await;
        }

        let total_span_seconds = last_ts - first_ts;
        let wfa_span_seconds = (total_span_seconds as f64 * 0.8) as i64;
        let wfa_end_ts = first_ts + wfa_span_seconds;
        let holdout_start_ts = wfa_end_ts;

        let start_iso = DateTime::from_timestamp(first_ts, 0)
            .unwrap_or_default()
            .with_timezone(&Utc)
            .format("%Y-%m-%d")
            .to_string();
        let end_iso = DateTime::from_timestamp(last_ts, 0)
            .unwrap_or_default()
            .with_timezone(&Utc)
            .format("%Y-%m-%d")
            .to_string();
        let wfa_end_iso = DateTime::from_timestamp(wfa_end_ts, 0)
            .unwrap_or_default()
            .with_timezone(&Utc)
            .format("%Y-%m-%d")
            .to_string();
        let actual_years = total_span_seconds as f64 / 31_557_600.0;

        // ─ Phase 1 config seed ──
        let probe_stop = {
            let first_mgr = tpl
                .stop_manager
                .first()
                .ok_or_else(|| anyhow!("No stop manager in template"))?
                .clone();
            let mut fixed_mgr = first_mgr.clone();
            if let Value::Object(ref mut map) = fixed_mgr {
                if let Some(dist_val) = map.get("stop_distance") {
                    let mut temp = dist_val.clone();
                    resolve_samples(&mut temp, None);
                    map.insert("stop_distance".to_string(), temp);
                }
                if let Some(rr_val) = map.get("start_rr") {
                    let mut temp = rr_val.clone();
                    resolve_samples(&mut temp, None);
                    map.insert("start_rr".to_string(), temp);
                }
            }
            fixed_mgr
        };

        let probe_config_val =
            tpl.build_fixed_config(symbol, tf, &htf, &start_iso, &end_iso, probe_stop, None);
        let probe_config: BacktestConfig = serde_json::from_value(probe_config_val)?;

        // ─ Phase 0: repaint gate ──
        let cache = OhlcvCache::new(&data_dir);
        if strategy_repaint_detected(&probe_config, &cache)? {
            return Ok::<DiscoveryEntry, anyhow::Error>(create_failed_entry(
                symbol,
                tf,
                0,
                json!({}),
                PhaseMetrics::default(),
                trials_session_strategy,
                trials_historical_strategy,
                &ts_to_date(holdout_start_ts),
                &end_iso,
                pf_threshold,
                sharpe_threshold,
                "",
            ));
        }

        // ─ Phase 1: Probe (Trade Density Baseline) ──
        let probe_run = backtest_execute(probe_config, 1, None).await?;
        let probe_trades = probe_run
            .results
            .first()
            .map(|r| r.metrics.total_trades)
            .unwrap_or(0);

        if probe_trades < gates.min_probe_trades {
            return Ok::<DiscoveryEntry, anyhow::Error>(create_failed_entry(
                symbol,
                tf,
                1,
                json!({}),
                PhaseMetrics {
                    trades: probe_trades,
                    ..Default::default()
                },
                trials_session_strategy,
                trials_historical_strategy,
                &ts_to_date(holdout_start_ts),
                &end_iso,
                pf_threshold,
                sharpe_threshold,
                "",
            ));
        }

        // Calibrate WFA windows
        let wfa_bars_count = cache
            .load(&symbol.symbol, parse_timeframe(tf)?, first_ts, wfa_end_ts)?
            .len();

        let (is_bars, oos_bars, step_bars) = calibrate_wfa_windows(
            probe_trades,
            total_bars,
            wfa_bars_count,
            gates.target_is_trades,
            gates.min_rounds,
        )?;

        // ─ Phase 2: Walk-Forward Optimization ──
        let mut wfa_config_val = tpl.build_wfa_config(
            symbol,
            tf,
            &htf,
            &start_iso,
            &wfa_end_iso,
            is_bars,
            oos_bars,
            step_bars,
            gates,
        );
        wfa_config_val["min_oos_trades_per_round"] = json!(gates.min_oos_trades_per_round);
        wfa_config_val["min_oos_consistency_lcb"] = json!(gates.min_oos_consistency_lcb);
        wfa_config_val["consistency_confidence_z"] = json!(gates.consistency_confidence_z);

        let wfa_path = format!("{autogen_dir}/wfa_{safe_sym}_{tf}.json");
        write_json(&wfa_path, &wfa_config_val)?;

        let base_config: BacktestConfig = serde_json::from_value(wfa_config_val.clone())?;
        let wf_config: WalkforwardConfig = serde_json::from_value(wfa_config_val.clone())?;

        let wfa_report = walkforward_execute(base_config, wf_config, None).await?;
        if !wfa_report.passed {
            return Ok::<DiscoveryEntry, anyhow::Error>(create_failed_entry(
                symbol,
                tf,
                2,
                json!({}),
                PhaseMetrics {
                    trades: probe_trades,
                    wfe: wfa_report.wf_efficiency,
                    consistency: wfa_report.oos_consistency,
                    consistency_lcb: wfa_report.oos_consistency_lcb,
                    ..Default::default()
                },
                trials_session_strategy,
                trials_historical_strategy,
                &ts_to_date(holdout_start_ts),
                &end_iso,
                pf_threshold,
                sharpe_threshold,
                "",
            ));
        }

        let (consensus_val, consensus_count) = wfa_report.consensus().ok_or_else(|| {
            anyhow!("Walk-forward passed but failed to resolve consensus parameters")
        })?;
        let consensus_hash = canonical_value(&consensus_val);
        let total_rounds = wfa_report.rounds.len().max(1);
        let runner_up_count = consensus_runner_up_count(&wfa_report.rounds, &consensus_hash);
        let consensus_share = consensus_count as f64 / total_rounds as f64;
        let consensus_margin =
            (consensus_count.saturating_sub(runner_up_count)) as f64 / total_rounds as f64;

        // ─ Phase 3: Dual-Metric Confirmation ──
        let metric = tpl.metric().to_string();
        let alt_metric = if metric == "enhanced_score" {
            "profit_factor".to_string()
        } else {
            "enhanced_score".to_string()
        };

        let mut alt_wfa_config_val = wfa_config_val.clone();
        alt_wfa_config_val["metric"] = json!(alt_metric);
        let alt_wfa_path = format!("{autogen_dir}/wfa_alt_{safe_sym}_{tf}.json");
        write_json(&alt_wfa_path, &alt_wfa_config_val)?;

        let alt_base: BacktestConfig = serde_json::from_value(alt_wfa_config_val.clone())?;
        let alt_wf: WalkforwardConfig = serde_json::from_value(alt_wfa_config_val.clone())?;

        let alt_report = walkforward_execute(alt_base, alt_wf, None).await?;
        if !alt_report.passed {
            return Ok::<DiscoveryEntry, anyhow::Error>(create_failed_entry(
                symbol,
                tf,
                3,
                consensus_val,
                PhaseMetrics {
                    trades: probe_trades,
                    wfe: wfa_report.wf_efficiency,
                    consistency: wfa_report.oos_consistency,
                    consistency_lcb: wfa_report.oos_consistency_lcb,
                    ..Default::default()
                },
                trials_session_strategy,
                trials_historical_strategy,
                &ts_to_date(holdout_start_ts),
                &end_iso,
                pf_threshold,
                sharpe_threshold,
                "",
            ));
        }

        // ─ Persist OOS reference distribution for live decay monitoring ──
        // Expressed as "mean R per trade per round" so it's directly
        // comparable to a live rolling window computed the same way.
        let oos_mean_r_per_trade: Vec<f64> = wfa_report
            .rounds
            .iter()
            .map(|r| r.oos_total_r / (r.oos_trades.max(1) as f64))
            .collect();
        let oos_dist_dir = data_dir.join("results").join("oos_distributions");
        fs::create_dir_all(&oos_dist_dir)?;
        let oos_dist_path = oos_dist_dir.join(format!("{}_{safe_sym}_{tf}.json", tpl.strategy));
        write_json(
            &oos_dist_path.to_string_lossy(),
            &json!({
                "strategy": tpl.strategy,
                "symbol": symbol.symbol,
                "tf": tf,
                "generated_at": Utc::now().to_rfc3339(),
                "oos_mean_r_per_trade": oos_mean_r_per_trade,
            }),
        )?;
        let oos_dist_path_str = oos_dist_path.to_string_lossy().to_string();

        // ─ Holdout reuse cap ──
        let seen_hashes = unique_holdout_phase2_hashes(
            &historical,
            &tpl.strategy,
            &symbol.symbol,
            tf,
            &ts_to_date(holdout_start_ts),
            &end_iso,
        );
        if !seen_hashes.contains(&consensus_hash)
            && seen_hashes.len() >= gates.max_holdout_evaluations
        {
            return Ok::<DiscoveryEntry, anyhow::Error>(create_failed_entry(
                symbol,
                tf,
                4,
                consensus_val,
                PhaseMetrics {
                    trades: probe_trades,
                    wfe: wfa_report.wf_efficiency,
                    consistency: wfa_report.oos_consistency,
                    consistency_lcb: wfa_report.oos_consistency_lcb,
                    ..Default::default()
                },
                trials_session_strategy,
                trials_historical_strategy,
                &ts_to_date(holdout_start_ts),
                &end_iso,
                pf_threshold,
                sharpe_threshold,
                &oos_dist_path_str,
            ));
        }

        // ─ Phase 4: Full-Range Backtest ──
        let full_config_val =
            config_from_consensus(tpl, symbol, tf, &consensus_val, &start_iso, &end_iso);
        let full_path = format!("{autogen_dir}/full_{safe_sym}_{tf}.json");
        write_json(&full_path, &full_config_val)?;

        let full_config: BacktestConfig = serde_json::from_value(full_config_val.clone())?;
        let full_run = backtest_execute(full_config, 1, None).await?;
        let full_res = full_run
            .results
            .first()
            .ok_or_else(|| anyhow!("Full-range backtest returned no results"))?;

        let pf = full_res.metrics.profit_factor;
        let sharpe = full_res.metrics.sharpe_annualized;
        let wr = full_res.metrics.win_rate;
        let dd = full_res.metrics.max_drawdown;
        let trades = full_res.metrics.total_trades;
        let net_r = full_res.metrics.total_r;
        let density = if actual_years > 0.0 {
            net_r / actual_years
        } else {
            0.0
        };
        let full_csv_path = full_run.csv_path.clone().unwrap_or_default();

        let avg_open_time_secs = full_res.metrics.avg_open_time_secs;
        let avg_open_time_days = avg_open_time_secs / 86400.0;

        let open_time_exceeded = gates.max_avg_open_time_days > 0.0
            && avg_open_time_days > gates.max_avg_open_time_days;

        if pf < pf_threshold
            || sharpe < sharpe_threshold
            || trades < gates.min_trades
            || wr < gates.min_wr
            || dd > gates.max_dd_pct
            || density < gates.min_density
            || open_time_exceeded
        {
            return Ok::<DiscoveryEntry, anyhow::Error>(create_failed_entry(
                symbol,
                tf,
                4,
                consensus_val,
                PhaseMetrics {
                    trades,
                    wfe: wfa_report.wf_efficiency,
                    consistency: wfa_report.oos_consistency,
                    consistency_lcb: wfa_report.oos_consistency_lcb,
                    pf,
                    sharpe,
                    wr,
                    dd,
                    net_r,
                    density,
                    avg_open_time_days
                },
                trials_session_strategy,
                trials_historical_strategy,
                &ts_to_date(holdout_start_ts),
                &end_iso,
                pf_threshold,
                sharpe_threshold,
                &oos_dist_path_str,
            ));
        }

        // ─ Phase 5: Plateau Robustness ──
        let (plat_config_val, plat_dims) =
            build_plateau_config(tpl, symbol, tf, &htf, &consensus_val, &start_iso, &end_iso)?;
        let plat_path = format!("{autogen_dir}/plateau_{safe_sym}_{tf}.json");
        write_json(&plat_path, &plat_config_val)?;

        let plat_config: BacktestConfig = serde_json::from_value(plat_config_val.clone())?;
        let plat_run = backtest_execute(plat_config, 100, None).await?;
        let profitable = plat_run
            .results
            .iter()
            .filter(|r| r.metrics.profit_factor >= gates.plateau_min_pf)
            .count();
        let total_plat = plat_run.results.len();
        let plateau_pass_rate = if total_plat > 0 {
            profitable as f64 / total_plat as f64
        } else {
            0.0
        };
        let plateau_neighbor_profitable_count =
            count_profitable_neighbors(&plat_run.results, &plat_dims, gates.plateau_min_pf);

        if plateau_pass_rate < gates.plateau_pass_rate
            && plateau_neighbor_profitable_count < gates.plateau_min_neighbors
        {
            let mut e = create_failed_entry(
                symbol,
                tf,
                5,
                consensus_val,
                PhaseMetrics {
                    trades,
                    wfe: wfa_report.wf_efficiency,
                    consistency: wfa_report.oos_consistency,
                    consistency_lcb: wfa_report.oos_consistency_lcb,
                    pf,
                    sharpe,
                    wr,
                    dd,
                    net_r,
                    density,
                    avg_open_time_days
                },
                trials_session_strategy,
                trials_historical_strategy,
                &ts_to_date(holdout_start_ts),
                &end_iso,
                pf_threshold,
                sharpe_threshold,
                &oos_dist_path_str,
            );
            e.plateau_pass_rate = plateau_pass_rate;
            e.plateau_neighbor_profitable_count = plateau_neighbor_profitable_count;
            return Ok::<DiscoveryEntry, anyhow::Error>(e);
        }

        // ─ Phase 6: Synthetic Stress Test (Black-Swan Injection) ──
        let full_bars = cache.load(&symbol.symbol, parse_timeframe(tf)?, first_ts, last_ts)?;
        let stress_seed = seed_from_key(&format!("{}_{tf}", symbol.symbol));
        let shocked_bars = inject_synthetic_shocks(&full_bars, stress_seed);
        let stress_symbol_name = format!("{}__STRESS", symbol.symbol);
        cache.save(&stress_symbol_name, parse_timeframe(tf)?, &shocked_bars)?;

        let mut stress_symbol = symbol.clone();
        stress_symbol.symbol = stress_symbol_name;
        let stress_config_val = config_from_consensus(
            tpl,
            &stress_symbol,
            tf,
            &consensus_val,
            &start_iso,
            &end_iso,
        );
        let stress_config: BacktestConfig = serde_json::from_value(stress_config_val)?;
        let stress_run = backtest_execute(stress_config, 1, None).await?;
        let stress_dd_pct = stress_run
            .results
            .first()
            .map(|r| r.metrics.max_drawdown_pct)
            .unwrap_or(0.0);
        let stress_test_passed = stress_dd_pct <= gates.max_stress_dd_pct;

        if !stress_test_passed {
            let mut e = create_failed_entry(
                symbol,
                tf,
                6,
                consensus_val,
                PhaseMetrics {
                    trades,
                    wfe: wfa_report.wf_efficiency,
                    consistency: wfa_report.oos_consistency,
                    consistency_lcb: wfa_report.oos_consistency_lcb,
                    pf,
                    sharpe,
                    wr,
                    dd,
                    net_r,
                    density,
                    avg_open_time_days
                },
                trials_session_strategy,
                trials_historical_strategy,
                &ts_to_date(holdout_start_ts),
                &end_iso,
                pf_threshold,
                sharpe_threshold,
                &oos_dist_path_str,
            );
            e.plateau_pass_rate = plateau_pass_rate;
            e.plateau_neighbor_profitable_count = plateau_neighbor_profitable_count;
            e.stress_test_status = "failed".to_string();
            e.stress_test_dd_pct = stress_dd_pct;
            return Ok::<DiscoveryEntry, anyhow::Error>(e);
        }

        // ─ Phase 7: Cross-Asset Validation ──
        let mut cross_symbol_name = "N/A".to_string();
        let mut cross_pf = 0.0;
        let mut cross_net_r = 0.0;
        let mut cross_trades = 0;
        let mut cross_asset_status = "unavailable".to_string();
        let mut cross_asset_correlation: Option<f64> = None;

        if let Some(ref corr_name) = symbol.correlated_asset {
            cross_symbol_name = corr_name.clone();
            if let Some(corr_sym) = full_catalog.iter().find(|s| &s.symbol == corr_name) {
                let _ = ensure_data(
                    &corr_sym.symbol,
                    tf,
                    &symbol.backtest_range,
                    tpl.data_provider(),
                    &data_dir,
                )
                .await;
                let corr_bars = cache
                    .load(&corr_sym.symbol, parse_timeframe(tf)?, first_ts, last_ts)
                    .unwrap_or_default();
                let correlation = rolling_pearson_correlation(
                    &full_bars,
                    &corr_bars,
                    gates.correlation_window_months * DAYS_PER_MONTH,
                );
                cross_asset_correlation = correlation;

                let decorrelated = correlation
                    .map(|c| c < gates.min_cross_asset_correlation)
                    .unwrap_or(false);

                if decorrelated {
                    cross_asset_status = "skipped-decorrelated".to_string();
                    tracing::warn!(
                        symbol = %symbol.symbol,
                        cross = %corr_name,
                        correlation = ?correlation,
                        threshold = gates.min_cross_asset_correlation,
                        "cross-asset validation skipped: rolling correlation dropped below threshold"
                    );
                } else {
                    let cross_config_val = config_from_consensus(
                        tpl,
                        corr_sym,
                        tf,
                        &consensus_val,
                        &start_iso,
                        &end_iso,
                    );
                    let cross_config: BacktestConfig = serde_json::from_value(cross_config_val)?;

                    if let Ok(cross_run) = backtest_execute(cross_config, 1, None).await {
                        if let Some(cross_res) = cross_run.results.first() {
                            cross_pf = cross_res.metrics.profit_factor;
                            cross_net_r = cross_res.metrics.total_r;
                            cross_trades = cross_res.metrics.total_trades;
                            cross_asset_status = if cross_pf >= gates.cross_min_pf {
                                "confirmed".to_string()
                            } else {
                                "failed".to_string()
                            };
                        }
                    }
                }
            }
        }

        if gates.require_cross_asset && cross_asset_status != "confirmed" {
            let mut e = create_failed_entry(
                symbol,
                tf,
                7,
                consensus_val,
                PhaseMetrics {
                    trades,
                    wfe: wfa_report.wf_efficiency,
                    consistency: wfa_report.oos_consistency,
                    consistency_lcb: wfa_report.oos_consistency_lcb,
                    pf,
                    sharpe,
                    wr,
                    dd,
                    net_r,
                    density,
                    avg_open_time_days
                },
                trials_session_strategy,
                trials_historical_strategy,
                &ts_to_date(holdout_start_ts),
                &end_iso,
                pf_threshold,
                sharpe_threshold,
                &oos_dist_path_str,
            );
            e.plateau_pass_rate = plateau_pass_rate;
            e.plateau_neighbor_profitable_count = plateau_neighbor_profitable_count;
            e.stress_test_status = "passed".to_string();
            e.stress_test_dd_pct = stress_dd_pct;
            e.cross_asset_status = cross_asset_status;
            e.cross_asset_correlation = cross_asset_correlation;
            return Ok::<DiscoveryEntry, anyhow::Error>(e);
        }

        let regime_segments = estimate_regime_segments(&full_bars);

        // ─ Passed ─
        let entry = DiscoveryEntry {
            symbol: symbol.symbol.clone(),
            tf: tf.to_string(),
            asset_class: format!("{:?}", symbol.asset_class),
            phase_reached: 7,
            verdict: Verdict::Passed,
            wfe: wfa_report.wf_efficiency,
            consistency: wfa_report.oos_consistency,
            consistency_lcb: wfa_report.oos_consistency_lcb,
            pf,
            sharpe,
            wr,
            dd,
            trades,
            net_r,
            density,
            avg_open_time_days,
            plateau_pass_rate,
            cross_symbol: cross_symbol_name,
            cross_pf,
            cross_net_r,
            cross_trades,
            cross_asset_status,
            cross_asset_required: gates.require_cross_asset,
            cross_asset_correlation,
            trials_session_strategy,
            trials_historical_strategy,
            consensus_share,
            consensus_margin,
            regime_segments,
            holdout_start: ts_to_date(holdout_start_ts),
            holdout_end: end_iso.clone(),
            consensus: consensus_val,
            pf_threshold_used: pf_threshold,
            sharpe_threshold_used: sharpe_threshold,
            plateau_neighbor_profitable_count,
            stress_test_status: "passed".to_string(),
            stress_test_dd_pct: stress_dd_pct,
            wfa_config_path: wfa_path,
            full_backtest_config_path: full_path.clone(),
            plateau_config_path: plat_path,
            csv_path: full_csv_path.to_string_lossy().to_string(),
            oos_distribution_path: oos_dist_path_str,
        };

        append_finding(&tpl.strategy, &entry, gates)?;
        Ok::<DiscoveryEntry, anyhow::Error>(entry)
    }
    .await
    {
        Ok(entry) => entry,
        Err(err) => DiscoveryEntry {
            symbol: symbol.symbol.clone(),
            tf: tf.to_string(),
            asset_class: format!("{:?}", symbol.asset_class),
            phase_reached: 0,
            verdict: Verdict::Error(err.to_string()),
            wfe: 0.0,
            consistency: 0.0,
            consistency_lcb: 0.0,
            pf: 0.0,
            sharpe: 0.0,
            wr: 0.0,
            dd: 0.0,
            trades: 0,
            net_r: 0.0,
            density: 0.0,
            avg_open_time_days: 0.0,
            plateau_pass_rate: 0.0,
            cross_symbol: "N/A".to_string(),
            cross_pf: 0.0,
            cross_net_r: 0.0,
            cross_trades: 0,
            cross_asset_status: "not-run".to_string(),
            cross_asset_required: gates.require_cross_asset,
            cross_asset_correlation: None,
            trials_session_strategy,
            trials_historical_strategy,
            consensus_share: 0.0,
            consensus_margin: 0.0,
            regime_segments: 0,
            holdout_start: "".to_string(),
            holdout_end: "".to_string(),
            consensus: json!({}),
            pf_threshold_used: pf_threshold,
            sharpe_threshold_used: sharpe_threshold,
            plateau_neighbor_profitable_count: 0,
            stress_test_status: "not-run".to_string(),
            stress_test_dd_pct: 0.0,
            wfa_config_path: "".to_string(),
            full_backtest_config_path: "".to_string(),
            plateau_config_path: "".to_string(),
            csv_path: "".to_string(),
            oos_distribution_path: "".to_string(),
        },
    };

    append_ledger(
        &ledger_path,
        &TrialRecord {
            session_id: session_id.to_string(),
            timestamp: Utc::now().to_rfc3339(),
            strategy: tpl.strategy.clone(),
            symbol: symbol.symbol.clone(),
            tf: tf.to_string(),
            gates_hash,
            holdout_start: Some(entry.holdout_start.clone()),
            holdout_end: Some(entry.holdout_end.clone()),
            phase2_hash: if entry.phase_reached >= 2 {
                Some(canonical_value(&entry.consensus))
            } else {
                None
            },
            phase_reached: entry.phase_reached,
            verdict: verdict_str(&entry.verdict),
            cross_asset_status: Some(entry.cross_asset_status.clone()),
            wfe: entry.wfe,
            consistency: entry.consistency,
            consistency_lcb: entry.consistency_lcb,
            pf: entry.pf,
            sharpe: entry.sharpe,
            trades: entry.trades,
        },
    )?;

    Ok(entry)
}

pub async fn backtest_execute(
    cfg: BacktestConfig,
    top: usize,
    metric: Option<String>,
) -> Result<BacktestRun> {
    let output_dir = cfg.output_dir.clone();
    let results =
        tokio::task::spawn_blocking(move || backtest::run(&cfg, top, metric.as_deref())).await??;
    tracing::info!(results = %results.len(), "backtest finished");

    if results.is_empty() {
        tracing::info!("no results to write");
        return Ok(BacktestRun {
            results,
            csv_path: None,
        });
    }

    std::fs::create_dir_all(&output_dir)?;
    let ts = Utc::now().format("%Y%m%d_%H%M").to_string();
    let csv = output_dir.join(format!("results_{ts}.csv"));
    backtest::write_csv(&results, &csv)?;
    tracing::info!(csv = %csv.display(), "results written");

    Ok(BacktestRun {
        results,
        csv_path: Some(csv),
    })
}

pub async fn walkforward_execute(
    base: BacktestConfig,
    wf: WalkforwardConfig,
    output_path: Option<PathBuf>,
) -> Result<walkforward::WfReport> {
    let symbol =
        first_str(&base.symbol).ok_or_else(|| anyhow!("config 'symbol' missing/invalid"))?;
    let tf_str =
        first_str(&base.timeframe).ok_or_else(|| anyhow!("config 'timeframe' missing/invalid"))?;
    let tf = parse_timeframe(&tf_str)?;

    let (start, end) = base.date_range()?;

    let provider = build_data_provider(&base.data_provider)?;
    let cache = OhlcvCache::new(&base.data_dir);

    let mut bars = cache.load(&symbol, tf, start, end)?;
    if bars.len() < wf.is_bars + wf.oos_bars {
        tracing::info!(symbol = %symbol, tf = ?tf, "insufficient cached bars — downloading");
        let downloaded = provider.ohlcv(&symbol, tf, start, end).await?;
        if !downloaded.is_empty() {
            cache.save(&symbol, tf, &downloaded)?;
            bars = cache.load(&symbol, tf, start, end)?;
        }
    }
    let symbol_info = provider.symbol_info(&symbol).await?;
    drop(provider);

    tracing::info!(symbol = %symbol, tf = ?tf, bars = %bars.len(), "walk-forward starting");

    let output_dir = base.output_dir.clone();

    let report = tokio::task::spawn_blocking(move || {
        walkforward::run(&base, &wf, &symbol, tf, &bars, &symbol_info)
    })
    .await??;

    report.print();

    let out_path = output_path.unwrap_or_else(|| {
        let ts = Utc::now().format("%Y%m%d_%H%M").to_string();
        output_dir.join(format!("wf_report_{ts}.html"))
    });

    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = if out_path.extension().and_then(|s| s.to_str()) == Some("html") {
        report.to_html()
    } else {
        report.to_markdown()
    };
    std::fs::write(&out_path, content)
        .with_context(|| format!("failed to write report to {}", out_path.display()))?;
    tracing::info!(path = %out_path.display(), "walk-forward report written");

    Ok(report)
}

fn first_str(v: &serde_json::Value) -> Option<String> {
    match v {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Array(a) => a.first().and_then(|x| x.as_str().map(|s| s.to_string())),
        _ => None,
    }
}

async fn ensure_data(
    symbol: &str,
    tf_str: &str,
    range_str: &str,
    data_provider_name: &str,
    data_dir: &Path,
) -> Result<(i64, i64, usize)> {
    let tf = parse_timeframe(tf_str)?;
    let cache = OhlcvCache::new(data_dir);

    let years = parse_range_years(range_str)?;
    let total_days = (years * 365.25) as i64;
    let now = Utc::now();
    let start = (now - chrono::Duration::days(total_days)).timestamp();
    let end = now.timestamp();

    if let Some((first, last, count)) = cache.time_range(symbol, tf, start, end)? {
        if count >= 2000 {
            return Ok((first, last, count));
        }
    }

    let provider = build_data_provider(data_provider_name)?;
    tracing::info!(
        "Downloading historical data for {} {} from {} to {}",
        symbol,
        tf_str,
        start,
        end
    );
    let downloaded = provider.ohlcv(symbol, tf, start, end).await?;
    if !downloaded.is_empty() {
        cache.save(symbol, tf, &downloaded)?;
    }

    let final_range = cache.time_range(symbol, tf, start, end)?.ok_or_else(|| {
        anyhow!(
            "No data available for {} {} after download attempt",
            symbol,
            tf_str
        )
    })?;
    Ok(final_range)
}

fn write_json(path: &str, value: &Value) -> Result<()> {
    if let Some(parent) = Path::new(path).parent() {
        fs::create_dir_all(parent)?;
    }
    let mut f =
        fs::File::create(path).with_context(|| format!("Cannot create config file: {path}"))?;
    serde_json::to_writer_pretty(&mut f, value)?;
    Ok(())
}

/// Bundles the metrics that vary by pipeline phase so `create_failed_entry`
/// doesn't need a growing list of positional f64/usize arguments.
#[derive(Default)]
struct PhaseMetrics {
    trades: usize,
    wfe: f64,
    consistency: f64,
    consistency_lcb: f64,
    pf: f64,
    sharpe: f64,
    wr: f64,
    dd: f64,
    net_r: f64,
    density: f64,
    avg_open_time_days: f64,
}

#[allow(clippy::too_many_arguments)]
fn create_failed_entry(
    symbol: &SymbolMeta,
    tf: &str,
    phase: usize,
    consensus: Value,
    m: PhaseMetrics,
    trials_session_strategy: usize,
    trials_historical_strategy: usize,
    holdout_start: &str,
    holdout_end: &str,
    pf_threshold_used: f64,
    sharpe_threshold_used: f64,
    oos_distribution_path: &str,
) -> DiscoveryEntry {
    DiscoveryEntry {
        symbol: symbol.symbol.clone(),
        tf: tf.to_string(),
        asset_class: format!("{:?}", symbol.asset_class),
        phase_reached: phase,
        verdict: Verdict::FailedGate,
        wfe: m.wfe,
        consistency: m.consistency,
        consistency_lcb: m.consistency_lcb,
        pf: m.pf,
        sharpe: m.sharpe,
        wr: m.wr,
        dd: m.dd,
        trades: m.trades,
        net_r: m.net_r,
        density: m.density,
        avg_open_time_days: m.avg_open_time_days,
        plateau_pass_rate: 0.0,
        cross_symbol: "N/A".to_string(),
        cross_pf: 0.0,
        cross_net_r: 0.0,
        cross_trades: 0,
        cross_asset_status: "not-run".to_string(),
        cross_asset_required: false,
        cross_asset_correlation: None,
        trials_session_strategy,
        trials_historical_strategy,
        consensus_share: 0.0,
        consensus_margin: 0.0,
        regime_segments: 0,
        holdout_start: holdout_start.to_string(),
        holdout_end: holdout_end.to_string(),
        consensus,
        pf_threshold_used,
        sharpe_threshold_used,
        plateau_neighbor_profitable_count: 0,
        stress_test_status: "not-run".to_string(),
        stress_test_dd_pct: 0.0,
        wfa_config_path: "".to_string(),
        full_backtest_config_path: "".to_string(),
        plateau_config_path: "".to_string(),
        csv_path: "".to_string(),
        oos_distribution_path: oos_distribution_path.to_string(),
    }
}

/// Topological-connectivity check for Phase 5 (see `PlateauDimension`): a
/// plateau-grid result counts as an "immediate neighbor" of the consensus
/// point if exactly one dimension's grid index differs from consensus by
/// exactly 1 step and every other dimension matches consensus exactly.
fn count_profitable_neighbors(
    results: &[backtest::BacktestResult],
    dims: &[PlateauDimension],
    min_pf: f64,
) -> usize {
    if dims.is_empty() {
        return 0;
    }

    let mut count = 0;
    for r in results {
        let config_map: HashMap<&str, &str> = r
            .config
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();

        let mut single_step_diffs = 0usize;
        let mut valid = true;
        for dim in dims {
            let Some(val_str) = config_map.get(dim.key.as_str()) else {
                valid = false;
                break;
            };
            let Ok(val_f64) = val_str.parse::<f64>() else {
                valid = false;
                break;
            };
            let Some(idx) = dim
                .values
                .iter()
                .position(|v| (v - val_f64).abs() < 1e-9)
            else {
                valid = false;
                break;
            };
            let delta = idx as i64 - dim.consensus_index as i64;
            match delta {
                0 => continue,
                1 | -1 => {
                    single_step_diffs += 1;
                    if single_step_diffs > 1 {
                        valid = false;
                        break;
                    }
                }
                _ => {
                    valid = false;
                    break;
                }
            }
        }

        if valid && single_step_diffs == 1 && r.metrics.profit_factor >= min_pf {
            count += 1;
        }
    }
    count
}

fn strategy_repaint_detected(cfg: &BacktestConfig, cache: &OhlcvCache) -> Result<bool> {
    let combos = generate_combos(cfg)?;
    let combo = combos
        .first()
        .ok_or_else(|| anyhow!("no combo generated for repaint gate"))?;

    let (start, end) = cfg.date_range()?;
    let mut bars = cache.load(&combo.symbol, combo.timeframe, start, end)?;
    if bars.len() < 300 {
        return Ok(false);
    }
    if bars.len() > 600 {
        bars = bars[bars.len() - 600..].to_vec();
    }
    let warmup = 100.min(bars.len().saturating_sub(2));

    let cols = build_indicator_set_from_defs(&combo.indicators, &bars, None)?;
    let strat = build_strategy(&combo.strategy_name)?;
    let params = Params(combo.strategy_params.clone());
    let ind_params: HashMap<String, Params> = combo
        .indicators
        .iter()
        .map(|d| (d.name.clone(), Params(d.params.clone())))
        .collect();

    let full_signals = strat.generate_signals(&bars, &cols, &params, &ind_params);
    for i in (warmup + 2)..=bars.len() {
        let slice = &bars[..i];
        let slice_cols = build_indicator_set_from_defs(&combo.indicators, slice, None)?;
        let slice_signals = strat.generate_signals(slice, &slice_cols, &params, &ind_params);
        let idx = i - 2;
        let full_dir = signal_dir(&full_signals[idx]);
        let slice_dir = signal_dir(&slice_signals[idx]);
        if full_dir != slice_dir {
            return Ok(true);
        }
    }
    Ok(false)
}

fn consensus_runner_up_count(rounds: &[walkforward::WfRound], consensus_hash: &str) -> usize {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for r in rounds {
        let key = canonical_value(&r.params);
        *counts.entry(key).or_insert(0) += 1;
    }
    counts
        .into_iter()
        .filter(|(k, _)| k != consensus_hash)
        .map(|(_, v)| v)
        .max()
        .unwrap_or(0)
}

fn estimate_regime_segments(bars: &[ts_core::Bar]) -> usize {
    if bars.len() < 60 {
        return 1;
    }
    let mut vols = Vec::new();
    let chunk = (bars.len() / 12).max(30);
    let mut i = 1usize;
    while i + chunk < bars.len() {
        let mut rets = Vec::with_capacity(chunk);
        for j in i..(i + chunk) {
            let p0 = bars[j - 1].close;
            let p1 = bars[j].close;
            if p0 > 0.0 {
                rets.push((p1 / p0).ln());
            }
        }
        if !rets.is_empty() {
            let mean = rets.iter().sum::<f64>() / rets.len() as f64;
            let var = rets.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / rets.len() as f64;
            vols.push(var.sqrt());
        }
        i += chunk;
    }
    if vols.is_empty() {
        return 1;
    }
    let mut sorted = vols.clone();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let t1 = sorted[sorted.len() / 3];
    let t2 = sorted[(sorted.len() * 2) / 3];

    let mut last = 3u8;
    let mut segments = 0usize;
    for v in vols {
        let cls = if v <= t1 {
            0
        } else if v <= t2 {
            1
        } else {
            2
        };
        if cls != last {
            segments += 1;
            last = cls;
        }
    }
    segments.max(1)
}

fn verdict_str(v: &Verdict) -> String {
    match v {
        Verdict::Passed => "Passed".to_string(),
        Verdict::FailedGate => "FailedGate".to_string(),
        Verdict::Generated => "Generated".to_string(),
        Verdict::Error(e) => format!("Error:{e}"),
    }
}

fn ts_to_date(ts: i64) -> String {
    DateTime::from_timestamp(ts, 0)
        .unwrap_or_default()
        .with_timezone(&Utc)
        .format("%Y-%m-%d")
        .to_string()
}

fn signal_dir(s: &Option<ts_core::Signal>) -> i8 {
    match s {
        Some(sig) if sig.is_valid() => match sig.direction {
            ts_core::Direction::Buy => 1,
            ts_core::Direction::Sell => -1,
            _ => 0,
        },
        _ => 0,
    }
}
