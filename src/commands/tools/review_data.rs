use super::super::parse_date;
use super::utils::{fmt_ts, print_table};
use anyhow::Result;
use data::OhlcvCache;
use std::path::PathBuf;
use ts_core::parse_timeframe;

pub fn run(
    symbol: String,
    timeframe: String,
    from: Option<String>,
    to: Option<String>,
    tail: usize,
    data_dir: PathBuf,
) -> Result<()> {
    let tf = parse_timeframe(&timeframe)?;
    let cache = OhlcvCache::new(&data_dir);

    let start = from.as_deref().map(parse_date).transpose()?;
    let end = to.as_deref().map(parse_date).transpose()?;
    let bars = match (start, end) {
        (None, None) => cache.load_all(&symbol, tf)?,
        (s, e) => cache.load(&symbol, tf, s.unwrap_or(i64::MIN), e.unwrap_or(i64::MAX))?,
    };

    if bars.is_empty() {
        anyhow::bail!("no bars for {symbol}/{timeframe} in {}", data_dir.display());
    }

    let first_ts = fmt_ts(bars.first().map(|b| b.time).unwrap_or_default());
    let last_ts = fmt_ts(bars.last().map(|b| b.time).unwrap_or_default());
    let (min_close, max_close) = bars
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |acc, b| {
            (acc.0.min(b.close), acc.1.max(b.close))
        });

    println!("Symbol:      {symbol}");
    println!("Timeframe:   {timeframe}");
    println!("Bars:        {}", bars.len());
    println!("Range:       {first_ts} → {last_ts}");
    println!("Close range: {:.6} → {:.6}", min_close, max_close);
    println!();

    if tail == 0 {
        return Ok(());
    }

    let start_idx = bars.len().saturating_sub(tail);
    let headers = vec![
        "timestamp".to_string(),
        "open".to_string(),
        "high".to_string(),
        "low".to_string(),
        "close".to_string(),
        "volume".to_string(),
    ];
    let rows: Vec<Vec<String>> = bars[start_idx..]
        .iter()
        .map(|b| {
            vec![
                fmt_ts(b.time),
                format!("{:.6}", b.open),
                format!("{:.6}", b.high),
                format!("{:.6}", b.low),
                format!("{:.6}", b.close),
                format!("{:.6}", b.volume),
            ]
        })
        .collect();

    print_table(&headers, &rows, &[false, true, true, true, true, true]);
    Ok(())
}
