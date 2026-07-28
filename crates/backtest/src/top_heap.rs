use std::cmp::{Ordering, Reverse};
use std::collections::BinaryHeap;

use crate::result::BacktestResult;

/// Bounded min-heap that retains at most `capacity` results ranked by a caller-supplied
/// score function (higher score = better). Uses O(log N) push/eviction.
pub struct TopNHeap {
    capacity: usize,
    heap: BinaryHeap<Reverse<HeapEntry>>,
    score_fn: fn(&BacktestResult) -> f64,
}

struct HeapEntry {
    score: F64Total,
    result: BacktestResult,
}

/// f64 with a total ordering (NaN sorts last via `total_cmp`).
#[derive(Clone, Copy, PartialEq)]
struct F64Total(f64);

impl Eq for F64Total {}

impl PartialOrd for F64Total {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for F64Total {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.total_cmp(&other.0)
    }
}

impl PartialEq for HeapEntry {
    fn eq(&self, other: &Self) -> bool {
        self.score == other.score
    }
}

impl Eq for HeapEntry {}

impl PartialOrd for HeapEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for HeapEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        self.score.cmp(&other.score)
    }
}

impl TopNHeap {
    pub fn new(capacity: usize, score_fn: fn(&BacktestResult) -> f64) -> Self {
        Self {
            capacity,
            heap: BinaryHeap::with_capacity(capacity + 1),
            score_fn,
        }
    }

    /// Push a result. Evicts the current lowest-scoring entry when full.
    pub fn push(&mut self, result: BacktestResult) {
        // Non-finite scores (e.g. profit_factor / sortino = +∞ when a combo has
        // zero losing trades) and zero-trade combos are degenerate/overfit
        // artifacts; rank them last instead of first.
        let raw = (self.score_fn)(&result);
        let score = if result.metrics.total_trades == 0 || !raw.is_finite() {
            f64::NEG_INFINITY
        } else {
            raw
        };
        self.push_entry(HeapEntry {
            score: F64Total(score),
            result,
        });
    }

    fn push_entry(&mut self, entry: HeapEntry) {
        if self.heap.len() < self.capacity {
            self.heap.push(Reverse(entry));
            return;
        }
        // Copy score out first so the immutable borrow ends before mutation.
        let worst_score = match self.heap.peek() {
            Some(Reverse(worst)) => worst.score,
            None => {
                self.heap.push(Reverse(entry));
                return;
            }
        };
        if entry.score > worst_score {
            self.heap.pop();
            self.heap.push(Reverse(entry));
        }
    }

    /// Merge all entries from `other` into `self`, respecting capacity.
    pub fn merge(&mut self, other: TopNHeap) {
        for Reverse(entry) in other.heap {
            self.push_entry(entry);
        }
    }

    /// Drain the heap in descending score order (best first). Uses the same
    /// sanitized scores stored on push (so non-finite/zero-trade combos stay last).
    pub fn into_sorted_vec(self) -> Vec<BacktestResult> {
        let mut v: Vec<(F64Total, BacktestResult)> = self
            .heap
            .into_iter()
            .map(|Reverse(e)| (e.score, e.result))
            .collect();
        v.sort_by_key(|b| std::cmp::Reverse(b.0));
        v.into_iter().map(|(_, r)| r).collect()
    }

    pub fn len(&self) -> usize {
        self.heap.len()
    }

    pub fn is_empty(&self) -> bool {
        self.heap.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ComboKind;
    use crate::metrics::Metrics;

    fn res(sharpe: f64, trades: usize) -> BacktestResult {
        BacktestResult {
            combo_hash: String::new(),
            signal_hash: String::new(),
            combo_kind: ComboKind::TypeA,
            strategy_name: String::new(),
            symbol: String::new(),
            timeframe: String::new(),
            stop_manager: String::new(),
            config: vec![],
            metrics: Metrics {
                sharpe_ratio: sharpe,
                total_trades: trades,
                ..Default::default()
            },
        }
    }

    fn by_sharpe(r: &BacktestResult) -> f64 {
        r.metrics.sharpe_ratio
    }

    #[test]
    fn infinite_and_zero_trade_scores_rank_last() {
        let mut h = TopNHeap::new(4, by_sharpe);
        h.push(res(2.0, 10));
        h.push(res(f64::INFINITY, 1)); // overfit zero-loss artifact
        h.push(res(1.0, 0)); // zero-trade combo
        h.push(res(3.0, 5));

        let sorted = h.into_sorted_vec();
        // Finite, non-empty combos first and in descending order.
        assert_eq!(sorted[0].metrics.sharpe_ratio, 3.0);
        assert_eq!(sorted[1].metrics.sharpe_ratio, 2.0);
        // The INFINITY and zero-trade combos are demoted to the bottom.
        let tail: Vec<_> = sorted[2..]
            .iter()
            .map(|r| (r.metrics.sharpe_ratio, r.metrics.total_trades))
            .collect();
        assert!(tail.contains(&(f64::INFINITY, 1)));
        assert!(tail.contains(&(1.0, 0)));
    }

    #[test]
    fn bounded_capacity_keeps_best() {
        let mut h = TopNHeap::new(2, by_sharpe);
        for s in [1.0, 5.0, 3.0, 2.0] {
            h.push(res(s, 10));
        }
        let sorted = h.into_sorted_vec();
        assert_eq!(sorted.len(), 2);
        assert_eq!(sorted[0].metrics.sharpe_ratio, 5.0);
        assert_eq!(sorted[1].metrics.sharpe_ratio, 3.0);
    }
}
