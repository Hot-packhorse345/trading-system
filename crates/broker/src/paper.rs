//! Paper-trading broker: real market data + streams (delegated to Binance public
//! endpoints) with fully simulated execution and account. This is the safe
//! default for `trade_executor` so that "testing" never accidentally places live
//! orders. No API keys are required.

use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;
use tracing::info;
use ts_core::{
    AccountInfo, Bar, Direction, ExitReason, Position, SymbolInfo, Tick, Timeframe, TradeRecord,
};

use crate::binance::BinanceBroker;
use crate::traits::{BarStream, DataProvider, Executor, OrderRequest, TickStream};

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

#[derive(Clone)]
pub struct PaperBroker {
    /// Underlying public-data/streams provider (no execution).
    data: BinanceBroker,
    balance: Arc<Mutex<f64>>,
    /// Simulated open positions, keyed by trade_id, for realised-P&L on close.
    open: Arc<Mutex<HashMap<u64, Position>>>,
}

impl PaperBroker {
    pub fn new(initial_balance: f64) -> Self {
        Self {
            data: BinanceBroker::new(String::new(), String::new(), false),
            balance: Arc::new(Mutex::new(initial_balance)),
            open: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Build from the `PAPER_BALANCE` env var (default 100_000).
    pub fn from_env() -> Self {
        let bal = std::env::var("PAPER_BALANCE")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(100_000.0);
        info!(
            initial_balance = bal,
            "building paper broker (simulated execution)"
        );
        Self::new(bal)
    }
}

#[async_trait]
impl DataProvider for PaperBroker {
    async fn ohlcv(&self, sym: &str, tf: Timeframe, start: i64, end: i64) -> Result<Vec<Bar>> {
        self.data.ohlcv(sym, tf, start, end).await
    }

    async fn account(&self) -> Result<AccountInfo> {
        // Realised P&L is booked to `balance` on close; intraday equity == balance.
        let balance = *self.balance.lock().unwrap();
        Ok(AccountInfo {
            balance,
            equity: balance,
            profit: 0.0,
            currency: "USDT".to_string(),
            margin: 0.0,
            margin_free: balance,
        })
    }

    async fn symbol_info(&self, sym: &str) -> Result<SymbolInfo> {
        self.data.symbol_info(sym).await
    }
}

#[async_trait]
impl Executor for PaperBroker {
    async fn open(&self, req: &OrderRequest) -> Result<Position> {
        // Fill at the current touch (ask for buys, bid for sells); fall back to the
        // requested entry price if the book is unavailable.
        let info = self.data.symbol_info(&req.symbol).await.ok();
        let fill = match (req.direction, &info) {
            (Direction::Buy, Some(i)) if i.ask > 0.0 => i.ask,
            (Direction::Sell, Some(i)) if i.bid > 0.0 => i.bid,
            _ => req.entry_price,
        };
        let pos = Position {
            trade_id: req.trade_id,
            strategy_id: req.strategy_id,
            direction: req.direction,
            entry_price: fill,
            initial_stop_loss: req.stop_loss,
            current_stop_loss: req.stop_loss,
            take_profit: req.take_profit,
            volume: req.volume,
            open_risk: (fill - req.stop_loss).abs() * req.volume,
            entry_time: now_secs(),
            is_split_chunk: false,
            group_id: 0,
        };
        self.open.lock().unwrap().insert(req.trade_id, pos);
        info!(
            "PAPER OPEN {} {:?} {:.4} @ {:.5}",
            req.symbol, req.direction, req.volume, fill
        );
        Ok(pos)
    }

    async fn close(&self, pos: &Position, symbol: &str) -> Result<TradeRecord> {
        let info = self.data.symbol_info(symbol).await.ok();
        let exit = match (pos.direction, &info) {
            (Direction::Buy, Some(i)) if i.bid > 0.0 => i.bid,
            (Direction::Sell, Some(i)) if i.ask > 0.0 => i.ask,
            _ => pos.current_stop_loss,
        };
        self.open.lock().unwrap().remove(&pos.trade_id);
        let pnl = pos.pnl(exit);
        *self.balance.lock().unwrap() += pnl;
        info!(
            "PAPER CLOSE {} {:?} @ {:.5} pnl={:.2}",
            symbol, pos.direction, exit, pnl
        );
        Ok(TradeRecord::close_position(
            pos,
            symbol,
            exit,
            now_secs(),
            ExitReason::ExitRule,
        ))
    }

    async fn update_sl(&self, pos: &Position, sl: f64) -> Result<Position> {
        let mut updated = *pos;
        updated.current_stop_loss = sl;
        if let Some(p) = self.open.lock().unwrap().get_mut(&pos.trade_id) {
            p.current_stop_loss = sl;
        }
        Ok(updated)
    }
}

#[async_trait]
impl BarStream for PaperBroker {
    async fn subscribe(&self, sym: &str, tf: Timeframe, tx: mpsc::Sender<Bar>) -> Result<()> {
        BarStream::subscribe(&self.data, sym, tf, tx).await
    }
}

#[async_trait]
impl TickStream for PaperBroker {
    async fn subscribe(&self, sym: &str, tx: mpsc::Sender<Tick>) -> Result<()> {
        TickStream::subscribe(&self.data, sym, tx).await
    }
}
