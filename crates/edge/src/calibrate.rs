use anyhow::{anyhow, Result};

pub fn calibrate_wfa_windows(
    probe_trades: usize,
    total_bars: usize,
    wfa_bars: usize,
    target_is_trades: f64,
    min_rounds: usize,
) -> Result<(usize /*is*/, usize /*oos*/, usize /*step*/)> {
    if total_bars == 0 {
        return Err(anyhow!("Cannot calibrate WFA: total_bars is 0"));
    }
    let density = probe_trades as f64 / total_bars as f64;
    if density <= 0.0 {
        return Err(anyhow!(
            "Cannot calibrate WFA: density is 0 (probe_trades = {})",
            probe_trades
        ));
    }

    let mut is = (target_is_trades / density).ceil() as usize;

    // clamp is in [2000, 50000]
    is = is.clamp(2000, 50000);

    // enforce >= min_rounds via is <= wfa_bars * 4 / (min_rounds + 4)
    let max_is = (wfa_bars * 4) / (min_rounds + 4);
    if is > max_is {
        is = max_is;
    }

    // oos = is/4, step = oos, clamp oos >= 500
    let mut oos = is / 4;
    if oos < 500 {
        oos = 500;
    }
    let step = oos;

    // Check if the data can support it: total walkforward length = is + min_rounds * step
    let required_bars = is + min_rounds * step;
    if required_bars > wfa_bars {
        return Err(anyhow!(
            "Insufficient data for WFA: calibrated requirements need {} bars but only have {} WFA bars (calibrated is={}, oos={}). Consider shorter timeframes, longer ranges, or lower target_is_trades.",
            required_bars,
            wfa_bars,
            is,
            oos
        ));
    }

    Ok((is, oos, step))
}

pub fn htf_timeframes(tf: &str) -> Vec<String> {
    match tf {
        "5m" => vec!["1h".to_string()],
        "15m" => vec!["1h".to_string(), "4h".to_string()],
        "30m" => vec!["2h".to_string(), "8h".to_string()],
        "1h" => vec!["4h".to_string(), "1d".to_string()],
        "4h" => vec!["1d".to_string()],
        _ => vec!["4h".to_string(), "1d".to_string()],
    }
}

pub fn parse_range_years(range: &str) -> Result<f64> {
    let parts: Vec<&str> = range.splitn(2, '_').collect();
    if parts.len() < 2 {
        return Err(anyhow!("Invalid range format: {}", range));
    }
    let n: f64 = parts[0]
        .parse()
        .map_err(|_| anyhow!("Invalid number in range: {}", parts[0]))?;
    let unit = parts[1];
    if unit.starts_with("year") {
        Ok(n)
    } else if unit.starts_with("month") {
        Ok(n / 12.0)
    } else if unit.starts_with("day") {
        Ok(n / 365.25)
    } else {
        Err(anyhow!("Unknown range unit: {}", unit))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_range_years() {
        assert_eq!(parse_range_years("5_years").unwrap(), 5.0);
        assert_eq!(parse_range_years("18_months").unwrap(), 1.5);
        assert!(parse_range_years("garbage").is_err());
    }

    #[test]
    fn test_calibrate_wfa_windows() {
        // normal case
        // D = 1000 / 100000 = 0.01
        // target_is = 50.0
        // is = 50.0 / 0.01 = 5000
        // oos = 1250
        // total required = 5000 + 8 * 1250 = 15000
        let (is, oos, step) = calibrate_wfa_windows(1000, 100000, 20000, 50.0, 8).unwrap();
        assert_eq!(is, 5000);
        assert_eq!(oos, 1250);
        assert_eq!(step, 1250);

        // insufficient data error
        let err = calibrate_wfa_windows(1000, 100000, 5000, 50.0, 8);
        assert!(err.is_err());
    }
}
