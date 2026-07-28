use crate::Direction;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Signal {
    pub direction: Direction,
    pub entry_price: f64,
    pub stop_loss: f64,
    pub take_profit: f64,
}

impl Signal {
    pub fn new(direction: Direction, entry_price: f64, stop_loss: f64, take_profit: f64) -> Self {
        Self {
            direction,
            entry_price,
            stop_loss,
            take_profit,
        }
    }

    pub fn is_valid(&self) -> bool {
        let no_tp = self.take_profit == 0.0;
        match self.direction {
            Direction::Buy => {
                self.entry_price > self.stop_loss && (no_tp || self.take_profit > self.entry_price)
            }
            Direction::Sell => {
                self.entry_price < self.stop_loss && (no_tp || self.take_profit < self.entry_price)
            }
            Direction::Hold => false,
        }
    }
}
