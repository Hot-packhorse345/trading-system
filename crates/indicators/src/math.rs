pub fn rma(values: &[f64], length: usize) -> Vec<f64> {
    let n = values.len();
    let mut out = vec![f64::NAN; n];
    if length == 0 || n < length {
        return out;
    }
    let alpha = 1.0 / length as f64;
    out[length - 1] = values[..length].iter().sum::<f64>() / length as f64;
    for i in length..n {
        out[i] = alpha * values[i] + (1.0 - alpha) * out[i - 1];
    }
    out
}
pub fn ema(values: &[f64], length: usize) -> Vec<f64> {
    let n = values.len();
    let mut out = vec![f64::NAN; n];
    if length == 0 || n < length {
        return out;
    }
    let alpha = 2.0 / (length as f64 + 1.0);
    out[length - 1] = values[..length].iter().sum::<f64>() / length as f64;
    for i in length..n {
        out[i] = alpha * values[i] + (1.0 - alpha) * out[i - 1];
    }
    out
}
pub fn sma(values: &[f64], length: usize) -> Vec<f64> {
    let n = values.len();
    let mut out = vec![f64::NAN; n];
    if length == 0 {
        return out;
    }
    for i in (length - 1)..n {
        out[i] = values[(i + 1 - length)..=i].iter().sum::<f64>() / length as f64;
    }
    out
}
pub fn wma(values: &[f64], length: usize) -> Vec<f64> {
    let n = values.len();
    let mut out = vec![f64::NAN; n];
    if length == 0 {
        return out;
    }
    let denom: f64 = (1..=length).map(|x| x as f64).sum();
    for i in (length - 1)..n {
        out[i] = (0..length)
            .map(|j| values[i - j] * (length - j) as f64)
            .sum::<f64>()
            / denom;
    }
    out
}
pub fn rolling_std(values: &[f64], length: usize) -> Vec<f64> {
    let n = values.len();
    let mut out = vec![f64::NAN; n];
    if length == 0 {
        return out;
    }
    for i in (length - 1)..n {
        let s = &values[(i + 1 - length)..=i];
        let m = s.iter().sum::<f64>() / length as f64;
        out[i] = (s.iter().map(|&x| (x - m).powi(2)).sum::<f64>() / length as f64).sqrt();
    }
    out
}
pub fn rolling_max(values: &[f64], length: usize) -> Vec<f64> {
    let n = values.len();
    let mut out = vec![f64::NAN; n];
    if length == 0 {
        return out;
    }
    for i in (length - 1)..n {
        out[i] = values[(i + 1 - length)..=i]
            .iter()
            .cloned()
            .fold(f64::NEG_INFINITY, f64::max);
    }
    out
}
pub fn rolling_min(values: &[f64], length: usize) -> Vec<f64> {
    let n = values.len();
    let mut out = vec![f64::NAN; n];
    if length == 0 {
        return out;
    }
    for i in (length - 1)..n {
        out[i] = values[(i + 1 - length)..=i]
            .iter()
            .cloned()
            .fold(f64::INFINITY, f64::min);
    }
    out
}

pub fn rma_from(values: &[f64], length: usize, start: usize) -> Vec<f64> {
    let n = values.len();
    let mut out = vec![f64::NAN; n];
    let end = start + length;
    if n < end {
        return out;
    }
    let alpha = 1.0 / length as f64;
    out[end - 1] = values[start..end].iter().sum::<f64>() / length as f64;
    for i in end..n {
        out[i] = alpha * values[i] + (1.0 - alpha) * out[i - 1];
    }
    out
}

pub fn talib_atr(tr: &[f64], period: usize) -> Vec<f64> {
    rma_from(tr, period, 1)
}

pub fn ema_from(values: &[f64], length: usize, start: usize) -> Vec<f64> {
    let n = values.len();
    let mut out = vec![f64::NAN; n];
    let end = start + length;
    if n < end {
        return out;
    }
    let alpha = 2.0 / (length as f64 + 1.0);
    out[end - 1] = values[start..end].iter().sum::<f64>() / length as f64;
    for i in end..n {
        out[i] = alpha * values[i] + (1.0 - alpha) * out[i - 1];
    }
    out
}

pub fn rsi_values(closes: &[f64], period: usize) -> Vec<f64> {
    let n = closes.len();
    let mut out = vec![f64::NAN; n];
    if n < period + 1 {
        return out;
    }
    let mut gains = 0.0f64;
    let mut losses = 0.0f64;
    for i in 1..=period {
        let d = closes[i] - closes[i - 1];
        if d > 0.0 {
            gains += d;
        } else {
            losses -= d;
        }
    }
    let alpha = 1.0 / period as f64;
    let mut avg_gain = gains / period as f64;
    let mut avg_loss = losses / period as f64;
    let rs = if avg_loss == 0.0 {
        f64::INFINITY
    } else {
        avg_gain / avg_loss
    };
    out[period] = 100.0 - 100.0 / (1.0 + rs);
    for i in (period + 1)..n {
        let d = closes[i] - closes[i - 1];
        let (g, l) = if d > 0.0 { (d, 0.0) } else { (0.0, -d) };
        avg_gain = alpha * g + (1.0 - alpha) * avg_gain;
        avg_loss = alpha * l + (1.0 - alpha) * avg_loss;
        let rs = if avg_loss == 0.0 {
            f64::INFINITY
        } else {
            avg_gain / avg_loss
        };
        out[i] = 100.0 - 100.0 / (1.0 + rs);
    }
    out
}

pub fn mean_deviation(values: &[f64], length: usize) -> Vec<f64> {
    let n = values.len();
    let mut out = vec![f64::NAN; n];
    if length == 0 {
        return out;
    }
    for i in (length - 1)..n {
        let s = &values[(i + 1 - length)..=i];
        let m = s.iter().sum::<f64>() / length as f64;
        out[i] = s.iter().map(|&x| (x - m).abs()).sum::<f64>() / length as f64;
    }
    out
}

// ── MA family helpers ─────────────────────────────────────────────────────────

/// Rolling sum treating NaN as 0.
pub fn rolling_sum_nz(values: &[f64], length: usize) -> Vec<f64> {
    let n = values.len();
    let nz: Vec<f64> = values
        .iter()
        .map(|&x| if x.is_nan() { 0.0 } else { x })
        .collect();
    let mut psum = vec![0.0f64; n + 1];
    for i in 0..n {
        psum[i + 1] = psum[i] + nz[i];
    }
    (0..n)
        .map(|i| psum[i + 1] - psum[i.saturating_sub(length - 1)])
        .collect()
}

/// DEMA = 2·EMA₁ − EMA₂.
pub fn dema(values: &[f64], length: usize) -> Vec<f64> {
    let e1 = ema(values, length);
    let e2 = ema(&e1, length);
    let n = values.len();
    (0..n)
        .map(|i| {
            if e1[i].is_nan() || e2[i].is_nan() {
                f64::NAN
            } else {
                2.0 * e1[i] - e2[i]
            }
        })
        .collect()
}

/// Hull MA = WMA(2·WMA(src, len/2) − WMA(src, len), sqrt(len)).
pub fn hull_ma(values: &[f64], length: usize) -> Vec<f64> {
    let half = (length / 2).max(1);
    let sqrt_len = ((length as f64).sqrt().round() as usize).max(1);
    let w1 = wma(values, half);
    let w2 = wma(values, length);
    let n = values.len();
    let diff: Vec<f64> = (0..n)
        .map(|i| {
            if w1[i].is_nan() || w2[i].is_nan() {
                f64::NAN
            } else {
                2.0 * w1[i] - w2[i]
            }
        })
        .collect();
    wma(&diff, sqrt_len)
}

/// Zero-Lag EMA: EMA(src + (src − shift(src, lag)), length).
pub fn zlema(values: &[f64], length: usize) -> Vec<f64> {
    let lag = (length / 2).max(1);
    let n = values.len();
    let mut zx = vec![f64::NAN; n];
    for i in lag..n {
        if !values[i].is_nan() && !values[i - lag].is_nan() {
            zx[i] = 2.0 * values[i] - values[i - lag];
        }
    }
    ema(&zx, length)
}

/// Triangular MA ≈ SMA(SMA(src, ceil(n/2)), floor(n/2)+1) (matches talib TRIMA).
pub fn tma(values: &[f64], length: usize) -> Vec<f64> {
    let len1 = ((length as f64) / 2.0).ceil() as usize;
    let len2 = ((length as f64) / 2.0).floor() as usize + 1;
    let s1 = sma(values, len1);
    sma(&s1, len2)
}

/// Tillson T3.
pub fn tillson_t3(values: &[f64], length: usize, a: f64) -> Vec<f64> {
    let e1 = ema(values, length);
    let e2 = ema(&e1, length);
    let e3 = ema(&e2, length);
    let e4 = ema(&e3, length);
    let e5 = ema(&e4, length);
    let e6 = ema(&e5, length);
    let n = values.len();
    let c1 = -a.powi(3);
    let c2 = 3.0 * a.powi(2) + 3.0 * a.powi(3);
    let c3 = -6.0 * a.powi(2) - 3.0 * a - 3.0 * a.powi(3);
    let c4 = 1.0 + 3.0 * a + a.powi(3) + 3.0 * a.powi(2);
    (0..n)
        .map(|i| {
            if e3[i].is_nan() || e4[i].is_nan() || e5[i].is_nan() || e6[i].is_nan() {
                f64::NAN
            } else {
                c1 * e6[i] + c2 * e5[i] + c3 * e4[i] + c4 * e3[i]
            }
        })
        .collect()
}

/// VIDYA (Variable Index Dynamic Average). Period-9 CMO window is hardcoded to
pub fn var_ma(values: &[f64], length: usize) -> Vec<f64> {
    let n = values.len();
    let mut out = vec![f64::NAN; n];
    if n == 0 {
        return out;
    }

    let alpha = 2.0 / (length as f64 + 1.0);
    const CMO_LEN: usize = 9;

    let mut vud = vec![0.0f64; n];
    let mut vdd = vec![0.0f64; n];
    for i in 1..n {
        if !values[i].is_nan() && !values[i - 1].is_nan() {
            let d = values[i] - values[i - 1];
            if d > 0.0 {
                vud[i] = d;
            } else if d < 0.0 {
                vdd[i] = -d;
            }
        }
    }

    let vud_s = rolling_sum_nz(&vud, CMO_LEN);
    let vdd_s = rolling_sum_nz(&vdd, CMO_LEN);

    if values[0].is_nan() {
        return out;
    }
    out[0] = values[0];
    let mut prev = out[0];
    for i in 1..n {
        if values[i].is_nan() {
            out[i] = f64::NAN;
            prev = f64::NAN;
            continue;
        }
        let denom = vud_s[i] + vdd_s[i];
        let vcmo = if denom != 0.0 {
            (vud_s[i] - vdd_s[i]) / denom
        } else {
            0.0
        };
        let da = alpha * vcmo.abs();
        let p = if prev.is_nan() { 0.0 } else { prev };
        prev = da * values[i] + (1.0 - da) * p;
        out[i] = prev;
    }
    out
}

/// Time-Series Forecast: linear regression projected one step ahead.
/// Matches talib.TSF() = LINEARREG + LINEARREG_SLOPE.
///
/// x = 0, 1, …, length-1 over the rolling window.
/// TSF[i] = a + b·length  where a/b are OLS intercept/slope.
pub fn tsf(values: &[f64], length: usize) -> Vec<f64> {
    let n = values.len();
    let mut out = vec![f64::NAN; n];
    if length < 2 || n < length {
        return out;
    }

    let m = length as f64;
    let sum_x = m * (m - 1.0) / 2.0;
    let sum_x2 = m * (m - 1.0) * (2.0 * m - 1.0) / 6.0;
    let denom = m * sum_x2 - sum_x * sum_x;
    if denom == 0.0 {
        return out;
    }

    for i in (length - 1)..n {
        let slice = &values[(i + 1 - length)..=i];
        let sum_y = slice.iter().sum::<f64>();
        let sum_xy: f64 = slice.iter().enumerate().map(|(j, &y)| j as f64 * y).sum();
        let slope = (m * sum_xy - sum_x * sum_y) / denom;
        let intercept = (sum_y - slope * sum_x) / m;
        out[i] = intercept + slope * m;
    }
    out
}

/// Dispatch MA computation by name.
/// Note: EMA uses SMA-seeded variant (Rust default). For inputs with leading NaN
/// (e.g. a MACD line), WWMA/RMA and most other types will propagate NaN
pub fn apply_ma(values: &[f64], length: usize, mav: &str, t3_factor: f64) -> Vec<f64> {
    match mav.to_uppercase().as_str() {
        "SMA" => sma(values, length),
        "EMA" => ema(values, length),
        "WMA" => wma(values, length),
        "WWMA" | "RMA" => rma(values, length),
        "DEMA" | "DMA" => dema(values, length),
        "HULL" | "HMA" => hull_ma(values, length),
        "ZLEMA" | "ZEMA" => zlema(values, length),
        "TMA" => tma(values, length),
        "TSF" => tsf(values, length),
        "TILL" | "T3" => tillson_t3(values, length, t3_factor),
        "VAR" | "VIDYA" => var_ma(values, length),
        _ => var_ma(values, length),
    }
}
