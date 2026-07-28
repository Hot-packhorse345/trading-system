use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use tracing::{error, info, warn};

use super::utils::{csv_value_to_json, read_csv_rows, row_get, set_nested_json};
use backtest::{run as run_backtest, write_csv, BacktestConfig};
use infra::{Notifier, NullNotifier, TelegramNotifier};

// ── Public entry point ────────────────────────────────────────────────────────

pub async fn run_cmd(
    configs: Vec<PathBuf>,
    workflow_file: Option<PathBuf>,
    metric: String,
    top: usize,
    rank: usize,
    ascending: bool,
    stop_on_fail: bool,
    no_alert: bool,
    cleanup: bool,
    export_best_config: bool,
    best_config_rank: usize,
    best_config_dir: Option<PathBuf>,
    best_config_param_prefixes: Vec<String>,
) -> Result<()> {
    let (configs, opts) = if let Some(wf) = workflow_file {
        load_workflow_file(&wf)?
    } else {
        let opts = WorkflowOpts {
            metric,
            top,
            rank,
            ascending,
            stop_on_fail,
            no_alert,
            cleanup,
            export_best_config,
            best_config_rank,
            best_config_dir,
            best_config_param_prefixes,
        };
        (configs, opts)
    };

    if configs.is_empty() {
        anyhow::bail!("no configs specified");
    }
    for c in &configs {
        if !c.exists() {
            anyhow::bail!("config not found: {}", c.display());
        }
    }

    let notifier: std::sync::Arc<dyn Notifier> = build_notifier(opts.no_alert);

    let ts = chrono::Utc::now().format("%Y%m%d-%H%M%S").to_string();
    let out_dir = PathBuf::from("results").join(format!("workflow_{ts}"));
    std::fs::create_dir_all(&out_dir)?;

    let best_cfg_dir = opts
        .best_config_dir
        .clone()
        .unwrap_or_else(|| out_dir.clone());

    info!(out_dir=%out_dir.display(), configs=%configs.len(), "workflow starting");

    let mut summaries: Vec<JobSummary> = Vec::new();

    for (idx, cfg_path) in configs.iter().enumerate() {
        info!("{}", "━".repeat(70));
        info!(
            "  Job {} / {} — {}",
            idx + 1,
            configs.len(),
            cfg_path.display()
        );
        info!("{}", "━".repeat(70));

        let summary = run_job(cfg_path, &opts, &out_dir, &best_cfg_dir, notifier.as_ref()).await;

        let ok = summary.backtest_ok;
        summaries.push(summary);

        if opts.stop_on_fail && !ok {
            error!("stop_on_fail=true — aborting after job {}", idx + 1);
            break;
        }
    }

    write_workflow_summary(&summaries, &out_dir);
    print_summary_table(&summaries);
    Ok(())
}

// ── Single job ────────────────────────────────────────────────────────────────

async fn run_job(
    cfg_path: &Path,
    opts: &WorkflowOpts,
    out_dir: &Path,
    best_cfg_dir: &Path,
    notifier: &dyn Notifier,
) -> JobSummary {
    let label = label_for(cfg_path);
    let mut summary = JobSummary {
        label: label.clone(),
        config: cfg_path.display().to_string(),
        backtest_ok: false,
        csv: None,
        analysis_txt: None,
        best_config: None,
        deleted: vec![],
    };

    // ── 1. Load and run backtest ──────────────────────────────────────────────
    let cfg: BacktestConfig = match load_json(cfg_path) {
        Ok(c) => c,
        Err(e) => {
            error!("failed to load config {}: {e}", cfg_path.display());
            return summary;
        }
    };

    let _t_before = unix_now();
    let results = match run_backtest(&cfg, opts.top, Some(opts.metric.as_str())) {
        Ok(r) => r,
        Err(e) => {
            error!("backtest failed: {e}");
            return summary;
        }
    };
    let _t_after = unix_now();
    summary.backtest_ok = true;

    if results.is_empty() {
        warn!("backtest produced no results for {}", cfg_path.display());
        return summary;
    }

    // ── 2. Write CSV ──────────────────────────────────────────────────────────
    std::fs::create_dir_all(&cfg.output_dir).ok();
    let csv_path = cfg.output_dir.join("results.csv");
    if let Err(e) = write_csv(&results, &csv_path) {
        warn!("failed to write CSV: {e}");
    } else {
        summary.csv = Some(csv_path.clone());
        info!("Result CSV: {}", csv_path.display());
    }

    // ── 3. Print top results table ────────────────────────────────────────────
    let csv_path = match &summary.csv {
        Some(p) => p.clone(),
        None => return summary,
    };

    let table = build_top_table(&csv_path, &opts.metric, opts.top, opts.rank, opts.ascending);
    println!("{table}");

    let txt_path = out_dir.join(format!("{label}_top_results.txt"));
    std::fs::write(&txt_path, &table).ok();
    info!("Saved TXT: {}", txt_path.display());
    summary.analysis_txt = Some(txt_path);

    // ── 4. Export best config ─────────────────────────────────────────────────
    if opts.export_best_config {
        if let Some(path) = export_best(
            &csv_path,
            &opts.metric,
            opts.best_config_rank,
            opts.ascending,
            best_cfg_dir,
            &label,
            &opts.best_config_param_prefixes,
        ) {
            info!("Saved best config: {}", path.display());
            summary.best_config = Some(path);
        }
    }

    // ── 5. Telegram alert ─────────────────────────────────────────────────────
    if !opts.no_alert {
        let msg = format!(
            "📊 *Backtest: {label}*\nMetric: `{}` (rank #{})\n\n```\n{}\n```",
            opts.metric,
            opts.rank,
            table
                .lines()
                .take(opts.top + 5)
                .collect::<Vec<_>>()
                .join("\n")
        );
        notifier.send(&msg).await.ok();
    }

    // ── 6. Cleanup ────────────────────────────────────────────────────────────
    if opts.cleanup {
        if let Err(e) = std::fs::remove_file(&csv_path) {
            warn!("delete CSV: {e}");
        } else {
            summary.deleted.push(csv_path.to_string_lossy().into());
            info!("Deleted CSV: {}", csv_path.display());
        }
    }

    summary
}

// ── Metric name → CSV column name translation ─────────────────────────────────

fn metric_col(metric: &str) -> &str {
    match metric {
        "sharpe" => "sharpe_ratio",
        "profit_factor" => "profit_factor_r",
        "expectancy" => "expectancy_r",
        "total_r" => "total_net_r",
        "gross_profit" => "gross_profit_r",
        "gross_loss" => "gross_loss_r",
        "enhanced_score" => "enhanced_score",
        "calmar" => "calmar_ratio",
        other => other,
    }
}

// ── CSV top-N table ───────────────────────────────────────────────────────────

fn build_top_table(
    csv_path: &Path,
    metric: &str,
    top: usize,
    rank: usize,
    ascending: bool,
) -> String {
    let rows = match read_csv_rows(csv_path) {
        Ok(r) if !r.is_empty() => r,
        Ok(_) => return format!("[CSV empty: {}]", csv_path.display()),
        Err(e) => return format!("[CSV error: {e}]"),
    };

    let col = metric_col(metric);
    let mut sorted = rows;
    sorted.sort_by(|a, b| {
        let va = row_get(a, col)
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(f64::NEG_INFINITY);
        let vb = row_get(b, col)
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(f64::NEG_INFINITY);
        if ascending {
            va.partial_cmp(&vb)
        } else {
            vb.partial_cmp(&va)
        }
        .unwrap_or(std::cmp::Ordering::Equal)
    });

    let sep = "━".repeat(100);
    let mut lines = vec![
        sep.clone(),
        format!(
            "  TOP {} RESULTS  |  metric: {}  |  {}",
            top.min(sorted.len()),
            metric,
            csv_path.display()
        ),
        sep.clone(),
        format!(
            "{:>3}  {:<12} {:<6} {:<20} {:>8} {:>7} {:>7} {:>7} {:>10}  {}",
            "Rk", "Strategy", "TF", "StopMgr", "Sharpe", "PF", "WR", "Trades", "NetPnl", "Params"
        ),
        "-".repeat(100),
    ];

    for (i, row) in sorted.iter().enumerate().take(top) {
        let r = i + 1;
        let marker = if r == rank { "►" } else { " " };
        let g = |k: &str| row_get(row, k).unwrap_or("");
        let stop_label = format!(
            "{}(d={},rr={})",
            g("stop_manager.type"),
            g("stop_manager.stop_distance"),
            g("stop_manager.start_rr"),
        );
        let params_str: String = {
            let prefix = "strategy_parameters.";
            let mut kv: Vec<String> = row
                .iter()
                .filter(|(k, _)| k.starts_with(prefix))
                .map(|(k, v)| format!("{}={}", &k[prefix.len()..], v))
                .collect();
            kv.sort();
            kv.join(" ")
        };
        let params_short = if params_str.len() > 60 {
            &params_str[..60]
        } else {
            &params_str
        };
        lines.push(format!(
            "{}{:>2}  {:<12} {:<6} {:<20} {:>8} {:>7} {:>7} {:>7} {:>10}  {}",
            marker,
            r,
            g("strategy"),
            g("timeframe"),
            stop_label,
            g("sharpe_ratio"),
            g("profit_factor_r"),
            g("win_rate"),
            g("total_trades"),
            g("net_profit"),
            params_short,
        ));
    }
    lines.push(sep);
    lines.join("\n")
}

// ── Best config export ────────────────────────────────────────────────────────

fn export_best(
    csv_path: &Path,
    metric: &str,
    rank: usize,
    ascending: bool,
    out_dir: &Path,
    label: &str,
    prefixes: &[String],
) -> Option<PathBuf> {
    let rows = read_csv_rows(csv_path).ok()?;
    if rows.is_empty() {
        return None;
    }

    let col = metric_col(metric);
    let mut sorted = rows;
    sorted.sort_by(|a, b| {
        let va = row_get(a, col)
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(f64::NEG_INFINITY);
        let vb = row_get(b, col)
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(f64::NEG_INFINITY);
        if ascending {
            va.partial_cmp(&vb)
        } else {
            vb.partial_cmp(&va)
        }
        .unwrap_or(std::cmp::Ordering::Equal)
    });

    let row = sorted.get(rank.saturating_sub(1))?;
    let mut cfg: serde_json::Map<String, Value> = serde_json::Map::new();
    let mut applied = 0;

    for (col, val) in row {
        let Some(prefix) = prefixes.iter().find(|p| col.starts_with(p.as_str())) else {
            continue;
        };
        let key = col[prefix.len()..].trim_matches('.');
        if key.is_empty() {
            continue;
        }
        set_nested_json(&mut cfg, key, csv_value_to_json(val));
        applied += 1;
    }

    if applied == 0 {
        warn!("no columns matched prefixes {:?} in CSV", prefixes);
        return None;
    }

    std::fs::create_dir_all(out_dir).ok()?;
    let out = out_dir.join(format!("{label}_best_config.json"));
    let json = serde_json::to_string_pretty(&cfg).ok()?;
    std::fs::write(&out, json).ok()?;
    Some(out)
}

// ── Summary ───────────────────────────────────────────────────────────────────

fn print_summary_table(summaries: &[JobSummary]) {
    let ok = summaries.iter().filter(|s| s.backtest_ok).count();
    let fail = summaries.len() - ok;
    let sep = "━".repeat(70);
    println!("\n{sep}");
    println!("  WORKFLOW COMPLETE");
    println!("{sep}");
    println!("  Total: {}  OK: {}  FAILED: {}", summaries.len(), ok, fail);
    println!();
    for (i, s) in summaries.iter().enumerate() {
        let status = if s.backtest_ok { "OK  " } else { "FAIL" };
        let del_lbl = if s.deleted.is_empty() {
            String::new()
        } else {
            format!(
                "  (deleted: {})",
                s.deleted
                    .iter()
                    .map(|p| Path::new(p)
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        let best_lbl = s
            .best_config
            .as_ref()
            .map(|p| {
                format!(
                    "  (best_cfg: {})",
                    p.file_name().unwrap_or_default().to_string_lossy()
                )
            })
            .unwrap_or_default();
        println!("  {:>2}. [{status}]  {}{del_lbl}{best_lbl}", i + 1, s.label);
    }
    println!("{sep}\n");
}

fn write_workflow_summary(summaries: &[JobSummary], out_dir: &Path) {
    let mut lines = vec![
        "Workflow Summary".to_string(),
        "═".repeat(70),
        String::new(),
    ];
    for (i, s) in summaries.iter().enumerate() {
        let status = if s.backtest_ok { "OK" } else { "FAILED" };
        let csv_lbl = s
            .csv
            .as_ref()
            .map(|p| {
                p.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string()
            })
            .unwrap_or_else(|| "--".into());
        let best_lbl = s
            .best_config
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "--".into());
        lines.extend([
            format!("  {:>2}. [{status}]  {}", i + 1, s.label),
            format!("      config  : {}", s.config),
            format!("      csv     : {csv_lbl}"),
            format!("      best cfg: {best_lbl}"),
            format!(
                "      deleted : {}",
                if s.deleted.is_empty() {
                    "none".into()
                } else {
                    s.deleted.join(", ")
                }
            ),
            String::new(),
        ]);
    }
    let path = out_dir.join("workflow_summary.txt");
    std::fs::write(&path, lines.join("\n")).ok();
    info!("Summary written: {}", path.display());
}

// ── Workflow file ─────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Default)]
struct WorkflowFile {
    configs: Vec<PathBuf>,
    #[serde(default = "default_metric")]
    metric: String,
    #[serde(default = "default_10")]
    top: usize,
    #[serde(default = "default_1")]
    rank: usize,
    #[serde(default)]
    ascending: bool,
    #[serde(default)]
    stop_on_fail: bool,
    #[serde(default)]
    no_alert: bool,
    #[serde(default)]
    cleanup: bool,
    #[serde(default)]
    export_best_config: bool,
    #[serde(default = "default_1")]
    best_config_rank: usize,
    #[serde(default)]
    best_config_dir: Option<PathBuf>,
    #[serde(default = "default_prefixes")]
    best_config_param_prefixes: Vec<String>,
}

fn default_metric() -> String {
    "enhanced_score".to_string()
}
fn default_10() -> usize {
    10
}
fn default_1() -> usize {
    1
}
fn default_prefixes() -> Vec<String> {
    vec!["params.".to_string()]
}

fn load_workflow_file(path: &Path) -> Result<(Vec<PathBuf>, WorkflowOpts)> {
    let wf: WorkflowFile = load_json(path)?;
    let opts = WorkflowOpts {
        metric: wf.metric,
        top: wf.top,
        rank: wf.rank,
        ascending: wf.ascending,
        stop_on_fail: wf.stop_on_fail,
        no_alert: wf.no_alert,
        cleanup: wf.cleanup,
        export_best_config: wf.export_best_config,
        best_config_rank: wf.best_config_rank,
        best_config_dir: wf.best_config_dir,
        best_config_param_prefixes: wf.best_config_param_prefixes,
    };
    Ok((wf.configs, opts))
}

// ── Helpers ───────────────────────────────────────────────────────────────────

struct WorkflowOpts {
    metric: String,
    top: usize,
    rank: usize,
    ascending: bool,
    stop_on_fail: bool,
    no_alert: bool,
    cleanup: bool,
    export_best_config: bool,
    best_config_rank: usize,
    best_config_dir: Option<PathBuf>,
    best_config_param_prefixes: Vec<String>,
}

#[derive(Default)]
struct JobSummary {
    label: String,
    config: String,
    backtest_ok: bool,
    csv: Option<PathBuf>,
    analysis_txt: Option<PathBuf>,
    best_config: Option<PathBuf>,
    deleted: Vec<String>,
}

fn label_for(p: &Path) -> String {
    p.file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .replace(' ', "_")
}

fn load_json<T: for<'de> serde::Deserialize<'de>>(p: &Path) -> Result<T> {
    let text = std::fs::read_to_string(p).with_context(|| format!("read {}", p.display()))?;
    serde_json::from_str(&text).with_context(|| format!("parse {}", p.display()))
}

fn build_notifier(no_alert: bool) -> std::sync::Arc<dyn Notifier> {
    if no_alert {
        return std::sync::Arc::new(NullNotifier);
    }
    if let (Ok(token), Ok(chat_id)) = (
        std::env::var("TELEGRAM_BOT_TOKEN"),
        std::env::var("TELEGRAM_CHAT_ID"),
    ) {
        return std::sync::Arc::new(TelegramNotifier::new(token, chat_id));
    }
    std::sync::Arc::new(NullNotifier)
}

fn unix_now() -> f64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}
