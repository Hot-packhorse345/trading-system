use ts_core::{Bar, Direction, IndicatorSet, Params, Position, Signal};

// ── Trait ─────────────────────────────────────────────────────────────────────

pub trait ExitManager: Send + Sync {
    fn should_exit(
        &self,
        position: &Position,
        bar_idx: usize,
        bars: &[Bar],
        cols: &IndicatorSet,
        params: &Params,
        signal: &Signal,
        prev_signal: Option<Direction>,
    ) -> bool;
}

/// True if any exit manager in `exit_mgrs` decides `position` should be closed now.
pub fn any_should_exit(
    exit_mgrs: &[Box<dyn ExitManager>],
    position: &Position,
    bar_idx: usize,
    bars: &[Bar],
    cols: &IndicatorSet,
    params: &Params,
    signal: &Signal,
    prev_signal: Option<Direction>,
) -> bool {
    exit_mgrs
        .iter()
        .any(|em| em.should_exit(position, bar_idx, bars, cols, params, signal, prev_signal))
}

// ── StrategyExit ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StrategyExitCondition {
    #[default]
    OppositeSignal,
    BiasFlip,
}

pub struct StrategyExit {
    pub condition: StrategyExitCondition,
}

impl Default for StrategyExit {
    fn default() -> Self {
        Self {
            condition: StrategyExitCondition::OppositeSignal,
        }
    }
}

impl ExitManager for StrategyExit {
    fn should_exit(
        &self,
        position: &Position,
        _bar_idx: usize,
        _bars: &[Bar],
        _cols: &IndicatorSet,
        _params: &Params,
        signal: &Signal,
        prev_signal: Option<Direction>,
    ) -> bool {
        match self.condition {
            // ── OppositeSignal ─────────────────────────────────────────────
            StrategyExitCondition::OppositeSignal => {
                let sig_dir = signal.direction;
                if sig_dir == Direction::Hold {
                    return false;
                }
                matches!(
                    (position.direction, sig_dir),
                    (Direction::Buy, Direction::Sell) | (Direction::Sell, Direction::Buy)
                )
            }

            // ── BiasFlip ───────────────────────────────────────────────────
            StrategyExitCondition::BiasFlip => {
                let prev = match prev_signal {
                    Some(d) if d != Direction::Hold => d,
                    _ => return false,
                };

                signal.direction != prev && prev == position.direction
            }
        }
    }
}

// ── TimeExit ──────────────────────────────────────────────────────────────────

pub struct TimeExit {
    pub delta_time: Option<String>,
    pub max_bars: Option<usize>,
    pub exit_hour_utc: Option<u32>,
}

impl ExitManager for TimeExit {
    fn should_exit(
        &self,
        position: &Position,
        bar_idx: usize,
        bars: &[Bar],
        _cols: &IndicatorSet,
        _params: &Params,
        _signal: &Signal,
        _prev_signal: Option<Direction>,
    ) -> bool {
        // ── delta_time check ───────────────────────────────────────────────
        if let Some(dt) = &self.delta_time {
            let entry_ts = position.entry_time;
            let cur_ts = bars[bar_idx].time;
            let elapsed = (cur_ts - entry_ts).max(0) as u64;
            if let Some(limit_secs) = parse_delta_time(dt) {
                if elapsed >= limit_secs {
                    return true;
                }
            }
        }

        // ── max_bars check ─────────────────────────────────────────────────
        if let Some(limit) = self.max_bars {
            let entry_ts = position.entry_time;

            let entry_idx = bars[..=bar_idx]
                .iter()
                .rposition(|b| b.time <= entry_ts)
                .unwrap_or(0);

            let bars_elapsed = bar_idx.saturating_sub(entry_idx);
            if bars_elapsed >= limit {
                return true;
            }
        }

        // ── exit_hour_utc check ────────────────────────────────────────────
        if let Some(target_hour) = self.exit_hour_utc {
            let bar_time = bars[bar_idx].time;

            let secs_in_day = bar_time.rem_euclid(86_400) as u32;
            let bar_hour = secs_in_day / 3_600;
            if bar_hour >= target_hour {
                return true;
            }
        }

        false
    }
}

fn parse_delta_time(s: &str) -> Option<u64> {
    let parts: Vec<&str> = s.splitn(2, '_').collect();
    if parts.len() != 2 {
        return None;
    }
    let qty: u64 = parts[0].parse().ok()?;
    let secs = match parts[1].to_lowercase().trim_end_matches('s') {
        "minute" => qty * 60,
        "hour" => qty * 3600,
        "day" => qty * 86400,
        "week" => qty * 604800,
        _ => return None,
    };
    Some(secs)
}
