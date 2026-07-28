use anyhow::Result;
use backtest::{run, write_csv, BacktestConfig};
use chrono::Utc;
use std::path::PathBuf;
use tracing::info;

#[allow(dead_code)]
pub struct BacktestRun {
    pub results: Vec<backtest::BacktestResult>,
    pub csv_path: Option<PathBuf>,
}

pub async fn run_cmd(config_path: PathBuf, top: usize, metric: Option<String>) -> Result<()> {
    let cfg: BacktestConfig = serde_json::from_str(&std::fs::read_to_string(&config_path)?)?;
    info!(config=%config_path.display(), "backtest starting");
    execute(cfg, top, metric).await?;
    Ok(())
}

pub async fn execute(
    cfg: BacktestConfig,
    top: usize,
    metric: Option<String>,
) -> Result<BacktestRun> {
    // The grid search is CPU-bound (Rayon). Run it on the blocking pool so it does
    // not monopolise a Tokio worker thread.
    let output_dir = cfg.output_dir.clone();
    let results = tokio::task::spawn_blocking(move || run(&cfg, top, metric.as_deref())).await??;
    info!(results=%results.len(), "backtest finished");

    if results.is_empty() {
        info!("no results to write");
        return Ok(BacktestRun {
            results,
            csv_path: None,
        });
    }

    std::fs::create_dir_all(&output_dir)?;
    let ts = Utc::now().format("%Y%m%d_%H%M").to_string();
    let csv = output_dir.join(format!("results_{ts}.csv"));
    write_csv(&results, &csv)?;
    info!(csv=%csv.display(), "results written");

    Ok(BacktestRun {
        results,
        csv_path: Some(csv),
    })
}
