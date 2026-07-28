use super::{Mt5Client, Mt5TradeResult};
use crate::traits::{Executor, OrderRequest};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use std::ffi::CString;
use std::os::raw::c_int;
use tracing::{error, info, warn};
use ts_core::{Direction, ExitReason, Position, TradeRecord};

#[async_trait]
impl Executor for Mt5Client {
    async fn open(&self, req: &OrderRequest) -> Result<Position> {
        let sym_c = CString::new(req.symbol.as_str())?;
        let cmt_c = CString::new(req.comment.as_str())?;
        let otype: c_int = if req.direction == Direction::Buy {
            0
        } else {
            1
        };
        let fn_send = self.fn_send;
        let volume = req.volume;
        let stop_loss = req.stop_loss;
        let take_profit = req.take_profit;

        let (ret, res) = tokio::task::spawn_blocking(move || {
            let mut res = Mt5TradeResult::default();
            let ret = unsafe {
                fn_send(
                    sym_c.as_ptr(),
                    otype,
                    volume,
                    0.0,
                    stop_loss,
                    take_profit,
                    cmt_c.as_ptr(),
                    &mut res,
                )
            };
            (ret, res)
        })
        .await
        .map_err(|e| anyhow!("MT5 OrderSend thread joined with error: {e}"))?;

        if ret != 1 {
            let retcode = res.retcode;
            return Err(anyhow!(
                "MT5 OrderSend failed for {} (retcode={})",
                req.symbol,
                retcode
            ));
        }

        let mut res_price = res.price;
        if res_price <= 0.0 {
            res_price = req.entry_price;
        }
        let (res_order, res_volume) = (res.order, res.volume);
        info!(
            "MT5 OPEN | {} {:?} {:.3} @ {:.5} ticket={}",
            req.symbol, req.direction, req.volume, res_price, res_order
        );

        Ok(Position {
            trade_id: res_order,
            strategy_id: req.strategy_id,
            direction: req.direction,
            entry_price: res_price,
            initial_stop_loss: req.stop_loss,
            current_stop_loss: req.stop_loss,
            take_profit: req.take_profit,
            volume: res_volume,
            open_risk: 0.0,
            entry_time: chrono::Utc::now().timestamp(),
            is_split_chunk: false,
            group_id: 0,
        })
    }

    async fn close(&self, pos: &Position, symbol: &str) -> Result<TradeRecord> {
        let fn_close = self.fn_close;
        let trade_id = pos.trade_id;

        let (ret, res) = tokio::task::spawn_blocking(move || {
            let mut res = Mt5TradeResult::default();
            let ret = unsafe { fn_close(trade_id, &mut res) };
            (ret, res)
        })
        .await
        .map_err(|e| anyhow!("MT5 OrderClose thread joined with error: {e}"))?;

        if ret != 1 {
            let retcode = res.retcode;
            return Err(anyhow!(
                "MT5 OrderClose failed for trade {} (retcode={})",
                pos.trade_id,
                retcode
            ));
        }

        let mut res_price = res.price;
        if res_price <= 0.0 {
            res_price = pos.entry_price;
        }
        let exit_time = chrono::Utc::now().timestamp();
        let rec =
            TradeRecord::close_position(pos, symbol, res_price, exit_time, ExitReason::ExitRule);
        info!("MT5 CLOSE | {} R={:.2}", symbol, rec.profit);
        Ok(rec)
    }

    async fn close_at_price(
        &self,
        pos: &Position,
        symbol: &str,
        price: f64,
    ) -> Result<TradeRecord> {
        let sym_c = CString::new(symbol)?;
        let cmt_c = CString::new(format!("sl_limit_{}", pos.trade_id))?;
        // MT5 ORDER_TYPE: 2 = BUY_LIMIT (closes a short), 3 = SELL_LIMIT (closes a long)
        let otype: c_int = match pos.direction {
            Direction::Buy => 3,
            Direction::Sell => 2,
            Direction::Hold => return self.close(pos, symbol).await,
        };
        let fn_send = self.fn_send;
        let volume = pos.volume;
        let limit_price = price;

        let (ret, res) = tokio::task::spawn_blocking(move || {
            let mut res = Mt5TradeResult::default();
            let ret = unsafe {
                fn_send(
                    sym_c.as_ptr(),
                    otype,
                    volume,
                    limit_price,
                    0.0,
                    0.0,
                    cmt_c.as_ptr(),
                    &mut res,
                )
            };
            (ret, res)
        })
        .await
        .map_err(|e| anyhow!("MT5 close_at_price thread error: {e}"))?;

        if ret != 1 {
            let retcode = res.retcode;
            warn!(
                "MT5 close_at_price failed (retcode={}), falling back to market close",
                retcode
            );
            return self.close(pos, symbol).await;
        }

        let fill_price = if res.price > 0.0 { res.price } else { price };
        let exit_time = chrono::Utc::now().timestamp();
        let rec =
            TradeRecord::close_position(pos, symbol, fill_price, exit_time, ExitReason::StopLoss);
        info!(
            "MT5 CLOSE_LIMIT | {} @ {:.5} R={:.2}",
            symbol, fill_price, rec.profit
        );
        Ok(rec)
    }

    async fn update_sl(&self, pos: &Position, sl: f64) -> Result<Position> {
        let fn_mod = self.fn_modify.ok_or_else(|| {
            error!(
                "MT5 update_sl: 'OrderModify' not exported by DLL. \
                 Rebuild the DLL or implement via OrderClose+OrderSend. \
                 Trade {} SL NOT updated at broker.",
                pos.trade_id
            );
            anyhow!(
                "MT5 DLL missing 'OrderModify' export — cannot update SL for trade {}",
                pos.trade_id
            )
        })?;

        let trade_id = pos.trade_id;
        let take_profit = pos.take_profit;

        let ret = tokio::task::spawn_blocking(move || unsafe { fn_mod(trade_id, sl, take_profit) })
            .await
            .map_err(|e| anyhow!("MT5 OrderModify thread joined with error: {e}"))?;

        if ret != 1 {
            return Err(anyhow!(
                "MT5 OrderModify failed for trade {} (retcode={ret})",
                pos.trade_id
            ));
        }

        info!("MT5 update_sl | trade={} sl={:.5}", pos.trade_id, sl);
        let mut updated = *pos;
        updated.current_stop_loss = sl;
        Ok(updated)
    }
}
