use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AccountInfo {
    pub balance: f64,
    pub equity: f64,
    pub profit: f64,
    pub currency: String,
    pub margin: f64,
    pub margin_free: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SymbolInfo {
    pub symbol: String,
    pub ask: f64,
    pub bid: f64,
    pub point: f64,
    pub tick_value: f64,
    pub lot_step: f64,
    pub min_lot: f64,
    pub max_lot: f64,
    pub spread: f64,
    pub digits: u32,
    pub time: f64,
}

impl SymbolInfo {
    pub fn round_lot(&self, lot: f64) -> f64 {
        let steps = (lot / self.lot_step).round();
        let rounded = steps * self.lot_step;
        rounded.max(self.min_lot).min(self.max_lot)
    }

    pub fn mid(&self) -> f64 {
        (self.ask + self.bid) / 2.0
    }
}
