use risk::{advance_stop, any_should_exit, ExitManager, StopManager};
use ts_core::{Bar, Direction, ExitReason, IndicatorSet, Params, Position, Signal, Tick};

pub struct TradeManager {
    pub position: Position,
    pub symbol: String,
    stop_manager: Box<dyn StopManager>,
    exit_managers: Vec<Box<dyn ExitManager>>,
    pub stopped_out: bool,
    pub tp_hit: bool,
}

impl TradeManager {
    pub fn new(
        position: Position,
        symbol: String,
        stop_manager: Box<dyn StopManager>,
        exit_managers: Vec<Box<dyn ExitManager>>,
    ) -> Self {
        Self {
            position,
            symbol,
            stop_manager,
            exit_managers,
            stopped_out: false,
            tp_hit: false,
        }
    }

    /// Returns true if the tick triggers the stop loss.
    /// Buy: stopped when bid <= stop; Sell: stopped when ask >= stop.
    pub fn check_tick_stop(&mut self, tick: &Tick) -> bool {
        if self.stopped_out || self.tp_hit {
            return false;
        }
        let triggered = self.position.sl_hit(tick.ask, tick.bid);
        if triggered {
            self.stopped_out = true;
        }
        triggered
    }

    /// Returns true if the tick triggers the take profit.
    /// Buy: tp hit when ask >= tp; Sell: tp hit when bid <= tp.
    pub fn check_tick_tp(&mut self, tick: &Tick) -> bool {
        if self.stopped_out || self.tp_hit {
            return false;
        }
        let triggered = self.position.tp_hit(tick.ask, tick.bid);
        if triggered {
            self.tp_hit = true;
        }
        triggered
    }

    /// Called on a stop-timeframe bar close. Returns Some(new_sl) if the stop
    /// moved favorably (tighter for the risk-on direction).
    pub fn update_stop(&mut self, bar: &Bar) -> Option<f64> {
        advance_stop(
            self.stop_manager.as_mut(),
            &mut self.position,
            bar.close,
            bar.high,
            bar.low,
        )
    }

    /// Returns true if any exit manager decides this position should be closed now.
    pub fn check_exit_rules(
        &self,
        bar_idx: usize,
        bars: &[Bar],
        cols: &IndicatorSet,
        params: &Params,
        signal: &Signal,
        prev_dir: Option<Direction>,
    ) -> bool {
        any_should_exit(
            &self.exit_managers,
            &self.position,
            bar_idx,
            bars,
            cols,
            params,
            signal,
            prev_dir,
        )
    }

    pub fn is_closed(&self) -> bool {
        self.stopped_out || self.tp_hit
    }

    pub fn close_reason(&self) -> ExitReason {
        if self.stopped_out {
            ExitReason::StopLoss
        } else if self.tp_hit {
            ExitReason::TakeProfit
        } else {
            ExitReason::ExitRule
        }
    }
}

// ── GroupTradeManager ────────────────────────────────────────────────────────

/// Manages a set of split-volume chunks that share an authority stop manager.
/// All chunks enter at the same signal; their SL moves in lockstep.
pub struct GroupTradeManager {
    pub group_id: u64,
    pub chunks: Vec<TradeManager>,
    stop_manager: Box<dyn StopManager>,
}

impl GroupTradeManager {
    pub fn new(
        group_id: u64,
        chunks: Vec<TradeManager>,
        stop_manager: Box<dyn StopManager>,
    ) -> Self {
        Self {
            group_id,
            chunks,
            stop_manager,
        }
    }

    pub fn is_fully_closed(&self) -> bool {
        self.chunks.is_empty()
    }

    pub fn direction(&self) -> Option<Direction> {
        self.chunks.first().map(|c| c.position.direction)
    }

    /// Check all chunks against tick. Returns indices of chunks that were stopped out.
    pub fn check_tick_stop(&mut self, tick: &Tick) -> Vec<usize> {
        let mut closed = Vec::new();
        for (i, chunk) in self.chunks.iter_mut().enumerate() {
            if chunk.check_tick_stop(tick) {
                closed.push(i);
            }
        }
        closed
    }

    /// Check all chunks for TP hit. Returns indices of chunks that hit TP.
    pub fn check_tick_tp(&mut self, tick: &Tick) -> Vec<usize> {
        let mut closed = Vec::new();
        for (i, chunk) in self.chunks.iter_mut().enumerate() {
            if chunk.check_tick_tp(tick) {
                closed.push(i);
            }
        }
        closed
    }

    /// Advance the group authority stop. Returns Some(new_sl) if stop moved favorably.
    /// Updates all active chunk positions to the new SL.
    pub fn update_stop(&mut self, bar: &Bar) -> Option<f64> {
        if self.chunks.is_empty() {
            return None;
        }

        let new_sl = advance_stop(
            self.stop_manager.as_mut(),
            &mut self.chunks[0].position,
            bar.close,
            bar.high,
            bar.low,
        )?;
        for chunk in self.chunks.iter_mut().skip(1) {
            chunk.position.current_stop_loss = new_sl;
        }
        Some(new_sl)
    }

    /// Remove chunks at the given indices (in descending order to preserve other indices).
    pub fn remove_chunks(&mut self, mut indices: Vec<usize>) {
        indices.sort_unstable_by(|a, b| b.cmp(a));
        for i in indices {
            self.chunks.swap_remove(i);
        }
    }
}
