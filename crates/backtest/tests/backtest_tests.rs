use backtest::{
    config::{parse_duration, resolve_stop_tf, BacktestConfig, ComboKind},
    engine,
    grid::{count_combos, generate_combos, generate_group_specs},
    metrics::compute_metrics,
    report::{write_csv, write_trades_csv},
    result::BacktestResult,
    simulator::{simulate, SimParams},
    swap::SwapConfig,
};
use risk::{exit::StrategyExit, volume::FixedPercent, ExitManager};
use serde_json::json;
use std::collections::HashMap;
use ts_core::{
    Bar, Direction, ExitReason, IndicatorSet, Signal, SymbolInfo, Timeframe, TradeRecord,
};

#[test]
fn test_parse_duration() {
    assert_eq!(parse_duration("10_minute").unwrap().num_minutes(), 10);
    assert_eq!(parse_duration("2_hour").unwrap().num_hours(), 2);
    assert_eq!(parse_duration("3_day").unwrap().num_days(), 3);
    assert_eq!(parse_duration("1_week").unwrap().num_weeks(), 1);
    assert_eq!(parse_duration("1_month").unwrap().num_days(), 30);
    assert_eq!(parse_duration("1_year").unwrap().num_days(), 365);
    assert!(parse_duration("invalid").is_err());
    assert!(parse_duration("abc_minute").is_err());
    assert!(parse_duration("10_invalid_unit").is_err());
}

#[test]
fn test_resolve_stop_tf() {
    let tf = Timeframe::M15;
    assert_eq!(resolve_stop_tf(&json!("timeframe"), tf).unwrap(), tf);
    assert_eq!(resolve_stop_tf(&json!("1h"), tf).unwrap(), Timeframe::H1);
    assert_eq!(resolve_stop_tf(&json!(["5m"]), tf).unwrap(), Timeframe::M5);
    assert_eq!(resolve_stop_tf(&json!([]), tf).unwrap(), tf);
    assert_eq!(resolve_stop_tf(&json!(123), tf).unwrap(), tf);
}

#[test]
fn test_compute_metrics_empty() {
    let metrics = compute_metrics(
        &[],
        0,
        86400 * 365 * 5,
        10000.0,
        0.01,
        0.001,
        0.0,
        &SwapConfig::none(),
        0.0,
    );
    assert_eq!(metrics.total_trades, 0);
    assert_eq!(metrics.final_balance, 10000.0);
}

#[test]
fn test_compute_metrics_single_win() {
    let trades = vec![TradeRecord {
        trade_id: 1,
        strategy_id: 1,
        symbol: "BTCUSDT".to_string(),
        direction: Direction::Buy,
        entry_price: 10000.0,
        exit_price: 10100.0,
        initial_stop_loss: 9900.0,
        current_stop_loss: 9900.0,
        take_profit: 10200.0,
        volume: 1.0,
        open_risk: 100.0,
        entry_time: 1719000000,
        exit_time: 1719003600,
        exit_reason: ExitReason::TakeProfit,
        profit: 1.0,
        currency_pnl: 100.0,
        group_id: 1,
    }];
    let metrics = compute_metrics(
        &trades,
        0,
        86400 * 365 * 5,
        10000.0,
        0.01,
        0.001,
        0.5,
        &SwapConfig::none(),
        0.0,
    );
    assert_eq!(metrics.total_trades, 1);
    assert_eq!(metrics.winning_trades, 1);
    assert_eq!(metrics.losing_trades, 0);
    assert!(metrics.net_profit > 0.0);
}

#[test]
fn test_compute_metrics_win_loss() {
    let trades = vec![
        TradeRecord {
            trade_id: 1,
            strategy_id: 1,
            symbol: "BTCUSDT".to_string(),
            direction: Direction::Buy,
            entry_price: 10000.0,
            exit_price: 10200.0,
            initial_stop_loss: 9900.0,
            current_stop_loss: 9900.0,
            take_profit: 10200.0,
            volume: 1.0,
            open_risk: 100.0,
            entry_time: 1719000000,
            exit_time: 1719003600,
            exit_reason: ExitReason::TakeProfit,
            profit: 2.0,
            currency_pnl: 200.0,
            group_id: 1,
        },
        TradeRecord {
            trade_id: 2,
            strategy_id: 1,
            symbol: "BTCUSDT".to_string(),
            direction: Direction::Sell,
            entry_price: 10000.0,
            exit_price: 10100.0,
            initial_stop_loss: 9900.0,
            current_stop_loss: 9900.0,
            take_profit: 10200.0,
            volume: 1.0,
            open_risk: 100.0,
            entry_time: 1719007200,
            exit_time: 1719010800,
            exit_reason: ExitReason::StopLoss,
            profit: -1.0,
            currency_pnl: -100.0,
            group_id: 1,
        },
    ];
    let metrics = compute_metrics(
        &trades,
        0,
        86400 * 365 * 5,
        10000.0,
        0.01,
        0.0,
        0.0,
        &SwapConfig::none(),
        0.0,
    );
    assert_eq!(metrics.total_trades, 2);
    assert_eq!(metrics.winning_trades, 1);
    assert_eq!(metrics.losing_trades, 1);
    assert_eq!(metrics.win_rate, 0.5);
    assert_eq!(metrics.expectancy, 0.5);
    assert_eq!(metrics.profit_factor, 2.0);
}

#[test]
fn test_grid_generation() {
    let config_json = json!({
        "strategy": "ema_cross",
        "symbol": ["BTCUSDT", "ETHUSDT"],
        "timeframe": ["15m", "1h"],
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
            "stop_distance": 10.0,
            "start_rr": 0.0
        },
        "strategy_parameters": {
            "stop_pct": [0.02, 0.03]
        },
        "indicators": {
            "ema_fast": { "type": "ema", "period": 9 },
            "ema_slow": { "type": "ema", "period": [21, 50] }
        }
    });
    let config: BacktestConfig = serde_json::from_value(config_json).unwrap();
    let combos = generate_combos(&config).unwrap();
    assert_eq!(combos.len(), 16);
    let group_specs = generate_group_specs(&config).unwrap();
    assert_eq!(group_specs.len(), 16);
    for spec in &group_specs {
        assert_eq!(spec.stop_managers.len(), 1);
    }
    let count = count_combos(&config).unwrap();
    assert_eq!(count, 16);
}

#[test]
fn test_grid_generation_advanced() {
    let config_json = json!({
        "strategy": "ema_cross",
        "symbol": "BTCUSDT",
        "timeframe": "15m",
        "stop_timeframe": "timeframe",
        "pyramiding": true,
        "backtest_start": "2024-06-22",
        "backtest_end": "2024-06-23",
        "data_provider": "binance",
        "initial_balance": 100000.0,
        "risk_percentage": 0.001,
        "stop_manager": {
            "type": "fixed",
            "stop_distance": 10.0,
            "start_rr": 0.0
        },
        "strategy_parameters": {
            // Literal fixed parameter prefix
            "#fixed_param": 42.0,
            // Grouped zipped parameters
            "[mygroup]zip1": [1.0, 2.0],
            "[mygroup]zip2": [10.0, 20.0],
            // Sampler range
            "samp_range": {
                "$sample": "range",
                "start": 1.0,
                "stop": 5.0,
                "step": 2.0
            },
            // Sampler log
            "samp_log": {
                "$sample": "log",
                "start": 1.0,
                "stop": 100.0,
                "n": 2
            },
            // Sampler linspace
            "samp_linspace": {
                "$sample": "linspace",
                "start": 0.0,
                "stop": 10.0,
                "n": 2
            },
            // Sampler values
            "samp_values": {
                "$sample": "values",
                "items": ["a", "b"]
            },
            // Nested object
            "nested_obj": {
                "sub_param": [100, 200]
            },
            // Array of objects
            "array_obj": [
                { "obj_p": 1 },
                { "obj_p": 2 }
            ]
        },
        "indicators": {
            "ema_fast": { "type": "ema", "period": 9 },
            "ema_slow": { "type": "ema", "period": 21 }
        }
    });
    let config: BacktestConfig = serde_json::from_value(config_json).unwrap();
    let combos = generate_combos(&config).unwrap();
    assert_eq!(combos.len(), 128);
}

#[test]
fn test_grid_generation_grouped_indicators() {
    let config_json = json!({
        "strategy": "ema_cross",
        "symbol": "BTCUSDT",
        "timeframe": "15m",
        "stop_timeframe": "timeframe",
        "pyramiding": true,
        "backtest_start": "2024-06-22",
        "backtest_end": "2024-06-23",
        "data_provider": "binance",
        "initial_balance": 100000.0,
        "risk_percentage": 0.001,
        "stop_manager": {
            "type": "fixed",
            "stop_distance": 10.0,
            "start_rr": 0.0
        },
        "strategy_parameters": {},
        "indicators": {
            "ema1": {
                "type": "ema",
                "period": 20,
                "[g]timeframe": ["30m", "1h", "4h", "1d"]
            },
            "ema2": {
                "type": "ema",
                "period": 50,
                "[g]timeframe": ["30m", "1h", "4h", "1d"]
            }
        }
    });
    let config: BacktestConfig = serde_json::from_value(config_json).unwrap();
    let combos = generate_combos(&config).unwrap();
    // Since the 4 timeframes are grouped as 'g', they should only produce 4 combinations
    // instead of 4 * 4 = 16 combinations.
    assert_eq!(combos.len(), 4);
    for combo in &combos {
        let tf1 = combo
            .indicators
            .iter()
            .find(|i| i.name == "ema1")
            .unwrap()
            .timeframe
            .as_deref();
        let tf2 = combo
            .indicators
            .iter()
            .find(|i| i.name == "ema2")
            .unwrap()
            .timeframe
            .as_deref();
        assert_eq!(tf1, tf2);
    }
}

#[test]
fn test_simulator_simulate() {
    let mut strategy_params = HashMap::new();
    strategy_params.insert("stop_pct".to_string(), json!(0.02));
    strategy_params.insert("tp_pct".to_string(), json!(0.04));

    let stop_manager_json = json!({
        "type": "fixed",
        "stop_distance": 10.0,
        "start_rr": 0.0
    });
    let stop_config = serde_json::from_value(stop_manager_json).unwrap();

    let params = SimParams {
        symbol: "BTCUSDT",
        timeframe: Timeframe::M15,
        stop_timeframe: Timeframe::M15,
        pyramiding: true,
        initial_balance: 10000.0,
        risk_pct: 0.01,
        commission_pct: 0.0,
        commission_per_lot: 0.0,
        swap: SwapConfig::none(),
        stop_manager: &stop_config,
        strategy_params: &strategy_params,
        collect_trades: true,
        trading_hours: None,
    };

    let bars = vec![
        Bar {
            time: 1719000000,
            open: 100.0,
            high: 105.0,
            low: 95.0,
            close: 101.0,
            volume: 1000.0,
        },
        Bar {
            time: 1719000900,
            open: 101.0,
            high: 110.0,
            low: 100.0,
            close: 109.0,
            volume: 1100.0,
        },
        Bar {
            time: 1719001800,
            open: 109.0,
            high: 115.0,
            low: 108.0,
            close: 114.0,
            volume: 1200.0,
        },
        Bar {
            time: 1719002700,
            open: 114.0,
            high: 125.0,
            low: 113.0,
            close: 124.0,
            volume: 1300.0,
        },
    ];
    let cols = IndicatorSet::default();
    let signals = vec![
        Some(Signal::new(Direction::Buy, 100.0, 90.0, 120.0)),
        None,
        None,
        None,
    ];
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
    let vol_mgr = FixedPercent {
        pct: 0.01,
        initial_balance: 10000.0,
    };
    let exit_mgrs: Vec<Box<dyn ExitManager>> = vec![Box::new(StrategyExit::default())];

    let result = simulate(
        &params,
        &bars,
        &bars,
        &cols,
        &signals,
        &symbol_info,
        &vol_mgr,
        &exit_mgrs,
    );
    assert!(!result.trades.is_empty());
    let trade = &result.trades[0];
    assert_eq!(trade.direction, Direction::Buy);
    assert_eq!(trade.entry_price, 101.0);
    assert_eq!(trade.entry_time, bars[1].time);
    assert!(trade.exit_price >= 120.0);
    assert_eq!(trade.exit_reason, ExitReason::TakeProfit);
}

#[test]
fn test_simulator_sell_and_stopped() {
    let mut strategy_params = HashMap::new();
    strategy_params.insert("stop_pct".to_string(), json!(0.02));

    let stop_manager_json = json!({
        "type": "fixed",
        "stop_distance": 10.0,
        "start_rr": 0.0
    });
    let stop_config = serde_json::from_value(stop_manager_json).unwrap();

    let params = SimParams {
        symbol: "BTCUSDT",
        timeframe: Timeframe::M15,
        stop_timeframe: Timeframe::M15,
        pyramiding: false,
        initial_balance: 10000.0,
        risk_pct: 0.01,
        commission_pct: 0.0,
        commission_per_lot: 0.0,
        swap: SwapConfig::none(),
        stop_manager: &stop_config,
        strategy_params: &strategy_params,
        collect_trades: true,
        trading_hours: None,
    };

    let bars = vec![
        Bar {
            time: 1719000000,
            open: 100.0,
            high: 105.0,
            low: 95.0,
            close: 100.0,
            volume: 1000.0,
        },
        Bar {
            time: 1719000900,
            open: 100.0,
            high: 105.0,
            low: 99.0,
            close: 101.0,
            volume: 1100.0,
        },
        Bar {
            time: 1719001800,
            open: 101.0,
            high: 108.0,
            low: 100.0,
            close: 105.0,
            volume: 1200.0,
        },
        Bar {
            time: 1719002700,
            open: 105.0,
            high: 112.0,
            low: 104.0,
            close: 111.0,
            volume: 1300.0,
        },
    ];
    let cols = IndicatorSet::default();
    let signals = vec![
        Some(Signal::new(Direction::Sell, 100.0, 110.0, 80.0)),
        None,
        None,
        None,
    ];
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
    let vol_mgr = FixedPercent {
        pct: 0.01,
        initial_balance: 10000.0,
    };
    let exit_mgrs: Vec<Box<dyn ExitManager>> = vec![Box::new(StrategyExit::default())];

    let result = simulate(
        &params,
        &bars,
        &bars,
        &cols,
        &signals,
        &symbol_info,
        &vol_mgr,
        &exit_mgrs,
    );
    assert!(!result.trades.is_empty());
    let trade = &result.trades[0];
    assert_eq!(trade.direction, Direction::Sell);
    assert_eq!(trade.entry_price, 100.0);
    assert_eq!(trade.entry_time, bars[1].time);
    assert_eq!(trade.exit_price, 110.0);
    assert_eq!(trade.exit_reason, ExitReason::StopLoss);
}

#[test]
fn test_reports_writing() {
    let uuid = uuid::Uuid::new_v4().to_string();
    let temp_dir = std::env::temp_dir().join(format!("test_reports_{}", uuid));
    std::fs::create_dir_all(&temp_dir).unwrap();
    let csv_path = temp_dir.join("results.csv");
    let trades_csv_path = temp_dir.join("trades.csv");

    let results = vec![BacktestResult {
        combo_hash: "hash123".to_string(),
        signal_hash: "sighash".to_string(),
        combo_kind: ComboKind::TypeA,
        strategy_name: "ema_cross".to_string(),
        symbol: "BTCUSDT".to_string(),
        timeframe: "15m".to_string(),
        stop_manager: "fixed".to_string(),
        config: vec![
            ("strategy".to_string(), "ema_cross".to_string()),
            ("symbol".to_string(), "BTCUSDT".to_string()),
        ],
        metrics: compute_metrics(
            &[],
            0,
            86400 * 365 * 5,
            10000.0,
            0.01,
            0.0,
            0.0,
            &SwapConfig::none(),
            0.0,
        ),
    }];
    assert!(write_csv(&results, &csv_path).is_ok());
    assert!(csv_path.exists());

    let trades = vec![TradeRecord {
        trade_id: 1,
        strategy_id: 1,
        symbol: "BTCUSDT".to_string(),
        direction: Direction::Buy,
        entry_price: 10000.0,
        exit_price: 10100.0,
        initial_stop_loss: 9900.0,
        current_stop_loss: 9950.0,
        take_profit: 10200.0,
        volume: 1.0,
        open_risk: 100.0,
        entry_time: 1719000000,
        exit_time: 1719003600,
        exit_reason: ExitReason::ExitRule,
        profit: 1.0,
        currency_pnl: 100.0,
        group_id: 1,
    }];
    assert!(write_trades_csv(&trades, &trades_csv_path).is_ok());
    assert!(trades_csv_path.exists());

    std::fs::remove_dir_all(&temp_dir).ok();
}

#[test]
fn test_engine_run() {
    let uuid = uuid::Uuid::new_v4().to_string();
    let temp_dir = std::env::temp_dir().join(format!("test_engine_{}", uuid));
    std::fs::create_dir_all(&temp_dir).unwrap();
    let data_dir = temp_dir.clone();

    let cache = data::OhlcvCache::new(&data_dir);
    let sym = "BTCUSDT";
    let tf = Timeframe::M15;
    let start_time = 1719000000;
    let end_time = 1719003600;
    let mut bars = Vec::new();
    let mut curr_time = start_time;
    while curr_time <= end_time {
        bars.push(Bar {
            time: curr_time,
            open: 100.0,
            high: 105.0,
            low: 95.0,
            close: 101.0,
            volume: 1000.0,
        });
        curr_time += tf.seconds();
    }
    cache.save(sym, tf, &bars).unwrap();

    let config_json = json!({
        "strategy": "rsi_reversion",
        "symbol": "BTCUSDT",
        "timeframe": "15m",
        "stop_timeframe": "timeframe",
        "pyramiding": true,
        "backtest_start": "2024-06-22",
        "backtest_end": "2024-06-22",
        "data_provider": "paper",
        "initial_balance": 10000.0,
        "risk_percentage": 0.01,
        "commission_percent": 0.0003,
        "stop_manager": {
            "type": "fixed",
            "stop_distance": 10.0,
            "start_rr": 0.0
        },
        "strategy_parameters": {
            "oversold": 30.0,
            "overbought": 70.0,
            "stop_pct": 0.02,
            "tp_pct": 0.04
        },
        "indicators": {
            "rsi": {
                "type": "rsi",
                "period": 14
            }
        },
        "data_dir": data_dir,
        "output_dir": data_dir.join("results")
    });
    let config: BacktestConfig = serde_json::from_value(config_json).unwrap();
    let run_res = engine::run(&config, 10, None);
    println!("engine run result: {:?}", run_res);
    std::fs::remove_dir_all(&temp_dir).ok();
}

#[test]
fn test_engine_run_with_htf_indicators() {
    let uuid = uuid::Uuid::new_v4().to_string();
    let temp_dir = std::env::temp_dir().join(format!("test_engine_htf_{}", uuid));
    std::fs::create_dir_all(&temp_dir).unwrap();
    let data_dir = temp_dir.clone();

    let cache = data::OhlcvCache::new(&data_dir);
    let sym = "BTCUSDT";
    let tf = Timeframe::M30;
    let start_time = 1719000000;
    let end_time = start_time + 48 * 30 * 60;
    let mut bars = Vec::new();
    let mut curr_time = start_time;
    while curr_time <= end_time {
        bars.push(Bar {
            time: curr_time,
            open: 100.0,
            high: 105.0,
            low: 95.0,
            close: 101.0,
            volume: 1000.0,
        });
        curr_time += tf.seconds();
    }
    cache.save(sym, tf, &bars).unwrap();

    let config_json = json!({
        "strategy": "ema_cross",
        "symbol": "BTCUSDT",
        "timeframe": "30m",
        "stop_timeframe": "timeframe",
        "pyramiding": true,
        "backtest_start": "2024-06-22",
        "backtest_end": "2024-06-22",
        "data_provider": "paper",
        "initial_balance": 10000.0,
        "risk_percentage": 0.01,
        "commission_percent": 0.0003,
        "stop_manager": {
            "type": "fixed",
            "stop_distance": 10.0,
            "start_rr": 0.0
        },
        "strategy_parameters": {
            "stop_pct": 0.02,
            "tp_pct": 0.04
        },
        "indicators": {
            "ema_fast": {
                "type": "ema",
                "period": 9
            },
            "ema_slow": {
                "type": "ema",
                "period": 21,
                "timeframe": "1h"
            }
        },
        "data_dir": data_dir,
        "output_dir": data_dir.join("results")
    });
    let config: BacktestConfig = serde_json::from_value(config_json).unwrap();
    let run_res = engine::run(&config, 10, None);
    assert!(run_res.is_ok());
    std::fs::remove_dir_all(&temp_dir).ok();
}

// ── METRICS RATIO TESTS ────────────────────────────────────────────────────

#[test]
fn test_compute_metrics_sharpe_and_sortino() {
    let trades = vec![
        make_trade(
            Direction::Buy,
            10000.0,
            10100.0,
            9900.0,
            1.0,
            ExitReason::TakeProfit,
        ),
        make_trade(
            Direction::Buy,
            10000.0,
            10200.0,
            9900.0,
            2.0,
            ExitReason::TakeProfit,
        ),
        make_trade(
            Direction::Sell,
            10000.0,
            10050.0,
            9900.0,
            -0.5,
            ExitReason::StopLoss,
        ),
        make_trade(
            Direction::Buy,
            10000.0,
            10150.0,
            9900.0,
            1.5,
            ExitReason::TakeProfit,
        ),
    ];
    let metrics = compute_metrics(
        &trades,
        0,
        86400 * 365 * 5,
        10000.0,
        0.01,
        0.0,
        0.0,
        &SwapConfig::none(),
        0.0,
    );
    assert!(metrics.sharpe_ratio > 0.0);
    assert!(metrics.sortino_ratio > 0.0);
    assert!(metrics.sortino_ratio >= metrics.sharpe_ratio);
}

#[test]
fn test_compute_metrics_all_winners() {
    let trades = vec![
        make_trade(
            Direction::Buy,
            100.0,
            110.0,
            90.0,
            1.0,
            ExitReason::TakeProfit,
        ),
        make_trade(
            Direction::Buy,
            100.0,
            115.0,
            90.0,
            1.5,
            ExitReason::TakeProfit,
        ),
        make_trade(
            Direction::Buy,
            100.0,
            120.0,
            90.0,
            2.0,
            ExitReason::TakeProfit,
        ),
    ];
    let metrics = compute_metrics(
        &trades,
        0,
        86400 * 365 * 5,
        10000.0,
        0.01,
        0.0,
        0.0,
        &SwapConfig::none(),
        0.0,
    );
    assert_eq!(metrics.win_rate, 1.0);
    assert_eq!(metrics.losing_trades, 0);
    assert_eq!(metrics.profit_factor, f64::INFINITY);
    assert_eq!(metrics.max_drawdown, 0.0);
    assert_eq!(metrics.expectancy, 0.0);
}

#[test]
fn test_compute_metrics_all_losers() {
    let trades = vec![
        make_trade(
            Direction::Buy,
            100.0,
            90.0,
            90.0,
            -1.0,
            ExitReason::StopLoss,
        ),
        make_trade(
            Direction::Buy,
            100.0,
            85.0,
            90.0,
            -1.5,
            ExitReason::StopLoss,
        ),
    ];
    let metrics = compute_metrics(
        &trades,
        0,
        86400 * 365 * 5,
        10000.0,
        0.01,
        0.0,
        0.0,
        &SwapConfig::none(),
        0.0,
    );
    assert_eq!(metrics.win_rate, 0.0);
    assert_eq!(metrics.winning_trades, 0);
    assert!(metrics.net_profit < 0.0);
    assert!(metrics.max_drawdown > 0.0);
    assert_eq!(metrics.recovery_factor, 0.0);
    assert_eq!(metrics.expectancy, 0.0);
}

#[test]
fn test_compute_metrics_avg_open_time() {
    let trades = vec![
        TradeRecord {
            trade_id: 1,
            strategy_id: 1,
            symbol: "BTCUSDT".to_string(),
            direction: Direction::Buy,
            entry_price: 0.0,
            exit_price: 0.0,
            initial_stop_loss: 0.0,
            current_stop_loss: 0.0,
            take_profit: 0.0,
            volume: 1.0,
            open_risk: 0.0,
            entry_time: 1000,
            exit_time: 1000 + 86400,
            exit_reason: ExitReason::EndOfData,
            profit: 0.0,
            currency_pnl: 0.0,
            group_id: 1,
        },
        TradeRecord {
            trade_id: 1,
            strategy_id: 1,
            symbol: "BTCUSDT".to_string(),
            direction: Direction::Buy,
            entry_price: 0.0,
            exit_price: 0.0,
            initial_stop_loss: 0.0,
            current_stop_loss: 0.0,
            take_profit: 0.0,
            volume: 1.0,
            open_risk: 0.0,
            entry_time: 2000,
            exit_time: 2000 + (3 * 86400),
            exit_reason: ExitReason::EndOfData,
            profit: 0.0,
            currency_pnl: 0.0,
            group_id: 1,
        },
    ];
    let m = compute_metrics(
        &trades,
        0,
        86400 * 365 * 5,
        10000.0,
        0.01,
        0.0,
        0.0,
        &SwapConfig::none(),
        0.0,
    );
    assert!((m.avg_open_time_secs - 172800.0).abs() < 1.0);
}

#[test]
fn test_compute_metrics_streaks() {
    let trades = vec![
        make_trade(
            Direction::Buy,
            100.0,
            110.0,
            90.0,
            1.0,
            ExitReason::TakeProfit,
        ),
        make_trade(
            Direction::Buy,
            100.0,
            110.0,
            90.0,
            1.0,
            ExitReason::TakeProfit,
        ),
        make_trade(
            Direction::Buy,
            100.0,
            110.0,
            90.0,
            1.0,
            ExitReason::TakeProfit,
        ),
        make_trade(
            Direction::Buy,
            100.0,
            90.0,
            90.0,
            -1.0,
            ExitReason::StopLoss,
        ),
        make_trade(
            Direction::Buy,
            100.0,
            90.0,
            90.0,
            -1.0,
            ExitReason::StopLoss,
        ),
    ];
    let metrics = compute_metrics(
        &trades,
        0,
        86400 * 365 * 5,
        10000.0,
        0.01,
        0.0,
        0.0,
        &SwapConfig::none(),
        0.0,
    );
    assert_eq!(metrics.longest_win_streak, 3);
    assert_eq!(metrics.longest_loss_streak, 2);
}

#[test]
fn test_compute_metrics_with_commission() {
    let trades = vec![make_trade(
        Direction::Buy,
        10000.0,
        10100.0,
        9900.0,
        1.0,
        ExitReason::TakeProfit,
    )];
    let no_comm = compute_metrics(
        &trades,
        0,
        86400 * 365 * 5,
        10000.0,
        0.01,
        0.0,
        0.0,
        &SwapConfig::none(),
        0.0,
    );
    let with_comm = compute_metrics(
        &trades,
        0,
        86400 * 365 * 5,
        10000.0,
        0.01,
        0.001,
        0.0,
        &SwapConfig::none(),
        0.0,
    );
    assert!(with_comm.total_r < no_comm.total_r);
    assert!(with_comm.net_profit < no_comm.net_profit);
}

#[test]
fn test_compute_metrics_long_short_sharpe() {
    let trades = vec![
        make_trade(
            Direction::Buy,
            100.0,
            110.0,
            90.0,
            1.0,
            ExitReason::TakeProfit,
        ),
        make_trade(
            Direction::Buy,
            100.0,
            108.0,
            90.0,
            0.8,
            ExitReason::TakeProfit,
        ),
        make_trade(
            Direction::Sell,
            100.0,
            90.0,
            110.0,
            1.0,
            ExitReason::TakeProfit,
        ),
    ];
    let metrics = compute_metrics(
        &trades,
        0,
        86400 * 365 * 5,
        10000.0,
        0.01,
        0.0,
        0.0,
        &SwapConfig::none(),
        0.0,
    );
    assert!(metrics.long_sharpe_ratio > 0.0);
    assert!(metrics.short_sharpe_ratio == 0.0);
}

#[test]
fn test_compute_metrics_enhanced_score() {
    let trades = vec![
        make_trade(
            Direction::Buy,
            100.0,
            110.0,
            90.0,
            1.0,
            ExitReason::TakeProfit,
        ),
        make_trade(
            Direction::Buy,
            100.0,
            108.0,
            90.0,
            0.8,
            ExitReason::TakeProfit,
        ),
        make_trade(
            Direction::Buy,
            100.0,
            90.0,
            90.0,
            -1.0,
            ExitReason::StopLoss,
        ),
        make_trade(
            Direction::Buy,
            100.0,
            112.0,
            90.0,
            1.2,
            ExitReason::TakeProfit,
        ),
    ];
    let metrics = compute_metrics(
        &trades,
        0,
        86400 * 365 * 5,
        10000.0,
        0.01,
        0.0,
        0.0,
        &SwapConfig::none(),
        0.0,
    );
    assert!(metrics.enhanced_score > 0.0);
    assert!(metrics.calmar_ratio >= 0.0);
    assert!(metrics.ulcer_index >= 0.0);
    assert!(metrics.trades_significance >= 0.0 && metrics.trades_significance <= 1.0);
}

#[test]
fn test_compute_metrics_median() {
    let trades = vec![
        make_trade(
            Direction::Buy,
            100.0,
            110.0,
            90.0,
            1.0,
            ExitReason::TakeProfit,
        ),
        make_trade(
            Direction::Buy,
            100.0,
            120.0,
            90.0,
            2.0,
            ExitReason::TakeProfit,
        ),
        make_trade(
            Direction::Buy,
            100.0,
            130.0,
            90.0,
            3.0,
            ExitReason::TakeProfit,
        ),
        make_trade(
            Direction::Buy,
            100.0,
            90.0,
            90.0,
            -1.0,
            ExitReason::StopLoss,
        ),
    ];
    let metrics = compute_metrics(
        &trades,
        0,
        86400 * 365 * 5,
        10000.0,
        0.01,
        0.0,
        0.0,
        &SwapConfig::none(),
        0.0,
    );
    assert_eq!(metrics.median_win_r, 2.0);
    assert_eq!(metrics.median_loss_r, -1.0);
}

// ── SIMULATOR EDGE CASES ────────────────────────────────────────────────────

#[test]
fn test_simulator_no_signals() {
    let stop_config =
        serde_json::from_value(json!({ "type": "fixed", "stop_distance": 10.0, "start_rr": 0.0 }))
            .unwrap();
    let params = SimParams {
        symbol: "BTCUSDT",
        timeframe: Timeframe::M15,
        stop_timeframe: Timeframe::M15,
        pyramiding: false,
        initial_balance: 10000.0,
        risk_pct: 0.01,
        commission_pct: 0.0,
        commission_per_lot: 0.0,
        swap: SwapConfig::none(),
        stop_manager: &stop_config,
        strategy_params: &HashMap::new(),
        collect_trades: true,
        trading_hours: None,
    };
    let bars = vec![
        Bar {
            time: 1719000000,
            open: 100.0,
            high: 105.0,
            low: 95.0,
            close: 101.0,
            volume: 1000.0,
        },
        Bar {
            time: 1719000900,
            open: 101.0,
            high: 110.0,
            low: 100.0,
            close: 109.0,
            volume: 1100.0,
        },
    ];
    let cols = IndicatorSet::default();
    let signals: Vec<Option<Signal>> = vec![None, None];
    let symbol_info = SymbolInfo {
        symbol: "BTCUSDT".to_string(),
        point: 1.0,
        tick_value: 1.0,
        min_lot: 0.1,
        max_lot: 100.0,
        lot_step: 0.1,
        ..Default::default()
    };
    let vol_mgr = FixedPercent {
        pct: 0.01,
        initial_balance: 10000.0,
    };
    let exit_mgrs: Vec<Box<dyn ExitManager>> = vec![];

    let result = simulate(
        &params,
        &bars,
        &bars,
        &cols,
        &signals,
        &symbol_info,
        &vol_mgr,
        &exit_mgrs,
    );
    assert!(result.trades.is_empty());
    assert_eq!(result.metrics.total_trades, 0);
}

#[test]
fn test_simulator_end_of_data_close() {
    let stop_config =
        serde_json::from_value(json!({ "type": "fixed", "stop_distance": 10.0, "start_rr": 0.0 }))
            .unwrap();
    let strategy_params = HashMap::new();
    let params = SimParams {
        symbol: "BTCUSDT",
        timeframe: Timeframe::M15,
        stop_timeframe: Timeframe::M15,
        pyramiding: false,
        initial_balance: 10000.0,
        risk_pct: 0.01,
        commission_pct: 0.0,
        commission_per_lot: 0.0,
        swap: SwapConfig::none(),
        stop_manager: &stop_config,
        strategy_params: &strategy_params,
        collect_trades: true,
        trading_hours: None,
    };
    let bars = vec![
        Bar {
            time: 1719000000,
            open: 100.0,
            high: 101.0,
            low: 99.0,
            close: 100.0,
            volume: 1000.0,
        },
        Bar {
            time: 1719000900,
            open: 100.0,
            high: 101.0,
            low: 99.0,
            close: 100.0,
            volume: 1000.0,
        },
    ];
    let cols = IndicatorSet::default();
    let signals = vec![Some(Signal::new(Direction::Buy, 100.0, 95.0, 115.0)), None];
    let symbol_info = SymbolInfo {
        symbol: "BTCUSDT".to_string(),
        point: 1.0,
        tick_value: 1.0,
        min_lot: 0.1,
        max_lot: 100.0,
        lot_step: 0.1,
        ..Default::default()
    };
    let vol_mgr = FixedPercent {
        pct: 0.01,
        initial_balance: 10000.0,
    };
    let exit_mgrs: Vec<Box<dyn ExitManager>> = vec![];

    let result = simulate(
        &params,
        &bars,
        &bars,
        &cols,
        &signals,
        &symbol_info,
        &vol_mgr,
        &exit_mgrs,
    );
    assert!(!result.trades.is_empty());
    assert_eq!(result.trades[0].exit_reason, ExitReason::EndOfData);
}

// ── GRID COUNT COMBOS ──────────────────────────────────────────────────────

#[test]
fn test_count_combos_single() {
    let config_json = json!({
        "strategy": "ema_cross",
        "symbol": "BTCUSDT",
        "timeframe": "15m",
        "stop_timeframe": "timeframe",
        "pyramiding": true,
        "backtest_start": "2024-06-22",
        "backtest_end": "2024-06-23",
        "data_provider": "binance",
        "initial_balance": 100000.0,
        "risk_percentage": 0.001,
        "stop_manager": { "type": "fixed", "stop_distance": 10.0, "start_rr": 0.0 },
        "strategy_parameters": {},
        "indicators": {
            "ema_fast": { "type": "ema", "period": 9 },
            "ema_slow": { "type": "ema", "period": 21 }
        }
    });
    let config: BacktestConfig = serde_json::from_value(config_json).unwrap();
    assert_eq!(count_combos(&config).unwrap(), 1);
}

// ── BACKTEST CONFIG PARSING ─────────────────────────────────────────────────

#[test]
fn test_backtest_config_commission_defaults() {
    let config_json = json!({
        "strategy": "ema_cross",
        "symbol": "BTCUSDT",
        "timeframe": "15m",
        "stop_timeframe": "timeframe",
        "pyramiding": false,
        "backtest_start": "2024-06-22",
        "backtest_end": "2024-06-23",
        "data_provider": "binance",
        "initial_balance": 50000.0,
        "risk_percentage": 0.01,
        "stop_manager": { "type": "fixed", "stop_distance": 10.0, "start_rr": 0.0 },
        "strategy_parameters": {},
        "indicators": {}
    });
    let config: BacktestConfig = serde_json::from_value(config_json).unwrap();
    assert_eq!(config.commission_pct(), 0.0);
    assert_eq!(config.commission_per_lot_val(), 0.0);
}

#[test]
fn test_backtest_config_with_explicit_commission() {
    let config_json = json!({
        "strategy": "ema_cross",
        "symbol": "BTCUSDT",
        "timeframe": "15m",
        "stop_timeframe": "timeframe",
        "pyramiding": false,
        "backtest_start": "2024-06-22",
        "backtest_end": "2024-06-23",
        "data_provider": "binance",
        "initial_balance": 50000.0,
        "risk_percentage": 0.01,
        "commission_percent": 0.001,
        "commission_per_lot": 2.5,
        "stop_manager": { "type": "fixed", "stop_distance": 10.0, "start_rr": 0.0 },
        "strategy_parameters": {},
        "indicators": {}
    });
    let config: BacktestConfig = serde_json::from_value(config_json).unwrap();
    assert_eq!(config.commission_pct(), 0.001);
    assert_eq!(config.commission_per_lot_val(), 2.5);
}

// ── NEXT-BAR-OPEN ENTRY TESTS ───────────────────────────────────────────────

#[test]
fn test_simulator_next_bar_open_entry_price() {
    let stop_config =
        serde_json::from_value(json!({ "type": "fixed", "stop_distance": 10.0, "start_rr": 0.0 }))
            .unwrap();
    let params = SimParams {
        symbol: "BTCUSDT",
        timeframe: Timeframe::M15,
        stop_timeframe: Timeframe::M15,
        pyramiding: false,
        initial_balance: 10000.0,
        risk_pct: 0.01,
        commission_pct: 0.0,
        commission_per_lot: 0.0,
        swap: SwapConfig::none(),
        stop_manager: &stop_config,
        strategy_params: &HashMap::new(),
        collect_trades: true,
        trading_hours: None,
    };
    let bars = vec![
        Bar {
            time: 1719000000,
            open: 100.0,
            high: 103.0,
            low: 99.0,
            close: 102.0,
            volume: 1000.0,
        },
        Bar {
            time: 1719000900,
            open: 105.0,
            high: 110.0,
            low: 104.0,
            close: 109.0,
            volume: 1100.0,
        },
        Bar {
            time: 1719001800,
            open: 109.0,
            high: 115.0,
            low: 108.0,
            close: 114.0,
            volume: 1200.0,
        },
        Bar {
            time: 1719002700,
            open: 114.0,
            high: 130.0,
            low: 113.0,
            close: 125.0,
            volume: 1300.0,
        },
    ];
    let cols = IndicatorSet::default();
    let signals = vec![
        Some(Signal::new(Direction::Buy, 102.0, 90.0, 125.0)),
        None,
        None,
        None,
    ];
    let symbol_info = SymbolInfo {
        symbol: "BTCUSDT".to_string(),
        point: 1.0,
        tick_value: 1.0,
        min_lot: 0.1,
        max_lot: 100.0,
        lot_step: 0.1,
        ..Default::default()
    };
    let vol_mgr = FixedPercent {
        pct: 0.01,
        initial_balance: 10000.0,
    };
    let exit_mgrs: Vec<Box<dyn ExitManager>> = vec![];

    let result = simulate(
        &params,
        &bars,
        &bars,
        &cols,
        &signals,
        &symbol_info,
        &vol_mgr,
        &exit_mgrs,
    );
    assert!(!result.trades.is_empty());
    let trade = &result.trades[0];
    assert_eq!(trade.entry_price, 105.0, "entry must be bar[1].open");
    assert_eq!(trade.entry_time, bars[1].time);
    assert_eq!(trade.direction, Direction::Buy);
}

#[test]
fn test_simulator_gap_past_sl_skipped() {
    let stop_config =
        serde_json::from_value(json!({ "type": "fixed", "stop_distance": 10.0, "start_rr": 0.0 }))
            .unwrap();
    let params = SimParams {
        symbol: "BTCUSDT",
        timeframe: Timeframe::M15,
        stop_timeframe: Timeframe::M15,
        pyramiding: false,
        initial_balance: 10000.0,
        risk_pct: 0.01,
        commission_pct: 0.0,
        commission_per_lot: 0.0,
        swap: SwapConfig::none(),
        stop_manager: &stop_config,
        strategy_params: &HashMap::new(),
        collect_trades: true,
        trading_hours: None,
    };
    let bars = vec![
        Bar {
            time: 1719000000,
            open: 100.0,
            high: 103.0,
            low: 99.0,
            close: 102.0,
            volume: 1000.0,
        },
        Bar {
            time: 1719000900,
            open: 85.0,
            high: 87.0,
            low: 83.0,
            close: 86.0,
            volume: 1100.0,
        },
        Bar {
            time: 1719001800,
            open: 86.0,
            high: 90.0,
            low: 85.0,
            close: 88.0,
            volume: 1200.0,
        },
    ];
    let cols = IndicatorSet::default();
    let signals = vec![
        Some(Signal::new(Direction::Buy, 102.0, 90.0, 120.0)),
        None,
        None,
    ];
    let symbol_info = SymbolInfo {
        symbol: "BTCUSDT".to_string(),
        point: 1.0,
        tick_value: 1.0,
        min_lot: 0.1,
        max_lot: 100.0,
        lot_step: 0.1,
        ..Default::default()
    };
    let vol_mgr = FixedPercent {
        pct: 0.01,
        initial_balance: 10000.0,
    };
    let exit_mgrs: Vec<Box<dyn ExitManager>> = vec![];

    let result = simulate(
        &params,
        &bars,
        &bars,
        &cols,
        &signals,
        &symbol_info,
        &vol_mgr,
        &exit_mgrs,
    );
    assert!(result.trades.is_empty(), "gap past SL must prevent entry");
}

#[test]
fn test_simulator_last_bar_signal_not_entered() {
    let stop_config =
        serde_json::from_value(json!({ "type": "fixed", "stop_distance": 10.0, "start_rr": 0.0 }))
            .unwrap();
    let params = SimParams {
        symbol: "BTCUSDT",
        timeframe: Timeframe::M15,
        stop_timeframe: Timeframe::M15,
        pyramiding: false,
        initial_balance: 10000.0,
        risk_pct: 0.01,
        commission_pct: 0.0,
        commission_per_lot: 0.0,
        swap: SwapConfig::none(),
        stop_manager: &stop_config,
        strategy_params: &HashMap::new(),
        collect_trades: true,
        trading_hours: None,
    };
    let bars = vec![
        Bar {
            time: 1719000000,
            open: 100.0,
            high: 103.0,
            low: 99.0,
            close: 102.0,
            volume: 1000.0,
        },
        Bar {
            time: 1719000900,
            open: 102.0,
            high: 108.0,
            low: 101.0,
            close: 107.0,
            volume: 1100.0,
        },
    ];
    let cols = IndicatorSet::default();
    let signals = vec![None, Some(Signal::new(Direction::Buy, 107.0, 95.0, 130.0))];
    let symbol_info = SymbolInfo {
        symbol: "BTCUSDT".to_string(),
        point: 1.0,
        tick_value: 1.0,
        min_lot: 0.1,
        max_lot: 100.0,
        lot_step: 0.1,
        ..Default::default()
    };
    let vol_mgr = FixedPercent {
        pct: 0.01,
        initial_balance: 10000.0,
    };
    let exit_mgrs: Vec<Box<dyn ExitManager>> = vec![];

    let result = simulate(
        &params,
        &bars,
        &bars,
        &cols,
        &signals,
        &symbol_info,
        &vol_mgr,
        &exit_mgrs,
    );
    assert!(
        result.trades.is_empty(),
        "signal on last bar must not produce a trade"
    );
}

// ── ADDITIONAL SIMULATOR EDGE CASES ────────────────────────────────────────

#[test]
fn test_simulator_buy_sl_hit() {
    let stop_config =
        serde_json::from_value(json!({ "type": "fixed", "stop_distance": 10.0, "start_rr": 0.0 }))
            .unwrap();
    let params = SimParams {
        symbol: "BTCUSDT",
        timeframe: Timeframe::M15,
        stop_timeframe: Timeframe::M15,
        pyramiding: false,
        initial_balance: 10000.0,
        risk_pct: 0.01,
        commission_pct: 0.0,
        commission_per_lot: 0.0,
        swap: SwapConfig::none(),
        stop_manager: &stop_config,
        strategy_params: &HashMap::new(),
        collect_trades: true,
        trading_hours: None,
    };
    let bars = vec![
        Bar {
            time: 1719000000,
            open: 100.0,
            high: 103.0,
            low: 99.0,
            close: 102.0,
            volume: 1000.0,
        },
        Bar {
            time: 1719000900,
            open: 100.0,
            high: 101.0,
            low: 99.0,
            close: 100.0,
            volume: 1100.0,
        },
        Bar {
            time: 1719001800,
            open: 100.0,
            high: 101.0,
            low: 99.0,
            close: 100.0,
            volume: 1200.0,
        },
        Bar {
            time: 1719002700,
            open: 100.0,
            high: 91.0,
            low: 87.0,
            close: 89.0,
            volume: 1300.0,
        },
    ];
    let cols = IndicatorSet::default();
    let signals = vec![
        Some(Signal::new(Direction::Buy, 102.0, 90.0, 130.0)),
        None,
        None,
        None,
    ];
    let symbol_info = SymbolInfo {
        symbol: "BTCUSDT".to_string(),
        point: 1.0,
        tick_value: 1.0,
        min_lot: 0.1,
        max_lot: 100.0,
        lot_step: 0.1,
        ..Default::default()
    };
    let vol_mgr = FixedPercent {
        pct: 0.01,
        initial_balance: 10000.0,
    };
    let exit_mgrs: Vec<Box<dyn ExitManager>> = vec![];

    let result = simulate(
        &params,
        &bars,
        &bars,
        &cols,
        &signals,
        &symbol_info,
        &vol_mgr,
        &exit_mgrs,
    );
    assert!(!result.trades.is_empty());
    let trade = &result.trades[0];
    assert_eq!(trade.direction, Direction::Buy);
    assert_eq!(trade.entry_price, 100.0);
    assert_eq!(trade.exit_price, 90.0);
    assert_eq!(trade.exit_reason, ExitReason::StopLoss);
}

#[test]
fn test_simulator_sell_tp_hit() {
    let stop_config =
        serde_json::from_value(json!({ "type": "fixed", "stop_distance": 10.0, "start_rr": 0.0 }))
            .unwrap();
    let params = SimParams {
        symbol: "BTCUSDT",
        timeframe: Timeframe::M15,
        stop_timeframe: Timeframe::M15,
        pyramiding: false,
        initial_balance: 10000.0,
        risk_pct: 0.01,
        commission_pct: 0.0,
        commission_per_lot: 0.0,
        swap: SwapConfig::none(),
        stop_manager: &stop_config,
        strategy_params: &HashMap::new(),
        collect_trades: true,
        trading_hours: None,
    };
    let bars = vec![
        Bar {
            time: 1719000000,
            open: 100.0,
            high: 103.0,
            low: 99.0,
            close: 100.0,
            volume: 1000.0,
        },
        Bar {
            time: 1719000900,
            open: 100.0,
            high: 101.0,
            low: 99.0,
            close: 100.0,
            volume: 1100.0,
        },
        Bar {
            time: 1719001800,
            open: 100.0,
            high: 101.0,
            low: 99.0,
            close: 100.0,
            volume: 1200.0,
        },
        Bar {
            time: 1719002700,
            open: 100.0,
            high: 101.0,
            low: 78.0,
            close: 79.0,
            volume: 1300.0,
        },
    ];
    let cols = IndicatorSet::default();
    let signals = vec![
        Some(Signal::new(Direction::Sell, 100.0, 110.0, 80.0)),
        None,
        None,
        None,
    ];
    let symbol_info = SymbolInfo {
        symbol: "BTCUSDT".to_string(),
        point: 1.0,
        tick_value: 1.0,
        min_lot: 0.1,
        max_lot: 100.0,
        lot_step: 0.1,
        ..Default::default()
    };
    let vol_mgr = FixedPercent {
        pct: 0.01,
        initial_balance: 10000.0,
    };
    let exit_mgrs: Vec<Box<dyn ExitManager>> = vec![];

    let result = simulate(
        &params,
        &bars,
        &bars,
        &cols,
        &signals,
        &symbol_info,
        &vol_mgr,
        &exit_mgrs,
    );
    assert!(!result.trades.is_empty());
    let trade = &result.trades[0];
    assert_eq!(trade.direction, Direction::Sell);
    assert_eq!(trade.entry_price, 100.0);
    assert_eq!(trade.exit_price, 80.0);
    assert_eq!(trade.exit_reason, ExitReason::TakeProfit);
}

#[test]
fn test_simulator_gap_past_sl_sell() {
    let stop_config =
        serde_json::from_value(json!({ "type": "fixed", "stop_distance": 10.0, "start_rr": 0.0 }))
            .unwrap();
    let params = SimParams {
        symbol: "BTCUSDT",
        timeframe: Timeframe::M15,
        stop_timeframe: Timeframe::M15,
        pyramiding: false,
        initial_balance: 10000.0,
        risk_pct: 0.01,
        commission_pct: 0.0,
        commission_per_lot: 0.0,
        swap: SwapConfig::none(),
        stop_manager: &stop_config,
        strategy_params: &HashMap::new(),
        collect_trades: true,
        trading_hours: None,
    };
    let bars = vec![
        Bar {
            time: 1719000000,
            open: 100.0,
            high: 103.0,
            low: 99.0,
            close: 100.0,
            volume: 1000.0,
        },
        Bar {
            time: 1719000900,
            open: 115.0,
            high: 118.0,
            low: 114.0,
            close: 116.0,
            volume: 1100.0,
        },
        Bar {
            time: 1719001800,
            open: 116.0,
            high: 119.0,
            low: 115.0,
            close: 117.0,
            volume: 1200.0,
        },
    ];
    let cols = IndicatorSet::default();
    let signals = vec![
        Some(Signal::new(Direction::Sell, 100.0, 110.0, 80.0)),
        None,
        None,
    ];
    let symbol_info = SymbolInfo {
        symbol: "BTCUSDT".to_string(),
        point: 1.0,
        tick_value: 1.0,
        min_lot: 0.1,
        max_lot: 100.0,
        lot_step: 0.1,
        ..Default::default()
    };
    let vol_mgr = FixedPercent {
        pct: 0.01,
        initial_balance: 10000.0,
    };
    let exit_mgrs: Vec<Box<dyn ExitManager>> = vec![];

    let result = simulate(
        &params,
        &bars,
        &bars,
        &cols,
        &signals,
        &symbol_info,
        &vol_mgr,
        &exit_mgrs,
    );
    assert!(
        result.trades.is_empty(),
        "gap above SL for sell must prevent entry"
    );
}

#[test]
fn test_simulator_pyramiding_accumulates_positions() {
    let stop_config =
        serde_json::from_value(json!({ "type": "fixed", "stop_distance": 10.0, "start_rr": 0.0 }))
            .unwrap();
    let params = SimParams {
        symbol: "BTCUSDT",
        timeframe: Timeframe::M15,
        stop_timeframe: Timeframe::M15,
        pyramiding: true,
        initial_balance: 10000.0,
        risk_pct: 0.01,
        commission_pct: 0.0,
        commission_per_lot: 0.0,
        swap: SwapConfig::none(),
        stop_manager: &stop_config,
        strategy_params: &HashMap::new(),
        collect_trades: true,
        trading_hours: None,
    };
    let bars = vec![
        Bar {
            time: 1719000000,
            open: 100.0,
            high: 103.0,
            low: 99.0,
            close: 100.0,
            volume: 1000.0,
        },
        Bar {
            time: 1719000900,
            open: 100.0,
            high: 101.0,
            low: 99.0,
            close: 100.0,
            volume: 1000.0,
        },
        Bar {
            time: 1719001800,
            open: 100.0,
            high: 101.0,
            low: 99.0,
            close: 100.0,
            volume: 1000.0,
        },
        Bar {
            time: 1719002700,
            open: 100.0,
            high: 101.0,
            low: 99.0,
            close: 100.0,
            volume: 1000.0,
        },
        Bar {
            time: 1719003600,
            open: 100.0,
            high: 101.0,
            low: 99.0,
            close: 100.0,
            volume: 1000.0,
        },
        Bar {
            time: 1719004500,
            open: 100.0,
            high: 101.0,
            low: 99.0,
            close: 100.0,
            volume: 1000.0,
        },
    ];
    let cols = IndicatorSet::default();
    let signals = vec![
        Some(Signal::new(Direction::Buy, 100.0, 85.0, 200.0)),
        None,
        Some(Signal::new(Direction::Buy, 100.0, 85.0, 200.0)),
        None,
        None,
        None,
    ];
    let symbol_info = SymbolInfo {
        symbol: "BTCUSDT".to_string(),
        point: 1.0,
        tick_value: 1.0,
        min_lot: 0.1,
        max_lot: 100.0,
        lot_step: 0.1,
        ..Default::default()
    };
    let vol_mgr = FixedPercent {
        pct: 0.01,
        initial_balance: 10000.0,
    };
    let exit_mgrs: Vec<Box<dyn ExitManager>> = vec![];

    let result = simulate(
        &params,
        &bars,
        &bars,
        &cols,
        &signals,
        &symbol_info,
        &vol_mgr,
        &exit_mgrs,
    );
    assert_eq!(
        result.trades.len(),
        2,
        "pyramiding must allow two buy positions"
    );
    assert!(result.trades.iter().all(|t| t.direction == Direction::Buy));
    assert!(result
        .trades
        .iter()
        .all(|t| t.exit_reason == ExitReason::EndOfData));
}

#[test]
fn test_simulator_no_pyramiding_blocks_second_buy() {
    let stop_config =
        serde_json::from_value(json!({ "type": "fixed", "stop_distance": 10.0, "start_rr": 0.0 }))
            .unwrap();
    let params = SimParams {
        symbol: "BTCUSDT",
        timeframe: Timeframe::M15,
        stop_timeframe: Timeframe::M15,
        pyramiding: false,
        initial_balance: 10000.0,
        risk_pct: 0.01,
        commission_pct: 0.0,
        commission_per_lot: 0.0,
        swap: SwapConfig::none(),
        stop_manager: &stop_config,
        strategy_params: &HashMap::new(),
        collect_trades: true,
        trading_hours: None,
    };
    let bars = vec![
        Bar {
            time: 1719000000,
            open: 100.0,
            high: 103.0,
            low: 99.0,
            close: 100.0,
            volume: 1000.0,
        },
        Bar {
            time: 1719000900,
            open: 100.0,
            high: 101.0,
            low: 99.0,
            close: 100.0,
            volume: 1000.0,
        },
        Bar {
            time: 1719001800,
            open: 100.0,
            high: 101.0,
            low: 99.0,
            close: 100.0,
            volume: 1000.0,
        },
        Bar {
            time: 1719002700,
            open: 100.0,
            high: 101.0,
            low: 99.0,
            close: 100.0,
            volume: 1000.0,
        },
        Bar {
            time: 1719003600,
            open: 100.0,
            high: 101.0,
            low: 99.0,
            close: 100.0,
            volume: 1000.0,
        },
        Bar {
            time: 1719004500,
            open: 100.0,
            high: 101.0,
            low: 99.0,
            close: 100.0,
            volume: 1000.0,
        },
    ];
    let cols = IndicatorSet::default();
    let signals = vec![
        Some(Signal::new(Direction::Buy, 100.0, 85.0, 200.0)),
        None,
        Some(Signal::new(Direction::Buy, 100.0, 85.0, 200.0)),
        None,
        None,
        None,
    ];
    let symbol_info = SymbolInfo {
        symbol: "BTCUSDT".to_string(),
        point: 1.0,
        tick_value: 1.0,
        min_lot: 0.1,
        max_lot: 100.0,
        lot_step: 0.1,
        ..Default::default()
    };
    let vol_mgr = FixedPercent {
        pct: 0.01,
        initial_balance: 10000.0,
    };
    let exit_mgrs: Vec<Box<dyn ExitManager>> = vec![];

    let result = simulate(
        &params,
        &bars,
        &bars,
        &cols,
        &signals,
        &symbol_info,
        &vol_mgr,
        &exit_mgrs,
    );
    assert_eq!(
        result.trades.len(),
        1,
        "pyramiding=false must not allow a second buy"
    );
}

#[test]
fn test_compute_metrics_commission_per_lot_reduces_r() {
    let trades = vec![make_trade(
        Direction::Buy,
        10000.0,
        10200.0,
        9900.0,
        2.0,
        ExitReason::TakeProfit,
    )];
    let no_comm = compute_metrics(
        &trades,
        0,
        86400 * 365 * 5,
        10000.0,
        0.01,
        0.0,
        0.0,
        &SwapConfig::none(),
        0.0,
    );
    let with_comm = compute_metrics(
        &trades,
        0,
        86400 * 365 * 5,
        10000.0,
        0.01,
        0.0,
        5.0,
        &SwapConfig::none(),
        0.0,
    );
    assert!(with_comm.total_r < no_comm.total_r);
    assert!(with_comm.net_profit < no_comm.net_profit);
}

#[test]
fn test_compute_metrics_zero_trades_final_balance_unchanged() {
    let m = compute_metrics(
        &[],
        0,
        86400 * 365 * 5,
        50000.0,
        0.01,
        0.001,
        0.0,
        &SwapConfig::none(),
        0.0,
    );
    assert_eq!(m.final_balance, 50000.0);
    assert_eq!(m.total_trades, 0);
    assert_eq!(m.win_rate, 0.0);
    assert_eq!(m.profit_factor, 0.0);
    assert_eq!(m.max_drawdown, 0.0);
}

#[test]
fn test_simulator_collect_trades_false_returns_empty_vec() {
    let stop_config =
        serde_json::from_value(json!({ "type": "fixed", "stop_distance": 10.0, "start_rr": 0.0 }))
            .unwrap();
    let params = SimParams {
        symbol: "BTCUSDT",
        timeframe: Timeframe::M15,
        stop_timeframe: Timeframe::M15,
        pyramiding: false,
        initial_balance: 10000.0,
        risk_pct: 0.01,
        commission_pct: 0.0,
        commission_per_lot: 0.0,
        swap: SwapConfig::none(),
        stop_manager: &stop_config,
        strategy_params: &HashMap::new(),
        collect_trades: false,
        trading_hours: None,
    };
    let bars = vec![
        Bar {
            time: 1719000000,
            open: 100.0,
            high: 103.0,
            low: 99.0,
            close: 100.0,
            volume: 1000.0,
        },
        Bar {
            time: 1719000900,
            open: 100.0,
            high: 130.0,
            low: 99.0,
            close: 125.0,
            volume: 1000.0,
        },
    ];
    let cols = IndicatorSet::default();
    let signals = vec![Some(Signal::new(Direction::Buy, 100.0, 85.0, 120.0)), None];
    let symbol_info = SymbolInfo {
        symbol: "BTCUSDT".to_string(),
        point: 1.0,
        tick_value: 1.0,
        min_lot: 0.1,
        max_lot: 100.0,
        lot_step: 0.1,
        ..Default::default()
    };
    let vol_mgr = FixedPercent {
        pct: 0.01,
        initial_balance: 10000.0,
    };
    let exit_mgrs: Vec<Box<dyn ExitManager>> = vec![];

    let result = simulate(
        &params,
        &bars,
        &bars,
        &cols,
        &signals,
        &symbol_info,
        &vol_mgr,
        &exit_mgrs,
    );
    assert!(
        result.trades.is_empty(),
        "collect_trades=false must produce empty trades vec"
    );
}

// ── HELPER ─────────────────────────────────────────────────────────────────

fn make_trade(
    dir: Direction,
    entry: f64,
    exit: f64,
    sl: f64,
    r: f64,
    reason: ExitReason,
) -> TradeRecord {
    TradeRecord {
        trade_id: 1,
        strategy_id: 1,
        symbol: "BTCUSDT".to_string(),
        direction: dir,
        entry_price: entry,
        exit_price: exit,
        initial_stop_loss: sl,
        current_stop_loss: sl,
        take_profit: 0.0,
        volume: 1.0,
        open_risk: (entry - sl).abs(),
        entry_time: 1719000000,
        exit_time: 1719003600,
        exit_reason: reason,
        profit: r,
        currency_pnl: r * (entry - sl).abs(),
        group_id: 1,
    }
}

#[test]
fn test_simulator_stop_timeframe_sub_bars() {
    let stop_config = serde_json::from_value(json!({
        "type": "fixed", "stop_distance": 10.0, "start_rr": 0.0
    }))
    .unwrap();
    let params = SimParams {
        symbol: "BTCUSDT",
        timeframe: Timeframe::M30,
        stop_timeframe: Timeframe::M15,
        pyramiding: false,
        initial_balance: 10000.0,
        risk_pct: 0.01,
        commission_pct: 0.0,
        commission_per_lot: 0.0,
        swap: SwapConfig::none(),
        stop_manager: &stop_config,
        strategy_params: &HashMap::new(),
        collect_trades: true,
        trading_hours: None,
    };
    let bars = vec![
        Bar {
            time: 1719000000,
            open: 100.0,
            high: 102.0,
            low: 98.0,
            close: 101.0,
            volume: 1000.0,
        },
        Bar {
            time: 1719001800,
            open: 101.0,
            high: 102.0,
            low: 98.0,
            close: 101.0,
            volume: 1100.0,
        },
    ];
    let stop_bars = vec![
        Bar {
            time: 1719000000,
            open: 100.0,
            high: 102.0,
            low: 98.0,
            close: 101.0,
            volume: 1000.0,
        },
        Bar {
            time: 1719000900,
            open: 101.0,
            high: 102.0,
            low: 98.0,
            close: 101.0,
            volume: 500.0,
        },
        Bar {
            time: 1719001800,
            open: 101.0,
            high: 102.0,
            low: 98.0,
            close: 101.0,
            volume: 600.0,
        },
        Bar {
            time: 1719002700,
            open: 101.0,
            high: 102.0,
            low: 85.0,
            close: 100.0,
            volume: 500.0,
        },
    ];
    let cols = IndicatorSet::default();
    let signals = vec![Some(Signal::new(Direction::Buy, 101.0, 91.0, 200.0)), None];
    let symbol_info = SymbolInfo {
        symbol: "BTCUSDT".to_string(),
        point: 1.0,
        tick_value: 1.0,
        min_lot: 0.1,
        max_lot: 100.0,
        lot_step: 0.1,
        ..Default::default()
    };
    let vol_mgr = FixedPercent {
        pct: 0.01,
        initial_balance: 10000.0,
    };
    let exit_mgrs: Vec<Box<dyn ExitManager>> = vec![];

    let result = simulate(
        &params,
        &bars,
        &stop_bars,
        &cols,
        &signals,
        &symbol_info,
        &vol_mgr,
        &exit_mgrs,
    );
    assert!(!result.trades.is_empty());
    let trade = &result.trades[0];
    assert_eq!(trade.direction, Direction::Buy);
    assert_eq!(trade.entry_price, 101.0);
    assert_eq!(trade.exit_price, 91.0);
    assert_eq!(trade.exit_time, 1719002700);
    assert_eq!(trade.exit_reason, ExitReason::StopLoss);
}

#[test]
fn test_canonical_value() {
    use backtest::canonical_value;
    use serde_json::json;
    assert_eq!(canonical_value(&json!(123)), "123.00000000");
    assert_eq!(canonical_value(&json!(1.5)), "1.50000000");
    assert_eq!(canonical_value(&json!([1.5, 2])), "[1.50000000,2.00000000]");
    let obj = json!({ "z": 1, "a": 1.5 });
    assert_eq!(canonical_value(&obj), "{a:1.50000000,z:1.00000000}");
}

#[test]
fn test_expand_sample_values() {
    use backtest::expand_sample_values;
    use serde_json::json;
    let range_obj = json!({ "$sample": "range", "start": 1.0, "stop": 3.0, "step": 0.5 })
        .as_object()
        .unwrap()
        .clone();
    let res = expand_sample_values(&range_obj).unwrap();
    assert_eq!(res, vec![json!(1.0), json!(1.5), json!(2.0), json!(2.5)]);

    let linspace_obj = json!({ "$sample": "linspace", "start": 1.0, "stop": 2.0, "n": 3 })
        .as_object()
        .unwrap()
        .clone();
    let res = expand_sample_values(&linspace_obj).unwrap();
    assert_eq!(res, vec![json!(1.0), json!(1.5), json!(2.0)]);

    let log_obj = json!({ "$sample": "log", "start": 1.0, "stop": 100.0, "n": 3 })
        .as_object()
        .unwrap()
        .clone();
    let res = expand_sample_values(&log_obj).unwrap();
    assert_eq!(res.len(), 3);
    assert!((res[0].as_f64().unwrap() - 1.0).abs() < 1e-5);
    assert!((res[1].as_f64().unwrap() - 10.0).abs() < 1e-5);
    assert!((res[2].as_f64().unwrap() - 100.0).abs() < 1e-5);

    let values_obj = json!({ "$sample": "values", "items": ["a", 1.5] })
        .as_object()
        .unwrap()
        .clone();
    let res = expand_sample_values(&values_obj).unwrap();
    assert_eq!(res, vec![json!("a"), json!(1.5)]);
}

#[test]
fn test_compute_metrics_swap_per_lot_reduces_net_profit_for_long() {
    let trades = vec![TradeRecord {
        trade_id: 1,
        strategy_id: 1,
        symbol: "EURUSD".to_string(),
        direction: Direction::Buy,
        entry_price: 1.1000,
        exit_price: 1.1050,
        initial_stop_loss: 1.0950,
        current_stop_loss: 1.0950,
        take_profit: 1.1100,
        volume: 1.0,
        open_risk: 0.0050,
        entry_time: 1_780_300_800,
        exit_time: 1_780_300_800 + 3 * 86_400,
        exit_reason: ExitReason::TakeProfit,
        profit: 1.0,
        currency_pnl: 0.0050,
        group_id: 1,
    }];
    let no_swap = compute_metrics(
        &trades,
        0,
        86400 * 365 * 5,
        10_000.0,
        0.01,
        0.0,
        0.0,
        &SwapConfig::none(),
        0.0,
    );
    let with_swap = compute_metrics(
        &trades,
        0,
        86400 * 365 * 5,
        10_000.0,
        0.01,
        0.0,
        0.0,
        &SwapConfig {
            long_per_lot: -6.5,
            ..Default::default()
        },
        0.0,
    );
    assert!(with_swap.net_profit < no_swap.net_profit);
    assert!(with_swap.total_swap_cost < 0.0);
}

#[test]
fn test_compute_metrics_swap_credit_increases_net_profit_for_short() {
    let trades = vec![TradeRecord {
        trade_id: 1,
        strategy_id: 1,
        symbol: "EURUSD".to_string(),
        direction: Direction::Sell,
        entry_price: 1.1000,
        exit_price: 1.0950,
        initial_stop_loss: 1.1050,
        current_stop_loss: 1.1050,
        take_profit: 1.0900,
        volume: 1.0,
        open_risk: 0.0050,
        entry_time: 1_780_300_800,
        exit_time: 1_780_300_800 + 2 * 86_400,
        exit_reason: ExitReason::TakeProfit,
        profit: 1.0,
        currency_pnl: 0.0050,
        group_id: 1,
    }];
    let swap = SwapConfig {
        short_per_lot: 1.2,
        ..Default::default()
    };
    let with_swap = compute_metrics(
        &trades,
        0,
        86400 * 365 * 5,
        10_000.0,
        0.01,
        0.0,
        0.0,
        &swap,
        0.0,
    );
    assert!(with_swap.total_swap_cost > 0.0);
}
