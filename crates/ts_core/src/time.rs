use anyhow::{anyhow, Result};
use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};

/// Date/datetime formats accepted (in addition to RFC 3339) when no timezone
/// is present. Tried in order; the matched value is interpreted as UTC.
const NAIVE_DATETIME_FORMATS: &[&str] =
    &["%Y-%m-%dT%H:%M:%S", "%Y-%m-%d %H:%M:%S", "%Y-%m-%dT%H:%M"];

/// Parse an ISO 8601 date or datetime string to a Unix timestamp (seconds).
/// Date-only strings (`YYYY-MM-DD`) default to midnight (00:00:00).
pub fn parse_iso_date(s: &str) -> Result<i64> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Ok(dt.timestamp());
    }
    for fmt in NAIVE_DATETIME_FORMATS {
        if let Ok(ndt) = NaiveDateTime::parse_from_str(s, fmt) {
            return Ok(DateTime::<Utc>::from_naive_utc_and_offset(ndt, Utc).timestamp());
        }
    }
    if let Ok(nd) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        return Ok(nd.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp());
    }
    Err(anyhow!("cannot parse date: {s}"))
}

/// Like [`parse_iso_date`] but date-only strings default to end-of-day
/// (23:59:59) so that specifying "today" as a range end includes all of
/// today's bars.
pub fn parse_iso_date_end(s: &str) -> Result<i64> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Ok(dt.timestamp());
    }
    for fmt in NAIVE_DATETIME_FORMATS {
        if let Ok(ndt) = NaiveDateTime::parse_from_str(s, fmt) {
            return Ok(DateTime::<Utc>::from_naive_utc_and_offset(ndt, Utc).timestamp());
        }
    }
    if let Ok(nd) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        return Ok(nd.and_hms_opt(23, 59, 59).unwrap().and_utc().timestamp());
    }
    Err(anyhow!("cannot parse date: {s}"))
}
