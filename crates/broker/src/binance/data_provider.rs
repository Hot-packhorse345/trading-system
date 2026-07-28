use super::BinanceBroker;
use crate::traits::DataProvider;
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tokio::time::{sleep, Duration};
use ts_core::{AccountInfo, Bar, SymbolInfo, Timeframe};

#[async_trait]
impl DataProvider for BinanceBroker {
    async fn ohlcv(&self, sym: &str, tf: Timeframe, start: i64, end: i64) -> Result<Vec<Bar>> {
        if start > end {
            tracing::warn!("Binance ohlcv: start timestamp ({start}) is greater than end timestamp ({end}). Returning empty bar list.");
            return Ok(Vec::new());
        }
        let mut bars = Vec::new();
        let mut from = start * 1_000_i64;

        loop {
            let url = format!(
                "{}/fapi/v1/klines?symbol={}&interval={}&startTime={}&endTime={}&limit=1000",
                self.rest,
                sym.to_uppercase(),
                tf.to_binance(),
                from,
                end * 1_000,
            );
            let resp: serde_json::Value = self.client.get(&url).send().await?.json().await?;
            let arr = resp.as_array().context("expected array from klines")?;
            if arr.is_empty() {
                break;
            }

            for k in arr {
                let k = k.as_array().context("kline entry not an array")?;
                bars.push(Bar::new(
                    k[0].as_i64().unwrap_or(0) / 1_000,
                    k[1].as_str().unwrap_or("0").parse().unwrap_or(0.0),
                    k[2].as_str().unwrap_or("0").parse().unwrap_or(0.0),
                    k[3].as_str().unwrap_or("0").parse().unwrap_or(0.0),
                    k[4].as_str().unwrap_or("0").parse().unwrap_or(0.0),
                    k[5].as_str().unwrap_or("0").parse().unwrap_or(0.0),
                ));
            }

            let last_ts = bars.last().map(|b| b.time * 1_000 + 1).unwrap_or(0);
            if last_ts >= end * 1_000 || arr.len() < 1_000 {
                break;
            }
            from = last_ts;

            sleep(Duration::from_millis(100)).await;
        }

        // Drop a still-forming final candle: the klines REST endpoint returns the
        // current (incomplete) candle as the last element, which would otherwise be
        // persisted as if it were a closed bar.
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let tf_secs = tf.seconds();
        bars.retain(|b| b.time + tf_secs <= now);

        Ok(bars)
    }

    async fn account(&self) -> Result<AccountInfo> {
        let j = self.get_signed("/fapi/v2/account", "").await?;
        Ok(AccountInfo {
            balance: j["totalWalletBalance"]
                .as_str()
                .unwrap_or("0")
                .parse()
                .unwrap_or(0.0),
            equity: j["totalMarginBalance"]
                .as_str()
                .unwrap_or("0")
                .parse()
                .unwrap_or(0.0),
            profit: j["totalUnrealizedProfit"]
                .as_str()
                .unwrap_or("0")
                .parse()
                .unwrap_or(0.0),
            currency: "USDT".to_string(),
            margin: j["totalInitialMargin"]
                .as_str()
                .unwrap_or("0")
                .parse()
                .unwrap_or(0.0),
            margin_free: j["availableBalance"]
                .as_str()
                .unwrap_or("0")
                .parse()
                .unwrap_or(0.0),
        })
    }

    async fn symbol_info(&self, sym: &str) -> Result<SymbolInfo> {
        let upper = sym.to_uppercase();

        // ── Serve from cache when fresh ───────────────────────────────
        // The heavy `exchangeInfo` payload (all symbols) was previously fetched on
        // every entry and every stop-loss update, risking an IP weight ban.
        if let Some((info, ts)) = self.symbol_cache.lock().unwrap().get(&upper).cloned() {
            if ts.elapsed() < super::SYMBOL_INFO_TTL {
                return Ok(info);
            }
        }

        // ── exchangeInfo ──────────────────────────────────────────────
        let exch_url = format!("{}/fapi/v1/exchangeInfo", self.rest);
        let exch: serde_json::Value = self.client.get(&exch_url).send().await?.json().await?;

        let sym_data = exch["symbols"]
            .as_array()
            .context("exchangeInfo missing symbols")?
            .iter()
            .find(|s| s["symbol"].as_str() == Some(upper.as_str()))
            .cloned()
            .ok_or_else(|| anyhow!("symbol {sym} not found on Binance"))?;

        let price_precision = sym_data["pricePrecision"].as_u64().unwrap_or(8) as u32;

        // `point` is the real price increment (tickSize), used both for price
        // rounding and as the sizing unit. For linear (USDⓈ-M) futures the P&L of
        // a `point` move per 1 contract is `point` quote-currency, so `tick_value`
        // must equal `point` — otherwise risk-based position sizing is scaled by
        // tickSize/10^-pricePrecision and `$` P&L is wrong by the same factor.
        let mut point = 10_f64.powi(-(price_precision as i32)); // fallback
        let mut lot_step = 0.001_f64;
        let mut min_lot = 0.001_f64;
        let mut max_lot = 1_000.0_f64;

        if let Some(filters) = sym_data["filters"].as_array() {
            for f in filters {
                match f["filterType"].as_str() {
                    Some("PRICE_FILTER") => {
                        if let Some(ts) = f["tickSize"].as_str().and_then(|s| s.parse::<f64>().ok())
                        {
                            if ts > 0.0 {
                                point = ts;
                            }
                        }
                    }
                    Some("LOT_SIZE") => {
                        lot_step = f["stepSize"]
                            .as_str()
                            .unwrap_or("0.001")
                            .parse()
                            .unwrap_or(0.001);
                        min_lot = f["minQty"]
                            .as_str()
                            .unwrap_or("0.001")
                            .parse()
                            .unwrap_or(0.001);
                        max_lot = f["maxQty"]
                            .as_str()
                            .unwrap_or("1000")
                            .parse()
                            .unwrap_or(1_000.0);
                    }
                    _ => {}
                }
            }
        }
        let tick_value = point;

        // ── bookTicker: live ask / bid ────────────────────────────────
        let ticker_url = format!("{}/fapi/v1/ticker/bookTicker?symbol={}", self.rest, upper);
        let ticker: serde_json::Value = self.client.get(&ticker_url).send().await?.json().await?;
        let ask: f64 = ticker["askPrice"]
            .as_str()
            .unwrap_or("0")
            .parse()
            .unwrap_or(0.0);
        let bid: f64 = ticker["bidPrice"]
            .as_str()
            .unwrap_or("0")
            .parse()
            .unwrap_or(0.0);

        let spread = if point > 0.0 {
            (ask - bid) / point
        } else {
            0.0
        };

        let info = SymbolInfo {
            symbol: sym.to_string(),
            ask,
            bid,
            point,
            tick_value,
            lot_step,
            min_lot,
            max_lot,
            spread,
            digits: price_precision,
            time: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs_f64(),
        };
        self.symbol_cache
            .lock()
            .unwrap()
            .insert(upper, (info.clone(), Instant::now()));
        Ok(info)
    }
}
