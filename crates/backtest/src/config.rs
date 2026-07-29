use crate::swap::SwapConfig;
use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Datelike, Duration, Timelike, Utc};
use risk::config::{ExitManagerConfig, StopManagerConfig};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use ts_core::{parse_iso_date, parse_iso_date_end, parse_timeframe, Timeframe};

// ── BacktestConfig ─────────────────────────────────────────────

fn default_binance() -> String {
    "binance".to_string()
}
fn default_true() -> bool {
    true
}
fn default_data() -> PathBuf {
    PathBuf::from("data")
}
fn default_results() -> PathBuf {
    PathBuf::from("data/results")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestConfig {
    pub strategy: String,

    pub symbol: Value,

    pub timeframe: Value,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_timeframe: Option<Value>,

    #[serde(default = "default_true")]
    pub pyramiding: bool,

    #[serde(default = "default_binance")]
    pub data_provider: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub backtest_start: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backtest_end: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backtest_range: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trading_session: Option<String>,

    pub initial_balance: f64,
    pub risk_percentage: f64,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commission_percent: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commission_per_lot: Option<f64>,

    #[serde(default)]
    pub swap: SwapConfig,

    #[serde(default)]
    pub split_trades: Value,

    #[serde(default)]
    pub strategy_parameters: HashMap<String, Value>,

    #[serde(default)]
    pub indicators: HashMap<String, Value>,

    pub stop_manager: Value,

    #[serde(default)]
    pub exit_rules: Vec<Value>,

    #[serde(default = "default_data")]
    pub data_dir: PathBuf,

    #[serde(default = "default_results")]
    pub output_dir: PathBuf,
}

impl BacktestConfig {
    pub fn date_range(&self) -> Result<(i64, i64)> {
        let (start, end) = if let (Some(s), Some(e)) = (&self.backtest_start, &self.backtest_end) {
            (parse_iso_date(s)?, parse_iso_date_end(e)?)
        } else if let Some(range) = &self.backtest_range {
            let end = Utc::now();
            // Floor start to midnight so hours/minutes don't eat into the window
            let start = (end - parse_duration(range)?)
                .date_naive()
                .and_hms_opt(0, 0, 0)
                .unwrap()
                .and_utc();
            (start.timestamp(), end.timestamp())
        } else {
            return Err(anyhow!(
                "backtest config must set (backtest_start + backtest_end) or backtest_range"
            ));
        };

        if start > end {
            return Err(anyhow!(
                "Invalid date range: start date ({}) cannot be after end date ({})",
                self.backtest_start.as_deref().unwrap_or(""),
                self.backtest_end.as_deref().unwrap_or("")
            ));
        }
        Ok((start, end))
    }

    pub fn commission_pct(&self) -> f64 {
        self.commission_percent.unwrap_or(0.0)
    }

    pub fn commission_per_lot_val(&self) -> f64 {
        self.commission_per_lot.unwrap_or(0.0)
    }
}

// ── Type A / Type B classification ───────────────────────────────────────────

/// A combo is **TypeA** when it introduces a new signal configuration
/// (unique combination of symbol + timeframe + stop_timeframe + indicators +
/// strategy_parameters). It is **TypeB** when it shares that signal
/// configuration with a previously-seen combo and only differs in execution
/// parameters (stop_manager, risk_percentage, commission, etc.).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComboKind {
    TypeA,
    TypeB,
}

// ── Resolved single-combination config ───────────────────────────────────────

/// Pre-grouped signal-level data shared by every stop-manager variant in a group.
/// Produced by `generate_group_specs`; replaces `N_stop_managers` clones of the
/// shared fields with a single allocation + a small `Vec<StopManagerDef>`.
#[derive(Debug)]
pub struct GroupSpec {
    pub strategy_name: String,
    pub symbol: String,
    pub timeframe: Timeframe,
    pub stop_timeframe: Timeframe,
    pub pyramiding: bool,
    pub start: i64,
    pub end: i64,
    pub trading_hours: Option<TradingHoursConfig>,
    pub initial_balance: f64,
    pub risk_percentage: f64,
    pub commission_pct: f64,
    pub commission_per_lot: f64,
    pub swap: SwapConfig,
    pub strategy_params: HashMap<String, Value>,
    pub indicators: Vec<IndicatorDef>,
    pub signal_hash: String,
    pub combo_kind: ComboKind,
    /// All stop-manager variants that share this signal configuration.
    pub stop_managers: Vec<StopManagerConfig>,
    pub exit_managers: Vec<ExitManagerConfig>,
}

#[derive(Debug, Clone)]
pub struct ResolvedCombo {
    pub strategy_name: String,
    pub symbol: String,
    pub timeframe: Timeframe,
    pub stop_timeframe: Timeframe,
    pub pyramiding: bool,
    pub start: i64,
    pub end: i64,
    pub trading_hours: Option<TradingHoursConfig>,
    pub initial_balance: f64,
    pub risk_percentage: f64,
    pub commission_pct: f64,
    pub commission_per_lot: f64,
    pub swap: SwapConfig,

    pub strategy_params: HashMap<String, Value>,

    pub indicators: Vec<IndicatorDef>,

    pub stop_manager: StopManagerConfig,

    pub signal_hash: String,

    pub combo_hash: String,

    pub combo_kind: ComboKind,
}

#[derive(Debug, Clone)]
pub struct IndicatorDef {
    pub name: String,
    pub ind_type: String,
    pub timeframe: Option<String>,
    pub params: HashMap<String, Value>,
}

// ── Helper parsers ────────────────────────────────────────────────────────────

pub fn parse_duration(s: &str) -> Result<Duration> {
    let parts: Vec<&str> = s.splitn(2, '_').collect();
    if parts.len() != 2 {
        return Err(anyhow!("invalid duration: {s}"));
    }
    let qty: i64 = parts[0]
        .parse()
        .map_err(|_| anyhow!("invalid duration quantity: {}", parts[0]))?;
    let unit = parts[1].to_lowercase();
    let dur = match unit.trim_end_matches('s') {
        "minute" => Duration::minutes(qty),
        "hour" => Duration::hours(qty),
        "day" => Duration::days(qty),
        "week" => Duration::weeks(qty),
        "month" => Duration::days(qty * 30),
        "year" => Duration::days(qty * 365),
        other => return Err(anyhow!("unknown duration unit: {other}")),
    };
    Ok(dur)
}

pub fn resolve_stop_tf(stop_tf: &Value, main_tf: Timeframe) -> Result<Timeframe> {
    match stop_tf {
        Value::String(s) => {
            if s == "timeframe" {
                Ok(main_tf)
            } else {
                Ok(parse_timeframe(s)?)
            }
        }
        Value::Array(arr) => {
            if let Some(v) = arr.first() {
                resolve_stop_tf(v, main_tf)
            } else {
                Ok(main_tf)
            }
        }
        _ => Ok(main_tf),
    }
}

// ── Resolved trading session config ───────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TradingHoursConfig {
    #[serde(default)]
    pub enabled: bool,
    /// Map of day (1=Mon, 7=Sun) to (start_minute_utc, end_minute_utc).
    /// Days not present in the map are blocked.
    /// A start and end of 0 (0000-0000) means 24/7 for that day.
    #[serde(default)]
    pub schedule: HashMap<u32, (u32, u32)>,
}

impl TradingHoursConfig {
    pub fn is_allowed(&self, bar_time: i64) -> bool {
        if !self.enabled {
            return true;
        }

        let Some(dt) = DateTime::from_timestamp(bar_time, 0) else {
            return true;
        };

        let day = dt.weekday().num_days_from_monday() + 1; // 1=Mon, 7=Sun

        // If the day is not in the schedule, it is blocked
        let Some((start_min, end_min)) = self.schedule.get(&day) else {
            return false;
        };

        // 0000-0000 means 24/7 for this specific day
        if *start_min == 0 && *end_min == 0 {
            return true;
        }

        let current_minute = dt.hour() * 60 + dt.minute();

        if start_min <= end_min {
            // Normal range (e.g., 0800-1600)
            current_minute >= *start_min && current_minute < *end_min
        } else {
            // Overnight range (e.g., 2200-0600)
            current_minute >= *start_min || current_minute < *end_min
        }
    }
}

pub fn parse_trading_session(s: &str) -> Result<TradingHoursConfig> {
    let mut schedule = HashMap::new();
    let s = s.trim();
    if s.is_empty() {
        return Ok(TradingHoursConfig {
            enabled: false,
            schedule,
        });
    }

    let rules: Vec<&str> = s.split(',').collect();
    for rule in rules {
        let rule = rule.trim();
        if rule.is_empty() {
            continue;
        }

        let parts: Vec<&str> = rule.split(':').collect();
        if parts.len() != 2 {
            anyhow::bail!(
                "invalid trading_session rule: '{}'. Expected 'days:HHMM-HHMM'",
                rule
            );
        }
        let days_str = parts[0].trim();
        let time_str = parts[1].trim();

        // 1. Parse Days
        let mut days = Vec::new();
        if days_str == "*" {
            days.extend(1..=7);
        } else if days_str.contains('-') {
            let range_parts: Vec<&str> = days_str.split('-').collect();
            if range_parts.len() != 2 {
                anyhow::bail!("invalid day range: '{}'", days_str);
            }
            let start: u32 = range_parts[0].trim().parse().context("invalid day start")?;
            let end: u32 = range_parts[1].trim().parse().context("invalid day end")?;
            if start > end || start < 1 || end > 7 {
                anyhow::bail!("day range out of bounds: '{}'", days_str);
            }
            for d in start..=end {
                days.push(d);
            }
        } else {
            for c in days_str.chars() {
                if let Some(d) = c.to_digit(10) {
                    if (1..=7).contains(&d) {
                        days.push(d);
                    } else {
                        anyhow::bail!("invalid day digit: '{}'", c);
                    }
                } else {
                    anyhow::bail!("invalid character in days: '{}'", c);
                }
            }
        }

        // 2. Parse Time
        let time_parts: Vec<&str> = time_str.split('-').collect();
        if time_parts.len() != 2 {
            anyhow::bail!("invalid time format: '{}'. Expected 'HHMM-HHMM'", time_str);
        }

        let parse_hhmm = |s: &str| -> Result<u32> {
            let s = s.trim();
            if s.len() != 4 {
                anyhow::bail!("invalid HHMM format: '{}'", s);
            }
            let h: u32 = s[0..2].parse().context("invalid hour")?;
            let m: u32 = s[2..4].parse().context("invalid minute")?;
            if h > 23 || m > 59 {
                anyhow::bail!("time out of range: '{}'", s);
            }
            Ok(h * 60 + m)
        };

        let start_min = parse_hhmm(time_parts[0])?;
        let end_min = parse_hhmm(time_parts[1])?;

        for d in days {
            schedule.insert(d, (start_min, end_min));
        }
    }

    Ok(TradingHoursConfig {
        enabled: true,
        schedule,
    })
}
