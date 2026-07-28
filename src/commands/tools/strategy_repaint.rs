use anyhow::Result;
use std::collections::HashMap;
use std::path::PathBuf;
use tracing::info;

use super::repaint_common::{classify_repaint_type, severity_label};
use super::utils::{ensure_bars_recent, tail_bars};
use backtest::config::{BacktestConfig, IndicatorDef};
use backtest::engine::build_indicator_set_from_defs;
use backtest::grid::generate_combos;
use strategy::{build_strategy, Strategy};
use ts_core::{Bar, Direction, Params, Signal};

pub async fn run_cmd(
    config: PathBuf,
    combo: usize,
    bars: usize,
    warmup: usize,
    shift_step: usize,
    test_mode: String,
    export: Option<PathBuf>,
    data_dir: Option<PathBuf>,
    verbose: bool,
) -> Result<()> {
    let text = std::fs::read_to_string(&config)?;
    let cfg: BacktestConfig = serde_json::from_str(&text)?;
    let broker = cfg.data_provider.clone();
    let combos = generate_combos(&cfg)?;
    if combos.is_empty() {
        anyhow::bail!("no combos from {}", config.display());
    }
    if combo >= combos.len() {
        anyhow::bail!("combo {combo} out of range (max {})", combos.len() - 1);
    }
    let c = &combos[combo];
    let data_dir = data_dir.unwrap_or_else(|| cfg.data_dir.clone());

    let all_bars = ensure_bars_recent(&c.symbol, c.timeframe, &data_dir, &broker, bars).await?;
    if all_bars.is_empty() {
        anyhow::bail!(
            "no bars for {}/{} in {}",
            c.symbol,
            c.timeframe,
            data_dir.display()
        );
    }

    let test_bars: Vec<Bar> = tail_bars(all_bars, bars);

    let strategy_params = Params(c.strategy_params.clone());
    let ind_params: HashMap<String, Params> = c
        .indicators
        .iter()
        .map(|d| (d.name.clone(), Params(d.params.clone())))
        .collect();

    let strat = build_strategy(&c.strategy_name)?;
    let effective_warmup = warmup;

    info!(
        strategy=%c.strategy_name, symbol=%c.symbol,
        timeframe=%c.timeframe, bars=%test_bars.len(),
        warmup=%effective_warmup,
        "strategy repaint check starting"
    );

    let result = check_strategy(
        &c.strategy_name,
        &c.symbol,
        c.timeframe.to_str(),
        &test_bars,
        &c.indicators,
        &strategy_params,
        &ind_params,
        &*strat,
        effective_warmup,
        shift_step,
        &test_mode,
        verbose,
    );

    print_report(&result);

    if let Some(out) = export {
        let json = serde_json::to_string_pretty(&result).unwrap_or_default();
        std::fs::create_dir_all(out.parent().unwrap_or(std::path::Path::new(".")))?;
        std::fs::write(&out, &json)?;
        println!("Detailed results exported to: {}", out.display());
    }

    std::process::exit(if result.repainted { 1 } else { 0 });
}

// ── Data structures ───────────────────────────────────────────────────────────

#[derive(Debug, serde::Serialize)]
struct StrategyRepaintResult {
    strategy_name: String,
    symbol: String,
    timeframe: String,
    total_bars: usize,
    warmup_bars: usize,

    forward_repainted: bool,
    forward_repainted_timestamps: usize,
    forward_total_comparisons: usize,
    forward_severity: f64,

    origin_repainted: bool,
    origin_repainted_timestamps: usize,
    origin_total_comparisons: usize,
    origin_severity: f64,

    repainted: bool,
    repaint_type: String,
    details: Vec<SignalRepaintDetail>,
}

#[derive(Debug, serde::Serialize)]
struct SignalRepaintDetail {
    test_type: String,
    timestamp: i64,
    initial_dir: String,
    changed_to: String,
}

// ── Signal direction helpers ──────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum SigDir {
    None,
    Buy,
    Sell,
}

fn extract_dir(sig: &Option<Signal>) -> SigDir {
    match sig {
        Some(s) if s.is_valid() => match s.direction {
            Direction::Buy => SigDir::Buy,
            Direction::Sell => SigDir::Sell,
            _ => SigDir::None,
        },
        _ => SigDir::None,
    }
}

fn dir_label(d: SigDir) -> &'static str {
    match d {
        SigDir::None => "none",
        SigDir::Buy => "buy",
        SigDir::Sell => "sell",
    }
}

// ── Main check orchestrator ───────────────────────────────────────────────────

fn check_strategy(
    strategy_name: &str,
    symbol: &str,
    timeframe: &str,
    bars: &[Bar],
    ind_defs: &[IndicatorDef],
    strategy_params: &Params,
    ind_params: &HashMap<String, Params>,
    strat: &dyn Strategy,
    warmup: usize,
    shift_step: usize,
    test_mode: &str,
    verbose: bool,
) -> StrategyRepaintResult {
    let n = bars.len();

    let (fw_repainted, fw_ts, fw_total, fw_sev, fw_details) =
        if test_mode == "forward" || test_mode == "both" {
            if verbose {
                println!("Testing FORWARD SIMULATION (incremental feeding)...");
            }
            forward_test(
                ind_defs,
                strategy_params,
                ind_params,
                strat,
                bars,
                warmup,
                verbose,
            )
        } else {
            (false, 0, 0, 0.0, vec![])
        };

    let (or_repainted, or_ts, or_total, or_sev, or_details) =
        if test_mode == "origin" || test_mode == "both" {
            if verbose {
                println!("Testing ORIGIN SHIFTING (sliding start positions)...");
            }
            origin_test(
                ind_defs,
                strategy_params,
                ind_params,
                strat,
                bars,
                warmup,
                shift_step,
                verbose,
            )
        } else {
            (false, 0, 0, 0.0, vec![])
        };

    let repainted = fw_repainted || or_repainted;
    let repaint_type = classify_repaint_type(fw_repainted, or_repainted).to_string();

    let mut details = fw_details;
    details.extend(or_details);

    StrategyRepaintResult {
        strategy_name: strategy_name.to_string(),
        symbol: symbol.to_string(),
        timeframe: timeframe.to_string(),
        total_bars: n,
        warmup_bars: warmup,
        forward_repainted: fw_repainted,
        forward_repainted_timestamps: fw_ts,
        forward_total_comparisons: fw_total,
        forward_severity: fw_sev,
        origin_repainted: or_repainted,
        origin_repainted_timestamps: or_ts,
        origin_total_comparisons: or_total,
        origin_severity: or_sev,
        repainted,
        repaint_type,
        details,
    }
}

// ── Forward test ─────────────────────────────────────────────────────────────
// Feeds bars[0..=i] incrementally. Checks if a historical bar's signal direction
// changes when a new bar is appended — the classic repainting pattern.

fn forward_test(
    ind_defs: &[IndicatorDef],
    strategy_params: &Params,
    ind_params: &HashMap<String, Params>,
    strat: &dyn Strategy,
    bars: &[Bar],
    warmup: usize,
    verbose: bool,
) -> (bool, usize, usize, f64, Vec<SignalRepaintDetail>) {
    let n = bars.len();
    let mut history: HashMap<i64, SigDir> = HashMap::new();
    let mut repainted_ts = 0usize;
    let mut total_comparisons = 0usize;
    let mut details: Vec<SignalRepaintDetail> = Vec::new();

    for i in (warmup + 2)..=n {
        let slice = &bars[..i];

        let cols = match build_indicator_set_from_defs(ind_defs, slice, None) {
            Ok(c) => c,
            Err(e) => {
                if verbose {
                    eprintln!("  compute_all error at i={i}: {e}");
                }
                continue;
            }
        };
        let signals = strat.generate_signals(slice, &cols, strategy_params, ind_params);

        let check_idx = i - 2;
        if check_idx < warmup {
            continue;
        }

        let ts = bars[check_idx].time;
        let dir = extract_dir(&signals[check_idx]);

        total_comparisons += 1;
        match history.get(&ts) {
            None => {
                history.insert(ts, dir);
            }
            Some(&prev) => {
                if prev != dir {
                    repainted_ts += 1;
                    if details.len() < 10 {
                        details.push(SignalRepaintDetail {
                            test_type: "forward".into(),
                            timestamp: ts,
                            initial_dir: dir_label(prev).into(),
                            changed_to: dir_label(dir).into(),
                        });
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
    (
        repainted_ts > 0,
        repainted_ts,
        total_comparisons,
        severity,
        details,
    )
}

// ── Origin test ───────────────────────────────────────────────────────────────
// Runs the strategy on overlapping windows starting at different positions.
// If a bar's signal direction differs depending on where the window starts,
// the strategy has origin dependency.

fn origin_test(
    ind_defs: &[IndicatorDef],
    strategy_params: &Params,
    ind_params: &HashMap<String, Params>,
    strat: &dyn Strategy,
    bars: &[Bar],
    warmup: usize,
    shift_step: usize,
    verbose: bool,
) -> (bool, usize, usize, f64, Vec<SignalRepaintDetail>) {
    let n = bars.len();
    let min_overlap = 50usize;
    let mut origin_dirs: HashMap<i64, Vec<SigDir>> = HashMap::new();

    let max_shifts = (n.saturating_sub(min_overlap + warmup)) / shift_step.max(1);
    for shift in 0..=max_shifts {
        let start = shift * shift_step;
        let end = (start + min_overlap + warmup).min(n);
        if end <= start + warmup {
            continue;
        }

        let slice = &bars[start..end];

        let cols = match build_indicator_set_from_defs(ind_defs, slice, None) {
            Ok(c) => c,
            Err(e) => {
                if verbose {
                    eprintln!("  compute_all error at shift={shift}: {e}");
                }
                continue;
            }
        };
        let signals = strat.generate_signals(slice, &cols, strategy_params, ind_params);

        for row_idx in warmup..slice.len() {
            let ts = slice[row_idx].time;
            let dir = extract_dir(&signals[row_idx]);
            origin_dirs.entry(ts).or_default().push(dir);
        }

        if verbose && shift % 5 == 0 {
            println!("  Processed origin shift {shift}/{max_shifts}");
        }
    }

    let mut repainted_ts = 0usize;
    let mut total_comparisons = 0usize;
    let mut details: Vec<SignalRepaintDetail> = Vec::new();

    for (ts, dirs) in &origin_dirs {
        if dirs.len() < 2 {
            continue;
        }
        total_comparisons += 1;
        let first = dirs[0];
        if let Some(&different) = dirs.iter().find(|&&d| d != first) {
            repainted_ts += 1;
            if details.len() < 10 {
                details.push(SignalRepaintDetail {
                    test_type: "origin".into(),
                    timestamp: *ts,
                    initial_dir: dir_label(first).into(),
                    changed_to: dir_label(different).into(),
                });
            }
        }
    }

    let severity = if total_comparisons > 0 {
        repainted_ts as f64 / total_comparisons as f64
    } else {
        0.0
    };
    (
        repainted_ts > 0,
        repainted_ts,
        total_comparisons,
        severity,
        details,
    )
}

// ── Report printer ────────────────────────────────────────────────────────────

fn print_report(r: &StrategyRepaintResult) {
    println!("\n{}", "=".repeat(80));
    println!("STRATEGY REPAINT CHECK REPORT: {}", r.strategy_name);
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
            println!("  NO REPAINTING DETECTED");
            println!("   Signal directions remain stable when new bars are added");
        } else {
            println!("  REPAINTING DETECTED");
            println!(
                "   Repainted timestamps: {} / {}  ({:.2}%)",
                r.forward_repainted_timestamps,
                r.forward_total_comparisons,
                r.forward_severity * 100.0
            );
            let sev = severity_label(r.forward_severity);
            println!("   Severity: {sev}");
        }
    }

    if r.origin_total_comparisons > 0 {
        println!("\n[2] ORIGIN SHIFTING (Start Position Dependency)");
        println!("{}", "-".repeat(80));
        if !r.origin_repainted {
            println!("  NO ORIGIN DEPENDENCY DETECTED");
            println!("   Signals are consistent regardless of dataset start position");
        } else {
            println!("  ORIGIN DEPENDENCY DETECTED");
            println!(
                "   Inconsistent timestamps: {} / {}  ({:.2}%)",
                r.origin_repainted_timestamps,
                r.origin_total_comparisons,
                r.origin_severity * 100.0
            );
            let sev = severity_label(r.origin_severity);
            println!("   Severity: {sev}");
        }
    }

    println!("\n{}", "=".repeat(80));
    println!("FINAL VERDICT");
    println!("{}", "=".repeat(80));
    if !r.repainted {
        println!("  SAFE FOR LIVE TRADING");
        println!("   No signal repainting or origin dependency detected");
    } else {
        println!("  UNSAFE FOR LIVE TRADING");
        println!("   Repaint type: {}", r.repaint_type.to_uppercase());
        match r.repaint_type.as_str() {
            "forward_only" => {
                println!("   -> Classic repaint: signal directions change when new bars arrive");
                println!("   -> Backtest results will not match live performance");
            }
            "origin_only" => {
                println!("   -> Origin dependency: signals differ based on dataset start position");
                println!("   -> Results may be inconsistent across sessions");
            }
            "both" => {
                println!("   -> SEVERE: both classic repaint and origin dependency detected");
                println!("   -> Highly unreliable for any trading purpose");
            }
            _ => {}
        }
    }

    if !r.details.is_empty() {
        println!("\n   Top repaint examples:");
        for d in r.details.iter().take(5) {
            println!(
                "     {} [{}] {} -> {}",
                d.test_type.to_uppercase(),
                d.timestamp,
                d.initial_dir,
                d.changed_to
            );
        }
    }

    println!("{}\n", "=".repeat(80));
}
