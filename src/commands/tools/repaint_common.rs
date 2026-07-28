//! Shared helpers for `tools::repaint` (indicator) and `tools::strategy_repaint`
//! (strategy) — both run the same forward-feed / origin-shift comparisons and
//! classify the results the same way.

/// Bucket a repaint severity ratio (repainted / total comparisons) into a label.
pub fn severity_label(sev: f64) -> &'static str {
    if sev > 0.3 {
        "HIGH"
    } else if sev > 0.1 {
        "MEDIUM"
    } else {
        "LOW"
    }
}

/// Classify the combined result of the forward and origin tests.
pub fn classify_repaint_type(forward_repainted: bool, origin_repainted: bool) -> &'static str {
    match (forward_repainted, origin_repainted) {
        (false, false) => "none",
        (true, false) => "forward_only",
        (false, true) => "origin_only",
        (true, true) => "both",
    }
}
