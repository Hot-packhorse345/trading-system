use std::collections::HashMap;

use strategy::build_strategy;
use ts_core::{Bar, Direction, IndicatorSet, Params};

fn make_bars(n: usize) -> Vec<Bar> {
    (0..n)
        .map(|i| {
            Bar::new(
                1_719_000_000 + (i as i64) * 60,
                100.0,
                101.0,
                99.0,
                100.0,
                1000.0,
            )
        })
        .collect()
}

fn bar(close: f64) -> Bar {
    Bar::new(0, close, close, close, close, 0.0)
}

#[test]
fn ema_cross_generates_buy_signal_on_bullish_cross() {
    let strategy = build_strategy("ema_cross").unwrap();
    let bars = make_bars(10);

    let mut indicators = IndicatorSet::default();

    indicators.insert(
        "ema_9",
        vec![
            f64::NAN,
            f64::NAN,
            98.0,
            99.0,
            102.0,
            104.0,
            105.0,
            105.0,
            105.0,
            105.0,
        ],
    );

    indicators.insert(
        "ema_21",
        vec![
            f64::NAN,
            f64::NAN,
            100.0,
            100.0,
            100.0,
            100.0,
            100.0,
            100.0,
            100.0,
            100.0,
        ],
    );

    let signals = strategy.generate_signals(
        &bars,
        &indicators,
        &Params::default(),
        &HashMap::new(),
    );

    assert_eq!(signals.len(), bars.len());

    let signal = signals[4].as_ref().expect("expected buy signal");
    assert_eq!(signal.direction, Direction::Buy);

    for (i, signal) in signals.iter().enumerate() {
        if i != 4 {
            assert!(
                signal.is_none(),
                "expected no signal at index {i}"
            );
        }
    }
}

#[test]
fn rsi_reversion_generates_buy_signal_after_oversold_bounce() {
    let strategy = build_strategy("rsi_reversion").unwrap();

    let bars: Vec<Bar> = (0..5).map(|i| bar(100.0 + i as f64)).collect();

    let mut indicators = IndicatorSet::default();
    indicators.insert("rsi_14", vec![50.0, 40.0, 25.0, 20.0, 35.0]);

    let signals = strategy.generate_signals(
        &bars,
        &indicators,
        &Params::default(),
        &HashMap::new(),
    );

    let signal = signals[4].as_ref().expect("expected buy signal");
    assert_eq!(signal.direction, Direction::Buy);
}

#[test]
fn rsi_reversion_generates_sell_signal_after_overbought_fade() {
    let strategy = build_strategy("rsi_reversion").unwrap();

    let bars: Vec<Bar> = (0..5).map(|i| bar(100.0 - i as f64)).collect();

    let mut indicators = IndicatorSet::default();
    indicators.insert("rsi_14", vec![50.0, 60.0, 75.0, 80.0, 65.0]);

    let signals = strategy.generate_signals(
        &bars,
        &indicators,
        &Params::default(),
        &HashMap::new(),
    );

    let signal = signals[4].as_ref().expect("expected sell signal");
    assert_eq!(signal.direction, Direction::Sell);
}

#[test]
fn rsi_reversion_returns_no_signals_when_indicator_is_missing() {
    let strategy = build_strategy("rsi_reversion").unwrap();

    let bars: Vec<Bar> = (0..3).map(|i| bar(100.0 + i as f64)).collect();
    let indicators = IndicatorSet::default();

    let signals = strategy.generate_signals(
        &bars,
        &indicators,
        &Params::default(),
        &HashMap::new(),
    );

    assert!(signals.iter().all(Option::is_none));
}

#[test]
fn build_strategy_returns_error_for_unknown_strategy() {
    assert!(build_strategy("non_existent_strategy").is_err());
}
