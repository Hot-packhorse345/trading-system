use serde::{Deserialize, Serialize};

fn default_min_probe_trades() -> usize {
    200
}
fn default_min_wfe() -> f64 {
    0.5
}
fn default_min_consistency() -> f64 {
    0.5
}
fn default_min_pf() -> f64 {
    1.5
}
fn default_min_sharpe() -> f64 {
    0.8
}
fn default_min_wr() -> f64 {
    0.30
}
fn default_max_dd_pct() -> f64 {
    25.0
}
fn default_min_trades() -> usize {
    200
}
fn default_min_density() -> f64 {
    40.0
}
fn default_plateau_pass_rate() -> f64 {
    0.60
}
fn default_plateau_min_pf() -> f64 {
    1.2
}
fn default_cross_min_pf() -> f64 {
    1.2
}
fn default_target_is_trades() -> f64 {
    50.0
}
fn default_min_rounds() -> usize {
    8
}
fn default_require_cross_asset() -> bool {
    false
}
fn default_max_holdout_evaluations() -> usize {
    2
}
fn default_min_oos_trades_per_round() -> usize {
    3
}
fn default_min_oos_consistency_lcb() -> f64 {
    0.5
}
fn default_consistency_confidence_z() -> f64 {
    1.96
}
fn default_fdr_max_scale() -> f64 {
    3.0
}
fn default_plateau_min_neighbors() -> usize {
    2
}
fn default_max_stress_dd_pct() -> f64 {
    15.0
}
fn default_min_cross_asset_correlation() -> f64 {
    0.60
}
fn default_correlation_window_months() -> f64 {
    6.0
}
fn default_max_avg_open_time_days() -> f64 {
    0.0 // 0.0 means disabled
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Gates {
    #[serde(default = "default_min_probe_trades")]
    pub min_probe_trades: usize,
    #[serde(default = "default_min_wfe")]
    pub min_wfe: f64,
    #[serde(default = "default_min_consistency")]
    pub min_consistency: f64,
    #[serde(default = "default_min_pf")]
    pub min_pf: f64,
    #[serde(default = "default_min_sharpe")]
    pub min_sharpe: f64,
    #[serde(default = "default_min_wr")]
    pub min_wr: f64,
    #[serde(default = "default_max_dd_pct")]
    pub max_dd_pct: f64,
    #[serde(default = "default_min_trades")]
    pub min_trades: usize,
    #[serde(default = "default_min_density")]
    pub min_density: f64,
    #[serde(default = "default_plateau_pass_rate")]
    pub plateau_pass_rate: f64,
    #[serde(default = "default_plateau_min_pf")]
    pub plateau_min_pf: f64,
    #[serde(default = "default_cross_min_pf")]
    pub cross_min_pf: f64,
    #[serde(default = "default_target_is_trades")]
    pub target_is_trades: f64,
    #[serde(default = "default_min_rounds")]
    pub min_rounds: usize,
    #[serde(default = "default_require_cross_asset")]
    pub require_cross_asset: bool,
    #[serde(default = "default_max_holdout_evaluations")]
    pub max_holdout_evaluations: usize,
    #[serde(default = "default_min_oos_trades_per_round")]
    pub min_oos_trades_per_round: usize,
    #[serde(default = "default_min_oos_consistency_lcb")]
    pub min_oos_consistency_lcb: f64,
    #[serde(default = "default_consistency_confidence_z")]
    pub consistency_confidence_z: f64,
    /// Caps the multiple-comparisons scale factor applied to `min_pf`/`min_sharpe`
    /// so a very large historical trial count can't demand an absurd threshold.
    #[serde(default = "default_fdr_max_scale")]
    pub fdr_max_scale: f64,
    /// Alternative Phase 5 pass path: require this many immediate one-step
    /// neighbors of the consensus point (in the micro-plateau grid) to also be
    /// profitable, instead of requiring `plateau_pass_rate` over the whole grid.
    #[serde(default = "default_plateau_min_neighbors")]
    pub plateau_min_neighbors: usize,
    /// Max allowed max-drawdown% under synthetic black-swan injection (Phase 6).
    #[serde(default = "default_max_stress_dd_pct")]
    pub max_stress_dd_pct: f64,
    /// Minimum rolling correlation with the correlated asset required to run
    /// cross-asset validation (Phase 7). Below this, Phase 7 is skipped.
    #[serde(default = "default_min_cross_asset_correlation")]
    pub min_cross_asset_correlation: f64,
    /// Trailing window (in months) used to compute the rolling correlation.
    #[serde(default = "default_correlation_window_months")]
    pub correlation_window_months: f64,
    /// Maximum allowed average open time in days. 0.0 means disabled.
    #[serde(default = "default_max_avg_open_time_days")]
    pub max_avg_open_time_days: f64,
}

impl Default for Gates {
    fn default() -> Self {
        Self {
            min_probe_trades: default_min_probe_trades(),
            min_wfe: default_min_wfe(),
            min_consistency: default_min_consistency(),
            min_pf: default_min_pf(),
            min_sharpe: default_min_sharpe(),
            min_wr: default_min_wr(),
            max_dd_pct: default_max_dd_pct(),
            min_trades: default_min_trades(),
            min_density: default_min_density(),
            plateau_pass_rate: default_plateau_pass_rate(),
            plateau_min_pf: default_plateau_min_pf(),
            cross_min_pf: default_cross_min_pf(),
            target_is_trades: default_target_is_trades(),
            min_rounds: default_min_rounds(),
            require_cross_asset: default_require_cross_asset(),
            max_holdout_evaluations: default_max_holdout_evaluations(),
            min_oos_trades_per_round: default_min_oos_trades_per_round(),
            min_oos_consistency_lcb: default_min_oos_consistency_lcb(),
            consistency_confidence_z: default_consistency_confidence_z(),
            fdr_max_scale: default_fdr_max_scale(),
            plateau_min_neighbors: default_plateau_min_neighbors(),
            max_stress_dd_pct: default_max_stress_dd_pct(),
            min_cross_asset_correlation: default_min_cross_asset_correlation(),
            correlation_window_months: default_correlation_window_months(),
            max_avg_open_time_days: default_max_avg_open_time_days(),
        }
    }
}

/// Approximates the standard normal quantile function (probit) via Acklam's
/// rational approximation. Accurate to ~1.15e-9 absolute error, which is far
/// tighter than needed for scaling a gate threshold.
fn probit(p: f64) -> f64 {
    debug_assert!(p > 0.0 && p < 1.0);

    const A: [f64; 6] = [
        -3.969683028665376e+01,
        2.209460984245205e+02,
        -2.759285104469687e+02,
        1.383_577_518_672_69e2,
        -3.066479806614716e+01,
        2.506628277459239e+00,
    ];
    const B: [f64; 5] = [
        -5.447609879822406e+01,
        1.615858368580409e+02,
        -1.556989798598866e+02,
        6.680131188771972e+01,
        -1.328068155288572e+01,
    ];
    const C: [f64; 6] = [
        -7.784894002430293e-03,
        -3.223964580411365e-01,
        -2.400758277161838e+00,
        -2.549732539343734e+00,
        4.374664141464968e+00,
        2.938163982698783e+00,
    ];
    const D: [f64; 4] = [
        7.784695709041462e-03,
        3.224671290700398e-01,
        2.445134137142996e+00,
        3.754408661907416e+00,
    ];

    const P_LOW: f64 = 0.02425;
    let p_high = 1.0 - P_LOW;

    if p < P_LOW {
        let q = (-2.0 * p.ln()).sqrt();
        (((((C[0] * q + C[1]) * q + C[2]) * q + C[3]) * q + C[4]) * q + C[5])
            / ((((D[0] * q + D[1]) * q + D[2]) * q + D[3]) * q + 1.0)
    } else if p <= p_high {
        let q = p - 0.5;
        let r = q * q;
        (((((A[0] * r + A[1]) * r + A[2]) * r + A[3]) * r + A[4]) * r + A[5]) * q
            / (((((B[0] * r + B[1]) * r + B[2]) * r + B[3]) * r + B[4]) * r + 1.0)
    } else {
        let q = (-2.0 * (1.0 - p).ln()).sqrt();
        -(((((C[0] * q + C[1]) * q + C[2]) * q + C[3]) * q + C[4]) * q + C[5])
            / ((((D[0] * q + D[1]) * q + D[2]) * q + D[3]) * q + 1.0)
    }
}

/// Base per-comparison false-positive rate before Bonferroni-style correction.
const FDR_BASE_ALPHA: f64 = 0.05;

impl Gates {
    /// Scales `min_pf`/`min_sharpe` up as the historical trial count for this
    /// strategy grows, so that a fixed static gate doesn't get cleared purely
    /// by chance after enough data-mined attempts ("fishing expedition" bias).
    ///
    /// Returns `(min_pf_adjusted, min_sharpe_adjusted)`.
    pub fn fdr_adjusted_thresholds(&self, historical_trials: usize) -> (f64, f64) {
        let n = historical_trials.max(1) as f64;
        let z_base = probit(1.0 - FDR_BASE_ALPHA);
        let z_adj = probit((1.0 - FDR_BASE_ALPHA / n).clamp(0.5, 1.0 - 1e-12));
        let scale = (z_adj / z_base).clamp(1.0, self.fdr_max_scale);

        let min_pf_adj = 1.0 + (self.min_pf - 1.0) * scale;
        let min_sharpe_adj = self.min_sharpe * scale;
        (min_pf_adj, min_sharpe_adj)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gates_defaults() {
        let gates: Gates = serde_json::from_str("{}").unwrap();
        assert_eq!(gates.min_probe_trades, 200);
        assert_eq!(gates.min_wfe, 0.5);
        assert_eq!(gates.min_pf, 1.5);
        assert_eq!(gates.min_density, 40.0);
        assert_eq!(gates.min_rounds, 8);
        assert!(!gates.require_cross_asset);
        assert_eq!(gates.max_holdout_evaluations, 2);
        assert_eq!(gates.min_oos_trades_per_round, 3);
        assert_eq!(gates.min_oos_consistency_lcb, 0.5);
        assert_eq!(gates.consistency_confidence_z, 1.96);
        assert_eq!(gates.fdr_max_scale, 3.0);
        assert_eq!(gates.plateau_min_neighbors, 2);
        assert_eq!(gates.max_stress_dd_pct, 15.0);
        assert_eq!(gates.min_cross_asset_correlation, 0.60);
        assert_eq!(gates.correlation_window_months, 6.0);
    }

    #[test]
    fn test_fdr_no_op_at_n_one() {
        let gates = Gates::default();
        let (pf, sharpe) = gates.fdr_adjusted_thresholds(1);
        assert!((pf - gates.min_pf).abs() < 1e-6);
        assert!((sharpe - gates.min_sharpe).abs() < 1e-6);
    }

    #[test]
    fn test_fdr_scales_up_with_trial_count() {
        let gates = Gates::default();
        let (pf1, sharpe1) = gates.fdr_adjusted_thresholds(1);
        let (pf10, sharpe10) = gates.fdr_adjusted_thresholds(10);
        let (pf50, sharpe50) = gates.fdr_adjusted_thresholds(50);

        assert!(pf10 > pf1);
        assert!(pf50 > pf10);
        assert!(sharpe10 > sharpe1);
        assert!(sharpe50 > sharpe10);
    }

    #[test]
    fn test_fdr_capped_by_max_scale() {
        let mut gates = Gates::default();
        gates.fdr_max_scale = 1.5;
        let (pf_huge, sharpe_huge) = gates.fdr_adjusted_thresholds(1_000_000);
        let expected_pf = 1.0 + (gates.min_pf - 1.0) * 1.5;
        let expected_sharpe = gates.min_sharpe * 1.5;
        assert!((pf_huge - expected_pf).abs() < 1e-6);
        assert!((sharpe_huge - expected_sharpe).abs() < 1e-6);
    }

    #[test]
    fn test_gates_default_open_time_disabled() {
        let gates: Gates = serde_json::from_str("{}").unwrap();
        assert_eq!(gates.max_avg_open_time_days, 0.0);
    }
}
