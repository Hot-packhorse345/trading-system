use std::collections::BTreeMap;
use ts_core::{Direction, TradeRecord};

use crate::swap::SwapConfig;

#[derive(Debug, Clone, Default)]
pub struct Metrics {
    pub total_trades: usize,
    pub winning_trades: usize,
    pub losing_trades: usize,
    pub win_rate: f64,
    pub profit_factor: f64, // gross_profit_r / gross_loss_r
    pub total_r: f64,       // net R sum (commission-adjusted)
    pub avg_r: f64,
    pub gross_profit: f64, // sum of winning net R
    pub gross_loss: f64,   // sum of absolute losing net R
    pub avg_win: f64,
    pub avg_loss: f64,
    pub avg_open_time_secs: f64,
    pub expectancy: f64,    // 0 unless both wins and losses exist
    pub max_win_r: f64,     // largest single R
    pub max_loss_r: f64,    // most negative single R
    pub median_win_r: f64,  // median of winning trades' R
    pub median_loss_r: f64, // median of losing trades' R (negative)
    pub max_drawdown: f64,  // absolute $ drawdown from equity curve
    pub max_drawdown_pct: f64,
    pub worst_segment_dd_pct: f64, // worst segment drawdown %
    pub longest_win_streak: usize,
    pub longest_loss_streak: usize,
    pub volatility_r: f64,  // population std-dev of R values
    pub sharpe_ratio: f64,  // mean_R / std_R (population std)
    pub sharpe_annualized: f64, // annualized daily Sharpe (√252)
    pub sortino_ratio: f64, // mean_R / downside_std_R
    pub long_sharpe_ratio: f64,
    pub short_sharpe_ratio: f64,
    pub profit_bias: f64, // long_total_r / short_total_r
    pub net_profit: f64,  // total_r * risk_amount ($)
    pub final_balance: f64,
    pub total_swap_cost: f64, // $ sum of swap credits/costs (negative = net cost)
    pub recovery_factor: f64,   // net_profit / max_drawdown
    pub performance_score: f64, // composite score
    // ── Enhanced metrics ──
    pub calmar_ratio: f64,        // annualized return / max_drawdown
    pub mar_ratio: f64,           // total_r / max_drawdown_pct
    pub ulcer_index: f64,         // RMS of drawdown percentages over time
    pub enhanced_score: f64,      // enhanced composite score
    pub trades_significance: f64, // statistical significance factor (0-1)
}

pub fn compute_metrics(
    trades: &[TradeRecord],
    start_ts: i64,
    end_ts: i64,
    initial_balance: f64,
    risk_pct: f64,
    commission_pct: f64,
    commission_per_lot: f64,
    swap: &SwapConfig,
    tick_value: f64
) -> Metrics {
    if trades.is_empty() {
        return Metrics {
            final_balance: initial_balance,
            ..Default::default()
        };
    }

    let risk_amount = initial_balance * risk_pct;
    let inv_risk = if risk_amount > f64::EPSILON {
        1.0 / risk_amount
    } else {
        0.0
    };

    let mut balance = initial_balance;
    let mut peak = initial_balance;
    let mut max_dd = 0.0f64;
    let mut max_dd_pct = 0.0f64;
    let mut gross_profit_r = 0.0f64;
    let mut gross_loss_r = 0.0f64;
    let mut total_swap_cost = 0.0f64;
    let mut wins = 0usize;
    let mut losses = 0usize;
    let mut consec_win = 0usize;
    let mut consec_loss = 0usize;
    let mut max_consec_win = 0usize;
    let mut max_consec_loss = 0usize;
    let mut total_open_time_secs = 0.0f64;

    let n_trades = trades.len();
    let mut r_values: Vec<f64> = Vec::with_capacity(n_trades);
    let mut win_r: Vec<f64> = Vec::with_capacity(n_trades / 2 + 1);
    let mut loss_r: Vec<f64> = Vec::with_capacity(n_trades / 2 + 1);
    let mut long_r: Vec<f64> = Vec::with_capacity(n_trades);
    let mut short_r: Vec<f64> = Vec::with_capacity(n_trades);
    let mut exit_r_pairs: Vec<(i64, f64)> = Vec::with_capacity(n_trades);
    // Equity curve built in the same pass to avoid a second full iteration.
    let mut equity_curve: Vec<f64> = Vec::with_capacity(n_trades + 1);
    equity_curve.push(initial_balance);
    let mut eq = initial_balance;

    for t in trades {
        let r_gross = t.r_multiple();
        let comm_r = (t.entry_price + t.exit_price) * t.volume * commission_pct * inv_risk
            + commission_per_lot * t.volume * 2.0 * inv_risk;
        let swap_cost = swap.trade_swap_cost(
            t.direction,
            t.volume,
            t.entry_time,
            t.exit_time,
            tick_value,
        );
        total_swap_cost += swap_cost;
        // swap_cost is already signed (negative = cost, positive = credit),
        // unlike comm_r which is always a positive deduction.
        let r_net = r_gross - comm_r + swap_cost * inv_risk;

        r_values.push(r_net);
        exit_r_pairs.push((t.exit_time, r_net));
        balance += r_net * risk_amount;
        eq += r_net * risk_amount;
        equity_curve.push(eq);

        match t.direction {
            Direction::Buy => long_r.push(r_net),
            Direction::Sell => short_r.push(r_net),
            _ => {}
        }

        if r_net > 0.0 {
            gross_profit_r += r_net;
            wins += 1;
            win_r.push(r_net);
            consec_win += 1;
            consec_loss = 0;
            max_consec_win = max_consec_win.max(consec_win);
        } else {
            gross_loss_r += r_net.abs();
            losses += 1;
            loss_r.push(r_net);
            consec_loss += 1;
            consec_win = 0;
            max_consec_loss = max_consec_loss.max(consec_loss);
        }

        peak = peak.max(balance);
        max_dd = max_dd.max(peak - balance);
        let cur_dd_pct = if peak > 0.0 {
            (peak - balance) / peak * 100.0
        } else {
            0.0
        };
        max_dd_pct = max_dd_pct.max(cur_dd_pct);

        total_open_time_secs += (t.exit_time - t.entry_time) as f64;
    }

    let total = wins + losses;
    let total_r: f64 = r_values.iter().sum();
    let net_profit = total_r * risk_amount;
    let win_rate = if total > 0 {
        wins as f64 / total as f64
    } else {
        0.0
    };
    let pf = if gross_loss_r > 0.0 {
        gross_profit_r / gross_loss_r
    } else {
        f64::INFINITY
    };
    let avg_r = if total > 0 {
        total_r / total as f64
    } else {
        0.0
    };
    let avg_win = if wins > 0 {
        gross_profit_r / wins as f64
    } else {
        0.0
    };
    let avg_loss = if losses > 0 {
        gross_loss_r / losses as f64
    } else {
        0.0
    };
    let avg_open_time_secs = if total > 0 {
        total_open_time_secs / total as f64
    } else {
        0.0
    };

    let expectancy = if wins > 0 && losses > 0 {
        win_rate * avg_win - (1.0 - win_rate) * avg_loss
    } else {
        0.0
    };

    let recovery = if max_dd > 0.0 {
        (net_profit / max_dd).max(0.0)
    } else {
        0.0
    };

    let max_win_r = r_values.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let max_win_r = if r_values.is_empty() { 0.0 } else { max_win_r };
    let max_loss_r = r_values.iter().cloned().fold(f64::INFINITY, f64::min);
    let max_loss_r = if r_values.is_empty() { 0.0 } else { max_loss_r };

    let median_win_r = median(&mut win_r);
    let median_loss_r = median(&mut loss_r);

    let volatility_r = pop_std(&r_values);

    let sharpe = calc_sharpe(&r_values);
    let sharpe_ann = calc_annualized_sharpe(&exit_r_pairs, start_ts, end_ts);
    let sortino = calc_sortino(&r_values);
    let long_sharpe = calc_sharpe(&long_r);
    let short_sharpe = calc_sharpe(&short_r);

    let long_total: f64 = long_r.iter().sum();
    let short_total: f64 = short_r.iter().sum();
    let profit_bias = if short_total.abs() > 1e-9 {
        long_total / short_total
    } else {
        f64::INFINITY
    };

    let worst_seg_dd = worst_segment_dd_pct(&mut exit_r_pairs, initial_balance, risk_amount);
    let effective_dd = max_dd_pct.max(worst_seg_dd);
    let perf_score = calc_performance_score(total_r, effective_dd, sharpe, pf);

    // ── Enhanced metrics ──
    // equity_curve was built in the first loop above.
    // Find first and last trade timestamps for annualization
    let first_trade_ts = trades.first().map(|t| t.exit_time).unwrap_or(0);
    let last_trade_ts = trades.last().map(|t| t.exit_time).unwrap_or(0);
    let trading_days = ((last_trade_ts - first_trade_ts) as f64 / 86400.0).max(1.0);
    let years = trading_days / 365.0;

    let ulcer_idx = calc_ulcer_index(&equity_curve);
    let calmar = calc_calmar_ratio(net_profit, initial_balance, max_dd, years);
    let mar = if max_dd_pct > 0.0 {
        total_r / max_dd_pct
    } else {
        0.0
    };
    let sig_factor = calc_trades_significance(total as f64, win_rate);
    let enhanced = calc_enhanced_score(
        total_r,
        effective_dd,
        sharpe,
        sortino,
        pf,
        win_rate,
        calmar,
        mar,
        ulcer_idx,
        sig_factor,
        total as f64,
    );

    Metrics {
        total_trades: total,
        winning_trades: wins,
        losing_trades: losses,
        win_rate,
        profit_factor: pf,
        total_r,
        avg_r,
        gross_profit: gross_profit_r,
        gross_loss: gross_loss_r,
        avg_win,
        avg_loss,
        avg_open_time_secs,
        expectancy,
        max_win_r,
        max_loss_r,
        median_win_r,
        median_loss_r,
        max_drawdown: max_dd,
        max_drawdown_pct: max_dd_pct,
        worst_segment_dd_pct: worst_seg_dd,
        longest_win_streak: max_consec_win,
        longest_loss_streak: max_consec_loss,
        volatility_r,
        sharpe_ratio: sharpe,
        sharpe_annualized: sharpe_ann,
        sortino_ratio: sortino,
        long_sharpe_ratio: long_sharpe,
        short_sharpe_ratio: short_sharpe,
        profit_bias,
        net_profit,
        final_balance: initial_balance + net_profit,
        total_swap_cost,
        recovery_factor: recovery,
        performance_score: perf_score,
        // Enhanced metrics
        calmar_ratio: calmar,
        mar_ratio: mar,
        ulcer_index: ulcer_idx,
        enhanced_score: enhanced,
        trades_significance: sig_factor,
    }
}

// ── Helper functions ──────────────────────────────────────────────────────────

fn pop_std(vals: &[f64]) -> f64 {
    if vals.len() < 2 {
        return 0.0;
    }
    let mean = vals.iter().sum::<f64>() / vals.len() as f64;
    let var = vals.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / vals.len() as f64;
    var.sqrt()
}

fn calc_sharpe(r: &[f64]) -> f64 {
    if r.len() < 2 {
        return 0.0;
    }
    let std = pop_std(r);
    if std == 0.0 {
        return 0.0;
    }
    let mean = r.iter().sum::<f64>() / r.len() as f64;
    let v = mean / std;
    if v.is_finite() {
        v
    } else {
        0.0
    }
}

fn calc_annualized_sharpe(exit_r_pairs: &[(i64, f64)], start_ts: i64, end_ts: i64) -> f64 {
    if start_ts >= end_ts || exit_r_pairs.is_empty() {
        return 0.0;
    }

    let mut daily_r: BTreeMap<i64, f64> = BTreeMap::new();
    for &(ts, r) in exit_r_pairs {
        let day = ts / 86400;
        *daily_r.entry(day).or_insert(0.0) += r;
    }

    let start_day = start_ts / 86400;
    let end_day = end_ts / 86400;

    let mut daily_returns = Vec::new();
    for day in start_day..=end_day {
        // 1970-01-01 was Thursday.
        // day % 7: 0=Thu, 1=Fri, 2=Sat, 3=Sun, 4=Mon, 5=Tue, 6=Wed
        let dow = day.rem_euclid(7);
        if dow != 2 && dow != 3 { // Exclude Sat, Sun
            daily_returns.push(*daily_r.get(&day).unwrap_or(&0.0));
        }
    }

    if daily_returns.len() < 2 {
        return 0.0;
    }

    let mean = daily_returns.iter().sum::<f64>() / daily_returns.len() as f64;
    let var = daily_returns.iter().map(|&x| (x - mean).powi(2)).sum::<f64>() / daily_returns.len() as f64;
    let std = var.sqrt();

    if std == 0.0 {
        return 0.0;
    }

    let daily_sharpe = mean / std;
    let annualized = daily_sharpe * (252.0_f64).sqrt();

    if annualized.is_finite() {
        annualized
    } else {
        0.0
    }
}

fn calc_sortino(r: &[f64]) -> f64 {
    if r.len() < 2 {
        return 0.0;
    }
    let mean = r.iter().sum::<f64>() / r.len() as f64;
    let mut neg_sum_sq = 0.0f64;
    let mut neg_count = 0usize;
    for &x in r {
        if x < 0.0 {
            neg_sum_sq += x * x;
            neg_count += 1;
        }
    }
    if neg_count == 0 {
        return f64::INFINITY;
    }
    let downside_std = (neg_sum_sq / neg_count as f64).sqrt();
    if downside_std == 0.0 {
        0.0
    } else {
        mean / downside_std
    }
}

fn median(vals: &mut [f64]) -> f64 {
    if vals.is_empty() {
        return 0.0;
    }
    vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mid = vals.len() / 2;
    if vals.len().is_multiple_of(2) {
        (vals[mid - 1] + vals[mid]) / 2.0
    } else {
        vals[mid]
    }
}

fn worst_segment_dd_pct(
    exit_r_pairs: &mut [(i64, f64)],
    initial_balance: f64,
    risk_amount: f64,
) -> f64 {
    if exit_r_pairs.len() < 2 {
        return 0.0;
    }

    exit_r_pairs.sort_by_key(|&(t, _)| t);

    let first_day = exit_r_pairs.first().unwrap().0 / 86400;
    let last_day = exit_r_pairs.last().unwrap().0 / 86400;
    let total_days = last_day - first_day;
    if total_days < 1 {
        return 0.0;
    }

    let segments = 1usize.max(5usize.min((total_days as f64 / 30.0).round() as usize));

    // Aggregate R by day
    let mut daily: BTreeMap<i64, f64> = BTreeMap::new();
    for &(t, r) in exit_r_pairs.iter() {
        *daily.entry(t / 86400).or_insert(0.0) += r;
    }

    let daily_profits: Vec<f64> = daily.values().copied().collect();
    if daily_profits.len() < 2 {
        return 0.0;
    }

    // Build cumulative equity
    let mut equity = Vec::with_capacity(daily_profits.len());
    let mut cum = 0.0f64;
    for &r in &daily_profits {
        cum += r * risk_amount;
        equity.push(initial_balance + cum);
    }

    let n = equity.len();
    let mut worst_dd = 0.0f64;

    for i in 0..segments {
        let start = i * n / segments;
        let end = ((i + 1) * n / segments).min(n);
        if end - start < 2 {
            continue;
        }
        let seg = &equity[start..end];
        let mut seg_peak = seg[0];
        for &v in seg {
            seg_peak = seg_peak.max(v);
            let dd = (v - seg_peak) / seg_peak.max(1e-9);
            if dd < worst_dd {
                worst_dd = dd;
            }
        }
    }

    worst_dd.abs() * 100.0
}

fn calc_performance_score(
    total_r: f64,
    max_dd_pct: f64, // percentage (0-100)
    sharpe: f64,
    pf: f64,
) -> f64 {
    if !total_r.is_finite() || !max_dd_pct.is_finite() || !sharpe.is_finite() {
        return 0.0;
    }
    if max_dd_pct >= 80.0 || sharpe <= 0.0 || total_r <= 0.0 {
        return 0.0;
    }
    if !pf.is_finite() {
        return 0.0;
    }
    let risk_efficiency = total_r / f64::max(1.0, max_dd_pct.abs());
    let consistency_bonus = f64::min(2.5, sharpe);
    let edge_bonus = 1.0 + f64::min(pf, 5.0);
    let score = risk_efficiency * consistency_bonus * edge_bonus;
    if score.is_finite() {
        score
    } else {
        0.0
    }
}

// ── Enhanced metric helpers ──────────────────────────────────────────────────

/// Ulcer Index: RMS of percentage drawdowns from peak over the equity curve.
/// Lower is better. A value of 0 means no drawdowns at all.
fn calc_ulcer_index(equity_curve: &[f64]) -> f64 {
    if equity_curve.len() < 2 {
        return 0.0;
    }
    let mut peak = equity_curve[0];
    let mut sum_sq = 0.0f64;
    for &eq in &equity_curve[1..] {
        peak = peak.max(eq);
        let dd_pct = if peak > 0.0 {
            (peak - eq) / peak * 100.0
        } else {
            0.0
        };
        sum_sq += dd_pct * dd_pct;
    }
    (sum_sq / (equity_curve.len() - 1) as f64).sqrt()
}

/// Calmar Ratio: annualized return / max absolute drawdown.
fn calc_calmar_ratio(net_profit: f64, initial_balance: f64, max_dd: f64, years: f64) -> f64 {
    if years <= 0.0 || initial_balance <= 0.0 || max_dd <= 0.0 {
        return 0.0;
    }
    let annual_return = (net_profit / initial_balance) / years;
    let ratio = annual_return / (max_dd / initial_balance);
    if ratio.is_finite() {
        ratio.max(0.0)
    } else {
        0.0
    }
}

/// Statistical significance factor: how confident we are that the edge is real.
/// Uses a simplified model: sigmoid based on number of trades and win rate deviation from 0.5.
/// Returns 0.0–1.0.
fn calc_trades_significance(num_trades: f64, win_rate: f64) -> f64 {
    if num_trades < 3.0 {
        return 0.0;
    }
    // Edge magnitude: how far win_rate is from random (0.5)
    let edge = (win_rate - 0.5).abs();
    // Confidence grows with both trade count and edge size
    // Using a logistic-like curve: saturates around 30+ trades with decent edge
    let confidence = 1.0 - (-0.3 * (num_trades - 3.0) * edge).exp();
    confidence.clamp(0.0, 1.0)
}

/// Enhanced composite score — additive weighted model with multiplicative
/// significance penalty. Designed to better discriminate between strategies
/// by considering more dimensions of performance.
///
/// Components (all normalized to comparable scales):
///   1. Risk-adjusted return:    weighted blend of Sharpe + Sortino + Calmar
///   2. Drawdown penalty:        exponential penalty on max_dd_pct
///   3. Profit quality:          PF contribution with soft cap
///   4. Consistency:             win rate contribution (only if > 0.4)
///   5. Significance:            multiplicative penalty for few trades
///
/// Higher is better. A score above ~10 indicates a strong, robust strategy.
fn calc_enhanced_score(
    total_r: f64,
    max_dd_pct: f64,
    sharpe: f64,
    sortino: f64,
    pf: f64,
    win_rate: f64,
    calmar: f64,
    _mar: f64,
    ulcer_index: f64,
    sig_factor: f64,
    num_trades: f64,
) -> f64 {
    // Guard: reject degenerate inputs
    if !total_r.is_finite() || !max_dd_pct.is_finite() || !sharpe.is_finite() {
        return 0.0;
    }
    if max_dd_pct >= 80.0 || total_r <= 0.0 || num_trades < 3.0 {
        return 0.0;
    }

    // 1. Risk-adjusted return composite (weight: Sharpe 40%, Sortino 30%, Calmar 30%)
    // Normalize each to roughly 0–5 range for comparability
    let sharpe_norm = sharpe.clamp(0.0, 5.0);
    let sortino_norm = if sortino.is_finite() {
        sortino.clamp(0.0, 5.0)
    } else {
        0.0
    };
    let calmar_norm = calmar.clamp(0.0, 5.0);
    let risk_adj = 0.4 * sharpe_norm + 0.3 * sortino_norm + 0.3 * calmar_norm;

    // 2. Drawdown penalty — exponential decay as DD increases
    // At 10% DD: factor ~0.90; at 30% DD: factor ~0.50; at 50% DD: factor ~0.14
    let dd_penalty = (-0.05 * max_dd_pct).exp();

    // 3. Profit quality — profit factor with soft cap (diminishing returns above 3.0)
    let pf_capped = if pf.is_finite() { pf.min(5.0) } else { 3.0 };
    let pf_score = 1.0 - (-0.5 * pf_capped).exp(); // 0–1 scale, saturates around 3-4

    // 4. Consistency — win rate only counts if above 40%
    let wr_score = if win_rate > 0.4 {
        (win_rate - 0.4) / 0.6 // normalize 0.4–1.0 → 0–1
    } else {
        0.0
    };

    // 5. Ulcer Index penalty — penalizes strategies with deep/prolonged drawdowns
    // UI of 5 is excellent, 15 is moderate, 30+ is poor
    let ui_penalty = 1.0 / (1.0 + 0.05 * ulcer_index);

    // Weighted additive core (weights sum to 1.0)
    let core = 0.30 * risk_adj       // risk-adjusted returns
             + 0.25 * pf_score       // profit quality
             + 0.15 * wr_score       // consistency
             + 0.10 * dd_penalty * 5.0  // drawdown protection (scaled to 0–5)
             + 0.20 * ui_penalty * 5.0; // ulcer protection (scaled to 0–5)

    // Apply significance multiplier — strategies with few trades get penalized
    let significance_mult = 0.3 + 0.7 * sig_factor; // minimum 0.3x for 3+ trades

    let score = core * significance_mult * 10.0; // scale to ~0-50 range

    if score.is_finite() {
        score
    } else {
        0.0
    }
}
