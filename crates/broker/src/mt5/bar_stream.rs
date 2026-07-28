use super::{FnRates, Mt5Client, Mt5Rate};
use crate::traits::BarStream;
use anyhow::Result;
use async_trait::async_trait;
use std::ffi::CString;
use tokio::sync::mpsc;
use tokio::time::{sleep, Duration};
use tracing::{error, warn};
use ts_core::{Bar, Timeframe};

const POLL_MS: u64 = 100;

#[async_trait]
impl BarStream for Mt5Client {
    async fn subscribe(&self, sym: &str, tf: Timeframe, tx: mpsc::Sender<Bar>) -> Result<()> {
        let fn_rates: FnRates = self.fn_rates;
        let tf_const = Mt5Client::tf_const(tf);
        let sym_owned = sym.to_string();

        tokio::spawn(async move {
            let mut last_time: i64 = 0;

            loop {
                let now = chrono::Utc::now().timestamp();
                let lookback = tf.seconds() * 3;
                let start = now - lookback;

                let sym_c = match CString::new(sym_owned.as_str()) {
                    Ok(c) => c,
                    Err(e) => {
                        error!("BarStream CString error: {e}");
                        break;
                    }
                };

                let (filled, buf) = match tokio::task::spawn_blocking(move || {
                    let mut buf = vec![Mt5Rate::default(); 10];
                    let filled =
                        unsafe { fn_rates(sym_c.as_ptr(), tf_const, start, now, buf.as_mut_ptr()) };
                    (filled, buf)
                })
                .await
                {
                    Ok(res) => res,
                    Err(e) => {
                        error!("BarStream CopyRates thread joined with error: {e}");
                        break;
                    }
                };

                if filled < 2 {
                    if filled < 0 {
                        warn!("BarStream CopyRates error {filled} for {sym_owned}");
                    }
                    sleep(Duration::from_millis(POLL_MS)).await;
                    continue;
                }

                let filled_clamped = filled.min(10) as usize;
                if filled_clamped < 2 {
                    sleep(Duration::from_millis(POLL_MS)).await;
                    continue;
                }
                let closed = buf[filled_clamped - 2];

                if closed.time > last_time {
                    let bar = Bar::new(
                        closed.time,
                        closed.open,
                        closed.high,
                        closed.low,
                        closed.close,
                        if closed.volume > 0 {
                            closed.volume as f64
                        } else {
                            0.0
                        },
                    );

                    if last_time == 0 {
                        last_time = closed.time;
                    } else {
                        last_time = closed.time;
                        if tx.send(bar).await.is_err() {
                            return;
                        }
                    }
                }

                sleep(Duration::from_millis(POLL_MS)).await;
            }
        });

        Ok(())
    }
}
