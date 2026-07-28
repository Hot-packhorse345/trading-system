use strategy::build_strategy;
use ts_core::{Bar, Direction, IndicatorSet, Params};
use std::collections::HashMap;

fn make_bars(n: usize) -> Vec<Bar> {
    (0..n).map(|i| Bar::new(1719000000 + (i as i64) * 60, 100.0, 101.0, 99.0, 100.0, 1000.0)).collect()
}

#[test]
fn test_ema_cross_strategy_signals() {
    let strat = build_strategy("ema_cross").unwrap();
    let bars = make_bars(10);

    let mut cols = IndicatorSet::default();
    // Simulate an EMA cross: fast starts below slow, then crosses above
    let fast_vals = vec![f64::NAN, f64::NAN, 98.0, 99.0, 102.0, 104.0, 105.0, 105.0, 105.0, 105.0];
    let slow_vals = vec![f64::NAN, f64::NAN, 100.0, 100.0, 100.0, 100.0, 100.0, 100.0, 100.0, 100.0];

    cols.insert("ema_9", fast_vals);
    cols.insert("ema_21", slow_vals);

    let params = Params::default();
    let ind_params = HashMap::new();

    let signals = strat.generate_signals(&bars, &cols, &params, &ind_params);
    assert_eq!(signals.len(), 10);

    // Cross happens at index 4 (prev_fast 99 <= 100, curr_fast 102 > 100)
    assert!(signals[4].is_some());
    let sig4 = signals[4].as_ref().unwrap();
    assert_eq!(sig4.direction, Direction::Buy);

    // Other indexes should be None
    for i in [0, 1, 2, 3, 5, 6, 7, 8, 9] {
        assert!(signals[i].is_none(), "Expected None signal at index {}", i);
    }
}

#[test]
fn test_build_strategy_unknown() {
    let err = build_strategy("non_existent_strategy");
    assert!(err.is_err());
}
