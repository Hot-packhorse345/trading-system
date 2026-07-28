use super::{Mt5Client, Mt5Rate, Mt5SymInfo};
use crate::traits::DataProvider;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use std::ffi::CString;
use std::time::Instant;
use ts_core::{AccountInfo, Bar, SymbolInfo, Timeframe};

#[async_trait]
impl DataProvider for Mt5Client {
    async fn ohlcv(&self, sym: &str, tf: Timeframe, start: i64, end: i64) -> Result<Vec<Bar>> {
        if start > end {
            tracing::warn!("MT5 ohlcv: start timestamp ({start}) is greater than end timestamp ({end}). Returning empty bar list.");
            return Ok(Vec::new());
        }

        let sym_c = CString::new(sym)?;
        let tf_c = Mt5Client::tf_const(tf);
        let tf_secs = tf.seconds().max(1) as i64;

        // Define a safe maximum limit to prevent huge memory allocations,
        // and adjust the start time if the requested range exceeds it.
        let limit = 1_000_000;
        let mut start_val = start;
        let estimate_bars = (end - start) / tf_secs;
        if estimate_bars > limit {
            start_val = end - (limit * tf_secs);
        }

        // Chunking: MT5 functions are more stable when requested in smaller chunks.
        // We split the date range into chunks of 2000 bars.
        let chunk_size = 2000;
        let chunk_duration = chunk_size * tf_secs;
        let mut all_bars = Vec::new();
        let mut current_start = start_val;
        while current_start <= end {
            let current_end = (current_start + chunk_duration).min(end);
            let max_bars_chunk = ((current_end - current_start) / tf_secs + 10) as usize;

            let fn_rates = self.fn_rates;
            let sym_c_clone = sym_c.clone();

            // Retry/stabilization loop to allow background history loading
            let mut chunk_rates = Vec::new();
            let mut attempts = 0;
            let max_attempts = 30; // up to 3 seconds wait per chunk
            let mut last_count = -1;
            let mut error_count = 0;
            let mut zero_count = 0;

            while attempts < max_attempts {
                let buf = vec![Mt5Rate::default(); max_bars_chunk];
                let sym_c_thread = sym_c_clone.clone();

                let (filled, buf) = tokio::task::spawn_blocking(move || {
                    let mut buf = buf;
                    let filled = unsafe {
                        fn_rates(
                            sym_c_thread.as_ptr(),
                            tf_c,
                            current_start,
                            current_end,
                            buf.as_mut_ptr(),
                        )
                    };
                    (filled, buf)
                })
                .await
                .map_err(|e| anyhow!("MT5 CopyRates thread joined with error: {e}"))?;

                if filled > 0 {
                    error_count = 0;
                    zero_count = 0;
                    if filled > last_count {
                        last_count = filled;
                        chunk_rates = buf[..filled as usize].to_vec();
                    } else {
                        // Count stabilized (or decreased/stayed same after a valid read)
                        break;
                    }
                } else if filled == 0 {
                    error_count = 0;
                    zero_count += 1;
                    if zero_count >= 5 {
                        chunk_rates.clear();
                        break;
                    }
                    last_count = 0;
                    chunk_rates.clear();
                } else {
                    // MT5 returned error (-1), might still be loading
                    error_count += 1;
                    if error_count >= 5 {
                        tracing::warn!("MT5 CopyRates returned error multiple times for {sym}. Breaking early to prevent hang.");
                        break;
                    }
                }

                attempts += 1;
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            }

            let chunk_bars: Vec<Bar> = chunk_rates
                .iter()
                .copied()
                .filter(|r| r.time >= current_start && r.time <= current_end)
                .map(|r| Bar::new(r.time, r.open, r.high, r.low, r.close, r.volume as f64))
                .collect();

            all_bars.extend(chunk_bars);

            // Advance start time
            current_start = current_end + 1;
        }

        // Deduplicate and sort
        all_bars.sort_by_key(|b| b.time);
        all_bars.dedup_by_key(|b| b.time);

        Ok(all_bars)
    }

    async fn account(&self) -> Result<AccountInfo> {
        let fn_acct = self.fn_acct;
        let (ret, bal, eq, mg, free) = tokio::task::spawn_blocking(move || {
            let (mut bal, mut eq, mut mg, mut free) = (0_f64, 0_f64, 0_f64, 0_f64);
            let ret = unsafe { fn_acct(&mut bal, &mut eq, &mut mg, &mut free) };
            (ret, bal, eq, mg, free)
        })
        .await
        .map_err(|e| anyhow!("MT5 AccountInfo thread joined with error: {e}"))?;

        if ret != 1 {
            return Err(anyhow!("MT5 AccountInfo DLL call returned {ret}"));
        }

        Ok(AccountInfo {
            balance: bal,
            equity: eq,
            profit: eq - bal,
            currency: "USD".to_string(),
            margin: mg,
            margin_free: free,
        })
    }

    async fn symbol_info(&self, sym: &str) -> Result<SymbolInfo> {
        let upper = sym.to_uppercase();

        // Serve static symbol properties from cache — tick_value, point, lot sizes
        // change only on broker config changes. Ask/bid are excluded from the cache
        // key since position sizing doesn't need a live quote here (entry_price comes
        // from the signal, not from symbol_info).
        if let Some((info, ts)) = self.symbol_cache.lock().unwrap().get(&upper).cloned() {
            if ts.elapsed() < super::SYMBOL_INFO_TTL {
                return Ok(info);
            }
        }

        let sym_c = CString::new(sym)?;

        let fn_sym_tick = self.fn_sym_tick;
        let fn_sym_info = self.fn_sym_info;

        let (tick_ok, tick, props_ok, props) = tokio::task::spawn_blocking(move || {
            let mut tick = super::Mt5Tick::default();
            let tick_ok = if let Some(f) = fn_sym_tick {
                let ret = unsafe { f(sym_c.as_ptr(), &mut tick) };
                ret == 1
            } else {
                false
            };

            let mut props = Mt5SymInfo::default();
            let props_ok = if let Some(f) = fn_sym_info {
                let ret = unsafe { f(sym_c.as_ptr(), &mut props) };
                ret == 1
            } else {
                false
            };

            (tick_ok, tick, props_ok, props)
        })
        .await
        .map_err(|e| anyhow!("MT5 SymbolInfo thread joined with error: {e}"))?;

        // Fall back to safe defaults if DLL doesn't provide values.
        let point = if props_ok && props.point > 0.0 {
            props.point
        } else {
            0.00001
        };
        let tick_value = if props_ok && props.tick_value > 0.0 {
            props.tick_value
        } else {
            1.0
        };
        let lot_step = if props_ok && props.lot_step > 0.0 {
            props.lot_step
        } else {
            0.01
        };
        let min_lot = if props_ok && props.min_lot > 0.0 {
            props.min_lot
        } else {
            0.01
        };
        let max_lot = if props_ok && props.max_lot > 0.0 {
            props.max_lot
        } else {
            500.0
        };
        let digits = if props_ok && props.digits > 0 {
            props.digits as u32
        } else {
            5
        };
        let spread = if props_ok {
            props.spread
        } else if tick_ok && point > 0.0 {
            (tick.ask - tick.bid) / point
        } else {
            0.0
        };

        let info = SymbolInfo {
            symbol: sym.to_string(),
            ask: if tick_ok { tick.ask } else { 0.0 },
            bid: if tick_ok { tick.bid } else { 0.0 },
            point,
            tick_value,
            lot_step,
            min_lot,
            max_lot,
            spread,
            digits,
            time: if tick_ok { tick.time as f64 } else { 0.0 },
        };
        self.symbol_cache
            .lock()
            .unwrap()
            .insert(upper, (info.clone(), Instant::now()));
        Ok(info)
    }
}
