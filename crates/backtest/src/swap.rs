//! Overnight swap / rollover fee model.
//!
//! Mirrors the existing commission model (`commission_percent` +
//! `commission_per_lot`, "use whichever ones you need, unused ones default to
//! zero and are simply summed") but for the carrying cost of holding a
//! position overnight.
//!
//! Two independent rate specifications are supported, exactly like
//! commission's percent-vs-per-lot split, and both are summed so a symbol can
//! use either one (set the other to `0.0`):
//!   - `*_per_lot`: a fixed $ amount per lot per (weighted) rollover — the
//!     natural choice when you already know the broker's $ swap rate.
//!   - `*_points`: broker-native swap points per lot per (weighted) rollover,
//!     converted to $ via `SymbolInfo::tick_value` using the same
//!     points-to-$ convention already used for position sizing in
//!     `risk::volume` (`points * tick_value` — see that module's tests for
//!     the `tick_value == point` linear-futures invariant).
//!
//! Sign convention: negative = cost to the account, positive = credit.
//!
//! Weekend handling is controlled by [`RolloverMode`] and is independent of
//! the rate specification.

use serde::{Deserialize, Serialize};
use ts_core::Direction;

/// How overnight rollover is charged across non-trading (weekend) nights.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RolloverMode {
    /// Charge the plain per-night rate for every rollover boundary crossed,
    /// including Saturday and Sunday, at 1x — no weekend multiplier.
    /// Simplest model; slightly overstates real-world cost since most
    /// brokers don't roll positions when the market is shut, but it never
    /// silently understates carrying cost. **Default.**
    EveryNight,
    /// Broker/MT5 convention: 1x on ordinary weeknight rollovers, 0x on
    /// Saturday and Sunday, and `wednesday_multiplier`x (typically 3.0) on
    /// the Wednesday->Thursday rollover to account for the skipped weekend.
    TripleWednesday,
    /// 1x on ordinary weeknight rollovers, 0x on Saturday/Sunday, and no
    /// Wednesday compensation. Understates real broker cost (most brokers do
    /// apply the Wednesday 3x) but matches literal market-open hours.
    WeekdaysOnly,
}

impl Default for RolloverMode {
    fn default() -> Self {
        RolloverMode::EveryNight
    }
}

/// Overnight swap configuration for one symbol/combo.
///
/// All rate fields default to `0.0` and `rollover_mode` defaults to
/// [`RolloverMode::EveryNight`], so `SwapConfig::default()` /
/// [`SwapConfig::none()`] is a true no-op — safe to use anywhere a config
/// doesn't set `"swap"` at all.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SwapConfig {
    /// $ per lot per (weighted) rollover for long positions. Negative = cost.
    pub long_per_lot: f64,
    /// $ per lot per (weighted) rollover for short positions. Negative = cost.
    pub short_per_lot: f64,
    /// Broker swap points per lot per (weighted) rollover for long positions.
    /// Converted to $ via `tick_value`. Negative = cost.
    pub long_points: f64,
    /// Broker swap points per lot per (weighted) rollover for short
    /// positions. Converted to $ via `tick_value`. Negative = cost.
    pub short_points: f64,
    pub rollover_mode: RolloverMode,
    /// Multiplier applied to the Wednesday->Thursday rollover under
    /// `RolloverMode::TripleWednesday`. Ignored by other modes. Default 3.0.
    pub wednesday_multiplier: f64,
    /// UTC hour (0-23) at which the daily rollover boundary falls.
    /// Standard forex rollover is ~21:00-22:00 UTC depending on DST;
    /// default 22.
    pub rollover_hour_utc: u32,
}

impl Default for SwapConfig {
    fn default() -> Self {
        Self {
            long_per_lot: 0.0,
            short_per_lot: 0.0,
            long_points: 0.0,
            short_points: 0.0,
            rollover_mode: RolloverMode::EveryNight,
            wednesday_multiplier: 3.0,
            rollover_hour_utc: 22,
        }
    }
}

impl SwapConfig {
    /// Explicit no-op config — all rates zero. Equivalent to `default()`;
    /// provided as a named constructor for call sites that want to be
    /// explicit about "no swap modeling" (e.g. existing tests/call sites
    /// being migrated to the new `compute_metrics`/`SimParams` signature).
    pub fn none() -> Self {
        Self::default()
    }

    /// $ swap charged/credited per lot for a *single* rollover, before the
    /// weekday multiplier is applied. Negative = cost, positive = credit.
    fn per_night_rate(&self, direction: Direction, tick_value: f64) -> f64 {
        match direction {
            Direction::Buy => self.long_per_lot + self.long_points * tick_value,
            Direction::Sell => self.short_per_lot + self.short_points * tick_value,
            Direction::Hold => 0.0,
        }
    }

    /// Total swap $ for one closed trade. `entry_time`/`exit_time` are unix
    /// seconds (UTC), as stored on `Position`/`TradeRecord`. Negative = net
    /// cost to the account, positive = net credit.
    pub fn trade_swap_cost(
        &self,
        direction: Direction,
        volume: f64,
        entry_time: i64,
        exit_time: i64,
        tick_value: f64,
    ) -> f64 {
        let rate = self.per_night_rate(direction, tick_value);
        if rate == 0.0 {
            return 0.0;
        }
        let units = rollover_units(entry_time, exit_time, self.rollover_hour_utc, self.rollover_mode, self.wednesday_multiplier);
        rate * volume * units
    }
}

/// Number of weekday-weighted rollover boundaries crossed strictly between
/// `entry_time` (exclusive) and `exit_time` (exclusive). A boundary occurs
/// once per UTC day at `rollover_hour_utc:00:00`.
///
/// Both ends are treated exclusively: a boundary exactly equal to
/// `entry_time` doesn't count (the position wasn't held *through* it), and a
/// boundary exactly equal to `exit_time` doesn't count either. This keeps
/// two back-to-back trades that happen to meet exactly on a rollover instant
/// from ever double-charging that instant.
fn rollover_units(
    entry_time: i64,
    exit_time: i64,
    rollover_hour_utc: u32,
    mode: RolloverMode,
    wednesday_multiplier: f64,
) -> f64 {
    if exit_time <= entry_time {
        return 0.0;
    }
    use chrono::{DateTime, Datelike, Utc, Weekday};

    const DAY: i64 = 86_400;
    let offset = rollover_hour_utc as i64 * 3600;

    // First boundary at/after entry_time.
    let entry_day_start = entry_time.div_euclid(DAY) * DAY;
    let mut boundary = entry_day_start + offset;
    if boundary <= entry_time {
        boundary += DAY;
    }

    let mut units = 0.0;
    while boundary < exit_time {
        let weekday = DateTime::<Utc>::from_timestamp(boundary, 0)
            .map(|dt| dt.weekday())
            .unwrap_or(Weekday::Mon);
        units += match mode {
            RolloverMode::EveryNight => 1.0,
            RolloverMode::TripleWednesday => match weekday {
                Weekday::Sat | Weekday::Sun => 0.0,
                Weekday::Wed => wednesday_multiplier,
                _ => 1.0,
            },
            RolloverMode::WeekdaysOnly => match weekday {
                Weekday::Sat | Weekday::Sun => 0.0,
                _ => 1.0,
            },
        };
        boundary += DAY;
    }
    units
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use chrono::Utc;

    fn ts(y: i32, m: u32, d: u32, h: u32) -> i64 {
        Utc.with_ymd_and_hms(y, m, d, h, 0, 0).unwrap().timestamp()
    }

    #[test]
    fn no_rollover_within_same_day_before_boundary() {
        let entry = ts(2026, 6, 1, 10); // Monday
        let exit = ts(2026, 6, 1, 20);
        let units = rollover_units(entry, exit, 22, RolloverMode::EveryNight, 3.0);
        assert_eq!(units, 0.0);
    }

    #[test]
    fn single_rollover_crossed() {
        let entry = ts(2026, 6, 1, 10); // Monday
        let exit = ts(2026, 6, 2, 10); // Tuesday
        let units = rollover_units(entry, exit, 22, RolloverMode::EveryNight, 3.0);
        assert_eq!(units, 1.0);
    }

    #[test]
    fn exact_boundary_both_ends_excluded() {
        let entry = ts(2026, 6, 1, 22);
        let exit = ts(2026, 6, 2, 22);
        let units = rollover_units(entry, exit, 22, RolloverMode::EveryNight, 3.0);
        assert_eq!(units, 0.0);
    }

    #[test]
    fn every_night_mode_counts_weekend_nights_plain() {
        // Friday 10:00 -> Monday 10:00: crosses Fri22, Sat22, Sun22 = 3
        let entry = ts(2026, 6, 5, 10);
        let exit = ts(2026, 6, 8, 10);
        let units = rollover_units(entry, exit, 22, RolloverMode::EveryNight, 3.0);
        assert_eq!(units, 3.0);
    }

    #[test]
    fn triple_wednesday_mode_skips_weekend_and_triples_wednesday() {
        // Monday 10:00 -> next Monday 10:00: Mon,Tue,Wed,Thu,Fri,Sat,Sun boundaries.
        // Sat/Sun -> 0, Wed -> 3x, others -> 1x => 1+1+3+1+1+0+0 = 7
        let entry = ts(2026, 6, 1, 10);
        let exit = ts(2026, 6, 8, 10);
        let units = rollover_units(entry, exit, 22, RolloverMode::TripleWednesday, 3.0);
        assert_eq!(units, 7.0);
    }

    #[test]
    fn weekdays_only_mode_skips_weekend_no_triple() {
        let entry = ts(2026, 6, 1, 10);
        let exit = ts(2026, 6, 8, 10);
        let units = rollover_units(entry, exit, 22, RolloverMode::WeekdaysOnly, 3.0);
        assert_eq!(units, 5.0);
    }

    #[test]
    fn short_direction_uses_short_rate() {
        let cfg = SwapConfig {
            long_per_lot: -6.5,
            short_per_lot: 1.2,
            ..Default::default()
        };
        let entry = ts(2026, 6, 1, 10);
        let exit = ts(2026, 6, 2, 10); // 1 night
        let cost = cfg.trade_swap_cost(Direction::Sell, 2.0, entry, exit, 0.0);
        assert!((cost - 1.2 * 2.0).abs() < 1e-9);
    }

    #[test]
    fn points_mode_uses_tick_value_conversion() {
        // -70 swap points, tick_value = $0.1/point/lot -> -7.0 $/lot/night,
        // matching the `points / point * tick_value` convention used by
        // risk::volume for position sizing.
        let cfg = SwapConfig {
            long_points: -70.0,
            ..Default::default()
        };
        let entry = ts(2026, 6, 1, 10);
        let exit = ts(2026, 6, 2, 10);
        let cost = cfg.trade_swap_cost(Direction::Buy, 1.0, entry, exit, 0.1);
        assert!((cost - (-7.0)).abs() < 1e-9);
    }

    #[test]
    fn per_lot_and_points_rates_are_additive() {
        let cfg = SwapConfig {
            long_per_lot: -2.0,
            long_points: -10.0, // * tick_value 0.5 = -5.0
            ..Default::default()
        };
        let entry = ts(2026, 6, 1, 10);
        let exit = ts(2026, 6, 2, 10);
        let cost = cfg.trade_swap_cost(Direction::Buy, 1.0, entry, exit, 0.5);
        assert!((cost - (-7.0)).abs() < 1e-9);
    }

    #[test]
    fn none_config_never_charges() {
        let cfg = SwapConfig::none();
        let entry = ts(2026, 6, 1, 10);
        let exit = ts(2026, 6, 8, 10);
        assert_eq!(
            cfg.trade_swap_cost(Direction::Buy, 5.0, entry, exit, 1.0),
            0.0
        );
    }

    #[test]
    fn same_instant_trade_no_swap() {
        let cfg = SwapConfig {
            long_per_lot: -6.5,
            ..Default::default()
        };
        let t = ts(2026, 6, 1, 10);
        assert_eq!(cfg.trade_swap_cost(Direction::Buy, 1.0, t, t, 0.0), 0.0);
    }

    #[test]
    fn default_deserializes_from_empty_json() {
        let cfg: SwapConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(cfg, SwapConfig::none());
    }

    #[test]
    fn deserializes_partial_json_with_defaults() {
        let cfg: SwapConfig = serde_json::from_str(
            r#"{"long_per_lot": -6.5, "short_per_lot": 1.8, "rollover_mode": "triple_wednesday"}"#,
        )
        .unwrap();
        assert_eq!(cfg.long_per_lot, -6.5);
        assert_eq!(cfg.short_per_lot, 1.8);
        assert_eq!(cfg.rollover_mode, RolloverMode::TripleWednesday);
        assert_eq!(cfg.wednesday_multiplier, 3.0); // untouched default
        assert_eq!(cfg.rollover_hour_utc, 22); // untouched default
    }
}
