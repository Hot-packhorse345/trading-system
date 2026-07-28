use backtest::config::BacktestConfig;
use serde_json::json;
use ts_core::{Bar, SymbolInfo, Timeframe};
use walkforward::{run, WalkforwardConfig, WfReport, WfRound};

#[test]
fn test_walkforward_config_deserialization() {
    let json = r#"{
        "is_bars": 1000,
        "oos_bars": 300,
        "step_bars": 100,
        "min_wf_efficiency": 0.4,
        "min_oos_consistency": 0.5,
        "metric": "sharpe"
    }"#;

    let config: WalkforwardConfig = serde_json::from_str(json).unwrap();
    assert_eq!(config.is_bars, 1000);
    assert_eq!(config.oos_bars, 300);
    assert_eq!(config.step_bars, 100);
    assert_eq!(config.min_wf_efficiency, 0.4);
    assert_eq!(config.min_oos_consistency, 0.5);
    assert_eq!(config.metric, "sharpe");

    // Test deserializing empty config to cover defaults
    let empty_config: WalkforwardConfig = serde_json::from_str("{}").unwrap();
    assert_eq!(empty_config.is_bars, 2000);
    assert_eq!(empty_config.oos_bars, 500);
    assert_eq!(empty_config.step_bars, 250);
    assert_eq!(empty_config.min_wf_efficiency, 0.5);
    assert_eq!(empty_config.min_oos_consistency, 0.6);
    assert_eq!(empty_config.min_oos_trades_per_round, 0);
    assert_eq!(empty_config.min_oos_consistency_lcb, 0.0);
    assert_eq!(empty_config.consistency_confidence_z, 1.96);
    assert_eq!(empty_config.metric, "enhanced_score");
}

#[test]
fn test_walkforward_report_calculations() {
    let config = WalkforwardConfig {
        is_bars: 2000,
        oos_bars: 500,
        step_bars: 250,
        min_wf_efficiency: 0.5,
        min_oos_consistency: 0.6,
        min_oos_trades_per_round: 0,
        min_oos_consistency_lcb: 0.0,
        consistency_confidence_z: 1.96,
        metric: "enhanced_score".to_string(),
    };

    let rounds = vec![
        WfRound {
            window: 1,
            is_score: 2.0,
            oos_score: 1.2,
            oos_total_r: 5.0,
            oos_trades: 10,
            combo: "combo_1".to_string(),
            params: json!(null),
        },
        WfRound {
            window: 2,
            is_score: 2.0,
            oos_score: 0.8,
            oos_total_r: -2.0,
            oos_trades: 8,
            combo: "combo_2".to_string(),
            params: json!(null),
        },
    ];

    let report = WfReport::build(rounds, &config);

    assert_eq!(report.mean_is_score, 2.0);
    assert_eq!(report.mean_oos_score, 1.0);
    assert_eq!(report.wf_efficiency, 0.5);
    assert_eq!(report.oos_consistency, 0.5);

    assert!(!report.passed);

    // Call print to cover print output code paths
    report.print();

    // Cover empty rounds check and passed true branch
    let empty_report = WfReport::build(vec![], &config);
    empty_report.print();

    // Cover passed = true branch
    let passed_rounds = vec![
        WfRound {
            window: 1,
            is_score: 1.0,
            oos_score: 0.9,
            oos_total_r: 1.0,
            oos_trades: 5,
            combo: "combo_1".to_string(),
            params: json!(null),
        },
        WfRound {
            window: 2,
            is_score: 1.0,
            oos_score: 0.9,
            oos_total_r: 1.0,
            oos_trades: 5,
            combo: "combo_1".to_string(),
            params: json!(null),
        },
    ];
    let passed_report = WfReport::build(passed_rounds, &config);
    assert!(passed_report.passed);
    passed_report.print();
}

#[test]
fn test_walkforward_engine_run() {
    let base_json = json!({
        "strategy": "rx8",
        "symbol": "BTCUSDT",
        "timeframe": "15m",
        "stop_timeframe": "timeframe",
        "pyramiding": true,
        "backtest_start": "2024-06-22",
        "backtest_end": "2024-06-23",
        "data_provider": "binance",
        "initial_balance": 100000.0,
        "risk_percentage": 0.001,
        "commission_percent": 0.0003,
        "stop_manager": {
            "type": "fixed",
            "fixed_sl_pts": 10.0,
            "fixed_tp_pts": 20.0
        },
        "strategy_parameters": {
            "cross_only": [false],
            "model_type": "reversal",
            "rsi_upper_threshold": [70],
            "rsi_lower_threshold": [30]
        },
        "indicators": {
            "rsi": {
                "type": "rsi",
                "source": "close",
                "timeperiod": [2]
            },
            "stop_loss_finder": {
                "type": "stop_loss_finder",
                "length": 2,
                "multiplier": 0.1,
                "smoothing": "RMA"
            }
        }
    });
    let base: BacktestConfig = serde_json::from_value(base_json).unwrap();

    let wf = WalkforwardConfig {
        is_bars: 10,
        oos_bars: 5,
        step_bars: 5,
        min_wf_efficiency: 0.1,
        min_oos_consistency: 0.1,
        min_oos_trades_per_round: 0,
        min_oos_consistency_lcb: 0.0,
        consistency_confidence_z: 1.96,
        metric: "sharpe".to_string(),
    };

    let symbol = "BTCUSDT";
    let timeframe = Timeframe::M15;

    // Generate oscillating bars to trigger indicator changes and entries
    let mut bars = Vec::new();
    let start_time = 1719000000;
    for i in 0..30 {
        let price = if i % 2 == 0 { 100.0 } else { 120.0 };
        bars.push(Bar {
            time: start_time + i * 900,
            open: price,
            high: price + 10.0,
            low: price - 10.0,
            close: price,
            volume: 1000.0,
        });
    }

    let symbol_info = SymbolInfo {
        symbol: "BTCUSDT".to_string(),
        spread: 0.0,
        point: 1.0,
        tick_value: 1.0,
        min_lot: 0.1,
        max_lot: 100.0,
        lot_step: 0.1,
        ask: 100.0,
        bid: 100.0,
        digits: 2,
        time: 0.0,
    };

    let report = run(&base, &wf, symbol, timeframe, &bars, &symbol_info).unwrap();
    assert!(!report.rounds.is_empty());
}

// ── WALKFORWARD REPORT EDGE CASES ──────────────────────────────────────────

#[test]
fn test_walkforward_report_single_round() {
    let config = WalkforwardConfig {
        is_bars: 2000,
        oos_bars: 500,
        step_bars: 250,
        min_wf_efficiency: 0.5,
        min_oos_consistency: 0.6,
        min_oos_trades_per_round: 0,
        min_oos_consistency_lcb: 0.0,
        consistency_confidence_z: 1.96,
        metric: "enhanced_score".to_string(),
    };

    let rounds = vec![WfRound {
        window: 1,
        is_score: 2.0,
        oos_score: 1.5,
        oos_total_r: 3.0,
        oos_trades: 10,
        combo: "c1".to_string(),
        params: json!(null),
    }];

    let report = WfReport::build(rounds, &config);
    assert_eq!(report.mean_is_score, 2.0);
    assert_eq!(report.mean_oos_score, 1.5);
    assert_eq!(report.wf_efficiency, 0.75);
    assert_eq!(report.oos_consistency, 1.0);
    assert_eq!(report.oos_stability, 0.0); // n < 2 -> stability = 0
    assert!(report.passed);
}

#[test]
fn test_walkforward_report_zero_is_score() {
    let config = WalkforwardConfig {
        is_bars: 2000,
        oos_bars: 500,
        step_bars: 250,
        min_wf_efficiency: 0.5,
        min_oos_consistency: 0.6,
        min_oos_trades_per_round: 0,
        min_oos_consistency_lcb: 0.0,
        consistency_confidence_z: 1.96,
        metric: "sharpe".to_string(),
    };

    let rounds = vec![
        WfRound {
            window: 1,
            is_score: 0.0,
            oos_score: 1.0,
            oos_total_r: 1.0,
            oos_trades: 5,
            combo: "c1".to_string(),
            params: json!(null),
        },
        WfRound {
            window: 2,
            is_score: 0.0,
            oos_score: 0.5,
            oos_total_r: 0.5,
            oos_trades: 3,
            combo: "c2".to_string(),
            params: json!(null),
        },
    ];

    let report = WfReport::build(rounds, &config);
    assert_eq!(report.wf_efficiency, 0.0); // mean_is = 0 -> wfe = 0
    assert!(!report.passed);
}

#[test]
fn test_walkforward_report_all_unprofitable_oos() {
    let config = WalkforwardConfig {
        is_bars: 2000,
        oos_bars: 500,
        step_bars: 250,
        min_wf_efficiency: 0.5,
        min_oos_consistency: 0.6,
        min_oos_trades_per_round: 0,
        min_oos_consistency_lcb: 0.0,
        consistency_confidence_z: 1.96,
        metric: "sharpe".to_string(),
    };

    let rounds = vec![
        WfRound {
            window: 1,
            is_score: 2.0,
            oos_score: -0.5,
            oos_total_r: -3.0,
            oos_trades: 10,
            combo: "c1".to_string(),
            params: json!(null),
        },
        WfRound {
            window: 2,
            is_score: 1.5,
            oos_score: -0.3,
            oos_total_r: -2.0,
            oos_trades: 8,
            combo: "c2".to_string(),
            params: json!(null),
        },
    ];

    let report = WfReport::build(rounds, &config);
    assert_eq!(report.oos_consistency, 0.0); // no profitable OOS rounds
    assert!(!report.passed); // mean_oos < 0
}

#[test]
fn test_walkforward_report_high_stability() {
    let config = WalkforwardConfig {
        is_bars: 2000,
        oos_bars: 500,
        step_bars: 250,
        min_wf_efficiency: 0.3,
        min_oos_consistency: 0.5,
        min_oos_trades_per_round: 0,
        min_oos_consistency_lcb: 0.0,
        consistency_confidence_z: 1.96,
        metric: "sharpe".to_string(),
    };

    let rounds = vec![
        WfRound {
            window: 1,
            is_score: 2.0,
            oos_score: 1.0,
            oos_total_r: 5.0,
            oos_trades: 10,
            combo: "c1".to_string(),
            params: json!(null),
        },
        WfRound {
            window: 2,
            is_score: 2.0,
            oos_score: 1.0,
            oos_total_r: 5.0,
            oos_trades: 10,
            combo: "c1".to_string(),
            params: json!(null),
        },
        WfRound {
            window: 3,
            is_score: 2.0,
            oos_score: 1.0,
            oos_total_r: 5.0,
            oos_trades: 10,
            combo: "c1".to_string(),
            params: json!(null),
        },
    ];

    let report = WfReport::build(rounds, &config);
    // All identical OOS scores -> std = 0 -> stability = 0 (division by zero guarded)
    assert_eq!(report.oos_stability, 0.0);
    assert!(report.passed);
}

// ── WALKFORWARD CONFIG PARTIAL OVERRIDES ────────────────────────────────────

#[test]
fn test_walkforward_config_partial_fields() {
    let json = r#"{
        "is_bars": 3000,
        "metric": "sharpe"
    }"#;

    let config: WalkforwardConfig = serde_json::from_str(json).unwrap();
    assert_eq!(config.is_bars, 3000);
    assert_eq!(config.oos_bars, 500); // default
    assert_eq!(config.step_bars, 250); // default
    assert_eq!(config.min_wf_efficiency, 0.5); // default
    assert_eq!(config.metric, "sharpe");
}

#[test]
fn test_walkforward_report_exporting() {
    let config = WalkforwardConfig {
        is_bars: 2000,
        oos_bars: 500,
        step_bars: 250,
        min_wf_efficiency: 0.5,
        min_oos_consistency: 0.6,
        min_oos_trades_per_round: 0,
        min_oos_consistency_lcb: 0.0,
        consistency_confidence_z: 1.96,
        metric: "sharpe".to_string(),
    };

    let rounds = vec![WfRound {
        window: 1,
        is_score: 2.0,
        oos_score: 1.0,
        oos_total_r: 5.0,
        oos_trades: 10,
        combo: "c1".to_string(),
        params: json!(null),
    }];

    let report = WfReport::build(rounds, &config);
    let md = report.to_markdown();
    assert!(md.contains("# Walk-Forward Analysis Report"));
    assert!(md.contains("Verdict"));
    assert!(md.contains("Rounds"));

    let html = report.to_html();
    assert!(html.contains("<!DOCTYPE html>"));
    assert!(html.contains("Walk-Forward Analysis"));
    assert!(html.contains("Rounds"));
}

#[test]
fn test_walkforward_consensus() {
    let config = WalkforwardConfig {
        is_bars: 2000,
        oos_bars: 500,
        step_bars: 250,
        min_wf_efficiency: 0.5,
        min_oos_consistency: 0.6,
        min_oos_trades_per_round: 0,
        min_oos_consistency_lcb: 0.0,
        consistency_confidence_z: 1.96,
        metric: "sharpe".to_string(),
    };

    // Test empty
    let empty_report = WfReport::build(vec![], &config);
    assert!(empty_report.consensus().is_none());

    // Test majority
    let rounds = vec![
        WfRound {
            window: 1,
            is_score: 1.0,
            oos_score: 1.0,
            oos_total_r: 2.0,
            oos_trades: 5,
            combo: "".into(),
            params: json!({"a": 1}),
        },
        WfRound {
            window: 2,
            is_score: 1.0,
            oos_score: 1.0,
            oos_total_r: 3.0,
            oos_trades: 5,
            combo: "".into(),
            params: json!({"a": 2}),
        },
        WfRound {
            window: 3,
            is_score: 1.0,
            oos_score: 1.0,
            oos_total_r: 1.0,
            oos_trades: 5,
            combo: "".into(),
            params: json!({"a": 1}),
        },
    ];
    let report = WfReport::build(rounds, &config);
    let (cons, count) = report.consensus().unwrap();
    assert_eq!(cons, json!({"a": 1}));
    assert_eq!(count, 2);

    // Test tie-breaker (highest summed OOS R)
    let rounds2 = vec![
        WfRound {
            window: 1,
            is_score: 1.0,
            oos_score: 1.0,
            oos_total_r: 5.0,
            oos_trades: 5,
            combo: "".into(),
            params: json!({"a": 1}),
        },
        WfRound {
            window: 2,
            is_score: 1.0,
            oos_score: 1.0,
            oos_total_r: 6.0,
            oos_trades: 5,
            combo: "".into(),
            params: json!({"a": 2}),
        },
    ];
    let report2 = WfReport::build(rounds2, &config);
    let (cons2, count2) = report2.consensus().unwrap();
    assert_eq!(cons2, json!({"a": 2}));
    assert_eq!(count2, 1);
}
