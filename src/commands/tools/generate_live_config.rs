use super::utils::{csv_value_to_json, read_csv_rows, row_get, set_nested_json};
use anyhow::{anyhow, Result};
use backtest::config::BacktestConfig;
use serde_json::Value;
use std::path::PathBuf;
use tracing::{info, warn};

#[allow(clippy::too_many_arguments)]
pub fn run(
    csv_path: PathBuf,
    backtest_config: Option<PathBuf>,
    rank: usize,
    metric: Option<String>,
    ascending: bool,
    out: Option<PathBuf>,
    trade_executor: String,
    bar_streamer: String,
    tick_streamer: String,
    risk_manager_json: Option<String>,
) -> Result<()> {
    anyhow::ensure!(rank >= 1, "--rank must be >= 1");

    let rows =
        read_csv_rows(&csv_path).map_err(|e| anyhow!("cannot read {}: {e}", csv_path.display()))?;
    anyhow::ensure!(
        !rows.is_empty(),
        "CSV has no data rows: {}",
        csv_path.display()
    );

    let row: Vec<(String, String)> = if let Some(col) = metric.as_deref() {
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
        sorted
            .into_iter()
            .nth(rank - 1)
            .ok_or_else(|| anyhow!("rank {rank} not found after sorting by '{col}'"))?
    } else {
        rows.into_iter()
            .nth(rank - 1)
            .ok_or_else(|| anyhow!("rank {rank} not found in CSV"))?
    };

    let bt_cfg: Option<BacktestConfig> = backtest_config
        .as_ref()
        .map(|p| {
            let text = std::fs::read_to_string(p)?;
            Ok::<_, anyhow::Error>(serde_json::from_str::<BacktestConfig>(&text)?)
        })
        .transpose()?;

    let mut cfg: serde_json::Map<String, Value> = serde_json::Map::new();

    for field in &[
        "strategy",
        "symbol",
        "timeframe",
        "stop_timeframe",
        "pyramiding",
    ] {
        if let Some(v) = row_get(&row, field) {
            if !v.is_empty() {
                cfg.insert(field.to_string(), csv_value_to_json(v));
            }
        }
    }

    for (col, val) in &row {
        let prefix = if col.starts_with("strategy_parameters.") {
            "strategy_parameters."
        } else if col.starts_with("indicators.") {
            "indicators."
        } else if col.starts_with("stop_manager.") {
            "stop_manager."
        } else {
            continue;
        };
        let key = &col[prefix.len()..];
        if key.is_empty() {
            continue;
        }
        let section = prefix.trim_end_matches('.');
        let sub = cfg
            .entry(section)
            .or_insert_with(|| Value::Object(serde_json::Map::new()));
        if let Value::Object(m) = sub {
            set_nested_json(m, key, csv_value_to_json(val));
        }
    }

    let data_provider = bt_cfg
        .as_ref()
        .map(|c| c.data_provider.clone())
        .or_else(|| {
            row_get(&row, "data_provider")
                .filter(|v| !v.is_empty())
                .map(str::to_string)
        })
        .unwrap_or_else(|| "binance".to_string());
    cfg.insert("data_provider".to_string(), Value::String(data_provider));
    cfg.insert("trade_executor".to_string(), Value::String(trade_executor));
    cfg.insert("bar_streamer".to_string(), Value::String(bar_streamer));
    cfg.insert("tick_streamer".to_string(), Value::String(tick_streamer));

    if let Some(rm_json) = risk_manager_json {
        let rm: Value = serde_json::from_str(&rm_json)
            .map_err(|e| anyhow!("invalid --risk-manager JSON: {e}"))?;
        cfg.insert("risk_manager".to_string(), rm);
    } else if let Some(bc) = &bt_cfg {
        let mut rm = serde_json::Map::new();
        rm.insert(
            "type".to_string(),
            Value::String("fixed_percent".to_string()),
        );
        if let Some(n) = serde_json::Number::from_f64(bc.risk_percentage) {
            rm.insert("pct".to_string(), Value::Number(n));
        }
        if let Some(n) = serde_json::Number::from_f64(bc.initial_balance) {
            rm.insert("initial_balance".to_string(), Value::Number(n));
        }
        cfg.insert("risk_manager".to_string(), Value::Object(rm));
    } else {
        warn!(
            "no --risk-manager and no --backtest-config — risk_manager will be absent from output"
        );
    }

    if let Some(bc) = &bt_cfg {
        cfg.insert(
            "data_dir".to_string(),
            Value::String(bc.data_dir.to_string_lossy().to_string()),
        );
    }

    // Auto-embed the live decay monitor's reference distribution if the
    // edge-discovery pipeline produced one for this exact strategy/symbol/tf —
    // the live config carries the actual array, not a path to re-read later.
    let data_dir = bt_cfg
        .as_ref()
        .map(|bc| bc.data_dir.clone())
        .unwrap_or_else(|| PathBuf::from("data"));
    if let (Some(Value::String(strategy)), Some(Value::String(symbol)), Some(Value::String(tf))) =
        (cfg.get("strategy"), cfg.get("symbol"), cfg.get("timeframe"))
    {
        let safe_sym = symbol.replace('.', "_");
        let dist_path = data_dir
            .join("results")
            .join("oos_distributions")
            .join(format!("{strategy}_{safe_sym}_{tf}.json"));
        if let Ok(text) = std::fs::read_to_string(&dist_path) {
            match serde_json::from_str::<Value>(&text) {
                Ok(dist_json) => {
                    if let Some(arr) = dist_json["oos_mean_r_per_trade"].as_array() {
                        cfg.insert("oos_distribution".to_string(), Value::Array(arr.clone()));
                        info!(path=%dist_path.display(), "found matching OOS distribution — embedding into live config");
                    }
                }
                Err(e) => {
                    warn!(path=%dist_path.display(), err=%e, "invalid OOS distribution JSON — skipping decay monitor wiring");
                }
            }
        }
    }

    let json = serde_json::to_string_pretty(&Value::Object(cfg))?;

    let out_path = out.unwrap_or_else(|| {
        let stem = csv_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("live");
        csv_path.with_file_name(format!("{stem}_live.json"))
    });
    if let Some(parent) = out_path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    std::fs::write(&out_path, &json)?;

    info!(rank=%rank, out=%out_path.display(), "live config written");
    println!("{json}");
    println!();
    println!("Written: {}", out_path.display());
    Ok(())
}
