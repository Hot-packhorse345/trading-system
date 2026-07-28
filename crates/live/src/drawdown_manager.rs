use crate::formatter;
use anyhow::Result;
use broker::DataProvider;
use chrono::Utc;
use data::JsonCache;
use infra::Notifier;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::watch;
use tracing::{error, info, warn};

const ANCHOR_CACHE_NAME: &str = "daily_anchor";
const EMERGENCY_CACHE_NAME: &str = "drawdown_emergency";
const CHECK_INTERVAL_SECS: u64 = 60;

#[derive(Debug, Serialize, Deserialize)]
struct DailyAnchor {
    date: String,
    equity: f64,
    balance: f64,
}

/// Persisted so the daily halt survives a process restart within the same day —
/// otherwise restarting (or systemd `Restart=on-failure`) would silently resume
/// trading after the daily loss limit was already breached.
#[derive(Debug, Serialize, Deserialize)]
struct EmergencyState {
    date: String,
    active: bool,
}

pub async fn run(
    data_provider: Arc<dyn DataProvider>,
    daily_dd_limit_pct: f64,
    emergency_tx: watch::Sender<bool>,
    dd_tx: watch::Sender<f64>,
    notifier: Arc<dyn Notifier>,
    data_dir: PathBuf,
    mut shutdown_rx: watch::Receiver<bool>,
) {
    info!(limit=%daily_dd_limit_pct, "drawdown monitor started");
    let cache = JsonCache::new(data_dir.join("live"));

    // Initialise anchor for today.
    if let Err(e) = refresh_anchor(&*data_provider, &cache).await {
        error!(err=%e, "failed to initialise drawdown anchor");
    }

    let mut emergency_active = false;

    // Re-arm the emergency halt if it was tripped earlier the same trading day.
    if let Ok(Some(state)) = cache.get::<EmergencyState>(EMERGENCY_CACHE_NAME) {
        let today = Utc::now().date_naive().format("%Y-%m-%d").to_string();
        if state.date == today && state.active {
            emergency_active = true;
            emergency_tx.send(true).ok();
            warn!("re-arming drawdown emergency from persisted state (same trading day)");
        }
    }

    loop {
        tokio::select! {
            _ = tokio::time::sleep(tokio::time::Duration::from_secs(CHECK_INTERVAL_SECS)) => {
                // Reset at midnight UTC.
                let now = Utc::now();
                let today_str = now.date_naive().format("%Y-%m-%d").to_string();

                if let Ok(Some(anchor)) = cache.get::<DailyAnchor>(ANCHOR_CACHE_NAME) {
                    if anchor.date != today_str {
                        info!("new trading day — refreshing drawdown anchor");
                        if let Err(e) = refresh_anchor(&*data_provider, &cache).await {
                            error!(err=%e, "failed to refresh anchor");
                            continue;
                        }
                        emergency_active = false;
                        emergency_tx.send(false).ok();
                        dd_tx.send(0.0).ok();
                        cache.put(EMERGENCY_CACHE_NAME,
                            &EmergencyState { date: today_str.clone(), active: false }).ok();
                        continue;
                    }
                }

                // Check drawdown.
                let account = match data_provider.account().await {
                    Ok(a) => a,
                    Err(e) => { error!(err=%e, "failed to fetch account for DD check"); continue; }
                };

                let anchor: DailyAnchor = match cache.get(ANCHOR_CACHE_NAME) {
                    Ok(Some(a)) => a,
                    _ => {
                        if let Err(e) = refresh_anchor(&*data_provider, &cache).await {
                            error!(err=%e, "anchor not found, could not refresh");
                        }
                        continue;
                    }
                };

                let dd_pct = if anchor.equity > 0.0 {
                    ((anchor.equity - account.equity) / anchor.equity * 100.0).max(0.0)
                } else {
                    0.0
                };

                // Publish current drawdown as a fraction for tiered position sizing.
                dd_tx.send((dd_pct / 100.0).max(0.0)).ok();

                if dd_pct >= daily_dd_limit_pct && !emergency_active {
                    emergency_active = true;
                    warn!(dd_pct=%dd_pct, limit=%daily_dd_limit_pct, "DRAWDOWN LIMIT REACHED — emergency triggered");
                    emergency_tx.send(true).ok();
                    cache.put(EMERGENCY_CACHE_NAME,
                        &EmergencyState { date: today_str.clone(), active: true }).ok();
                    let msg = formatter::format_drawdown_emergency(dd_pct, daily_dd_limit_pct);
                    if let Err(e) = notifier.send(&msg).await {
                        error!(err=%e, "failed to send drawdown alert");
                    }
                } else if dd_pct < daily_dd_limit_pct && emergency_active {
                    // Equity recovered (e.g. after trades close) — keep emergency active
                    // until next day reset.
                }
            }
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() {
                    info!("drawdown monitor shutting down");
                    break;
                }
            }
        }
    }
}

async fn refresh_anchor(provider: &dyn DataProvider, cache: &JsonCache) -> Result<()> {
    let account = provider.account().await?;
    let today = Utc::now().date_naive().format("%Y-%m-%d").to_string();
    let anchor = DailyAnchor {
        date: today,
        equity: account.equity,
        balance: account.balance,
    };
    cache.put(ANCHOR_CACHE_NAME, &anchor)?;
    info!(equity=%account.equity, "drawdown anchor set");
    Ok(())
}
