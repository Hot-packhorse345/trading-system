use ts_core::{Bar, Direction, SymbolInfo};

/// Models how orders are filled during the backtest.
///
/// The simulator only asks for prices. It does not know how spreads,
/// slippage or execution costs are calculated.
pub trait CostModel {
    /// Must be called once per bar before any fills.
    fn update(&mut self, bar: &Bar);

    /// Entry fill.
    fn entry_price(&self, direction: Direction, bar: &Bar) -> f64;

    /// Exit fill.
    fn exit_price(&self, direction: Direction, requested_price: f64) -> f64;
}

/// Simple dynamic spread model.
///
/// Assumptions:
/// - Uses current broker spread as the baseline.
/// - Widens spread during volatile periods.
/// - Never becomes smaller than `min_factor`.
/// - Never becomes larger than `max_factor`.
pub struct DynamicSpreadCostModel {
    avg_range_fast: f64,  // ~20-bar EMA
    avg_range_slow: f64,  // ~100-bar EMA
    avg_volume_fast: f64, // ~20-bar EMA
    avg_volume_slow: f64, // ~100-bar EMA
    alpha_fast: f64,
    alpha_slow: f64,
    base_spread: f64,
    min_factor: f64,
    max_factor: f64,
    /// Below this fraction of the rolling average volume, a bar is considered
    /// a liquidity vacuum and gets an extra non-linear slippage multiplier.
    low_liquidity_threshold: f64,
    /// Cap on the extra liquidity multiplier so a single near-zero-volume bar
    /// can't blow the spread up to an absurd value.
    max_liquidity_multiplier: f64,
}

impl DynamicSpreadCostModel {
    pub fn new(symbol: &SymbolInfo) -> Self {
        Self {
            avg_range_fast: 0.0,
            avg_range_slow: 0.0,
            avg_volume_fast: 0.0,
            avg_volume_slow: 0.0,
            alpha_fast: 2.0 / 21.0,
            alpha_slow: 2.0 / 101.0,
            base_spread: symbol.spread * symbol.point,
            min_factor: 0.5,
            max_factor: 3.0,
            low_liquidity_threshold: 0.2,
            max_liquidity_multiplier: 8.0,
        }
    }

    fn spread(&self) -> f64 {
        let ratio = if self.avg_range_slow > 0.0 {
            self.avg_range_fast / self.avg_range_slow
        } else {
            1.0
        };
        let volatility_adjusted = self.base_spread * ratio.clamp(self.min_factor, self.max_factor);
        volatility_adjusted * self.liquidity_multiplier()
    }

    /// Volume-based slippage multiplier. A bar trading at a small fraction of
    /// its rolling average volume (a liquidity vacuum — e.g. a news spike or
    /// off-hours gap) gets a non-linear widening on top of the volatility
    /// adjustment, since real-world slippage in illiquid conditions is much
    /// worse than a backtest using only bar range would suggest.
    fn liquidity_multiplier(&self) -> f64 {
        if self.avg_volume_slow <= 0.0 {
            return 1.0;
        }
        let liquidity_factor = self.avg_volume_fast / self.avg_volume_slow;
        if liquidity_factor < self.low_liquidity_threshold {
            (1.0 / liquidity_factor.max(f64::EPSILON)).clamp(1.0, self.max_liquidity_multiplier)
        } else {
            1.0
        }
    }
}

impl CostModel for DynamicSpreadCostModel {
    fn update(&mut self, bar: &Bar) {
        let range = (bar.high - bar.low).max(f64::EPSILON);
        if self.avg_range_fast == 0.0 {
            self.avg_range_fast = range;
            self.avg_range_slow = range;
        } else {
            self.avg_range_fast =
                self.alpha_fast * range + (1.0 - self.alpha_fast) * self.avg_range_fast;
            self.avg_range_slow =
                self.alpha_slow * range + (1.0 - self.alpha_slow) * self.avg_range_slow;
        }

        let volume = bar.volume.max(0.0);
        if self.avg_volume_fast == 0.0 {
            self.avg_volume_fast = volume;
            self.avg_volume_slow = volume;
        } else {
            self.avg_volume_fast =
                self.alpha_fast * volume + (1.0 - self.alpha_fast) * self.avg_volume_fast;
            self.avg_volume_slow =
                self.alpha_slow * volume + (1.0 - self.alpha_slow) * self.avg_volume_slow;
        }
    }

    fn entry_price(&self, direction: Direction, bar: &Bar) -> f64 {
        let spread = self.spread();

        match direction {
            Direction::Buy => bar.open + spread * 0.5,
            Direction::Sell => bar.open - spread * 0.5,
            Direction::Hold => bar.open,
        }
    }

    fn exit_price(&self, direction: Direction, requested_price: f64) -> f64 {
        let spread = self.spread();

        match direction {
            Direction::Buy => requested_price - spread * 0.5,
            Direction::Sell => requested_price + spread * 0.5,
            Direction::Hold => requested_price,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn symbol() -> SymbolInfo {
        SymbolInfo {
            symbol: "TEST".to_string(),
            ask: 1.1,
            bid: 1.0995,
            point: 0.0001,
            tick_value: 1.0,
            lot_step: 0.01,
            min_lot: 0.01,
            max_lot: 100.0,
            spread: 5.0,
            digits: 5,
            time: 0.0,
        }
    }

    fn bar(volume: f64) -> Bar {
        Bar::new(0, 100.0, 100.5, 99.5, 100.0, volume)
    }

    /// Warms up both range and volume EMAs on constant bars so the returned
    /// model's spread() reflects steady state (ratio == 1) before the test
    /// perturbs volume.
    fn warmed_up_model(steady_volume: f64) -> DynamicSpreadCostModel {
        let mut model = DynamicSpreadCostModel::new(&symbol());
        for _ in 0..200 {
            model.update(&bar(steady_volume));
        }
        model
    }

    #[test]
    fn test_normal_volume_no_liquidity_multiplier() {
        let model = warmed_up_model(1000.0);
        let base_spread = model.base_spread;
        assert!((model.spread() - base_spread).abs() < 1e-9);
    }

    #[test]
    fn test_low_volume_widens_spread() {
        let mut model = warmed_up_model(1000.0);
        let base_spread = model.spread();
        // A sustained volume vacuum (not just one bar) is needed to pull the
        // fast EMA far enough below the slower one to cross the threshold.
        for _ in 0..40 {
            model.update(&bar(1.0));
        }
        assert!(model.spread() > base_spread);
    }

    #[test]
    fn test_liquidity_multiplier_is_capped() {
        let mut model = warmed_up_model(1000.0);
        for _ in 0..50 {
            model.update(&bar(0.001)); // sustained near-zero volume
        }
        let multiplier = model.liquidity_multiplier();
        assert!(multiplier <= model.max_liquidity_multiplier + 1e-9);
        assert!((multiplier - model.max_liquidity_multiplier).abs() < 1e-6);
    }
}
