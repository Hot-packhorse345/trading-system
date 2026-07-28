use super::BinanceBroker;
use crate::traits::TickStream;
use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;
use tokio::time::{sleep, Duration};
use tracing::error;
use ts_core::Tick;

const DUP_TIME_THRESHOLD: f64 = 1e-6;
const DUP_PRICE_THRESHOLD: f64 = 1e-9;

fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs_f64()
}

#[async_trait]
impl TickStream for BinanceBroker {
    async fn subscribe(&self, sym: &str, tx: mpsc::Sender<Tick>) -> Result<()> {
        use futures_util::StreamExt;
        use tokio_tungstenite::connect_async;

        // Combined stream endpoint: payloads are wrapped as {"stream":..,"data":..},
        // which is what the parser below expects. The raw "/ws/<a>/<b>" endpoint
        // does NOT support multiple streams and does NOT wrap payloads, so the old
        // URL silently produced zero ticks (breaking all tick-based SL/TP).
        let streams = format!(
            "{}@bookTicker/{}@aggTrade",
            sym.to_lowercase(),
            sym.to_lowercase()
        );
        let base = self.ws.trim_end_matches("/ws").trim_end_matches('/');
        let url = format!("{base}/stream?streams={streams}");
        let sym_name = sym.to_string();

        tokio::spawn(async move {
            let mut last_bid: f64 = 0.0;
            let mut last_ask: f64 = 0.0;
            let mut last_price: f64 = 0.0;
            let mut last_vol: f64 = 0.0;
            let mut last_ts: f64 = 0.0;

            let mut prev_time: f64 = 0.0;
            let mut prev_bid: f64 = 0.0;
            let mut prev_ask: f64 = 0.0;

            loop {
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

                                    let stream_name = v["stream"].as_str().unwrap_or("");
                                    let data = &v["data"];

                                    if stream_name.contains("bookTicker") {
                                        last_bid = data["b"]
                                            .as_str()
                                            .unwrap_or("0")
                                            .parse()
                                            .unwrap_or(0.0);
                                        last_ask = data["a"]
                                            .as_str()
                                            .unwrap_or("0")
                                            .parse()
                                            .unwrap_or(0.0);
                                    } else if stream_name.contains("aggTrade") {
                                        last_price = data["p"]
                                            .as_str()
                                            .unwrap_or("0")
                                            .parse()
                                            .unwrap_or(0.0);
                                        last_vol = data["q"]
                                            .as_str()
                                            .unwrap_or("0")
                                            .parse()
                                            .unwrap_or(0.0);
                                        last_ts = data["T"].as_i64().unwrap_or(0) as f64 / 1_000.0;
                                    }

                                    if last_bid <= 0.0
                                        || last_ask <= 0.0
                                        || last_price <= 0.0
                                        || last_ts <= 0.0
                                    {
                                        continue;
                                    }

                                    if last_ask < last_bid {
                                        continue;
                                    }

                                    let ts = if last_ts > 0.0 { last_ts } else { now_secs() };

                                    let time_dup = (ts - prev_time).abs() < DUP_TIME_THRESHOLD;
                                    let bid_dup = (last_bid - prev_bid).abs() < DUP_PRICE_THRESHOLD;
                                    let ask_dup = (last_ask - prev_ask).abs() < DUP_PRICE_THRESHOLD;
                                    if time_dup && bid_dup && ask_dup {
                                        continue;
                                    }

                                    prev_time = ts;
                                    prev_bid = last_bid;
                                    prev_ask = last_ask;

                                    let tick = Tick {
                                        symbol: sym_name.clone(),
                                        bid: last_bid,
                                        ask: last_ask,
                                        last: last_price,
                                        volume: last_vol,
                                        timestamp: ts,
                                    };

                                    if tx.send(tick).await.is_err() {
                                        return;
                                    }
                                }
                                Err(e) => {
                                    error!("TickStream WS error on {sym_name}: {e}");
                                    break;
                                }
                            }
                        }
                    }
                    Err(e) => {
                        error!("TickStream WS connect failed for {sym_name}: {e}");
                    }
                }
                sleep(Duration::from_secs(5)).await;
            }
        });

        Ok(())
    }
}
