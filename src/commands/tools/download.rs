use super::super::parse_date;
use super::utils::build_data_provider;
use anyhow::Result;
use data::OhlcvCache;
use std::path::PathBuf;
use tracing::info;
use ts_core::parse_timeframe;

pub async fn run(
    symbol: String,
    tf_str: String,
    from: String,
    to: String,
    output: PathBuf,
    broker: String,
) -> Result<()> {
    let tf = parse_timeframe(&tf_str)?;

    let from_ts = parse_date(&from)?;
    let to_ts = parse_date(&to)?;
    if from_ts > to_ts {
        anyhow::bail!("Invalid date range: --from date ({from}) cannot be after --to date ({to})");
    }
    let broker = broker.to_lowercase();
    let provider = build_data_provider(&broker)?;
    info!(
        "Downloading {} {} {} → {} via {}",
        symbol, tf, from, to, broker
    );

    let bars = provider.ohlcv(&symbol, tf, from_ts, to_ts).await?;
    info!("Downloaded {} bars", bars.len());

    OhlcvCache::new(&output).save(&symbol, tf, &bars)?;
    info!("Saved to {}/{}_{}.parquet", output.display(), symbol, tf);
    Ok(())
}
