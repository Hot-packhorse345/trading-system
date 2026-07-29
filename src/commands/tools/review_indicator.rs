use super::super::resolve_indicators;
use super::utils::{
    ensure_bars_recent, fmt_ts, fmt_value, print_table, resolve_data_provider, tail_bars,
};
use anyhow::Result;
use data::IndicatorCache;
use indicators::compute_all;
use std::path::PathBuf;
use ts_core::parse_timeframe;

#[allow(clippy::too_many_arguments)]
pub async fn run(
    config: Option<PathBuf>,
    indicator: Option<String>,
    indicator_config: String,
    symbol: String,
    timeframe: String,
    bars: usize,
    tail: usize,
    data_dir: PathBuf,
) -> Result<()> {
    let ind_cfgs = resolve_indicators(
        config.clone(),
        indicator,
        if indicator_config.is_empty() {
            None
        } else {
            Some(indicator_config)
        },
    )?;
    let tf = parse_timeframe(&timeframe)?;

    let broker = resolve_data_provider(config.as_ref())?;
    let all_bars = ensure_bars_recent(&symbol, tf, &data_dir, &broker, bars).await?;
    if all_bars.is_empty() {
        anyhow::bail!("no bars for {symbol}/{timeframe} in {}", data_dir.display());
    }
    let selected = tail_bars(all_bars, bars);

    let cache = IndicatorCache::new(data_dir.join("ind_cache"));
    let cols = compute_all(&ind_cfgs, &selected, Some(&cache))?;
    let mut names: Vec<String> = cols.column_names().map(|s| s.to_string()).collect();
    names.sort();

    println!("Symbol:     {symbol}");
    println!("Timeframe:  {timeframe}");
    println!("Bars:       {}", selected.len());
    println!("Indicators: {}", names.join(", "));
    println!();

    if tail == 0 {
        return Ok(());
    }
    let start_idx = selected.len().saturating_sub(tail);
    let mut headers = vec!["timestamp".to_string(), "close".to_string()];
    headers.extend(names.iter().cloned());
    let rows: Vec<Vec<String>> = (start_idx..selected.len())
        .map(|idx| {
            let b = &selected[idx];
            let mut row = vec![fmt_ts(b.time), format!("{:.6}", b.close)];
            for name in &names {
                row.push(fmt_value(cols.get(name)[idx]));
            }
            row
        })
        .collect();

    let mut align_right = vec![false, true];
    align_right.extend(vec![true; names.len()]);
    print_table(&headers, &rows, &align_right);
    Ok(())
}
