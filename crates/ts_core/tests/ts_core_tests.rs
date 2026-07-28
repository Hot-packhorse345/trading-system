use serde_json::Value;
use std::collections::HashMap;
use ts_core::{
    parse_iso_date, parse_iso_date_end, parse_timeframe, Bar, CircularBuffer, Direction,
    ExitReason, IndicatorSet, Params, Position, Signal, Timeframe, TradeRecord,
};

// ── BAR TESTS ────────────────────────────────────────────────────────────────

#[test]
fn test_bar_utilities() {
    let bar = Bar::new(1719000000, 100.0, 105.0, 95.0, 102.0, 1000.0);
    assert_eq!(bar.mid(), 100.0);
    assert_eq!(bar.typical(), 100.66666666666667);
}

#[test]
fn test_bar_true_range() {
    let bar = Bar::new(1719000000, 100.0, 105.0, 95.0, 102.0, 1000.0);

    // High-Low is largest: 105 - 95 = 10
    assert_eq!(bar.true_range(100.0), 10.0);

    // High - PrevClose is largest: 105 - 90 = 15
    assert_eq!(bar.true_range(90.0), 15.0);

    // PrevClose - Low is largest: 110 - 95 = 15
    assert_eq!(bar.true_range(110.0), 15.0);
}

#[test]
fn test_bar_true_range_edge_cases() {
    let bar = Bar::new(1719000000, 100.0, 100.0, 100.0, 100.0, 1000.0);
    // Flat bar
    assert_eq!(bar.true_range(100.0), 0.0);
}

// ── CIRCULAR BUFFER TESTS ───────────────────────────────────────────────────

#[test]
fn test_circular_buffer_basic() {
    let mut buf = CircularBuffer::new(3);
    assert!(buf.is_empty());
    assert_eq!(buf.len(), 0);

    let b1 = Bar::new(1, 10.0, 12.0, 9.0, 11.0, 100.0);
    let b2 = Bar::new(2, 11.0, 13.0, 10.0, 12.0, 100.0);

    buf.push(b1);
    assert_eq!(buf.len(), 1);
    assert!(!buf.is_empty());
    assert_eq!(buf.as_vec(), vec![b1]);

    buf.push(b2);
    assert_eq!(buf.len(), 2);
    assert_eq!(buf.as_vec(), vec![b1, b2]);
}

#[test]
fn test_circular_buffer_overflow() {
    let mut buf = CircularBuffer::new(3);
    let b1 = Bar::new(1, 1.0, 1.0, 1.0, 1.0, 1.0);
    let b2 = Bar::new(2, 2.0, 2.0, 2.0, 2.0, 2.0);
    let b3 = Bar::new(3, 3.0, 3.0, 3.0, 3.0, 3.0);
    let b4 = Bar::new(4, 4.0, 4.0, 4.0, 4.0, 4.0);

    buf.push(b1);
    buf.push(b2);
    buf.push(b3);
    assert_eq!(buf.len(), 3);
    assert_eq!(buf.as_vec(), vec![b1, b2, b3]);

    // Push 4th element (should overwrite b1 at write index 0)
    buf.push(b4);
    assert_eq!(buf.len(), 3);
    // elements should be in chronological/insertion order: b2, b3, b4
    assert_eq!(buf.as_vec(), vec![b2, b3, b4]);
}

#[test]
fn test_circular_buffer_capacity_one() {
    let mut buf = CircularBuffer::new(1);
    let b1 = Bar::new(1, 1.0, 1.0, 1.0, 1.0, 1.0);
    let b2 = Bar::new(2, 2.0, 2.0, 2.0, 2.0, 2.0);

    buf.push(b1);
    assert_eq!(buf.len(), 1);
    assert_eq!(buf.as_vec(), vec![b1]);

    buf.push(b2);
    assert_eq!(buf.len(), 1);
    assert_eq!(buf.as_vec(), vec![b2]);
}

#[test]
fn test_circular_buffer_capacity_zero() {
    // Capacity of 0 should clamp to 1 internally
    let mut buf = CircularBuffer::new(0);
    let b1 = Bar::new(1, 1.0, 1.0, 1.0, 1.0, 1.0);
    let b2 = Bar::new(2, 2.0, 2.0, 2.0, 2.0, 2.0);

    buf.push(b1);
    assert_eq!(buf.len(), 1);
    assert_eq!(buf.as_vec(), vec![b1]);

    buf.push(b2);
    assert_eq!(buf.len(), 1);
    assert_eq!(buf.as_vec(), vec![b2]);
}

// ── TIMEFRAME TESTS ──────────────────────────────────────────────────────────

#[test]
fn test_timeframe_seconds() {
    assert_eq!(Timeframe::M1.seconds(), 60);
    assert_eq!(Timeframe::M5.seconds(), 300);
    assert_eq!(Timeframe::H1.seconds(), 3600);
    assert_eq!(Timeframe::D1.seconds(), 86400);
    assert_eq!(Timeframe::TICK.try_seconds(), None);
}

#[test]
#[should_panic(expected = "TICK timeframe has no fixed bar duration")]
fn test_timeframe_tick_panic() {
    let _ = Timeframe::TICK.seconds();
}

#[test]
fn test_timeframe_to_str_and_binance() {
    assert_eq!(Timeframe::M15.to_str(), "M15");
    assert_eq!(Timeframe::M15.to_binance(), "15m");
    assert_eq!(Timeframe::H4.to_str(), "H4");
    assert_eq!(Timeframe::H4.to_binance(), "4h");
    assert_eq!(Timeframe::TICK.to_str(), "TICK");
    assert_eq!(Timeframe::TICK.to_binance(), "1s");
}

#[test]
fn test_timeframe_parsing() {
    assert_eq!(parse_timeframe("1m").unwrap(), Timeframe::M1);
    assert_eq!(parse_timeframe("M1").unwrap(), Timeframe::M1);
    assert_eq!(parse_timeframe("5m").unwrap(), Timeframe::M5);
    assert_eq!(parse_timeframe("15m").unwrap(), Timeframe::M15);
    assert_eq!(parse_timeframe("1h").unwrap(), Timeframe::H1);
    assert_eq!(parse_timeframe("D1").unwrap(), Timeframe::D1);
    assert_eq!(parse_timeframe("1w").unwrap(), Timeframe::W1);
    assert_eq!(parse_timeframe("1M").unwrap(), Timeframe::MN1);

    // Errors
    assert!(parse_timeframe("99m").is_err());
    assert!(parse_timeframe("invalid").is_err());
}

// ── DATE PARSING TESTS ────────────────────────────────────────────────────────

#[test]
fn test_parse_iso_date() {
    // RFC 3339
    assert_eq!(parse_iso_date("2024-06-22T12:00:00Z").unwrap(), 1719057600);

    // Naive datetimes (assumed UTC)
    assert_eq!(parse_iso_date("2024-06-22T12:00:00").unwrap(), 1719057600);
    assert_eq!(parse_iso_date("2024-06-22 12:00:00").unwrap(), 1719057600);
    assert_eq!(parse_iso_date("2024-06-22T12:00").unwrap(), 1719057600);

    // Date only defaults to midnight
    assert_eq!(parse_iso_date("2024-06-22").unwrap(), 1719014400);

    // Invalid format
    assert!(parse_iso_date("2024/06/22").is_err());
    assert!(parse_iso_date("invalid").is_err());
}

#[test]
fn test_parse_iso_date_end() {
    // RFC 3339 (should parse normally)
    assert_eq!(
        parse_iso_date_end("2024-06-22T12:00:00Z").unwrap(),
        1719057600
    );

    // Date only defaults to end of day (23:59:59)
    assert_eq!(parse_iso_date_end("2024-06-22").unwrap(), 1719100799); // 1719014400 + 86399
}

// ── PARAMS & INDICATOR SET TESTS ─────────────────────────────────────────────

#[test]
fn test_params_retrieval() {
    let mut map = HashMap::new();
    map.insert("f64_val".to_string(), Value::from(3.14));
    map.insert("u64_val".to_string(), Value::from(42u64));
    map.insert("bool_val".to_string(), Value::from(true));
    map.insert("str_val".to_string(), Value::from("hello"));

    let params = Params(map);

    assert_eq!(params.f64("f64_val"), Some(3.14));
    assert_eq!(params.f64_or("f64_val", 0.0), 3.14);
    assert_eq!(params.f64_or("non_existent", 1.5), 1.5);

    assert_eq!(params.u64("u64_val"), Some(42));
    assert_eq!(params.u64_or("u64_val", 0), 42);
    assert_eq!(params.u64_or("non_existent", 10), 10);

    assert_eq!(params.bool_or("bool_val", false), true);
    assert_eq!(params.bool_or("non_existent", false), false);

    assert_eq!(params.str_or("str_val", "default"), "hello");
    assert_eq!(params.str_or("non_existent", "default"), "default");
}

#[test]
fn test_indicator_set() {
    let mut set = IndicatorSet::default();
    set.insert("rsi", vec![50.0, 55.0, 60.0]);

    assert!(set.contains("rsi"));
    assert!(!set.contains("macd"));
    assert_eq!(set.get("rsi"), &[50.0, 55.0, 60.0]);
    assert_eq!(set.try_get("rsi"), Some(&[50.0, 55.0, 60.0][..]));
    assert_eq!(set.try_get("macd"), None);

    let names: Vec<&str> = set.column_names().collect();
    assert_eq!(names, vec!["rsi"]);
}

#[test]
#[should_panic(expected = "IndicatorSet: missing column 'missing'")]
fn test_indicator_set_panic() {
    let set = IndicatorSet::default();
    let _ = set.get("missing");
}

// ── POSITION & TRADE RECORD TESTS ────────────────────────────────────────────

#[test]
fn test_position_calculations() {
    let pos = Position {
        trade_id: 1,
        strategy_id: 10,
        direction: Direction::Buy,
        entry_price: 100.0,
        initial_stop_loss: 90.0,
        current_stop_loss: 92.0,
        take_profit: 130.0,
        volume: 2.0,
        open_risk: 20.0,
        entry_time: 1719000000,
        is_split_chunk: false,
        group_id: 0,
    };

    // R-multiple = (price - entry) / risk
    // Risk = 100 - 90 = 10
    // Price = 110 => (110 - 100) / 10 = 1.0 R
    assert_eq!(pos.r_multiple(110.0), 1.0);
    assert_eq!(pos.r_multiple(80.0), -2.0);

    // P&L = (price - entry) * volume
    // Price = 110 => (110 - 100) * 2 = 20.0
    assert_eq!(pos.pnl(110.0), 20.0);

    // Stop Loss / Take Profit checks
    assert!(pos.tp_hit(130.0, 120.0));
    assert!(!pos.tp_hit(129.0, 120.0));

    assert!(pos.sl_hit(100.0, 92.0));
    assert!(!pos.sl_hit(100.0, 92.1));
}

#[test]
fn test_sell_position_calculations() {
    let pos = Position {
        trade_id: 2,
        strategy_id: 10,
        direction: Direction::Sell,
        entry_price: 100.0,
        initial_stop_loss: 110.0,
        current_stop_loss: 108.0,
        take_profit: 70.0,
        volume: 2.0,
        open_risk: 20.0,
        entry_time: 1719000000,
        is_split_chunk: false,
        group_id: 0,
    };

    // Risk = |100 - 110| = 10
    // Price = 90 => (100 - 90) / 10 = 1.0 R
    assert_eq!(pos.r_multiple(90.0), 1.0);
    assert_eq!(pos.r_multiple(120.0), -2.0);

    // P&L = (entry - price) * volume
    assert_eq!(pos.pnl(90.0), 20.0);

    // Stop Loss / Take Profit checks
    assert!(pos.tp_hit(110.0, 70.0));
    assert!(!pos.tp_hit(110.0, 70.1));

    assert!(pos.sl_hit(108.0, 90.0));
    assert!(!pos.sl_hit(107.9, 90.0));
}

#[test]
fn test_zero_risk_position_r_multiple() {
    let mut pos = Position {
        trade_id: 1,
        strategy_id: 10,
        direction: Direction::Buy,
        entry_price: 100.0,
        initial_stop_loss: 100.0, // Risk is 0
        current_stop_loss: 100.0,
        take_profit: 130.0,
        volume: 2.0,
        open_risk: 0.0,
        entry_time: 1719000000,
        is_split_chunk: false,
        group_id: 0,
    };

    assert_eq!(pos.r_multiple(110.0), 0.0);

    // Hold direction
    pos.direction = Direction::Hold;
    assert_eq!(pos.r_multiple(110.0), 0.0);
    assert_eq!(pos.pnl(110.0), 0.0);
    assert!(!pos.tp_hit(150.0, 50.0));
    assert!(!pos.sl_hit(150.0, 50.0));
}

#[test]
fn test_trade_record_close() {
    let pos = Position {
        trade_id: 1,
        strategy_id: 10,
        direction: Direction::Buy,
        entry_price: 100.0,
        initial_stop_loss: 90.0,
        current_stop_loss: 95.0,
        take_profit: 130.0,
        volume: 1.5,
        open_risk: 15.0,
        entry_time: 1719000000,
        is_split_chunk: false,
        group_id: 5,
    };

    let rec =
        TradeRecord::close_position(&pos, "BTCUSDT", 110.0, 1719003600, ExitReason::TakeProfit);
    assert_eq!(rec.trade_id, 1);
    assert_eq!(rec.symbol, "BTCUSDT");
    assert_eq!(rec.profit, 1.0); // (110 - 100)/10
    assert_eq!(rec.currency_pnl, 15.0); // (110 - 100) * 1.5
    assert_eq!(rec.exit_time, 1719003600);
    assert_eq!(rec.group_id, 5);
}

#[test]
fn test_trade_record_from_open() {
    let pos = Position {
        trade_id: 1,
        strategy_id: 10,
        direction: Direction::Buy,
        entry_price: 100.0,
        initial_stop_loss: 90.0,
        current_stop_loss: 95.0,
        take_profit: 130.0,
        volume: 1.5,
        open_risk: 15.0,
        entry_time: 1719000000,
        is_split_chunk: false,
        group_id: 5,
    };

    let rec = TradeRecord::from_open_position(&pos, "BTCUSDT");
    assert_eq!(rec.trade_id, 1);
    assert_eq!(rec.symbol, "BTCUSDT");
    assert_eq!(rec.exit_price, 0.0);
    assert_eq!(rec.exit_time, 0);
    assert_eq!(rec.profit, 0.0);
    assert_eq!(rec.currency_pnl, 0.0);
}

// ── EXTRA COVERAGE FOR ENUMS & SIGNAL ───────────────────────────────────────

#[test]
fn test_direction_helpers() {
    assert_eq!(Direction::Buy.opposite(), Direction::Sell);
    assert_eq!(Direction::Sell.opposite(), Direction::Buy);
    assert_eq!(Direction::Hold.opposite(), Direction::Hold);

    assert!(Direction::Buy.is_long());
    assert!(!Direction::Buy.is_short());
    assert!(!Direction::Buy.is_hold());

    assert!(Direction::Sell.is_short());
    assert!(!Direction::Sell.is_long());
    assert!(!Direction::Sell.is_hold());

    assert!(Direction::Hold.is_hold());
    assert!(!Direction::Hold.is_long());
    assert!(!Direction::Hold.is_short());
}

#[test]
fn test_all_timeframe_methods() {
    let tfs = vec![
        (Timeframe::M1, "M1", "1m", Some(60)),
        (Timeframe::M2, "M2", "2m", Some(120)),
        (Timeframe::M3, "M3", "3m", Some(180)),
        (Timeframe::M5, "M5", "5m", Some(300)),
        (Timeframe::M6, "M6", "6m", Some(360)),
        (Timeframe::M10, "M10", "10m", Some(600)),
        (Timeframe::M12, "M12", "12m", Some(720)),
        (Timeframe::M15, "M15", "15m", Some(900)),
        (Timeframe::M20, "M20", "20m", Some(1200)),
        (Timeframe::M30, "M30", "30m", Some(1800)),
        (Timeframe::H1, "H1", "1h", Some(3600)),
        (Timeframe::H2, "H2", "2h", Some(7200)),
        (Timeframe::H3, "H3", "3h", Some(10800)),
        (Timeframe::H4, "H4", "4h", Some(14400)),
        (Timeframe::H6, "H6", "6h", Some(21600)),
        (Timeframe::H8, "H8", "8h", Some(28800)),
        (Timeframe::H12, "H12", "12h", Some(43200)),
        (Timeframe::D1, "D1", "1d", Some(86400)),
        (Timeframe::W1, "W1", "1w", Some(604800)),
        (Timeframe::MN1, "MN1", "1M", Some(2592000)),
        (Timeframe::TICK, "TICK", "1s", None),
    ];

    for (tf, s, b, secs) in tfs {
        assert_eq!(tf.to_str(), s);
        assert_eq!(tf.to_binance(), b);
        assert_eq!(tf.try_seconds(), secs);
        assert_eq!(format!("{}", tf), s);
        if tf != Timeframe::TICK {
            assert_eq!(s.parse::<Timeframe>().unwrap(), tf);
        }
    }
}

#[test]
fn test_signal_creation() {
    let sig = Signal::new(Direction::Buy, 100.0, 95.0, 110.0);
    assert_eq!(sig.direction, Direction::Buy);
    assert_eq!(sig.entry_price, 100.0);
    assert_eq!(sig.stop_loss, 95.0);
    assert_eq!(sig.take_profit, 110.0);
}

// ── SYMBOL INFO TESTS ───────────────────────────────────────────────────────

#[test]
fn test_symbol_info_round_lot() {
    let sym = ts_core::SymbolInfo {
        lot_step: 0.01,
        min_lot: 0.1,
        max_lot: 10.0,
        ..Default::default()
    };
    assert_eq!(sym.round_lot(0.55), 0.55);
    assert_eq!(sym.round_lot(0.556), 0.56);
    assert_eq!(sym.round_lot(0.05), 0.1); // clamps to min_lot
    assert_eq!(sym.round_lot(15.0), 10.0); // clamps to max_lot
    assert_eq!(sym.round_lot(0.1), 0.1); // exact min_lot
    assert_eq!(sym.round_lot(10.0), 10.0); // exact max_lot
}

#[test]
fn test_symbol_info_round_lot_large_step() {
    let sym = ts_core::SymbolInfo {
        lot_step: 0.1,
        min_lot: 0.1,
        max_lot: 100.0,
        ..Default::default()
    };
    assert!((sym.round_lot(0.36) - 0.4).abs() < 1e-9);
    assert!((sym.round_lot(0.34) - 0.3).abs() < 1e-9);
    assert!((sym.round_lot(1.0) - 1.0).abs() < 1e-9);
}

#[test]
fn test_symbol_info_mid() {
    let sym = ts_core::SymbolInfo {
        ask: 100.5,
        bid: 99.5,
        ..Default::default()
    };
    assert_eq!(sym.mid(), 100.0);

    let sym2 = ts_core::SymbolInfo {
        ask: 50000.0,
        bid: 50000.0,
        ..Default::default()
    };
    assert_eq!(sym2.mid(), 50000.0);
}

// ── SIGNAL VALIDITY TESTS ───────────────────────────────────────────────────

#[test]
fn test_signal_is_valid() {
    // Valid buy: entry > stop, tp > entry
    let buy = Signal::new(Direction::Buy, 100.0, 95.0, 110.0);
    assert!(buy.is_valid());

    // Valid sell: entry < stop, tp < entry
    let sell = Signal::new(Direction::Sell, 100.0, 105.0, 90.0);
    assert!(sell.is_valid());

    // Valid buy with no TP (tp = 0.0)
    let buy_no_tp = Signal::new(Direction::Buy, 100.0, 95.0, 0.0);
    assert!(buy_no_tp.is_valid());

    // Valid sell with no TP (tp = 0.0)
    let sell_no_tp = Signal::new(Direction::Sell, 100.0, 105.0, 0.0);
    assert!(sell_no_tp.is_valid());

    // Invalid buy: entry == stop
    let invalid_buy = Signal::new(Direction::Buy, 100.0, 100.0, 110.0);
    assert!(!invalid_buy.is_valid());

    // Invalid sell: entry == stop
    let invalid_sell = Signal::new(Direction::Sell, 100.0, 100.0, 90.0);
    assert!(!invalid_sell.is_valid());

    // Invalid buy: entry < stop
    let bad_buy = Signal::new(Direction::Buy, 100.0, 105.0, 110.0);
    assert!(!bad_buy.is_valid());

    // Invalid sell: entry > stop
    let bad_sell = Signal::new(Direction::Sell, 100.0, 95.0, 90.0);
    assert!(!bad_sell.is_valid());

    // Invalid buy: tp < entry
    let bad_tp_buy = Signal::new(Direction::Buy, 100.0, 95.0, 90.0);
    assert!(!bad_tp_buy.is_valid());

    // Invalid sell: tp > entry
    let bad_tp_sell = Signal::new(Direction::Sell, 100.0, 105.0, 110.0);
    assert!(!bad_tp_sell.is_valid());

    // Hold is always invalid
    let hold = Signal::new(Direction::Hold, 100.0, 95.0, 110.0);
    assert!(!hold.is_valid());
}

// ── TRADE RECORD R-MULTIPLE & PNL ──────────────────────────────────────────

#[test]
fn test_trade_record_r_multiple() {
    let rec = TradeRecord {
        trade_id: 1,
        strategy_id: 1,
        symbol: "BTC".to_string(),
        direction: Direction::Buy,
        entry_price: 100.0,
        exit_price: 110.0,
        initial_stop_loss: 90.0,
        current_stop_loss: 95.0,
        take_profit: 120.0,
        volume: 1.0,
        open_risk: 10.0,
        entry_time: 1000,
        exit_time: 2000,
        exit_reason: ExitReason::TakeProfit,
        profit: 1.0,
        currency_pnl: 10.0,
        group_id: 0,
    };
    assert_eq!(rec.r_multiple(), 1.0); // (110-100)/(100-90) = 1.0

    let rec_sell = TradeRecord {
        direction: Direction::Sell,
        entry_price: 100.0,
        exit_price: 90.0,
        initial_stop_loss: 110.0,
        ..rec.clone()
    };
    assert_eq!(rec_sell.r_multiple(), 1.0); // (100-90)/(110-100) = 1.0
}

#[test]
fn test_trade_record_pnl() {
    let rec = TradeRecord {
        trade_id: 1,
        strategy_id: 1,
        symbol: "BTC".to_string(),
        direction: Direction::Buy,
        entry_price: 100.0,
        exit_price: 110.0,
        initial_stop_loss: 90.0,
        current_stop_loss: 95.0,
        take_profit: 120.0,
        volume: 2.0,
        open_risk: 10.0,
        entry_time: 1000,
        exit_time: 2000,
        exit_reason: ExitReason::TakeProfit,
        profit: 1.0,
        currency_pnl: 20.0,
        group_id: 0,
    };
    assert_eq!(rec.pnl(), 20.0); // (110-100) * 2.0
}

// ── CIRCULAR BUFFER EXTRA TESTS ────────────────────────────────────────────

#[test]
fn test_circular_buffer_double_overflow() {
    let mut buf = CircularBuffer::new(2);
    for i in 0..10 {
        buf.push(Bar::new(i, i as f64, i as f64, i as f64, i as f64, 1.0));
    }
    assert_eq!(buf.len(), 2);
    let v = buf.as_vec();
    assert_eq!(v[0].time, 8);
    assert_eq!(v[1].time, 9);
}

// ── PARAMS EDGE CASES ──────────────────────────────────────────────────────

#[test]
fn test_params_integer_as_f64() {
    let mut map = HashMap::new();
    map.insert("int_val".to_string(), Value::from(42u64));
    let params = Params(map);
    // u64 value should also parse as f64
    assert_eq!(params.f64("int_val"), Some(42.0));
}

#[test]
fn test_params_default() {
    let params = Params::default();
    assert_eq!(params.f64("missing"), None);
    assert_eq!(params.u64("missing"), None);
    assert_eq!(params.f64_or("missing", 3.14), 3.14);
    assert_eq!(params.u64_or("missing", 99), 99);
    assert_eq!(params.bool_or("missing", true), true);
    assert_eq!(params.str_or("missing", "fallback"), "fallback");
}

// ── INDICATOR SET MERGE ─────────────────────────────────────────────────────

#[test]
fn test_indicator_set_overwrite() {
    let mut set = IndicatorSet::default();
    set.insert("col", vec![1.0, 2.0]);
    set.insert("col", vec![3.0, 4.0, 5.0]);
    assert_eq!(set.get("col"), &[3.0, 4.0, 5.0]);
}

// ── EXIT REASON SERDE ───────────────────────────────────────────────────────

#[test]
fn test_all_exit_reasons_serde() {
    let reasons = vec![
        ExitReason::StopLoss,
        ExitReason::StopProfit,
        ExitReason::TakeProfit,
        ExitReason::ExitRule,
        ExitReason::EndOfData,
    ];
    for reason in reasons {
        let s = serde_json::to_string(&reason).unwrap();
        let d: ExitReason = serde_json::from_str(&s).unwrap();
        assert_eq!(d, reason);
    }
}

// ── DIRECTION SERDE ─────────────────────────────────────────────────────────

#[test]
fn test_all_directions_serde() {
    for dir in [Direction::Buy, Direction::Sell, Direction::Hold] {
        let s = serde_json::to_string(&dir).unwrap();
        let d: Direction = serde_json::from_str(&s).unwrap();
        assert_eq!(d, dir);
    }
}

// ── SELL TRADE RECORD CLOSE ─────────────────────────────────────────────────

#[test]
fn test_trade_record_close_sell() {
    let pos = Position {
        trade_id: 2,
        strategy_id: 10,
        direction: Direction::Sell,
        entry_price: 100.0,
        initial_stop_loss: 110.0,
        current_stop_loss: 108.0,
        take_profit: 80.0,
        volume: 1.0,
        open_risk: 10.0,
        entry_time: 1719000000,
        is_split_chunk: false,
        group_id: 0,
    };
    let rec =
        TradeRecord::close_position(&pos, "ETHUSDT", 90.0, 1719003600, ExitReason::TakeProfit);
    assert_eq!(rec.profit, 1.0); // (100-90)/10 = 1.0 R
    assert_eq!(rec.currency_pnl, 10.0); // (100-90) * 1.0
    assert_eq!(rec.direction, Direction::Sell);
}

#[test]
fn test_serde_serialization() {
    let dir = Direction::Buy;
    let serialized = serde_json::to_string(&dir).unwrap();
    assert_eq!(serialized, "\"buy\"");
    let deserialized: Direction = serde_json::from_str(&serialized).unwrap();
    assert_eq!(deserialized, Direction::Buy);

    let reason = ExitReason::StopLoss;
    let reason_serialized = serde_json::to_string(&reason).unwrap();
    assert_eq!(reason_serialized, "\"stop_loss\"");
    let reason_deserialized: ExitReason = serde_json::from_str(&reason_serialized).unwrap();
    assert_eq!(reason_deserialized, ExitReason::StopLoss);
}
