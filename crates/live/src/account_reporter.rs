use crate::formatter;
use broker::DataProvider;
use chrono::{Duration, Timelike, Utc};
use data::JsonCache;
use infra::Notifier;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::watch;
use tracing::{error, info};

const LAST_REPORT_CACHE_NAME: &str = "account_report_last_hour";

/// Persisted so a process restart landing in the same hourly slot as the last
/// successful report doesn't re-send it — otherwise a brief overlap between
/// an old and a newly restarted process (e.g. during a redeploy) can each
/// independently hit the same hour boundary and send two near-identical
/// alerts back to back.
#[derive(Debug, Serialize, Deserialize)]
struct LastReportState {
    hour_bucket: String,
}

pub async fn run(
    data_provider: Arc<dyn DataProvider>,
    notifier: Arc<dyn Notifier>,
    data_dir: PathBuf,
    mut shutdown_rx: watch::Receiver<bool>,
) {
    info!("account reporter started");
    let cache = JsonCache::new(data_dir.join("live"));

    loop {
        // Sleep until the next UTC hour boundary.
        let now = Utc::now();
        let next_hour = (now + Duration::hours(1))
            .with_minute(0)
            .unwrap()
            .with_second(0)
            .unwrap()
            .with_nanosecond(0)
            .unwrap();
        let sleep_secs = (next_hour - now).num_seconds().max(1) as u64;

        tokio::select! {
            _ = tokio::time::sleep(tokio::time::Duration::from_secs(sleep_secs)) => {
                let hour_bucket = Utc::now()
                    .with_minute(0).unwrap()
                    .with_second(0).unwrap()
                    .with_nanosecond(0).unwrap()
                    .to_rfc3339();

                let already_reported = cache
                    .get::<LastReportState>(LAST_REPORT_CACHE_NAME)
                    .ok()
                    .flatten()
                    .is_some_and(|s| s.hour_bucket == hour_bucket);
                if already_reported {
                    info!(hour=%hour_bucket, "account report already sent for this hour — skipping duplicate");
                    continue;
                }

                match data_provider.account().await {
                    Ok(info) => {
                        let msg = formatter::format_account_report(&info);
                        if let Err(e) = notifier.send(&msg).await {
                            error!(err=%e, "failed to send account report");
                        } else {
                            cache.put(LAST_REPORT_CACHE_NAME, &LastReportState { hour_bucket }).ok();
                        }
                    }
                    Err(e) => {
                        error!(err=%e, "failed to fetch account for report");
                    }
                }
            }
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() {
                    info!("account reporter shutting down");
                    break;
                }
            }
        }
    }
}
