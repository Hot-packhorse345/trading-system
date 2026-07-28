use super::BinanceBroker;
use crate::traits::{BarStream, DataProvider};
use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;
use tokio::time::{sleep, Duration};
use tracing::{error, warn};
use ts_core::{Bar, Timeframe};

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

#[async_trait]
impl BarStream for BinanceBroker {
    async fn subscribe(&self, sym: &str, tf: Timeframe, tx: mpsc::Sender<Bar>) -> Result<()> {
        use futures_util::StreamExt;
        use tokio_tungstenite::connect_async;

        let stream = format!("{}@kline_{}", sym.to_lowercase(), tf.to_binance());
        let url = format!("{}/{}", self.ws, stream);
        let broker = self.clone();
        let symbol = sym.to_uppercase();

        tokio::spawn(async move {
            // Highest bar open-time forwarded so far; used both for de-duplication
            // and to bound REST gap backfill after a reconnect.
            let mut last_time: i64 = 0;

            loop {
                // ── Backfill any bars missed while disconnected ───────────────
                // On the first connect last_time is 0, so this is skipped (the
                // worker seeds history separately). On reconnect it replays closed
                // bars between the last forwarded bar and now, preventing holes in
                // the strategy buffer that would corrupt indicator computations.
                if last_time > 0 {
                    match broker.ohlcv(&symbol, tf, last_time + 1, now_secs()).await {
                        Ok(bars) => {
                            for b in bars {
                                if b.time > last_time {
                                    last_time = b.time;
                                    if tx.send(b).await.is_err() {
                                        return;
                                    }
                                }
                            }
                        }
                        Err(e) => warn!("BarStream backfill failed for {stream}: {e}"),
                    }
                }

                match connect_async(&url).await {
                    Ok((mut ws, _)) => {
                        while let Some(msg) = ws.next().await {
                            match msg {
                                Ok(m) => {
                                    let text = match m.to_text() {
                                        Ok(t) => t.to_owned(),
                                        Err(_) => continue,
                                    };
                                    let v: Value = match serde_json::from_str(&text) {
                                        Ok(v) => v,
                                        Err(_) => continue,
                                    };

                                    let k = &v["k"];

                                    if !k["x"].as_bool().unwrap_or(false) {
                                        continue;
                                    }

                                    let bar = Bar::new(
                                        k["t"].as_i64().unwrap_or(0) / 1_000,
                                        k["o"].as_str().unwrap_or("0").parse().unwrap_or(0.0),
                                        k["h"].as_str().unwrap_or("0").parse().unwrap_or(0.0),
                                        k["l"].as_str().unwrap_or("0").parse().unwrap_or(0.0),
                                        k["c"].as_str().unwrap_or("0").parse().unwrap_or(0.0),
                                        k["v"].as_str().unwrap_or("0").parse().unwrap_or(0.0),
                                    );

                                    // Skip duplicates/out-of-order (e.g. a bar already
                                    // delivered via backfill after a brief drop).
                                    if bar.time <= last_time {
                                        continue;
                                    }
                                    last_time = bar.time;

                                    if tx.send(bar).await.is_err() {
                                        return;
                                    }
                                }
                                Err(e) => {
                                    error!("BarStream WS error on {stream}: {e}");
                                    break;
                                }
                            }
                        }
                    }
                    Err(e) => {
                        error!("BarStream WS connect failed for {stream}: {e}");
                    }
                }
                sleep(Duration::from_secs(5)).await;
            }
        });

        Ok(())
    }
}
