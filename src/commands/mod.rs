pub mod backtest;
pub mod edge;
pub mod live;
pub mod tools;
pub mod walkforward;

use ::backtest as backtest_crate;
use anyhow::{Context, Result};
use chrono::{DateTime, NaiveDate, Utc};
use indicators::IndicatorConfig;
use std::collections::HashMap;
use std::path::PathBuf;

pub fn parse_date(s: &str) -> Result<i64> {
    use chrono::{FixedOffset, NaiveDateTime};
    if s == "now" {
        return Ok(Utc::now().timestamp());
    }
    // RFC3339 / ISO-8601 with timezone offset (e.g. "2024-01-01T15:00:00+00:00")
    if let Ok(dt) = DateTime::<FixedOffset>::parse_from_rfc3339(s) {
        return Ok(dt.with_timezone(&Utc).timestamp());
    }
    // Datetime formats (treated as UTC), with and without seconds, T or space separator
    let formats = [
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%dT%H:%M:%SZ",
        "%Y-%m-%dT%H:%M",
        "%Y-%m-%d %H:%M",
    ];
    for fmt in &formats {
        if let Ok(ndt) = NaiveDateTime::parse_from_str(s, fmt) {
            return Ok(DateTime::<Utc>::from_naive_utc_and_offset(ndt, Utc).timestamp());
        }
    }
    // Date-only: floor to midnight UTC
    let nd = NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|_| {
        anyhow::anyhow!(
            "invalid date '{}' — expected YYYY-MM-DD, YYYY-MM-DD HH:MM, or YYYY-MM-DDTHH:MM:SS",
            s
        )
    })?;
    Ok(
        DateTime::<Utc>::from_naive_utc_and_offset(nd.and_hms_opt(0, 0, 0).unwrap(), Utc)
            .timestamp(),
    )
}

pub fn resolve_indicators(
    config: Option<PathBuf>,
    indicator: Option<String>,
    indicator_cfg: Option<String>,
) -> Result<Vec<IndicatorConfig>> {
    if let Some(path) = config {
        let text = std::fs::read_to_string(&path)?;
        let bt: backtest_crate::BacktestConfig = serde_json::from_str(&text)?;
        let cfgs: Vec<IndicatorConfig> = bt
            .indicators
            .values()
            .filter_map(|def| {
                let obj = def.as_object()?;
                let kind = obj.get("type")?.as_str()?.to_string();
                let params: HashMap<String, serde_json::Value> = obj
                    .iter()
                    .filter(|(k, _)| *k != "type" && *k != "timeframe")
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                Some(IndicatorConfig { kind, params })
            })
            .collect();
        if cfgs.is_empty() {
            anyhow::bail!("no indicators found in config");
        }
        return Ok(cfgs);
    }

    if let Some(kind) = indicator {
        let params: HashMap<String, serde_json::Value> = indicator_cfg
            .as_deref()
            .filter(|s| !s.is_empty())
            .map(serde_json::from_str)
            .transpose()
            .context("invalid --indicator-config JSON")?
            .unwrap_or_default();
        return Ok(vec![IndicatorConfig { kind, params }]);
    }

    anyhow::bail!("specify either --config <backtest.json> or --indicator <type>");
}
