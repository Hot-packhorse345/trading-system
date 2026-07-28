use crate::config::WalkforwardConfig;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct WfRound {
    pub window: usize,
    pub is_score: f64,
    pub oos_score: f64,
    pub oos_total_r: f64,
    pub oos_trades: usize,
    pub combo: String,
    pub params: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct WfReport {
    pub rounds: Vec<WfRound>,
    /// Walk-Forward Efficiency: mean(OOS score) / mean(IS score).
    pub wf_efficiency: f64,
    /// Fraction of OOS rounds that were profitable (total_r > 0).
    pub oos_consistency: f64,
    /// Wilson lower confidence bound for OOS consistency.
    pub oos_consistency_lcb: f64,
    pub profitable_rounds: usize,
    pub eligible_rounds: usize,
    pub low_trade_rounds: usize,
    pub mean_is_score: f64,
    pub mean_oos_score: f64,
    /// Simplified stability ratio: mean(OOS score) / std(OOS score). A positive
    /// value with low dispersion indicates robust OOS performance.
    pub oos_stability: f64,
    pub min_wf_efficiency: f64,
    pub min_oos_consistency: f64,
    pub min_oos_consistency_lcb: f64,
    pub min_oos_trades_per_round: usize,
    pub passed: bool,
}

impl WfReport {
    pub fn build(rounds: Vec<WfRound>, wf: &WalkforwardConfig) -> Self {
        let n = rounds.len();
        let eligible: Vec<&WfRound> = rounds
            .iter()
            .filter(|r| r.oos_trades >= wf.min_oos_trades_per_round)
            .collect();
        let m = eligible.len();
        let low_trade_rounds = n.saturating_sub(m);
        let mean = |f: &dyn Fn(&WfRound) -> f64| -> f64 {
            if m == 0 {
                0.0
            } else {
                eligible.iter().map(|r| f(r)).sum::<f64>() / m as f64
            }
        };

        let mean_is = mean(&|r| r.is_score);
        let mean_oos = mean(&|r| r.oos_score);
        let wfe = if mean_is.abs() > 1e-12 {
            mean_oos / mean_is
        } else {
            0.0
        };

        let profitable = eligible.iter().filter(|r| r.oos_total_r > 0.0).count();
        let consistency = if m == 0 {
            0.0
        } else {
            profitable as f64 / m as f64
        };
        let consistency_lcb = wilson_lower_bound(profitable, m, wf.consistency_confidence_z);

        let oos_stability = if m >= 2 {
            let var = eligible
                .iter()
                .map(|r| (r.oos_score - mean_oos).powi(2))
                .sum::<f64>()
                / m as f64;
            let std = var.sqrt();
            if std > 1e-12 {
                mean_oos / std
            } else {
                0.0
            }
        } else {
            0.0
        };

        let passed = m > 0
            && wfe >= wf.min_wf_efficiency
            && consistency >= wf.min_oos_consistency
            && consistency_lcb >= wf.min_oos_consistency_lcb
            && mean_oos > 0.0;

        Self {
            rounds,
            wf_efficiency: wfe,
            oos_consistency: consistency,
            oos_consistency_lcb: consistency_lcb,
            profitable_rounds: profitable,
            eligible_rounds: m,
            low_trade_rounds,
            mean_is_score: mean_is,
            mean_oos_score: mean_oos,
            oos_stability,
            min_wf_efficiency: wf.min_wf_efficiency,
            min_oos_consistency: wf.min_oos_consistency,
            min_oos_consistency_lcb: wf.min_oos_consistency_lcb,
            min_oos_trades_per_round: wf.min_oos_trades_per_round,
            passed,
        }
    }

    pub fn consensus(&self) -> Option<(serde_json::Value, usize)> {
        if self.rounds.is_empty() {
            return None;
        }

        // Tally by canonicalized string representation of the parameters
        let mut counts: std::collections::HashMap<String, (serde_json::Value, usize, f64)> =
            std::collections::HashMap::new();
        for round in &self.rounds {
            let key = backtest::canonical_value(&round.params);
            let entry = counts.entry(key).or_insert((round.params.clone(), 0, 0.0));
            entry.1 += 1;
            entry.2 += round.oos_total_r;
        }

        // Find the best entry:
        // Sort/find max by: (count, summed_oos_r)
        let best = counts.into_values().max_by(|a, b| match a.1.cmp(&b.1) {
            std::cmp::Ordering::Equal => a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal),
            other => other,
        });

        best.map(|(params, count, _)| (params, count))
    }

    pub fn print(&self) {
        println!("\n{}", "=".repeat(72));
        println!("WALK-FORWARD ANALYSIS REPORT");
        println!("{}", "=".repeat(72));
        println!("Rounds:              {}", self.rounds.len());
        println!("Mean IS score:       {:.4}", self.mean_is_score);
        println!("Mean OOS score:      {:.4}", self.mean_oos_score);
        println!(
            "Walk-Forward Eff.:   {:.4}  (target >= {:.2})",
            self.wf_efficiency, self.min_wf_efficiency
        );
        println!(
            "OOS consistency:     {:.4}  (target >= {:.2})",
            self.oos_consistency, self.min_oos_consistency
        );
        if self.min_oos_consistency_lcb > 0.0 {
            println!(
                "OOS consistency LCB: {:.4}  (target >= {:.2})",
                self.oos_consistency_lcb, self.min_oos_consistency_lcb
            );
        }
        if self.min_oos_trades_per_round > 0 {
            println!(
                "Eligible rounds:     {} / {} (min OOS trades/round: {})",
                self.eligible_rounds,
                self.rounds.len(),
                self.min_oos_trades_per_round
            );
        }
        println!("OOS stability ratio: {:.4}", self.oos_stability);
        println!("{}", "-".repeat(72));
        println!(
            "{:>6}  {:>12}  {:>12}  {:>12}  {:>8}  {}",
            "window", "IS", "OOS", "OOS_total_R", "trades", "parameters"
        );
        for r in &self.rounds {
            println!(
                "{:>6}  {:>12.4}  {:>12.4}  {:>12.4}  {:>8}  {}",
                r.window, r.is_score, r.oos_score, r.oos_total_r, r.oos_trades, r.combo
            );
        }
        println!("{}", "=".repeat(72));
        if self.passed {
            println!("VERDICT: PASS — robust out-of-sample performance");
        } else {
            println!("VERDICT: FAIL — does not meet walk-forward robustness thresholds");
        }
        println!("{}\n", "=".repeat(72));
    }

    pub fn to_markdown(&self) -> String {
        let mut md = String::new();
        md.push_str("# Walk-Forward Analysis Report\n\n");

        let verdict = if self.passed { "PASS" } else { "FAIL" };
        md.push_str(&format!("**Verdict**: {}\n\n", verdict));

        md.push_str("## Summary Metrics\n\n");
        md.push_str("| Metric | Value | Target / Threshold |\n");
        md.push_str("| --- | --- | --- |\n");
        md.push_str(&format!("| **Rounds** | {} | - |\n", self.rounds.len()));
        md.push_str(&format!(
            "| **Mean IS Score** | {:.4} | - |\n",
            self.mean_is_score
        ));
        md.push_str(&format!(
            "| **Mean OOS Score** | {:.4} | - |\n",
            self.mean_oos_score
        ));
        md.push_str(&format!(
            "| **Walk-Forward Efficiency** | {:.4} | >= {:.2} |\n",
            self.wf_efficiency, self.min_wf_efficiency
        ));
        md.push_str(&format!(
            "| **OOS Consistency** | {:.4} | >= {:.2} |\n",
            self.oos_consistency, self.min_oos_consistency
        ));
        if self.min_oos_consistency_lcb > 0.0 {
            md.push_str(&format!(
                "| **OOS Consistency LCB (Wilson)** | {:.4} | >= {:.2} |\n",
                self.oos_consistency_lcb, self.min_oos_consistency_lcb
            ));
        }
        if self.min_oos_trades_per_round > 0 {
            md.push_str(&format!(
                "| **Eligible rounds** | {} / {} | min OOS trades/round: {} |\n",
                self.eligible_rounds,
                self.rounds.len(),
                self.min_oos_trades_per_round
            ));
        }
        md.push_str(&format!(
            "| **OOS Stability Ratio** | {:.4} | - |\n\n",
            self.oos_stability
        ));

        md.push_str("## Window Details\n\n");
        md.push_str(
            "| Window | IS Score | OOS Score | OOS Total R | Trades | Selected Parameters |\n",
        );
        md.push_str("| --- | --- | --- | --- | --- | --- |\n");
        for r in &self.rounds {
            md.push_str(&format!(
                "| {} | {:.4} | {:.4} | {:.4} | {} | `{}` |\n",
                r.window, r.is_score, r.oos_score, r.oos_total_r, r.oos_trades, r.combo
            ));
        }
        md
    }

    pub fn to_html(&self) -> String {
        let verdict_class = if self.passed { "pass" } else { "fail" };
        let verdict_text = if self.passed {
            "Verdict: PASS"
        } else {
            "Verdict: FAIL"
        };

        let mut html = String::new();
        html.push_str("<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n");
        html.push_str("    <meta charset=\"UTF-8\">\n");
        html.push_str("    <title>Walk-Forward Analysis Report</title>\n");
        html.push_str("    <style>\n");
        html.push_str("        @import url('https://fonts.googleapis.com/css2?family=Inter:wght@300;400;500;600;700&display=swap');\n");
        html.push_str("        :root {\n");
        html.push_str("            --bg-color: #0f172a;\n");
        html.push_str("            --card-bg: rgba(30, 41, 59, 0.7);\n");
        html.push_str("            --border-color: #334155;\n");
        html.push_str("            --text-main: #f8fafc;\n");
        html.push_str("            --text-muted: #94a3b8;\n");
        html.push_str("            --primary: #38bdf8;\n");
        html.push_str("            --success: #10b981;\n");
        html.push_str("            --danger: #ef4444;\n");
        html.push_str("        }\n");
        html.push_str("        body {\n");
        html.push_str("            font-family: 'Inter', sans-serif;\n");
        html.push_str("            background-color: var(--bg-color);\n");
        html.push_str("            color: var(--text-main);\n");
        html.push_str("            margin: 0;\n");
        html.push_str("            padding: 2rem;\n");
        html.push_str("            display: flex;\n");
        html.push_str("            justify-content: center;\n");
        html.push_str("        }\n");
        html.push_str("        .container {\n");
        html.push_str("            width: 100%;\n");
        html.push_str("            max-width: 900px;\n");
        html.push_str("        }\n");
        html.push_str("        .header {\n");
        html.push_str("            display: flex;\n");
        html.push_str("            justify-content: space-between;\n");
        html.push_str("            align-items: center;\n");
        html.push_str("            border-bottom: 1px solid var(--border-color);\n");
        html.push_str("            padding-bottom: 1.5rem;\n");
        html.push_str("            margin-bottom: 2rem;\n");
        html.push_str("        }\n");
        html.push_str("        h1 {\n");
        html.push_str("            margin: 0;\n");
        html.push_str("            font-size: 2rem;\n");
        html.push_str("            font-weight: 700;\n");
        html.push_str("            background: linear-gradient(135deg, #38bdf8, #8b5cf6);\n");
        html.push_str("            -webkit-background-clip: text;\n");
        html.push_str("            -webkit-text-fill-color: transparent;\n");
        html.push_str("        }\n");
        html.push_str("        .verdict-badge {\n");
        html.push_str("            padding: 0.5rem 1rem;\n");
        html.push_str("            border-radius: 9999px;\n");
        html.push_str("            font-weight: 600;\n");
        html.push_str("            text-transform: uppercase;\n");
        html.push_str("            font-size: 0.875rem;\n");
        html.push_str("            letter-spacing: 0.05em;\n");
        html.push_str("        }\n");
        html.push_str("        .verdict-badge.pass {\n");
        html.push_str("            background-color: rgba(16, 185, 129, 0.2);\n");
        html.push_str("            color: var(--success);\n");
        html.push_str("            border: 1px solid rgba(16, 185, 129, 0.4);\n");
        html.push_str("        }\n");
        html.push_str("        .verdict-badge.fail {\n");
        html.push_str("            background-color: rgba(239, 68, 68, 0.2);\n");
        html.push_str("            color: var(--danger);\n");
        html.push_str("            border: 1px solid rgba(239, 68, 68, 0.4);\n");
        html.push_str("        }\n");
        html.push_str("        .grid {\n");
        html.push_str("            display: grid;\n");
        html.push_str("            grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));\n");
        html.push_str("            gap: 1.5rem;\n");
        html.push_str("            margin-bottom: 2.5rem;\n");
        html.push_str("        }\n");
        html.push_str("        .card {\n");
        html.push_str("            background: var(--card-bg);\n");
        html.push_str("            border: 1px solid var(--border-color);\n");
        html.push_str("            border-radius: 12px;\n");
        html.push_str("            padding: 1.25rem;\n");
        html.push_str("            backdrop-filter: blur(8px);\n");
        html.push_str("        }\n");
        html.push_str("        .card-label {\n");
        html.push_str("            font-size: 0.75rem;\n");
        html.push_str("            text-transform: uppercase;\n");
        html.push_str("            letter-spacing: 0.05em;\n");
        html.push_str("            color: var(--text-muted);\n");
        html.push_str("            margin-bottom: 0.5rem;\n");
        html.push_str("        }\n");
        html.push_str("        .card-value {\n");
        html.push_str("            font-size: 1.75rem;\n");
        html.push_str("            font-weight: 700;\n");
        html.push_str("        }\n");
        html.push_str("        .card-value.highlight { color: var(--primary); }\n");
        html.push_str("        .card-value.success { color: var(--success); }\n");
        html.push_str("        .card-value.danger { color: var(--danger); }\n");
        html.push_str("        .card-sub {\n");
        html.push_str("            font-size: 0.75rem;\n");
        html.push_str("            color: var(--text-muted);\n");
        html.push_str("            margin-top: 0.25rem;\n");
        html.push_str("        }\n");
        html.push_str("        .section-title {\n");
        html.push_str("            font-size: 1.25rem;\n");
        html.push_str("            font-weight: 600;\n");
        html.push_str("            margin-bottom: 1rem;\n");
        html.push_str("        }\n");
        html.push_str("        table {\n");
        html.push_str("            width: 100%;\n");
        html.push_str("            border-collapse: collapse;\n");
        html.push_str("            background: var(--card-bg);\n");
        html.push_str("            border: 1px solid var(--border-color);\n");
        html.push_str("            border-radius: 12px;\n");
        html.push_str("            overflow: hidden;\n");
        html.push_str("            margin-bottom: 2rem;\n");
        html.push_str("        }\n");
        html.push_str("        th, td {\n");
        html.push_str("            padding: 1rem;\n");
        html.push_str("            text-align: left;\n");
        html.push_str("            border-bottom: 1px solid var(--border-color);\n");
        html.push_str("        }\n");
        html.push_str("        th {\n");
        html.push_str("            background-color: rgba(30, 41, 59, 0.9);\n");
        html.push_str("            font-weight: 600;\n");
        html.push_str("        }\n");
        html.push_str("        tr:last-child td {\n");
        html.push_str("            border-bottom: none;\n");
        html.push_str("        }\n");
        html.push_str("        .mono {\n");
        html.push_str("            font-family: monospace;\n");
        html.push_str("            font-size: 0.875rem;\n");
        html.push_str("            color: var(--text-muted);\n");
        html.push_str("        }\n");
        html.push_str("    </style>\n");
        html.push_str("</head>\n<body>\n");
        html.push_str("    <div class=\"container\">\n");
        html.push_str("        <div class=\"header\">\n");
        html.push_str("            <h1>Walk-Forward Analysis</h1>\n");
        html.push_str(&format!(
            "            <span class=\"verdict-badge {}\">{}</span>\n",
            verdict_class, verdict_text
        ));
        html.push_str("        </div>\n");

        html.push_str("        <div class=\"grid\">\n");

        html.push_str("            <div class=\"card\">\n");
        html.push_str("                <div class=\"card-label\">Rounds</div>\n");
        html.push_str(&format!(
            "                <div class=\"card-value\">{}</div>\n",
            self.rounds.len()
        ));
        html.push_str("            </div>\n");

        html.push_str("            <div class=\"card\">\n");
        html.push_str("                <div class=\"card-label\">Walk-Forward Eff.</div>\n");
        let wfe_class = if self.wf_efficiency >= self.min_wf_efficiency {
            "success"
        } else {
            "danger"
        };
        html.push_str(&format!(
            "                <div class=\"card-value {}\">{:.4}</div>\n",
            wfe_class, self.wf_efficiency
        ));
        html.push_str(&format!(
            "                <div class=\"card-sub\">Target: &gt;= {:.2}</div>\n",
            self.min_wf_efficiency
        ));
        html.push_str("            </div>\n");

        html.push_str("            <div class=\"card\">\n");
        html.push_str("                <div class=\"card-label\">OOS Consistency</div>\n");
        let cons_class = if self.oos_consistency >= self.min_oos_consistency {
            "success"
        } else {
            "danger"
        };
        html.push_str(&format!(
            "                <div class=\"card-value {}\">{:.4}</div>\n",
            cons_class, self.oos_consistency
        ));
        html.push_str(&format!(
            "                <div class=\"card-sub\">Target: &gt;= {:.2}</div>\n",
            self.min_oos_consistency
        ));
        html.push_str("            </div>\n");

        html.push_str("            <div class=\"card\">\n");
        html.push_str("                <div class=\"card-label\">OOS Stability Ratio</div>\n");
        html.push_str(&format!(
            "                <div class=\"card-value highlight\">{:.4}</div>\n",
            self.oos_stability
        ));
        html.push_str("            </div>\n");

        html.push_str("        </div>\n");

        // Mean values card group
        html.push_str("        <div class=\"grid\" style=\"grid-template-columns: repeat(auto-fit, minmax(300px, 1fr));\">\n");
        html.push_str("            <div class=\"card\">\n");
        html.push_str("                <div class=\"card-label\">Mean IS Score</div>\n");
        html.push_str(&format!(
            "                <div class=\"card-value\">{:.4}</div>\n",
            self.mean_is_score
        ));
        html.push_str("            </div>\n");
        html.push_str("            <div class=\"card\">\n");
        html.push_str("                <div class=\"card-label\">Mean OOS Score</div>\n");
        html.push_str(&format!(
            "                <div class=\"card-value\">{:.4}</div>\n",
            self.mean_oos_score
        ));
        html.push_str("            </div>\n");
        html.push_str("        </div>\n");

        html.push_str("        <div class=\"section-title\">Window Details</div>\n");
        html.push_str("        <table>\n");
        html.push_str("            <thead>\n");
        html.push_str("                <tr>\n");
        html.push_str("                    <th>Window</th>\n");
        html.push_str("                    <th>IS Score</th>\n");
        html.push_str("                    <th>OOS Score</th>\n");
        html.push_str("                    <th>OOS Total R</th>\n");
        html.push_str("                    <th>Trades</th>\n");
        html.push_str("                    <th>Selected Parameters</th>\n");
        html.push_str("                </tr>\n");
        html.push_str("            </thead>\n");
        html.push_str("            <tbody>\n");
        for r in &self.rounds {
            html.push_str("                <tr>\n");
            html.push_str(&format!("                    <td>{}</td>\n", r.window));
            html.push_str(&format!("                    <td>{:.4}</td>\n", r.is_score));
            html.push_str(&format!(
                "                    <td>{:.4}</td>\n",
                r.oos_score
            ));
            html.push_str(&format!(
                "                    <td>{:.4}</td>\n",
                r.oos_total_r
            ));
            html.push_str(&format!("                    <td>{}</td>\n", r.oos_trades));
            html.push_str(&format!(
                "                    <td class=\"mono\">{}</td>\n",
                r.combo
            ));
            html.push_str("                </tr>\n");
        }
        html.push_str("            </tbody>\n");
        html.push_str("        </table>\n");
        html.push_str("    </div>\n");
        html.push_str("</body>\n</html>\n");

        html
    }
}

fn wilson_lower_bound(successes: usize, n: usize, z: f64) -> f64 {
    if n == 0 {
        return 0.0;
    }
    let n_f = n as f64;
    let p = successes as f64 / n_f;
    let z2 = z * z;
    let denom = 1.0 + z2 / n_f;
    let center = p + z2 / (2.0 * n_f);
    let spread = ((p * (1.0 - p) + z2 / (4.0 * n_f)) / n_f).sqrt();
    ((center - z * spread) / denom).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::WalkforwardConfig;

    fn round(window: usize, oos_total_r: f64, oos_trades: usize) -> WfRound {
        WfRound {
            window,
            is_score: 1.0,
            oos_score: 1.0,
            oos_total_r,
            oos_trades,
            combo: "c".to_string(),
            params: serde_json::json!({"x": 1}),
        }
    }

    #[test]
    fn test_wilson_lcb_is_below_point_estimate() {
        let lcb = wilson_lower_bound(4, 5, 1.96);
        assert!(lcb < 0.8);
        assert!(lcb > 0.0);
    }

    #[test]
    fn test_build_excludes_low_trade_rounds() {
        let rounds = vec![round(0, 1.0, 4), round(1, -1.0, 2), round(2, 1.0, 5)];
        let mut cfg = WalkforwardConfig {
            is_bars: 100,
            oos_bars: 50,
            step_bars: 50,
            min_wf_efficiency: 0.1,
            min_oos_consistency: 0.5,
            min_oos_trades_per_round: 3,
            min_oos_consistency_lcb: 0.0,
            consistency_confidence_z: 1.96,
            metric: "enhanced_score".to_string(),
        };

        let report = WfReport::build(rounds.clone(), &cfg);
        assert_eq!(report.eligible_rounds, 2);
        assert_eq!(report.low_trade_rounds, 1);
        assert_eq!(report.profitable_rounds, 2);
        assert_eq!(report.oos_consistency, 1.0);

        cfg.min_oos_consistency_lcb = 0.9;
        let stricter = WfReport::build(rounds, &cfg);
        assert!(!stricter.passed);
    }
}
