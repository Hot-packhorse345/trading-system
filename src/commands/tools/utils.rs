use anyhow::Result;
use backtest::config::BacktestConfig;
use broker::traits::DataProvider;
use chrono::{TimeZone, Utc};
use data::OhlcvCache;
use serde_json::Value;
use std::path::{Path, PathBuf};
use tracing::info;
use ts_core::{Bar, Timeframe};

pub fn fmt_ts(ts: i64) -> String {
    Utc.timestamp_opt(ts, 0)
        .single()
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_else(|| ts.to_string())
}

pub fn fmt_value(v: f64) -> String {
    if v.is_nan() {
        "nan".to_string()
    } else {
        format!("{:.6}", v)
    }
}

pub fn fmt_date(ts: i64) -> String {
    Utc.timestamp_opt(ts, 0)
        .single()
        .map(|dt| dt.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| "today".to_string())
}

pub fn print_table(headers: &[String], rows: &[Vec<String>], align_right: &[bool]) {
    if headers.is_empty() {
        return;
    }
    let mut widths: Vec<usize> = headers.iter().map(|h| h.len()).collect();
    for row in rows {
        for (idx, val) in row.iter().enumerate() {
            if idx < widths.len() {
                widths[idx] = widths[idx].max(val.len());
            }
        }
    }
    let sep_len = widths.iter().sum::<usize>() + 3 * (widths.len().saturating_sub(1));
    let sep = "-".repeat(sep_len);
    let format_row = |vals: &[String]| -> String {
        vals.iter()
            .enumerate()
            .map(|(idx, val)| {
                let width = widths[idx];
                if align_right.get(idx).copied().unwrap_or(false) {
                    format!("{:>width$}", val, width = width)
                } else {
                    format!("{:<width$}", val, width = width)
                }
            })
            .collect::<Vec<_>>()
            .join(" | ")
    };
    println!("{sep}");
    println!("{}", format_row(headers));
    println!("{sep}");
    for row in rows {
        println!("{}", format_row(row));
    }
    println!("{sep}");
}

pub fn build_data_provider(broker: &str) -> Result<Box<dyn DataProvider>> {
    broker::build_data_provider(broker)
}

pub fn resolve_data_provider(config: Option<&PathBuf>) -> Result<String> {
    let Some(path) = config else {
        return Ok("binance".to_string());
    };
    let text = std::fs::read_to_string(path)?;
    let cfg: BacktestConfig = serde_json::from_str(&text)?;
    Ok(cfg.data_provider)
}

/// Load cached bars for `symbol`/`tf` covering `[start_ts, end_ts]`, downloading
/// the missing range from `broker` first if the cache has nothing for it.
pub async fn ensure_bars_range(
    symbol: &str,
    tf: Timeframe,
    data_dir: &PathBuf,
    broker: &str,
    start_ts: i64,
    end_ts: i64,
) -> Result<Vec<Bar>> {
    let cache = OhlcvCache::new(data_dir);
    let mut bars = cache.load(symbol, tf, start_ts, end_ts)?;
    if bars.is_empty() {
        let from = fmt_ts(start_ts);
        let to = fmt_ts(end_ts);
        info!(symbol=%symbol, timeframe=%tf, from=%from, to=%to, broker=%broker, "downloading missing bars");
        super::download::run(
            symbol.to_string(),
            tf.to_binance().to_string(),
            from,
            to,
            data_dir.clone(),
            broker.to_string(),
        )
        .await?;
        bars = cache.load(symbol, tf, start_ts, end_ts)?;
    }
    Ok(bars)
}

/// Load all cached bars for `symbol`/`tf`, downloading the most recent
/// `lookback_bars` worth of history from `broker` if the cache is empty.
pub async fn ensure_bars_recent(
    symbol: &str,
    tf: Timeframe,
    data_dir: &PathBuf,
    broker: &str,
    lookback_bars: usize,
) -> Result<Vec<Bar>> {
    let cache = OhlcvCache::new(data_dir);
    let mut all_bars = cache.load_all(symbol, tf)?;
    if all_bars.is_empty() {
        let end_ts = Utc::now().timestamp();
        let lookback = tf.seconds().saturating_mul(lookback_bars.max(1) as i64);
        let from = fmt_date(end_ts.saturating_sub(lookback));
        let to = fmt_date(end_ts);
        info!(symbol=%symbol, timeframe=%tf, from=%from, to=%to, broker=%broker, "downloading missing bars");
        super::download::run(
            symbol.to_string(),
            tf.to_binance().to_string(),
            from,
            to,
            data_dir.clone(),
            broker.to_string(),
        )
        .await?;
        all_bars = cache.load_all(symbol, tf)?;
    }
    Ok(all_bars)
}

/// Keep only the last `n` bars. `n == 0` means "no truncation".
pub fn tail_bars(bars: Vec<Bar>, n: usize) -> Vec<Bar> {
    if n > 0 && bars.len() > n {
        bars[bars.len() - n..].to_vec()
    } else {
        bars
    }
}

/// Read a CSV file into rows of `(column, value)` pairs, preserving column order.
pub fn read_csv_rows(path: &Path) -> Result<Vec<Vec<(String, String)>>> {
    let mut rdr = csv::Reader::from_path(path)?;
    let headers: Vec<String> = rdr.headers()?.iter().map(|s| s.to_string()).collect();
    let rows = rdr
        .records()
        .filter_map(|r| r.ok())
        .map(|r| {
            headers
                .iter()
                .cloned()
                .zip(r.iter().map(|s| s.to_string()))
                .collect()
        })
        .collect();
    Ok(rows)
}

/// Look up a value by column name in a row produced by [`read_csv_rows`].
pub fn row_get<'a>(row: &'a [(String, String)], key: &str) -> Option<&'a str> {
    row.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
}

/// Coerce a CSV cell into the most specific JSON scalar it represents
/// (bool, null, number, or string).
pub fn csv_value_to_json(s: &str) -> Value {
    let t = s.trim();
    match t.to_lowercase().as_str() {
        "true" => return Value::Bool(true),
        "false" => return Value::Bool(false),
        "null" | "none" => return Value::Null,
        _ => {}
    }
    if let Ok(i) = t.parse::<i64>() {
        return Value::Number(i.into());
    }
    if let Ok(f) = t.parse::<f64>() {
        if let Some(n) = serde_json::Number::from_f64(f) {
            return Value::Number(n);
        }
    }
    Value::String(t.to_string())
}

/// Insert `val` into `map` at a dotted path, creating nested objects as needed.
pub fn set_nested_json(map: &mut serde_json::Map<String, Value>, dotted: &str, val: Value) {
    let parts: Vec<&str> = dotted.splitn(2, '.').collect();
    if parts.len() == 1 {
        map.insert(parts[0].to_string(), val);
    } else {
        let sub = map
            .entry(parts[0])
            .or_insert_with(|| Value::Object(serde_json::Map::new()));
        if let Value::Object(m) = sub {
            set_nested_json(m, parts[1], val);
        }
    }
}
