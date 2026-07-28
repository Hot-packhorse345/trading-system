use anyhow::{Context, Result};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use tracing::info;

use super::repaint_common::{classify_repaint_type, severity_label};
use super::utils::tail_bars;
use data::OhlcvCache;
use indicators::{build, IndicatorConfig};
use ts_core::parse_timeframe;
use ts_core::Bar;

pub async fn run_cmd(
    config: Option<PathBuf>,
    indicator: Option<String>,
    indicator_cfg: Option<String>,
    symbol: String,
    timeframe: String,
    bars: usize,
    warmup: usize,
    shift_step: usize,
    test_mode: String,
    export: Option<PathBuf>,
    data_dir: PathBuf,
    verbose: bool,
) -> Result<()> {
    let ind_cfgs = super::super::resolve_indicators(config, indicator, indicator_cfg)?;
    let tf = parse_timeframe(&timeframe)?;

    let all_bars = OhlcvCache::new(&data_dir)
        .load_all(&symbol, tf)
        .context("no bars — run `download` first")?;

    if all_bars.is_empty() {
        anyhow::bail!("no bars for {symbol}/{timeframe} in {}", data_dir.display());
    }

    let test_bars: Vec<Bar> = tail_bars(all_bars, bars);

    info!(symbol=%symbol, tf=%timeframe, bars=%test_bars.len(), "repaint check starting");

    let mut all_results = Vec::new();

    for cfg in &ind_cfgs {
        let effective_warmup = if warmup == 0 {
            default_warmup(&cfg.kind)
        } else {
            warmup
        };

        let result = check_indicator(
            cfg,
            &test_bars,
            &symbol,
            &timeframe,
            effective_warmup,
            shift_step,
            &test_mode,
            verbose,
        );

        print_report(&result);
        all_results.push(result);
    }

    if let Some(out) = export {
        let json = serde_json::to_string_pretty(&all_results).unwrap_or_default();
        std::fs::create_dir_all(out.parent().unwrap_or(std::path::Path::new(".")))?;
        std::fs::write(&out, &json)?;
        println!("✅ Detailed results exported to: {}", out.display());
    }

    let any_repaint = all_results.iter().any(|r| r.repainted);
    std::process::exit(if any_repaint { 1 } else { 0 });
}

// ── Data structures ───────────────────────────────────────────────────────────

#[derive(Debug, serde::Serialize)]
struct RepaintResult {
    indicator_name: String,
    symbol: String,
    timeframe: String,
    total_bars: usize,
    warmup_bars: usize,

    forward_repainted: bool,
    forward_repainted_columns: Vec<String>,
    forward_repainted_timestamps: usize,
    forward_total_comparisons: usize,
    forward_severity: f64,

    origin_repainted: bool,
    origin_repainted_columns: Vec<String>,
    origin_repainted_timestamps: usize,
    origin_total_comparisons: usize,
    origin_severity: f64,

    repainted: bool,
    repaint_type: String,
    details: Vec<RepaintDetail>,
}

#[derive(Debug, serde::Serialize)]
struct RepaintDetail {
    test_type: String,
    timestamp: i64,
    column: String,
    initial_value: f64,
    changed_to: f64,
    change_pct: f64,
}

// ── Main check function ───────────────────────────────────────────────────────

fn check_indicator(
    cfg: &IndicatorConfig,
    bars: &[Bar],
    symbol: &str,
    timeframe: &str,
    warmup: usize,
    shift_step: usize,
    test_mode: &str,
    verbose: bool,
) -> RepaintResult {
    let n = bars.len();

    let (fw_repainted, fw_cols, fw_ts, fw_total, fw_sev, fw_details) =
        if test_mode == "forward" || test_mode == "both" {
            if verbose {
                println!("Testing FORWARD SIMULATION (incremental feeding)...");
            }
            forward_test(cfg, bars, warmup, verbose)
        } else {
            (false, vec![], 0, 0, 0.0, vec![])
        };

    let (or_repainted, or_cols, or_ts, or_total, or_sev, or_details) =
        if test_mode == "origin" || test_mode == "both" {
            if verbose {
                println!("Testing ORIGIN SHIFTING (sliding start positions)...");
            }
            origin_test(cfg, bars, warmup, shift_step, verbose)
        } else {
            (false, vec![], 0, 0, 0.0, vec![])
        };

    let repainted = fw_repainted || or_repainted;
    let repaint_type = classify_repaint_type(fw_repainted, or_repainted).to_string();

    let mut details = fw_details;
    details.extend(or_details);

    RepaintResult {
        indicator_name: cfg.kind.clone(),
        symbol: symbol.to_string(),
        timeframe: timeframe.to_string(),
        total_bars: n,
        warmup_bars: warmup,
        forward_repainted: fw_repainted,
        forward_repainted_columns: fw_cols,
        forward_repainted_timestamps: fw_ts,
        forward_total_comparisons: fw_total,
        forward_severity: fw_sev,
        origin_repainted: or_repainted,
        origin_repainted_columns: or_cols,
        origin_repainted_timestamps: or_ts,
        origin_total_comparisons: or_total,
        origin_severity: or_sev,
        repainted,
        repaint_type,
        details,
    }
}

// ── Forward test (incremental feeding) ───────────────────────────────────────
// Feeds bars[0..=i] for each i from warmup+1 to n.
// Checks if any historical bar's value changes when a new bar is added.

fn forward_test(
    cfg: &IndicatorConfig,
    bars: &[Bar],
    warmup: usize,
    verbose: bool,
) -> (bool, Vec<String>, usize, usize, f64, Vec<RepaintDetail>) {
    let n = bars.len();
    let ind = match build(cfg) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("⚠ build indicator {}: {e}", cfg.kind);
            return (false, vec![], 0, 0, 0.0, vec![]);
        }
    };

    let mut history: HashMap<(i64, String), f64> = HashMap::new();
    let mut repainted_cols: HashSet<String> = HashSet::new();
    let mut repainted_ts = 0usize;
    let mut total_comparisons = 0usize;
    let mut details: Vec<RepaintDetail> = Vec::new();

    for i in (warmup + 1)..=n {
        let slice = &bars[..i];
        let result = ind.compute(slice);

        let check_idx = i.saturating_sub(2);
        if check_idx < warmup {
            continue;
        }

        let ts = bars[check_idx].time;

        for (col, values) in &result {
            if values.len() <= check_idx {
                continue;
            }
            let v = values[check_idx];
            if v.is_nan() {
                continue;
            }

            total_comparisons += 1;
            let key = (ts, col.clone());

            match history.get(&key) {
                None => {
                    history.insert(key, v);
                }
                Some(&prev) => {
                    let abs_diff = (v - prev).abs();
                    let rel_diff = abs_diff / prev.abs().max(1e-9);
                    if abs_diff > 1e-9 && rel_diff > 1e-6 {
                        repainted_ts += 1;
                        repainted_cols.insert(col.clone());
                        if details.len() < 10 {
                            details.push(RepaintDetail {
                                test_type: "forward".into(),
                                timestamp: ts,
                                column: col.clone(),
                                initial_value: prev,
                                changed_to: v,
                                change_pct: if prev != 0.0 {
                                    (v - prev) / prev * 100.0
                                } else {
                                    0.0
                                },
                            });
                        }
                    }
                }
            }
        }

        if verbose && i % 50 == 0 {
            println!("  Processed {i}/{n} bars (forward)");
        }
    }

    let severity = if total_comparisons > 0 {
        repainted_ts as f64 / total_comparisons as f64
    } else {
        0.0
    };
    let mut cols: Vec<String> = repainted_cols.into_iter().collect();
    cols.sort();

    (
        repainted_ts > 0,
        cols,
        repainted_ts,
        total_comparisons,
        severity,
        details,
    )
}

// ── Origin shifting test (start position dependency) ─────────────────────────
// Computes the indicator on overlapping windows starting at different positions.
// If a bar's value differs depending on where the window starts, the indicator
// has origin dependency (its history affects current values).

fn origin_test(
    cfg: &IndicatorConfig,
    bars: &[Bar],
    warmup: usize,
    shift_step: usize,
    verbose: bool,
) -> (bool, Vec<String>, usize, usize, f64, Vec<RepaintDetail>) {
    let n = bars.len();
    let min_overlap = 50usize;
    let ind = match build(cfg) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("⚠ build indicator {}: {e}", cfg.kind);
            return (false, vec![], 0, 0, 0.0, vec![]);
        }
    };

    let mut origin_vals: HashMap<(i64, String), Vec<f64>> = HashMap::new();

    let max_shifts = (n.saturating_sub(min_overlap + warmup)) / shift_step.max(1);
    for shift in 0..=max_shifts {
        let start = shift * shift_step;
        let end = (start + min_overlap + warmup).min(n);
        if end <= start + warmup {
            continue;
        }

        let slice = &bars[start..end];
        let result = ind.compute(slice);

        for row_idx in warmup..slice.len() {
            let ts = slice[row_idx].time;
            for (col, values) in &result {
                if values.len() <= row_idx {
                    continue;
                }
                let v = values[row_idx];
                if v.is_nan() {
                    continue;
                }
                origin_vals.entry((ts, col.clone())).or_default().push(v);
            }
        }

        if verbose && shift % 5 == 0 {
            println!("  Processed origin shift {shift}/{max_shifts}");
        }
    }

    let mut repainted_cols: HashSet<String> = HashSet::new();
    let mut repainted_ts = 0usize;
    let mut total_comparisons = 0usize;
    let mut details: Vec<RepaintDetail> = Vec::new();

    for ((ts, col), vals) in &origin_vals {
        if vals.len() < 2 {
            continue;
        }
        total_comparisons += 1;

        let lo = vals.iter().cloned().fold(f64::INFINITY, f64::min);
        let hi = vals.iter().cloned().fold(f64::NEG_INFINITY, f64::max);

        if hi - lo > 1e-8 {
            repainted_ts += 1;
            repainted_cols.insert(col.clone());
            if details.len() < 10 {
                details.push(RepaintDetail {
                    test_type: "origin".into(),
                    timestamp: *ts,
                    column: col.clone(),
                    initial_value: lo,
                    changed_to: hi,
                    change_pct: if lo != 0.0 {
                        (hi - lo) / lo.abs() * 100.0
                    } else {
                        0.0
                    },
                });
            }
        }
    }

    let severity = if total_comparisons > 0 {
        repainted_ts as f64 / total_comparisons as f64
    } else {
        0.0
    };
    let mut cols: Vec<String> = repainted_cols.into_iter().collect();
    cols.sort();

    (
        repainted_ts > 0,
        cols,
        repainted_ts,
        total_comparisons,
        severity,
        details,
    )
}

// ── Report printer ─────────────────────────────

fn print_report(r: &RepaintResult) {
    println!("\n{}", "=".repeat(80));
    println!("REPAINT CHECK REPORT: {}", r.indicator_name);
    println!("{}", "=".repeat(80));
    println!("Symbol:       {}", r.symbol);
    println!("Timeframe:    {}", r.timeframe);
    println!("Bars tested:  {}", r.total_bars);
    println!("Warmup bars:  {}", r.warmup_bars);
    println!("{}", "-".repeat(80));

    if r.forward_total_comparisons > 0 {
        println!("\n[1] FORWARD SIMULATION (Incremental Feeding)");
        println!("{}", "-".repeat(80));
        if !r.forward_repainted {
            println!("✅ NO REPAINTING DETECTED");
            println!("   Indicator values remain stable when new bars are added");
        } else {
            println!("⚠️  REPAINTING DETECTED");
            println!(
                "   Affected columns: {}",
                r.forward_repainted_columns.join(", ")
            );
            println!(
                "   Repainted timestamps: {} / {}  ({:.2}%)",
                r.forward_repainted_timestamps,
                r.forward_total_comparisons,
                r.forward_severity * 100.0
            );
            println!("   Severity: {}", severity_label(r.forward_severity));
        }
    }

    if r.origin_total_comparisons > 0 {
        println!("\n[2] ORIGIN SHIFTING (Start Position Dependency)");
        println!("{}", "-".repeat(80));
        if !r.origin_repainted {
            println!("✅ NO ORIGIN DEPENDENCY DETECTED");
            println!("   Indicator produces consistent values regardless of dataset start");
        } else {
            println!("⚠️  ORIGIN DEPENDENCY DETECTED");
            println!(
                "   Affected columns: {}",
                r.origin_repainted_columns.join(", ")
            );
            println!(
                "   Inconsistent timestamps: {} / {}  ({:.2}%)",
                r.origin_repainted_timestamps,
                r.origin_total_comparisons,
                r.origin_severity * 100.0
            );
            println!("   Severity: {}", severity_label(r.origin_severity));
        }
    }

    println!("\n{}", "=".repeat(80));
    println!("FINAL VERDICT");
    println!("{}", "=".repeat(80));
    if !r.repainted {
        println!("✅ SAFE FOR LIVE TRADING");
        println!("   No repainting or origin dependency detected");
    } else {
        println!("❌ UNSAFE FOR LIVE TRADING");
        println!("   Repaint type: {}", r.repaint_type.to_uppercase());
        match r.repaint_type.as_str() {
            "forward_only" => {
                println!("   → Classic repaint: Values change when new bars arrive");
                println!("   → Will produce misleading backtest results");
            }
            "origin_only" => {
                println!("   → Origin dependency: Values depend on dataset start position");
                println!("   → May produce inconsistent results across sessions");
            }
            "both" => {
                println!("   → SEVERE: Both classic repaint AND origin dependency detected");
                println!("   → Highly unreliable for any trading purpose");
            }
            _ => {}
        }
    }

    if !r.details.is_empty() {
        println!("\n   Top repaint examples:");
        for d in r.details.iter().take(5) {
            if d.test_type == "forward" {
                println!(
                    "     FORWARD [{}] {}: {:.6} → {:.6}  ({:+.2}%)",
                    d.timestamp, d.column, d.initial_value, d.changed_to, d.change_pct
                );
            } else {
                println!(
                    "     ORIGIN  [{}] {}: {:.6} → {:.6}  (Δ={:.6})",
                    d.timestamp,
                    d.column,
                    d.initial_value,
                    d.changed_to,
                    (d.changed_to - d.initial_value).abs()
                );
            }
        }
    }
    println!("{}\n", "=".repeat(80));
}

fn default_warmup(kind: &str) -> usize {
    match kind {
        "sma" | "ema" | "wma" | "rma" | "rsi" | "stochastic" | "atr" | "adx" => 14,
        "macd" | "macd_reloaded" => 26,
        "bollinger" | "bbands" => 20,
        "msm" => 60,
        "supertrend" => 10,
        _ => 20,
    }
}
