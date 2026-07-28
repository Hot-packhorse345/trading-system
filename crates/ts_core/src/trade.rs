use crate::{Direction, ExitReason};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Position {
    pub trade_id: u64,
    pub strategy_id: u64,
    pub direction: Direction,
    pub entry_price: f64,
    pub initial_stop_loss: f64,
    pub current_stop_loss: f64,
    pub take_profit: f64,
    pub volume: f64,
    pub open_risk: f64,
    pub entry_time: i64,
    pub is_split_chunk: bool,
    pub group_id: u64,
}

impl Position {
    pub fn r_multiple(&self, price: f64) -> f64 {
        let risk = (self.entry_price - self.initial_stop_loss).abs();
        if risk == 0.0 {
            return 0.0;
        }
        match self.direction {
            Direction::Buy => (price - self.entry_price) / risk,
            Direction::Sell => (self.entry_price - price) / risk,
            Direction::Hold => 0.0,
        }
    }

    pub fn pnl(&self, price: f64) -> f64 {
        match self.direction {
            Direction::Buy => (price - self.entry_price) * self.volume,
            Direction::Sell => (self.entry_price - price) * self.volume,
            Direction::Hold => 0.0,
        }
    }

    /// True if `take_profit` would be triggered by a `(hi, lo)` price pair.
    /// For bar data pass `(bar.high, bar.low)`; for tick data pass `(ask, bid)`.
    pub fn tp_hit(&self, hi: f64, lo: f64) -> bool {
        if self.take_profit <= 0.0 {
            return false;
        }
        match self.direction {
            Direction::Buy => hi >= self.take_profit,
            Direction::Sell => lo <= self.take_profit,
            Direction::Hold => false,
        }
    }

    /// True if `current_stop_loss` would be triggered by a `(hi, lo)` price pair.
    /// For bar data pass `(bar.high, bar.low)`; for tick data pass `(ask, bid)`.
    pub fn sl_hit(&self, hi: f64, lo: f64) -> bool {
        match self.direction {
            Direction::Buy => lo <= self.current_stop_loss,
            Direction::Sell => hi >= self.current_stop_loss,
            Direction::Hold => false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeRecord {
    pub trade_id: u64,
    pub strategy_id: u64,
    pub symbol: String,
    pub direction: Direction,
    pub entry_price: f64,
    pub exit_price: f64,
    pub initial_stop_loss: f64,
    pub current_stop_loss: f64,
    pub take_profit: f64,
    pub volume: f64,
    pub open_risk: f64,
    pub entry_time: i64,
    pub exit_time: i64,
    pub exit_reason: ExitReason,
    /// R-multiple of the trade: (exit-entry)/risk normalised to the initial stop
    /// distance. NOT currency P&L — use `pnl()` for that.
    pub profit: f64,
    /// Currency P&L: (exit-entry)*volume, with sign for direction.
    #[serde(default)]
    pub currency_pnl: f64,
    pub group_id: u64,
}

impl TradeRecord {
    pub fn r_multiple(&self) -> f64 {
        let risk = (self.entry_price - self.initial_stop_loss).abs();
        if risk == 0.0 {
            return 0.0;
        }
        match self.direction {
            Direction::Buy => (self.exit_price - self.entry_price) / risk,
            Direction::Sell => (self.entry_price - self.exit_price) / risk,
            Direction::Hold => 0.0,
        }
    }

    pub fn pnl(&self) -> f64 {
        match self.direction {
            Direction::Buy => (self.exit_price - self.entry_price) * self.volume,
            Direction::Sell => (self.entry_price - self.exit_price) * self.volume,
            Direction::Hold => 0.0,
        }
    }

    pub fn close_position(
        pos: &Position,
        symbol: &str,
        exit_price: f64,
        exit_time: i64,
        reason: ExitReason,
    ) -> Self {
        let mut rec = TradeRecord {
            trade_id: pos.trade_id,
            strategy_id: pos.strategy_id,
            symbol: symbol.to_string(),
            direction: pos.direction,
            entry_price: pos.entry_price,
            exit_price,
            initial_stop_loss: pos.initial_stop_loss,
            current_stop_loss: pos.current_stop_loss,
            take_profit: pos.take_profit,
            volume: pos.volume,
            open_risk: pos.open_risk,
            entry_time: pos.entry_time,
            exit_time,
            exit_reason: reason,
            profit: 0.0,
            currency_pnl: 0.0,
            group_id: pos.group_id,
        };
        rec.profit = rec.r_multiple();
        rec.currency_pnl = rec.pnl();
        rec
    }

    /// Build a record representing `pos` while it is still open, e.g. for
    /// durable storage in a trade DB before the position is closed.
    pub fn from_open_position(pos: &Position, symbol: &str) -> Self {
        TradeRecord {
            trade_id: pos.trade_id,
            strategy_id: pos.strategy_id,
            symbol: symbol.to_string(),
            direction: pos.direction,
            entry_price: pos.entry_price,
            exit_price: 0.0,
            initial_stop_loss: pos.initial_stop_loss,
            current_stop_loss: pos.current_stop_loss,
            take_profit: pos.take_profit,
            volume: pos.volume,
            open_risk: pos.open_risk,
            entry_time: pos.entry_time,
            exit_time: 0,
            exit_reason: ExitReason::EndOfData,
            profit: 0.0,
            currency_pnl: 0.0,
            group_id: pos.group_id,
        }
    }
}
