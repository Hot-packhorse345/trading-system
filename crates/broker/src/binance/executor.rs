use super::{BinanceBroker, OpenOrderInfo};
use crate::traits::{DataProvider, Executor, OrderRequest};
use anyhow::{Context, Result};
use async_trait::async_trait;
use tracing::{error, info, warn};
use ts_core::{Direction, ExitReason, Position, TradeRecord};

// ── Private helpers on BinanceBroker ─────────────────────────────────────────

impl BinanceBroker {
    async fn place_stop_order(
        &self,
        symbol: &str,
        close_side: &str,
        position_side: &str,
        order_type: &str,
        stop_price: f64,
        tick_size: f64,
    ) -> Result<i64> {
        let rounded = Self::round_price(stop_price, tick_size);
        let params = format!(
            "symbol={}&side={}&positionSide={}&type={}&stopPrice={}&closePosition=true",
            symbol.to_uppercase(),
            close_side,
            position_side,
            order_type,
            rounded,
        );
        let j = self.post_signed("/fapi/v1/order", &params).await?;
        j["orderId"]
            .as_i64()
            .context("missing orderId in stop order response")
    }

    async fn cancel_order(&self, symbol: &str, order_id: i64) -> Result<()> {
        let params = format!("symbol={}&orderId={}", symbol.to_uppercase(), order_id);
        self.delete_signed("/fapi/v1/order", &params).await?;
        Ok(())
    }

    /// Query a previously placed order by its client id to read the realised
    /// average fill price (and update time). Used as a fallback when the order
    /// placement response reports `avgPrice: 0`.
    async fn query_order_fill(
        &self,
        sym: &str,
        strategy_id: u64,
        trade_id: u64,
    ) -> Result<(f64, i64)> {
        let params = format!(
            "symbol={}&origClientOrderId=ts_{}_{}",
            sym.to_uppercase(),
            strategy_id,
            trade_id
        );
        let j = self.get_signed("/fapi/v1/order", &params).await?;
        let p = j["avgPrice"]
            .as_str()
            .unwrap_or("0")
            .parse::<f64>()
            .unwrap_or(0.0);
        let t = j["updateTime"].as_i64().unwrap_or(0) / 1_000;
        Ok((p, t))
    }

    /// Reduce-only style market close used to roll back an entry whose protective
    /// stop could not be established.
    async fn market_close(&self, sym: &str, side: &str, pos_side: &str, volume: f64) -> Result<()> {
        let params = format!(
            "symbol={}&side={}&positionSide={}&type=MARKET&quantity={:.8}",
            sym.to_uppercase(),
            side,
            pos_side,
            volume
        );
        self.post_signed("/fapi/v1/order", &params).await?;
        Ok(())
    }
}

// ── Executor impl ─────────────────────────────────────────────────────────────

#[async_trait]
impl Executor for BinanceBroker {
    async fn open(&self, req: &OrderRequest) -> Result<Position> {
        let sym = req.symbol.to_uppercase();
        let side = if req.direction == Direction::Buy {
            "BUY"
        } else {
            "SELL"
        };
        let pos_side = if req.direction == Direction::Buy {
            "LONG"
        } else {
            "SHORT"
        };
        let close_side = if req.direction == Direction::Buy {
            "SELL"
        } else {
            "BUY"
        };

        let sym_info = self.symbol_info(&req.symbol).await?;
        let tick_sz = sym_info.point.max(1e-10);

        // ── Market entry (RESULT so avgPrice is populated) ───────────
        let params = format!(
            "symbol={sym}&side={side}&positionSide={pos_side}&type=MARKET&quantity={:.8}&newOrderRespType=RESULT&newClientOrderId=ts_{}_{}",
            req.volume, req.strategy_id, req.trade_id
        );
        let j = self.post_signed("/fapi/v1/order", &params).await?;
        let mut fill: f64 = j["avgPrice"].as_str().unwrap_or("0").parse().unwrap_or(0.0);
        let mut ts = j["updateTime"].as_i64().unwrap_or(0) / 1_000;

        // Fallback: a MARKET order response can still report avgPrice 0 right after
        // the fill. Query the order to read the true average price — an entry_price
        // of 0 would corrupt every downstream R-multiple and stop calculation.
        if fill <= 0.0 {
            if let Ok((p, t)) = self
                .query_order_fill(&sym, req.strategy_id, req.trade_id)
                .await
            {
                if p > 0.0 {
                    fill = p;
                    if t > 0 {
                        ts = t;
                    }
                }
            }
        }
        if fill <= 0.0 {
            let _ = self
                .market_close(&sym, close_side, pos_side, req.volume)
                .await;
            return Err(anyhow::anyhow!(
                "could not determine entry fill price for {} trade {} — entry rolled back",
                req.symbol,
                req.trade_id
            ));
        }

        // ── SL + TP brackets in parallel ───────────────────────────
        // SL is mandatory; TP is optional. Both are independent orders so we
        // place them concurrently to save one RTT on every entry.
        let has_tp = req.take_profit > 0.0;
        let (sl_res, tp_res) = tokio::join!(
            self.place_stop_order(
                &sym,
                close_side,
                pos_side,
                "STOP_MARKET",
                req.stop_loss,
                tick_sz
            ),
            async {
                if has_tp {
                    Some(
                        self.place_stop_order(
                            &sym,
                            close_side,
                            pos_side,
                            "TAKE_PROFIT_MARKET",
                            req.take_profit,
                            tick_sz,
                        )
                        .await,
                    )
                } else {
                    None
                }
            },
        );

        let sl_id = match sl_res {
            Ok(id) => Some(id),
            Err(e) => {
                // If TP was placed concurrently and succeeded, cancel it before rolling back.
                if let Some(Ok(tp_id)) = tp_res {
                    let _ = self.cancel_order(&sym, tp_id).await;
                }
                error!(
                    "SL order failed for {} trade {} ({e}) — closing position",
                    req.symbol, req.trade_id
                );
                let _ = self
                    .market_close(&sym, close_side, pos_side, req.volume)
                    .await;
                return Err(e.context("stop-loss placement failed; entry rolled back"));
            }
        };

        let tp_id = match tp_res {
            Some(Ok(id)) => Some(id),
            Some(Err(e)) => {
                warn!("TP order failed for {}: {e}", req.symbol);
                None
            }
            None => None,
        };

        // ── Register for later update_sl / close ────────────────────
        self.open_orders.lock().unwrap().insert(
            req.trade_id,
            OpenOrderInfo {
                symbol: req.symbol.clone(),
                sl_order_id: sl_id,
                tp_order_id: tp_id,
                position_side: pos_side.to_string(),
            },
        );

        info!(
            "OPEN {} {:?} {:.3} lots @ {:.5}",
            req.symbol, req.direction, req.volume, fill
        );

        Ok(Position {
            trade_id: req.trade_id,
            strategy_id: req.strategy_id,
            direction: req.direction,
            entry_price: fill,
            initial_stop_loss: req.stop_loss,
            current_stop_loss: req.stop_loss,
            take_profit: req.take_profit,
            volume: req.volume,
            open_risk: (fill - req.stop_loss).abs() * req.volume,
            entry_time: ts,
            is_split_chunk: false,
            group_id: 0,
        })
    }

    async fn close(&self, pos: &Position, symbol: &str) -> Result<TradeRecord> {
        let sym = symbol.to_uppercase();

        // ── Extract order IDs BEFORE any await ───────────────────────────
        let order_ids = {
            let mut orders = self.open_orders.lock().unwrap();
            orders.remove(&pos.trade_id)
        };

        // ── Cancel bracket orders in parallel ──────────────────────────
        if let Some(info) = order_ids {
            match (info.sl_order_id, info.tp_order_id) {
                (Some(sl), Some(tp)) => {
                    let (r1, r2) =
                        tokio::join!(self.cancel_order(&sym, sl), self.cancel_order(&sym, tp),);
                    if let Err(e) = r1 {
                        warn!("cancel SL order {sl} on {sym}: {e}");
                    }
                    if let Err(e) = r2 {
                        warn!("cancel TP order {tp} on {sym}: {e}");
                    }
                }
                (Some(sl), None) => {
                    if let Err(e) = self.cancel_order(&sym, sl).await {
                        warn!("cancel SL order {sl} on {sym}: {e}");
                    }
                }
                (None, Some(tp)) => {
                    if let Err(e) = self.cancel_order(&sym, tp).await {
                        warn!("cancel TP order {tp} on {sym}: {e}");
                    }
                }
                (None, None) => {}
            }
        } else {
            let _ = self
                .delete_signed("/fapi/v1/allOpenOrders", &format!("symbol={sym}"))
                .await;
        }

        // ── Place market close order ────────────────────────────────────
        let side = if pos.direction == Direction::Buy {
            "SELL"
        } else {
            "BUY"
        };
        let pos_side = if pos.direction == Direction::Buy {
            "LONG"
        } else {
            "SHORT"
        };
        let params   = format!(
                "symbol={sym}&side={side}&positionSide={pos_side}&type=MARKET&quantity={:.8}&newOrderRespType=RESULT",
                pos.volume
            );

        let j = self.post_signed("/fapi/v1/order", &params).await?;
        let mut exit_price = j["avgPrice"]
            .as_str()
            .unwrap_or("0")
            .parse::<f64>()
            .unwrap_or(0.0);
        let exit_time = j["updateTime"].as_i64().unwrap_or(0) / 1_000;
        // Fall back to the position's last stop price only if the exchange did
        // not report a fill price, so reported P&L is never computed off 0.
        if exit_price <= 0.0 {
            exit_price = pos.current_stop_loss;
        }

        Ok(TradeRecord::close_position(
            pos,
            symbol,
            exit_price,
            exit_time,
            ExitReason::ExitRule,
        ))
    }

    async fn update_sl(&self, pos: &Position, sl: f64) -> Result<Position> {
        let info = { self.open_orders.lock().unwrap().get(&pos.trade_id).cloned() };

        let Some(info) = info else {
            warn!(
                "update_sl: no order info for trade_id {} — updating local state only",
                pos.trade_id
            );
            let mut p = *pos;
            p.current_stop_loss = sl;
            return Ok(p);
        };

        let sym = info.symbol.to_uppercase();
        let close_side = if info.position_side == "LONG" {
            "SELL"
        } else {
            "BUY"
        };

        let tick_sz = self
            .symbol_info(&info.symbol)
            .await
            .map(|s| s.point.max(1e-10))
            .unwrap_or(0.01);

        if let Some(old_id) = info.sl_order_id {
            if let Err(e) = self.cancel_order(&sym, old_id).await {
                warn!("update_sl: cancel old SL {old_id} on {sym}: {e}");
            }
        }

        let new_sl_id = self
            .place_stop_order(
                &sym,
                close_side,
                &info.position_side,
                "STOP_MARKET",
                sl,
                tick_sz,
            )
            .await
            .map_err(|e| {
                error!(
                    "update_sl: new SL placement failed for trade {}: {e}",
                    pos.trade_id
                );
                e
            })?;

        {
            let mut map = self.open_orders.lock().unwrap();
            if let Some(rec) = map.get_mut(&pos.trade_id) {
                rec.sl_order_id = Some(new_sl_id);
            }
        }

        info!(
            "update_sl {} trade={} sl={:.5}",
            info.symbol, pos.trade_id, sl
        );
        let mut updated = *pos;
        updated.current_stop_loss = sl;
        Ok(updated)
    }

    async fn position_qty(&self, symbol: &str, direction: Direction) -> Result<f64> {
        let sym = symbol.to_uppercase();
        let j = self
            .get_signed("/fapi/v2/positionRisk", &format!("symbol={sym}"))
            .await?;
        let pos_side = if direction == Direction::Buy {
            "LONG"
        } else {
            "SHORT"
        };
        let qty = j
            .as_array()
            .and_then(|arr| {
                arr.iter()
                    .find(|p| p["positionSide"].as_str() == Some(pos_side))
            })
            .and_then(|p| p["positionAmt"].as_str())
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0)
            .abs();
        Ok(qty)
    }
}
