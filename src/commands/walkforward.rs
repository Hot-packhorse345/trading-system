use anyhow::{anyhow, Context, Result};
use backtest::BacktestConfig;
use broker::build_data_provider;
use data::OhlcvCache;
use std::path::PathBuf;
use tracing::info;
use ts_core::parse_timeframe;
use walkforward::{run as wf_run, WalkforwardConfig, WfReport};

pub async fn run_cmd(config_path: PathBuf, output_path: Option<PathBuf>) -> Result<()> {
    let text = std::fs::read_to_string(&config_path)
        .with_context(|| format!("cannot read config {}", config_path.display()))?;

    // The same file carries both the backtest grid definition and the window
    // parameters (each deserializer ignores the other's extra fields).
    let base: BacktestConfig = serde_json::from_str(&text).context("parse backtest config")?;
    let wf: WalkforwardConfig = serde_json::from_str(&text).context("parse walkforward params")?;

    execute(base, wf, output_path).await?;
    Ok(())
}

pub async fn execute(
    base: BacktestConfig,
    wf: WalkforwardConfig,
    output_path: Option<PathBuf>,
) -> Result<WfReport> {
    // Walk-forward runs on a single series; take the first symbol/timeframe.
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
        info!(symbol=%symbol, tf=?tf, "insufficient cached bars — downloading");
        let downloaded = provider.ohlcv(&symbol, tf, start, end).await?;
        if !downloaded.is_empty() {
            cache.save(&symbol, tf, &downloaded)?;
            bars = cache.load(&symbol, tf, start, end)?;
        }
    }
    let symbol_info = provider.symbol_info(&symbol).await?;
    drop(provider);

    info!(symbol=%symbol, tf=?tf, bars=%bars.len(), "walk-forward starting");

    let output_dir = base.output_dir.clone();

    // CPU-bound grid optimisation: run on the blocking pool.
    let report =
        tokio::task::spawn_blocking(move || wf_run(&base, &wf, &symbol, tf, &bars, &symbol_info))
            .await??;

    report.print();

    let out_path = output_path.unwrap_or_else(|| {
        let ts = chrono::Utc::now().format("%Y%m%d_%H%M").to_string();
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
    info!(path=%out_path.display(), "walk-forward report written");

    Ok(report)
}

fn first_str(v: &serde_json::Value) -> Option<String> {
    match v {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Array(a) => a.first().and_then(|x| x.as_str().map(|s| s.to_string())),
        _ => None,
    }
}
