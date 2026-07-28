use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use ts_core::{AccountInfo, Bar, Direction, Position, SymbolInfo, Timeframe, TradeRecord};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderRequest {
    pub symbol: String,
    pub direction: Direction,
    pub volume: f64,
    pub entry_price: f64,
    pub stop_loss: f64,
    pub take_profit: f64,
    pub strategy_id: u64,
    pub trade_id: u64,
    pub comment: String,
}

#[async_trait]
pub trait DataProvider: Send + Sync {
    async fn ohlcv(&self, sym: &str, tf: Timeframe, start: i64, end: i64) -> Result<Vec<Bar>>;
    async fn account(&self) -> Result<AccountInfo>;
    async fn symbol_info(&self, sym: &str) -> Result<SymbolInfo>;
}

#[async_trait]
pub trait Executor: Send + Sync {
    async fn open(&self, req: &OrderRequest) -> Result<Position>;
    async fn close(&self, pos: &Position, symbol: &str) -> Result<TradeRecord>;
    async fn update_sl(&self, pos: &Position, sl: f64) -> Result<Position>;

    /// Close a position at a specific limit price. The default implementation
    /// falls back to a market close. MT5 overrides this to place a SELL_LIMIT /
    /// BUY_LIMIT order at `price` so the fill is guaranteed at or better than
    /// the stop level, avoiding market-order slippage on stop-outs.
    async fn close_at_price(
        &self,
        pos: &Position,
        symbol: &str,
        _price: f64,
    ) -> Result<TradeRecord> {
        self.close(pos, symbol).await
    }

    /// Query the actual open position quantity on the exchange for a given symbol
    /// and direction. Returns the absolute quantity (0.0 = no position). Used to
    /// reconcile local state after a transient error on close — if the exchange
    /// position is already flat, the close succeeded despite the error response.
    async fn position_qty(&self, symbol: &str, direction: Direction) -> Result<f64> {
        let _ = (symbol, direction);
        Ok(0.0)
    }
}

#[async_trait]
pub trait BarStream: Send + Sync {
    async fn subscribe(&self, sym: &str, tf: Timeframe, tx: mpsc::Sender<Bar>) -> Result<()>;
}

#[async_trait]
pub trait TickStream: Send + Sync {
    async fn subscribe(&self, sym: &str, tx: mpsc::Sender<ts_core::Tick>) -> Result<()>;
}
