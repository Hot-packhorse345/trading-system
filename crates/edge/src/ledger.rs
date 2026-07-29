use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrialRecord {
    pub session_id: String,
    pub timestamp: String,
    pub strategy: String,
    pub symbol: String,
    pub tf: String,
    pub gates_hash: String,
    pub holdout_start: Option<String>,
    pub holdout_end: Option<String>,
    pub phase2_hash: Option<String>,
    pub phase_reached: usize,
    pub verdict: String,
    pub cross_asset_status: Option<String>,
    pub wfe: f64,
    pub consistency: f64,
    pub consistency_lcb: f64,
    pub pf: f64,
    pub sharpe: f64,
    pub trades: usize,
}

pub fn read_ledger(path: &Path) -> Result<Vec<TrialRecord>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(path)
        .with_context(|| format!("failed to read ledger {}", path.display()))?;
    let mut records = Vec::new();
    for (idx, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let rec: TrialRecord = serde_json::from_str(line)
            .with_context(|| format!("invalid ledger json at line {}", idx + 1))?;
        records.push(rec);
    }
    Ok(records)
}

pub fn append_ledger(path: &Path, record: &TrialRecord) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("failed to open ledger {}", path.display()))?;
    let line = serde_json::to_string(record)?;
    writeln!(f, "{line}")?;
    Ok(())
}

pub fn count_strategy_trials(records: &[TrialRecord], strategy: &str) -> usize {
    records.iter().filter(|r| r.strategy == strategy).count()
}

pub fn count_session_strategy_trials(
    records: &[TrialRecord],
    strategy: &str,
    session_id: &str,
) -> usize {
    records
        .iter()
        .filter(|r| r.strategy == strategy && r.session_id == session_id)
        .count()
}

pub fn unique_holdout_phase2_hashes(
    records: &[TrialRecord],
    strategy: &str,
    symbol: &str,
    tf: &str,
    holdout_start: &str,
    holdout_end: &str,
) -> HashSet<String> {
    records
        .iter()
        .filter(|r| {
            r.strategy == strategy
                && r.symbol == symbol
                && r.tf == tf
                && r.phase_reached >= 4
                && r.holdout_start.as_deref() == Some(holdout_start)
                && r.holdout_end.as_deref() == Some(holdout_end)
        })
        .filter_map(|r| r.phase2_hash.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unique_holdout_phase2_hashes() {
        let records = vec![
            TrialRecord {
                session_id: "s1".to_string(),
                timestamp: "t".to_string(),
                strategy: "rsi_reversion".to_string(),
                symbol: "BTCUSDT".to_string(),
                tf: "1h".to_string(),
                gates_hash: "g".to_string(),
                holdout_start: Some("2024-01-01".to_string()),
                holdout_end: Some("2024-02-01".to_string()),
                phase2_hash: Some("a".to_string()),
                phase_reached: 4,
                verdict: "Passed".to_string(),
                cross_asset_status: None,
                wfe: 0.0,
                consistency: 0.0,
                consistency_lcb: 0.0,
                pf: 0.0,
                sharpe: 0.0,
                trades: 0,
            },
            TrialRecord {
                session_id: "s2".to_string(),
                timestamp: "t".to_string(),
                strategy: "rsi_reversion".to_string(),
                symbol: "BTCUSDT".to_string(),
                tf: "1h".to_string(),
                gates_hash: "g".to_string(),
                holdout_start: Some("2024-01-01".to_string()),
                holdout_end: Some("2024-02-01".to_string()),
                phase2_hash: Some("b".to_string()),
                phase_reached: 5,
                verdict: "Passed".to_string(),
                cross_asset_status: None,
                wfe: 0.0,
                consistency: 0.0,
                consistency_lcb: 0.0,
                pf: 0.0,
                sharpe: 0.0,
                trades: 0,
            },
        ];
        let hashes = unique_holdout_phase2_hashes(
            &records,
            "rsi_reversion",
            "BTCUSDT",
            "1h",
            "2024-01-01",
            "2024-02-01",
        );
        assert_eq!(hashes.len(), 2);
        assert!(hashes.contains("a"));
        assert!(hashes.contains("b"));
    }
}
