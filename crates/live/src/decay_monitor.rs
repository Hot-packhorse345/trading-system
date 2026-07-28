use data::TradeDb;
use infra::Notifier;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::watch;
use tracing::{error, info, warn};

use crate::formatter;

const CHECK_INTERVAL_SECS: u64 = 6 * 60 * 60;
const ROLLING_WINDOW_DAYS: i64 = 30;
const SECONDS_PER_DAY: i64 = 86_400;
/// Below this many closed trades in the trailing window, there isn't enough
/// signal to distinguish concept drift from ordinary variance.
const MIN_TRADES_FOR_SIGNAL: usize = 5;
/// Percentile of the WFA OOS mean-R-per-trade distribution below which live
/// performance is considered to have decayed.
const DECAY_PERCENTILE: f64 = 0.10;

/// Background task comparing a worker's trailing 30-day live performance
/// against its walk-forward OOS distribution (persisted by the edge-discovery
/// pipeline). Unlike the daily drawdown halt — a hard reset at midnight — this
/// is a continuously re-evaluated rolling signal: it both raises and clears
/// the halt as trailing performance crosses the threshold.
#[allow(clippy::too_many_arguments)]
pub async fn run(
    trade_db: Arc<Mutex<TradeDb>>,
    strategy_id: u64,
    worker_id: String,
    oos_mean_r_per_trade: Vec<f64>,
    notifier: Arc<dyn Notifier>,
    decay_tx: watch::Sender<bool>,
    mut shutdown_rx: watch::Receiver<bool>,
) {
    if oos_mean_r_per_trade.is_empty() {
        warn!(worker = %worker_id, "decay monitor: empty OOS distribution, not starting");
        return;
    }
    let threshold = percentile(&oos_mean_r_per_trade, DECAY_PERCENTILE);
    info!(worker = %worker_id, threshold = %threshold, "decay monitor started");

    let mut active = false;
    loop {
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(CHECK_INTERVAL_SECS)) => {
                let since = chrono::Utc::now().timestamp() - ROLLING_WINDOW_DAYS * SECONDS_PER_DAY;
                let trades = {
                    let db = trade_db.lock().unwrap();
                    db.load_closed_since(strategy_id, since).unwrap_or_default()
                };
                if trades.len() < MIN_TRADES_FOR_SIGNAL {
                    continue;
                }
                let mean_r =
                    trades.iter().map(|t| t.r_multiple()).sum::<f64>() / trades.len() as f64;

                if mean_r < threshold && !active {
                    active = true;
                    warn!(worker = %worker_id, mean_r = %mean_r, threshold = %threshold,
                        "STRATEGY DECAY DETECTED — halting new entries");
                    decay_tx.send(true).ok();
                    let msg = formatter::format_decay_halt(&worker_id, mean_r, threshold, trades.len());
                    if let Err(e) = notifier.send(&msg).await {
                        error!(err = %e, "failed to send decay alert");
                    }
                } else if mean_r >= threshold && active {
                    active = false;
                    info!(worker = %worker_id, "decay cleared — resuming entries");
                    decay_tx.send(false).ok();
                    let msg = formatter::format_decay_cleared(&worker_id);
                    if let Err(e) = notifier.send(&msg).await {
                        error!(err = %e, "failed to send decay-cleared alert");
                    }
                }
            }
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() {
                    info!(worker = %worker_id, "decay monitor shutting down");
                    break;
                }
            }
        }
    }
}

/// Nearest-rank percentile (`p` in `[0, 1]`) of a small pre-computed sample.
fn percentile(source: &[f64], p: f64) -> f64 {
    let mut v = source.to_vec();
    v.sort_by(|a, b| a.total_cmp(b));
    let idx = ((v.len() as f64 - 1.0) * p).round() as usize;
    v[idx.min(v.len().saturating_sub(1))]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_percentile_nearest_rank() {
        let data = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0];
        assert!((percentile(&data, 0.10) - 2.0).abs() < 1e-9);
        assert!((percentile(&data, 0.50) - 6.0).abs() < 1e-9);
        assert!((percentile(&data, 1.0) - 10.0).abs() < 1e-9);
    }

    #[test]
    fn test_percentile_single_value() {
        assert_eq!(percentile(&[3.5], 0.10), 3.5);
    }
}
