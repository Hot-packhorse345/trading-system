use ts_core::{Direction, Position};

pub trait StopManager: Send + Sync {
    fn init(&mut self, entry: f64, stop: f64, direction: Direction);
    fn update(&mut self, bar_close: f64, bar_high: f64, bar_low: f64);
    fn stop(&self) -> f64;
    fn stopped_out(&self, bar_high: f64, bar_low: f64) -> bool;
}

/// Advance `sm` to the latest bar and, if the resulting stop has moved
/// favorably for `position.direction` (tighter in the risk-on direction),
/// write it to `position.current_stop_loss` and return it. Otherwise leaves
/// `position` untouched and returns `None`.
pub fn advance_stop(
    sm: &mut dyn StopManager,
    position: &mut Position,
    bar_close: f64,
    bar_high: f64,
    bar_low: f64,
) -> Option<f64> {
    let old_sl = position.current_stop_loss;
    sm.update(bar_close, bar_high, bar_low);
    let new_sl = sm.stop();

    let moved_favorably = match position.direction {
        Direction::Buy => new_sl > old_sl,
        Direction::Sell => new_sl < old_sl,
        Direction::Hold => false,
    };

    if moved_favorably {
        position.current_stop_loss = new_sl;
        Some(new_sl)
    } else {
        None
    }
}

// ── Fixed ─────────────────────────────────────────────────────────────────────
#[derive(Debug, Clone, Default)]
pub struct FixedStop {
    stop: f64,
    direction: Option<Direction>,
}
impl StopManager for FixedStop {
    fn init(&mut self, _e: f64, stop: f64, dir: Direction) {
        self.stop = stop;
        self.direction = Some(dir);
    }
    fn update(&mut self, _c: f64, _h: f64, _l: f64) {}
    fn stop(&self) -> f64 {
        self.stop
    }
    fn stopped_out(&self, h: f64, l: f64) -> bool {
        match self.direction {
            Some(Direction::Buy) => l <= self.stop,
            Some(Direction::Sell) => h >= self.stop,
            _ => false,
        }
    }
}

// ── TSL Variant 1 ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct TslVariant1 {
    pub stop_distance: f64,
    entry: f64,
    initial_sl: f64,
    stop: f64,
    current_r: f64,
    direction: Option<Direction>,
}
impl TslVariant1 {
    pub fn new(stop_distance: f64) -> Self {
        Self {
            stop_distance,
            ..Default::default()
        }
    }
}
impl StopManager for TslVariant1 {
    fn init(&mut self, entry: f64, stop: f64, dir: Direction) {
        self.entry = entry;
        self.initial_sl = stop;
        self.stop = stop;
        self.current_r = 0.0;
        self.direction = Some(dir);
    }
    fn update(&mut self, close: f64, _h: f64, _l: f64) {
        let stop_size = (self.entry - self.initial_sl).abs();
        if stop_size <= 0.0 {
            return;
        }
        let profit_size = match self.direction {
            Some(Direction::Buy) => close - self.entry,
            Some(Direction::Sell) => self.entry - close,
            _ => return,
        };
        let profit_r = profit_size / stop_size;
        let next_r = (profit_r / self.stop_distance).floor() + 1.0;
        let step_r = next_r - 1.0;
        if next_r > self.current_r {
            self.current_r = next_r;
            let offset = step_r * self.stop_distance * stop_size;
            let new_stop = match self.direction {
                Some(Direction::Buy) => self.initial_sl + offset,
                Some(Direction::Sell) => self.initial_sl - offset,
                _ => return,
            };
            match self.direction {
                Some(Direction::Buy) if new_stop > self.stop => {
                    self.stop = new_stop;
                }
                Some(Direction::Sell) if new_stop < self.stop => {
                    self.stop = new_stop;
                }
                _ => {}
            }
        }
    }
    fn stop(&self) -> f64 {
        self.stop
    }
    fn stopped_out(&self, h: f64, l: f64) -> bool {
        match self.direction {
            Some(Direction::Buy) => l <= self.stop,
            Some(Direction::Sell) => h >= self.stop,
            _ => false,
        }
    }
}

// ── TSL Variant 2 ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct TslVariant2 {
    pub stop_distance: f64,
    entry: f64,
    initial_sl: f64,
    stop: f64,
    current_r: f64,
    direction: Option<Direction>,
}
impl TslVariant2 {
    pub fn new(stop_distance: f64) -> Self {
        Self {
            stop_distance,
            ..Default::default()
        }
    }
}
impl StopManager for TslVariant2 {
    fn init(&mut self, entry: f64, stop: f64, dir: Direction) {
        self.entry = entry;
        self.initial_sl = stop;
        self.stop = stop;
        self.current_r = 0.0;
        self.direction = Some(dir);
    }
    fn update(&mut self, close: f64, _h: f64, _l: f64) {
        let stop_size = (self.entry - self.initial_sl).abs();
        if stop_size <= 0.0 {
            return;
        }
        let profit_size = match self.direction {
            Some(Direction::Buy) => close - self.entry,
            Some(Direction::Sell) => self.entry - close,
            _ => return,
        };
        let profit_r = profit_size / stop_size;
        if profit_r < 0.5 {
            return;
        }
        let k = ((profit_r - 0.5) / self.stop_distance).floor();
        let target_r = 0.5 + k * self.stop_distance;
        if target_r > self.current_r {
            self.current_r = target_r;
            let new_stop = match self.direction {
                Some(Direction::Buy) => self.entry + target_r * stop_size,
                Some(Direction::Sell) => self.entry - target_r * stop_size,
                _ => return,
            };
            match self.direction {
                Some(Direction::Buy) if new_stop > self.stop => {
                    self.stop = new_stop;
                }
                Some(Direction::Sell) if new_stop < self.stop => {
                    self.stop = new_stop;
                }
                _ => {}
            }
        }
    }
    fn stop(&self) -> f64 {
        self.stop
    }
    fn stopped_out(&self, h: f64, l: f64) -> bool {
        match self.direction {
            Some(Direction::Buy) => l <= self.stop,
            Some(Direction::Sell) => h >= self.stop,
            _ => false,
        }
    }
}

// ── TSL Variant 3 ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct TslVariant3 {
    pub stop_distance: f64,
    pub start_rr: f64,
    entry: f64,
    initial_sl: f64,
    stop: f64,
    current_r: f64,
    direction: Option<Direction>,
}
impl TslVariant3 {
    pub fn new(stop_distance: f64, start_rr: f64) -> Self {
        Self {
            stop_distance,
            start_rr,
            ..Default::default()
        }
    }
}
impl StopManager for TslVariant3 {
    fn init(&mut self, entry: f64, stop: f64, dir: Direction) {
        self.entry = entry;
        self.initial_sl = stop;
        self.stop = stop;
        self.current_r = 0.0;
        self.direction = Some(dir);
    }
    fn update(&mut self, close: f64, _h: f64, _l: f64) {
        let stop_size = (self.entry - self.initial_sl).abs();
        if stop_size <= 0.0 {
            return;
        }
        let profit_size = match self.direction {
            Some(Direction::Buy) => close - self.entry,
            Some(Direction::Sell) => self.entry - close,
            _ => return,
        };
        let current_rr = profit_size / stop_size;
        if current_rr < self.start_rr {
            return;
        }
        let steps_over = ((current_rr - self.start_rr) / self.stop_distance).floor();
        let target_rr = self.start_rr + steps_over * self.stop_distance;
        let stop_rr = target_rr - self.stop_distance;
        if target_rr > self.current_r {
            self.current_r = target_rr;
            let new_stop = match self.direction {
                Some(Direction::Buy) => self.entry + stop_rr * stop_size,
                Some(Direction::Sell) => self.entry - stop_rr * stop_size,
                _ => return,
            };
            match self.direction {
                Some(Direction::Buy) if new_stop > self.stop => {
                    self.stop = new_stop;
                }
                Some(Direction::Sell) if new_stop < self.stop => {
                    self.stop = new_stop;
                }
                _ => {}
            }
        }
    }
    fn stop(&self) -> f64 {
        self.stop
    }
    fn stopped_out(&self, h: f64, l: f64) -> bool {
        match self.direction {
            Some(Direction::Buy) => l <= self.stop,
            Some(Direction::Sell) => h >= self.stop,
            _ => false,
        }
    }
}

// ── TSL Variant 4 ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct TslVariant4 {
    pub stop_distance: f64,
    pub start_rr: f64,
    entry: f64,
    initial_sl: f64,
    stop: f64,
    current_r: f64,
    direction: Option<Direction>,
}
impl TslVariant4 {
    pub fn new(stop_distance: f64, start_rr: f64) -> Self {
        Self {
            stop_distance,
            start_rr,
            ..Default::default()
        }
    }
}
impl StopManager for TslVariant4 {
    fn init(&mut self, entry: f64, stop: f64, dir: Direction) {
        self.entry = entry;
        self.initial_sl = stop;
        self.stop = stop;
        self.current_r = 0.0;
        self.direction = Some(dir);
    }
    fn update(&mut self, close: f64, _h: f64, _l: f64) {
        let stop_size = (self.entry - self.initial_sl).abs();
        if stop_size <= 0.0 {
            return;
        }
        let profit_size = match self.direction {
            Some(Direction::Buy) => close - self.entry,
            Some(Direction::Sell) => self.entry - close,
            _ => return,
        };
        let current_rr = profit_size / stop_size;
        if current_rr < self.start_rr {
            return;
        }
        let steps_over = ((current_rr - self.start_rr) / self.stop_distance).floor();
        let target_rr = self.start_rr + steps_over * self.stop_distance;
        if target_rr > self.current_r {
            self.current_r = target_rr;
            let new_stop = match self.direction {
                Some(Direction::Buy) => self.entry + target_rr * stop_size,
                Some(Direction::Sell) => self.entry - target_rr * stop_size,
                _ => return,
            };
            match self.direction {
                Some(Direction::Buy) if new_stop > self.stop => {
                    self.stop = new_stop;
                }
                Some(Direction::Sell) if new_stop < self.stop => {
                    self.stop = new_stop;
                }
                _ => {}
            }
        }
    }
    fn stop(&self) -> f64 {
        self.stop
    }
    fn stopped_out(&self, h: f64, l: f64) -> bool {
        match self.direction {
            Some(Direction::Buy) => l <= self.stop,
            Some(Direction::Sell) => h >= self.stop,
            _ => false,
        }
    }
}

// ── Legacy implementations (kept for backward compat) ─────────────────────────
#[derive(Debug, Clone)]
pub struct BreakevenTrail {
    pub breakeven_r: f64,
    pub trail_distance: f64,
    entry: f64,
    stop: f64,
    initial_sl: f64,
    direction: Option<Direction>,
    at_be: bool,
}
impl BreakevenTrail {
    pub fn new(breakeven_r: f64, trail_distance: f64) -> Self {
        Self {
            breakeven_r,
            trail_distance,
            entry: 0.0,
            stop: 0.0,
            initial_sl: 0.0,
            direction: None,
            at_be: false,
        }
    }
}
impl StopManager for BreakevenTrail {
    fn init(&mut self, entry: f64, stop: f64, dir: Direction) {
        self.entry = entry;
        self.stop = stop;
        self.initial_sl = stop;
        self.direction = Some(dir);
        self.at_be = false;
    }
    fn update(&mut self, close: f64, _h: f64, _l: f64) {
        let risk = (self.entry - self.initial_sl).abs();
        if risk == 0.0 {
            return;
        }
        match self.direction {
            Some(Direction::Buy) => {
                let r = (close - self.entry) / risk;
                if !self.at_be && r >= self.breakeven_r {
                    self.stop = self.entry;
                    self.at_be = true;
                } else if self.at_be {
                    let ns = close - self.trail_distance;
                    if ns > self.stop {
                        self.stop = ns;
                    }
                }
            }
            Some(Direction::Sell) => {
                let r = (self.entry - close) / risk;
                if !self.at_be && r >= self.breakeven_r {
                    self.stop = self.entry;
                    self.at_be = true;
                } else if self.at_be {
                    let ns = close + self.trail_distance;
                    if ns < self.stop {
                        self.stop = ns;
                    }
                }
            }
            _ => {}
        }
    }
    fn stop(&self) -> f64 {
        self.stop
    }
    fn stopped_out(&self, h: f64, l: f64) -> bool {
        match self.direction {
            Some(Direction::Buy) => l <= self.stop,
            Some(Direction::Sell) => h >= self.stop,
            _ => false,
        }
    }
}

/// Incremental Wilder ATR fed one bar at a time. Used by the trailing-stop
/// managers that need an ATR but only receive `(close, high, low)` from
/// `StopManager::update`. Previously `AtrTrail`/`SupertrendTrail` reinterpreted
/// the `close` argument as a pre-computed candidate stop level, which the
/// backtest simulator and live trade manager never supplied — so both managers
/// silently ratcheted the stop to the raw close. They now derive ATR themselves.
#[derive(Debug, Clone, Default)]
struct WilderAtr {
    period: usize,
    prev_close: Option<f64>,
    atr: f64,
    seeded: bool,
    tr_sum: f64,
    tr_count: usize,
}
impl WilderAtr {
    fn new(period: usize) -> Self {
        Self {
            period: period.max(1),
            ..Default::default()
        }
    }
    fn reset(&mut self) {
        *self = Self::new(self.period);
    }
    /// Feed one bar; returns the current ATR, or `None` until seeded.
    fn update(&mut self, close: f64, high: f64, low: f64) -> Option<f64> {
        let tr = match self.prev_close {
            None => high - low,
            Some(pc) => (high - low).max((high - pc).abs()).max((low - pc).abs()),
        };
        self.prev_close = Some(close);
        if !self.seeded {
            self.tr_sum += tr;
            self.tr_count += 1;
            if self.tr_count >= self.period {
                self.atr = self.tr_sum / self.period as f64;
                self.seeded = true;
            }
        } else {
            let a = 1.0 / self.period as f64;
            self.atr = a * tr + (1.0 - a) * self.atr;
        }
        if self.seeded {
            Some(self.atr)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone)]
pub struct AtrTrail {
    pub multiplier: f64,
    atr: WilderAtr,
    stop: f64,
    direction: Option<Direction>,
}
impl AtrTrail {
    pub fn new(multiplier: f64, period: usize) -> Self {
        Self {
            multiplier,
            atr: WilderAtr::new(period),
            stop: 0.0,
            direction: None,
        }
    }
}
impl StopManager for AtrTrail {
    fn init(&mut self, _e: f64, stop: f64, dir: Direction) {
        self.stop = stop;
        self.direction = Some(dir);
        self.atr.reset();
    }
    fn update(&mut self, close: f64, high: f64, low: f64) {
        let Some(atr) = self.atr.update(close, high, low) else {
            return;
        };
        let band = self.multiplier * atr;
        match self.direction {
            Some(Direction::Buy) => {
                let c = close - band;
                if c > self.stop {
                    self.stop = c;
                }
            }
            Some(Direction::Sell) => {
                let c = close + band;
                if c < self.stop {
                    self.stop = c;
                }
            }
            _ => {}
        }
    }
    fn stop(&self) -> f64 {
        self.stop
    }
    fn stopped_out(&self, h: f64, l: f64) -> bool {
        match self.direction {
            Some(Direction::Buy) => l <= self.stop,
            Some(Direction::Sell) => h >= self.stop,
            _ => false,
        }
    }
}

#[cfg(test)]
mod atr_trail_tests {
    use super::*;

    #[test]
    fn atr_trail_trails_below_price_for_long_and_only_tightens() {
        // multiplier 1.0, ATR period 3.
        let mut sm = AtrTrail::new(1.0, 3);
        sm.init(100.0, 95.0, Direction::Buy);

        // Feed rising bars (range ~2 each) so ATR seeds then the stop ratchets up.
        let bars = [
            (100.0, 101.0, 99.0),
            (102.0, 103.0, 101.0),
            (104.0, 105.0, 103.0),
            (106.0, 107.0, 105.0),
            (108.0, 109.0, 107.0),
        ];
        let mut prev = sm.stop();
        for (c, h, l) in bars {
            sm.update(c, h, l);
            // The stop must never loosen (move down) for a long.
            assert!(
                sm.stop() >= prev - 1e-9,
                "stop loosened: {} < {}",
                sm.stop(),
                prev
            );
            prev = sm.stop();
        }
        // After trending up, the stop should have moved above the initial 95 and
        // stay below the latest close (it's a trailing stop, not at price).
        assert!(sm.stop() > 95.0);
        assert!(sm.stop() < 108.0);
    }

    #[test]
    fn atr_trail_does_not_collapse_to_close() {
        // Regression for the old bug where update() treated `close` as the stop
        // level, jamming the stop to the bar close (instant stop-out).
        let mut sm = AtrTrail::new(2.0, 3);
        sm.init(100.0, 90.0, Direction::Buy);
        for _ in 0..5 {
            sm.update(100.0, 101.0, 99.0);
        }
        assert!(
            sm.stop() < 99.0,
            "stop {} should sit well below the close band",
            sm.stop()
        );
    }
}

#[derive(Debug, Clone)]
pub struct SupertrendTrail {
    pub multiplier: f64,
    atr: WilderAtr,
    stop: f64,
    direction: Option<Direction>,
    prev_upper: Option<f64>,
    prev_lower: Option<f64>,
    prev_st: Option<f64>,
    st_dir: i8, // 1 = up/long, -1 = down/short
}
impl SupertrendTrail {
    pub fn new(period: usize, multiplier: f64) -> Self {
        Self {
            multiplier,
            atr: WilderAtr::new(period),
            stop: 0.0,
            direction: None,
            prev_upper: None,
            prev_lower: None,
            prev_st: None,
            st_dir: 1,
        }
    }
}
impl StopManager for SupertrendTrail {
    fn init(&mut self, _e: f64, stop: f64, dir: Direction) {
        self.stop = stop;
        self.direction = Some(dir);
        self.atr.reset();
        self.prev_upper = None;
        self.prev_lower = None;
        self.prev_st = None;
        self.st_dir = match dir {
            Direction::Sell => -1,
            _ => 1,
        };
    }
    fn update(&mut self, close: f64, high: f64, low: f64) {
        let Some(atr) = self.atr.update(close, high, low) else {
            return;
        };
        let mid = (high + low) / 2.0;
        let band = self.multiplier * atr;
        let mut upper = mid + band;
        let mut lower = mid - band;
        if let Some(pu) = self.prev_upper {
            upper = upper.min(pu);
        }
        if let Some(pl) = self.prev_lower {
            lower = lower.max(pl);
        }

        self.st_dir = match self.prev_st {
            None => self.st_dir,
            Some(ps) => {
                if self.st_dir == -1 {
                    if close > ps {
                        1
                    } else {
                        -1
                    }
                } else if close < ps {
                    -1
                } else {
                    1
                }
            }
        };
        let st = if self.st_dir == 1 { lower } else { upper };

        self.prev_upper = Some(upper);
        self.prev_lower = Some(lower);
        self.prev_st = Some(st);

        // Only trail favourably (never loosen the stop).
        match self.direction {
            Some(Direction::Buy) if st > self.stop => {
                self.stop = st;
            }
            Some(Direction::Sell) if st < self.stop => {
                self.stop = st;
            }
            _ => {}
        }
    }
    fn stop(&self) -> f64 {
        self.stop
    }
    fn stopped_out(&self, h: f64, l: f64) -> bool {
        match self.direction {
            Some(Direction::Buy) => l <= self.stop,
            Some(Direction::Sell) => h >= self.stop,
            _ => false,
        }
    }
}
