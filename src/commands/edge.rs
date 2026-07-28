use anyhow::{anyhow, Result};
use chrono::Local;
use std::path::PathBuf;

use edge::catalog::load_broker_catalog;
use edge::pipeline::run_pipeline;
use edge::summary::{DiscoveryEntry, DiscoverySummary, Verdict};
use edge::template::DiscoveryTemplate;

pub async fn run(
    template_path: String,
    broker: String,
    symbol_filter: Option<Vec<String>>,
    timeframes: Vec<String>,
    min_density_override: Option<f64>,
    resume_path: Option<PathBuf>,
) -> Result<()> {
    let session_id = format!("discover-edge-{}", Local::now().format("%Y%m%d_%H%M%S"));
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║           Edge Discoverer  (strategy-agnostic mode)          ║");
    println!("╚══════════════════════════════════════════════════════════════╝");
    println!("  Template : {template_path}");
    println!("  Broker   : {broker}");
    println!("  TFs      : {}", timeframes.join(", "));

    // 1. Load template
    let tpl = DiscoveryTemplate::load(&template_path)?;
    println!("  Strategy : {}", tpl.strategy);

    // 2. Resolve gates (merge template gates with CLI override)
    let mut gates = tpl.gates.clone().unwrap_or_default();
    if let Some(d_override) = min_density_override {
        gates.min_density = d_override;
    }
    println!("  Min density: {} R/yr", gates.min_density);

    // 3. Load broker catalog
    let mut catalog = load_broker_catalog(&broker)?;
    println!(
        "  Catalog  : {} symbols loaded from docs/broker/{}_SYMBOLS.md",
        catalog.len(),
        broker.to_uppercase()
    );

    let full_catalog = catalog.clone();

    // Apply optional symbol filter
    if let Some(ref filter) = symbol_filter {
        catalog.retain(|s| filter.contains(&s.symbol));
        println!(
            "  Filtered to: {}",
            catalog
                .iter()
                .map(|s| s.symbol.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    if catalog.is_empty() {
        return Err(anyhow!(
            "No symbols remain after filter. Check --symbols argument."
        ));
    }

    // 4. Handle resume mode
    let mut entries: Vec<DiscoveryEntry> = Vec::new();
    let mut skip_set = std::collections::HashSet::new();

    if let Some(ref r_path) = resume_path {
        if r_path.exists() {
            println!("  Resuming from prev run summary: {}", r_path.display());
            if let Ok(prev_summary) = DiscoverySummary::load_from_file(r_path) {
                for e in prev_summary.entries {
                    match &e.verdict {
                        Verdict::Passed | Verdict::FailedGate => {
                            skip_set.insert((e.symbol.clone(), e.tf.clone()));
                            entries.push(e);
                        }
                        Verdict::Generated | Verdict::Error(_) => {
                            // Re-run errors / generated
                        }
                    }
                }
            }
        }
    }

    let data_dir = PathBuf::from("data");

    // 5. Main loop
    for symbol in &catalog {
        for tf in &timeframes {
            if skip_set.contains(&(symbol.symbol.clone(), tf.clone())) {
                println!("  ▸ Skipping {} {} (already processed)", symbol.symbol, tf);
                continue;
            }

            println!("\n╔═══════════════════════════════════════════════════╗");
            println!(
                "║  PIPELINE  {}  {}  ({})",
                symbol.symbol,
                tf,
                format!("{:?}", symbol.asset_class).to_uppercase()
            );
            println!(
                "║  cost: lot=${:.2}  pct={:.6}  dir={}  range={}",
                symbol.commission_per_lot,
                symbol.commission_percent,
                symbol.trade_direction,
                symbol.backtest_range
            );
            println!("╚═══════════════════════════════════════════════════╝");

            // Run pipeline with error isolation
            let res = run_pipeline(
                &tpl,
                symbol,
                tf,
                &gates,
                &full_catalog,
                data_dir.clone(),
                &session_id,
            )
            .await;
            match res {
                Ok(entry) => {
                    match entry.verdict {
                        Verdict::Passed => {
                            println!(
                                "\n  ★ SUCCESS — Robust edge confirmed for {} {}!",
                                symbol.symbol, tf
                            );
                        }
                        Verdict::FailedGate => {
                            println!(
                                "\n  ✗ FAIL — Failed phase {} gate for {} {}.",
                                entry.phase_reached, symbol.symbol, tf
                            );
                        }
                        Verdict::Generated => {
                            println!(
                                "\n  ✗ GENERATED — Template generated for {} {}.",
                                symbol.symbol, tf
                            );
                        }
                        Verdict::Error(ref err) => {
                            println!(
                                "\n  ✗ ERROR — Phase {} error for {} {}: {}",
                                entry.phase_reached, symbol.symbol, tf, err
                            );
                        }
                    }
                    entries.push(entry);
                }
                Err(err) => {
                    println!(
                        "\n  ✗ UNEXPECTED SYSTEM ERROR for {} {}: {}",
                        symbol.symbol, tf, err
                    );
                    entries.push(DiscoveryEntry {
                        symbol: symbol.symbol.clone(),
                        tf: tf.clone(),
                        asset_class: format!("{:?}", symbol.asset_class),
                        phase_reached: 0,
                        verdict: Verdict::Error(err.to_string()),
                        wfe: 0.0,
                        consistency: 0.0,
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
                        trials_session_strategy: 0,
                        trials_historical_strategy: 0,
                        consistency_lcb: 0.0,
                        consensus_share: 0.0,
                        consensus_margin: 0.0,
                        regime_segments: 0,
                        holdout_start: "".to_string(),
                        holdout_end: "".to_string(),
                        consensus: serde_json::json!({}),
                        pf_threshold_used: gates.min_pf,
                        sharpe_threshold_used: gates.min_sharpe,
                        plateau_neighbor_profitable_count: 0,
                        stress_test_status: "not-run".to_string(),
                        stress_test_dd_pct: 0.0,
                        wfa_config_path: "".to_string(),
                        full_backtest_config_path: "".to_string(),
                        plateau_config_path: "".to_string(),
                        csv_path: "".to_string(),
                        oos_distribution_path: "".to_string(),
                    });
                }
            }
        }
    }

    // 6. Print summary & write file
    let ts = Local::now().format("%Y%m%d_%H%M%S").to_string();
    let summary = DiscoverySummary {
        created: Local::now().to_rfc3339(),
        template: template_path,
        broker,
        gates,
        entries,
    };

    summary.print_table();

    let out_path = PathBuf::from(format!("data/results/edge_discovery_{}.json", ts));
    summary.save_to_file(&out_path)?;
    println!("  ✓ Summary written to {}", out_path.display());

    Ok(())
}
