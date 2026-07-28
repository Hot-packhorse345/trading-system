use super::{FnSymTick, Mt5Client, Mt5Tick};
use crate::traits::TickStream;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use std::ffi::CString;
use tokio::sync::mpsc;
use tokio::time::{sleep, Duration};
use tracing::error;
use ts_core::Tick;

const FETCH_MS: u64 = 1;

const DUP_TIME_THRESHOLD: f64 = 1e-6;
const DUP_PRICE_THRESHOLD: f64 = 1e-9;

#[async_trait]
impl TickStream for Mt5Client {
    async fn subscribe(&self, sym: &str, tx: mpsc::Sender<Tick>) -> Result<()> {
        let fn_sym_tick: FnSymTick = self.fn_sym_tick.ok_or_else(|| {
            anyhow!(
                "MT5 TickStream unavailable: DLL does not export 'SymbolInfoTick'. \
                 Rebuild the DLL with this export to enable tick streaming."
            )
        })?;

        let sym_owned = sym.to_string();

        tokio::spawn(async move {
            let mut prev_time: f64 = 0.0;
            let mut prev_bid: f64 = 0.0;
            let mut prev_ask: f64 = 0.0;

            loop {
                let sym_c = match CString::new(sym_owned.as_str()) {
                    Ok(c) => c,
                    Err(e) => {
                        error!("TickStream CString error: {e}");
                        break;
                    }
                };

                let (ret, raw) = match tokio::task::spawn_blocking(move || {
                    let mut raw = Mt5Tick::default();
                    let ret = unsafe { fn_sym_tick(sym_c.as_ptr(), &mut raw) };
                    (ret, raw)
                })
                .await
                {
                    Ok(res) => res,
                    Err(e) => {
                        error!("TickStream SymbolInfoTick thread joined with error: {e}");
                        break;
                    }
                };

                if ret == 1 && raw.bid > 0.0 && raw.ask > 0.0 {
                    if raw.ask >= raw.bid && raw.time > 0 {
                        let ts = raw.time as f64;

                        let time_dup = (ts - prev_time).abs() < DUP_TIME_THRESHOLD;
                        let bid_dup = (raw.bid - prev_bid).abs() < DUP_PRICE_THRESHOLD;
                        let ask_dup = (raw.ask - prev_ask).abs() < DUP_PRICE_THRESHOLD;

                        if !(time_dup && bid_dup && ask_dup) {
                            prev_time = ts;
                            prev_bid = raw.bid;
                            prev_ask = raw.ask;

                            let tick = Tick {
                                symbol: sym_owned.clone(),
                                bid: raw.bid,
                                ask: raw.ask,
                                last: raw.last,
                                volume: raw.volume as f64,
                                timestamp: ts,
                            };

                            if tx.send(tick).await.is_err() {
                                return;
                            }
                        }
                    }
                }

                sleep(Duration::from_millis(FETCH_MS)).await;
            }
        });

        Ok(())
    }
}
