use ts_core::{AccountInfo, SymbolInfo};

pub trait VolumeManager: Send + Sync {
    fn position_size(
        &self,
        account: &AccountInfo,
        symbol: &SymbolInfo,
        entry: f64,
        stop: f64,
        drawdown_pct: f64,
    ) -> f64;
}

pub struct FixedAmount {
    pub amount: f64,
}
impl VolumeManager for FixedAmount {
    fn position_size(
        &self,
        _a: &AccountInfo,
        sym: &SymbolInfo,
        entry: f64,
        stop: f64,
        _dd: f64,
    ) -> f64 {
        let risk_pts = (entry - stop).abs();
        if risk_pts == 0.0 || sym.tick_value == 0.0 {
            return sym.min_lot;
        }
        let raw = self.amount / (risk_pts / sym.point * sym.tick_value);
        let steps = (raw / sym.lot_step).round();
        let rounded = steps * sym.lot_step;
        rounded.max(sym.min_lot)
    }
}

pub struct FixedPercent {
    pub pct: f64,
    pub initial_balance: f64,
}
impl VolumeManager for FixedPercent {
    fn position_size(
        &self,
        _a: &AccountInfo,
        sym: &SymbolInfo,
        entry: f64,
        stop: f64,
        _dd: f64,
    ) -> f64 {
        let risk_pts = (entry - stop).abs();
        if risk_pts == 0.0 || sym.tick_value == 0.0 {
            return sym.min_lot;
        }
        let raw = self.initial_balance * self.pct / (risk_pts / sym.point * sym.tick_value);
        let steps = (raw / sym.lot_step).round();
        let rounded = steps * sym.lot_step;
        rounded.max(sym.min_lot)
    }
}

pub struct TieredPercent {
    pub tiers: Vec<(f64, f64, f64)>,
    pub daily_dd_limit: f64,
}
impl VolumeManager for TieredPercent {
    fn position_size(
        &self,
        a: &AccountInfo,
        sym: &SymbolInfo,
        entry: f64,
        stop: f64,
        dd: f64,
    ) -> f64 {
        let dd_pct = dd * 100.0;
        if dd_pct >= self.daily_dd_limit {
            return 0.0;
        }

        let pct = match self
            .tiers
            .iter()
            .find(|(lo, hi, _)| dd_pct >= *lo && dd_pct < *hi)
        {
            Some((_, _, p)) => *p,
            None => return 0.0,
        };

        let risk_pts = (entry - stop).abs();
        if risk_pts == 0.0 || sym.tick_value == 0.0 {
            return sym.min_lot;
        }
        let raw = a.balance * pct / (risk_pts / sym.point * sym.tick_value);
        let steps = (raw / sym.lot_step).round();
        let rounded = steps * sym.lot_step;
        rounded.max(sym.min_lot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sym() -> SymbolInfo {
        // Linear-futures style: tick_value == point (the H-4 invariant).
        SymbolInfo {
            symbol: "BTCUSDT".into(),
            ask: 0.0,
            bid: 0.0,
            point: 0.1,
            tick_value: 0.1,
            lot_step: 0.001,
            min_lot: 0.001,
            max_lot: 1_000.0,
            spread: 0.0,
            digits: 1,
            time: 0.0,
        }
    }

    #[test]
    fn fixed_percent_risk_matches_intended_amount_when_tick_value_eq_point() {
        let bal = 100_000.0;
        let pct = 0.01; // risk 1% = 1000
        let vm = FixedPercent {
            pct,
            initial_balance: bal,
        };
        let acct = AccountInfo {
            balance: bal,
            equity: bal,
            margin_free: bal,
            ..Default::default()
        };
        let s = sym();
        let entry = 50_000.0;
        let stop = 49_000.0; // 1000 price distance
        let vol = vm.position_size(&acct, &s, entry, stop, 0.0);
        // For linear futures, currency risk = volume * |entry-stop|.
        let realised_risk = vol * (entry - stop).abs();
        assert!(
            (realised_risk - bal * pct).abs() < 1.0,
            "realised risk {realised_risk} should ≈ {}",
            bal * pct
        );
    }
}
