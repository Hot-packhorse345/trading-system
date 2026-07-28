use indicators::{
    ema::{Ema, Sma, Wma},
    factory::{build, split_indicator_def, IndicatorConfig},
    macd::Macd,
    rsi::Rsi,
    Indicator,
};
use serde_json::Value;
use std::collections::HashMap;
use ts_core::Bar;

// Helper to create a series of test bars
fn test_bars(n: usize) -> Vec<Bar> {
    (0..n)
        .map(|i| {
            let val = 100.0 + (i as f64);
            Bar::new(
                1719000000 + (i as i64) * 60,
                val - 1.0,
                val + 2.0,
                val - 2.0,
                val,
                1000.0,
            )
        })
        .collect()
}

// ── RSI TESTS ────────────────────────────────────────────────────────────────

#[test]
fn test_rsi_computation() {
    let rsi = Rsi::new(14);
    let bars = test_bars(30);
    let computed = rsi.compute(&bars);

    assert_eq!(computed.len(), 1);
    assert_eq!(computed[0].0, "rsi_14");

    let values = &computed[0].1;
    assert_eq!(values.len(), 30);

    // Warmup period (first 14 elements should be NaN)
    for i in 0..14 {
        assert!(values[i].is_nan(), "Expected NaN at index {}", i);
    }
    // Afterwards should be valid numbers
    for i in 14..30 {
        assert!(
            !values[i].is_nan(),
            "Expected valid RSI value at index {}",
            i
        );
        // Since price goes up on every single bar, RSI should be near/at 100
        assert!(values[i] > 90.0, "Expected high RSI, got {}", values[i]);
    }
}

#[test]
fn test_rsi_short_history() {
    let rsi = Rsi::new(14);
    let bars = test_bars(10); // Less than period
    let computed = rsi.compute(&bars);
    let values = &computed[0].1;

    assert_eq!(values.len(), 10);
    for v in values {
        assert!(v.is_nan());
    }
}

// ── MOVING AVERAGES TESTS ────────────────────────────────────────────────────

#[test]
fn test_sma_computation() {
    let sma = Sma { period: 5 };
    let bars = test_bars(10);
    let computed = sma.compute(&bars);
    let values = &computed[0].1;

    // Warmup: first 4 elements are NaN
    for i in 0..4 {
        assert!(values[i].is_nan());
    }
    // index 4 represents average of first 5 closes: 100, 101, 102, 103, 104 -> avg = 102
    assert!((values[4] - 102.0).abs() < 1e-9);
    // index 5: 101, 102, 103, 104, 105 -> avg = 103
    assert!((values[5] - 103.0).abs() < 1e-9);
}

#[test]
fn test_ema_computation() {
    let ema = Ema { period: 5 };
    let bars = test_bars(10);
    let computed = ema.compute(&bars);
    let values = &computed[0].1;

    for i in 0..4 {
        assert!(values[i].is_nan());
    }
    // EMA at period-1 is seeded with SMA (102.0)
    assert!((values[4] - 102.0).abs() < 1e-9);
    // EMA at 5 is calculated with multiplier: 2/(5+1) = 1/3
    // EMA_today = 105.0 * 1/3 + 102.0 * 2/3 = 35 + 68 = 103.0
    assert!((values[5] - 103.0).abs() < 1e-9);
}

#[test]
fn test_wma_computation() {
    let wma = Wma { period: 3 };
    let bars = test_bars(5);
    let computed = wma.compute(&bars);
    let values = &computed[0].1;

    for i in 0..2 {
        assert!(values[i].is_nan());
    }
    // weights: 1, 2, 3 -> sum = 6
    // index 2: closes 100, 101, 102 -> (100*1 + 101*2 + 102*3)/6 = (100 + 202 + 306)/6 = 608/6 = 101.33333
    assert!((values[2] - 101.33333333333333).abs() < 1e-9);
}

// ── ALL INDICATORS EXECUTION TESTS ───────────────────────────────────────────

#[test]
fn test_all_indicators_execution() {
    let kinds = vec![
        ("rsi", HashMap::new()),
        ("ema", HashMap::new()),
        ("sma", HashMap::new()),
        ("wma", HashMap::new()),
        ("macd", {
            let mut m = HashMap::new();
            m.insert("fast".to_string(), Value::from(12u64));
            m.insert("slow".to_string(), Value::from(26u64));
            m.insert("signal".to_string(), Value::from(9u64));
            m
        }),
    ];

    let bars = test_bars(250); // feed plenty of bars to satisfy window sizes/warmups

    for (kind, params) in kinds {
        let cfg = IndicatorConfig {
            kind: kind.to_string(),
            params,
        };
        let ind = build(&cfg).unwrap();
        let computed = ind.compute(&bars);
        assert!(!computed.is_empty());
        for (col_name, values) in computed {
            assert_eq!(values.len(), 250, "Mismatch for indicator: {}", col_name);
        }
    }
}

// ── FACTORY TESTS ────────────────────────────────────────────────────────────

#[test]
fn test_indicator_factory() {
    let mut params = HashMap::new();
    params.insert("period".to_string(), Value::from(14u64));
    let cfg = IndicatorConfig {
        kind: "rsi".to_string(),
        params,
    };

    let ind = build(&cfg).unwrap();
    let bars = test_bars(20);
    let computed = ind.compute(&bars);

    assert_eq!(computed.len(), 1);
    assert_eq!(computed[0].0, "rsi_14");
}

#[test]
fn test_unknown_indicator_err() {
    let cfg = IndicatorConfig {
        kind: "invalid_indicator_name".to_string(),
        params: HashMap::new(),
    };
    assert!(build(&cfg).is_err());
}

#[test]
fn test_compute_all_utility() {
    use indicators::compute_all;
    let cfgs = vec![
        IndicatorConfig {
            kind: "rsi".to_string(),
            params: HashMap::new(),
        },
        IndicatorConfig {
            kind: "atr".to_string(),
            params: HashMap::new(),
        },
    ];
    let bars = test_bars(30);
    let result = compute_all(&cfgs, &bars, None).unwrap();
    assert!(result.contains("rsi_14"));
    assert!(result.contains("atr_14"));
}

#[test]
fn test_math_functions() {
    use indicators::math::*;

    let values = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0];

    // Test basic moving averages
    assert_eq!(sma(&values, 3).len(), 10);
    assert_eq!(ema(&values, 3).len(), 10);
    assert_eq!(wma(&values, 3).len(), 10);
    assert_eq!(rma(&values, 3).len(), 10);
    assert_eq!(dema(&values, 3).len(), 10);
    assert_eq!(hull_ma(&values, 3).len(), 10);
    assert_eq!(zlema(&values, 3).len(), 10);
    assert_eq!(tma(&values, 3).len(), 10);
    assert_eq!(tillson_t3(&values, 3, 0.7).len(), 10);
    assert_eq!(var_ma(&values, 3).len(), 10);
    assert_eq!(tsf(&values, 3).len(), 10);

    // Test rolling utilities
    assert_eq!(rolling_std(&values, 3).len(), 10);
    assert_eq!(rolling_max(&values, 3).len(), 10);
    assert_eq!(rolling_min(&values, 3).len(), 10);
    assert_eq!(rolling_sum_nz(&values, 3).len(), 10);

    // Test from functions and specific variants
    assert_eq!(rma_from(&values, 3, 2).len(), 10);
    assert_eq!(ema_from(&values, 3, 2).len(), 10);
    assert_eq!(talib_atr(&values, 3).len(), 10);
    assert_eq!(rsi_values(&values, 3).len(), 10);
    assert_eq!(mean_deviation(&values, 3).len(), 10);

    // Test apply_ma dispatch
    for ma in &[
        "SMA",
        "EMA",
        "WMA",
        "WWMA",
        "RMA",
        "DEMA",
        "DMA",
        "HULL",
        "HMA",
        "ZLEMA",
        "ZEMA",
        "TMA",
        "TSF",
        "TILL",
        "T3",
        "VAR",
        "VIDYA",
        "UNKNOWN_DEFAULT",
    ] {
        let res = apply_ma(&values, 3, ma, 0.7);
        assert_eq!(res.len(), 10);
    }

    // Edge cases
    let empty: Vec<f64> = vec![];
    assert!(sma(&empty, 3).is_empty());
    assert!(var_ma(&empty, 3).is_empty());
    assert!(tsf(&empty, 3).is_empty());

    // TSF length < 2 or length > values.len()
    assert!(tsf(&values, 1).iter().all(|x| x.is_nan()));
    assert!(tsf(&values, 15).iter().all(|x| x.is_nan()));

    // NaN propagation in var_ma
    let nan_values = vec![f64::NAN, 1.0, 2.0, f64::NAN, 3.0];
    let v_ma = var_ma(&nan_values, 3);
    assert!(v_ma[0].is_nan());
    assert!(v_ma[3].is_nan());
}

// ── MACD TESTS ──────────────────────────────────────────────────────────────

#[test]
fn test_macd_computation() {
    let macd = Macd::new(12, 26, 9);
    let bars = test_bars(50);
    let computed = macd.compute(&bars);

    assert_eq!(computed.len(), 3);
    let names: Vec<&str> = computed.iter().map(|(n, _)| n.as_str()).collect();
    assert!(names.contains(&"macd"));
    assert!(names.contains(&"macd_signal"));
    assert!(names.contains(&"macd_hist"));

    for (_, vals) in &computed {
        assert_eq!(vals.len(), 50);
    }

    let macd_vals = &computed.iter().find(|(n, _)| n == "macd").unwrap().1;
    // After warmup, values should be finite
    let non_nan_count = macd_vals.iter().filter(|v| !v.is_nan()).count();
    assert!(non_nan_count > 10);
}

#[test]
fn test_macd_short_history() {
    let macd = Macd::new(12, 26, 9);
    let bars = test_bars(10);
    let computed = macd.compute(&bars);
    assert_eq!(computed.len(), 3);
    // All NaN since bars < slow period (26)
    let macd_vals = &computed.iter().find(|(n, _)| n == "macd").unwrap().1;
    assert!(macd_vals.iter().all(|v| v.is_nan()));
}

// ── FACTORY EDGE CASES ──────────────────────────────────────────────────────

#[test]
fn test_split_indicator_def() {
    let mut obj = serde_json::Map::new();
    obj.insert("type".to_string(), Value::from("rsi"));
    obj.insert("timeframe".to_string(), Value::from("1h"));
    obj.insert("period".to_string(), Value::from(14u64));

    let (ind_type, tf, params) = split_indicator_def(&obj);
    assert_eq!(ind_type.unwrap(), "rsi");
    assert_eq!(tf.unwrap(), "1h");
    assert_eq!(params.len(), 1);
    assert_eq!(params.get("period").unwrap(), &Value::from(14u64));
}

#[test]
fn test_split_indicator_def_no_type() {
    let obj = serde_json::Map::new();
    let (ind_type, tf, params) = split_indicator_def(&obj);
    assert!(ind_type.is_none());
    assert!(tf.is_none());
    assert!(params.is_empty());
}

#[test]
fn test_canonical_json_deterministic() {
    let mut params = HashMap::new();
    params.insert("z_param".to_string(), Value::from(1u64));
    params.insert("a_param".to_string(), Value::from(2u64));
    let cfg = IndicatorConfig {
        kind: "rsi".to_string(),
        params,
    };
    let json1 = cfg.canonical_json();
    let json2 = cfg.canonical_json();
    assert_eq!(json1, json2);
    // Verify sorted order
    assert!(json1.find("a_param").unwrap() < json1.find("z_param").unwrap());
}

// ── INDICATOR WITH EMPTY BARS ───────────────────────────────────────────────

#[test]
fn test_indicators_empty_bars() {
    let empty_bars: Vec<Bar> = vec![];

    let rsi = Rsi::new(14);
    let computed = rsi.compute(&empty_bars);
    assert_eq!(computed[0].1.len(), 0);

    let sma = Sma { period: 5 };
    let computed = sma.compute(&empty_bars);
    assert_eq!(computed[0].1.len(), 0);
}

// ── SINGLE BAR INDICATORS ──────────────────────────────────────────────────

#[test]
fn test_indicators_single_bar() {
    let one_bar = test_bars(1);

    let rsi = Rsi::new(14);
    let computed = rsi.compute(&one_bar);
    assert_eq!(computed[0].1.len(), 1);
    assert!(computed[0].1[0].is_nan());
}

// ── FACTORY WITH PARAMETER VARIANTS ─────────────────────────────────────────

#[test]
fn test_factory_with_timeperiod() {
    let mut params = HashMap::new();
    params.insert("timeperiod".to_string(), Value::from(7u64));
    let cfg = IndicatorConfig {
        kind: "rsi".to_string(),
        params,
    };
    let ind = build(&cfg).unwrap();
    let bars = test_bars(20);
    let computed = ind.compute(&bars);
    assert_eq!(computed[0].0, "rsi_7");
}

#[test]
fn test_factory_aliases() {
    // "ma" should resolve to EMA
    let cfg = IndicatorConfig {
        kind: "ma".to_string(),
        params: HashMap::new(),
    };
    assert!(build(&cfg).is_ok());
}

// ── MATH FUNCTION EDGE CASES ────────────────────────────────────────────────

#[test]
fn test_math_values_correctness() {
    use indicators::math::*;

    // SMA: known values
    let vals = vec![2.0, 4.0, 6.0, 8.0, 10.0];
    let s = sma(&vals, 3);
    assert!(s[0].is_nan());
    assert!(s[1].is_nan());
    assert!((s[2] - 4.0).abs() < 1e-9); // (2+4+6)/3
    assert!((s[3] - 6.0).abs() < 1e-9); // (4+6+8)/3
    assert!((s[4] - 8.0).abs() < 1e-9); // (6+8+10)/3

    // Rolling max/min
    let rm = rolling_max(&vals, 3);
    assert!((rm[2] - 6.0).abs() < 1e-9);
    assert!((rm[4] - 10.0).abs() < 1e-9);

    let rn = rolling_min(&vals, 3);
    assert!((rn[2] - 2.0).abs() < 1e-9);
    assert!((rn[4] - 6.0).abs() < 1e-9);

    // RSI values with all-up data
    let up_vals = vec![
        100.0, 101.0, 102.0, 103.0, 104.0, 105.0, 106.0, 107.0, 108.0, 109.0, 110.0, 111.0, 112.0,
        113.0, 114.0, 115.0,
    ];
    let rsi_v = rsi_values(&up_vals, 14);
    assert_eq!(rsi_v.len(), 16);
    // All-up should give RSI near 100
    let last_rsi = rsi_v[15];
    if !last_rsi.is_nan() {
        assert!(last_rsi > 90.0);
    }
}

#[test]
fn test_rolling_std_known_values() {
    use indicators::math::*;

    let vals = vec![1.0, 1.0, 1.0, 1.0, 1.0];
    let std = rolling_std(&vals, 3);
    // All same values -> std = 0
    for i in 2..5 {
        assert!((std[i]).abs() < 1e-9);
    }
}
