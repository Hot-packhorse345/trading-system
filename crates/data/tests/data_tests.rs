use data::{resample, IndicatorCache, JsonCache, OhlcvCache, TradeDb};
use serde::{Deserialize, Serialize};
use tempfile::tempdir;
use ts_core::{Bar, Direction, ExitReason, Timeframe, TradeRecord};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct TestData {
    id: String,
    val: f64,
}

#[test]
fn test_json_cache_operations() {
    let dir = tempdir().unwrap();
    let cache = JsonCache::new(dir.path());

    let name = "test_item";
    let data = TestData {
        id: "abc".to_string(),
        val: 12.34,
    };

    // Before put: get should return None
    let missing: Option<TestData> = cache.get(name).unwrap();
    assert!(missing.is_none());

    // Put data
    cache.put(name, &data).unwrap();

    // Get data should match
    let retrieved: Option<TestData> = cache.get(name).unwrap();
    assert!(retrieved.is_some());
    assert_eq!(retrieved.unwrap(), data);

    // Invalidate
    cache.invalidate(name);
    let after_invalidate: Option<TestData> = cache.get(name).unwrap();
    assert!(after_invalidate.is_none());
}

#[test]
fn test_resample_utility() {
    // 1-minute bars to 5-minute bars resampling (is_futures = true)
    let bars_1m = (0..10)
        .map(|i| {
            Bar::new(
                i * 60, // time in seconds
                100.0,
                101.0,
                99.0,
                100.0,
                10.0,
            )
        })
        .collect::<Vec<Bar>>();

    let resampled_5m = resample(&bars_1m, ts_core::Timeframe::M5, true).unwrap();
    assert_eq!(resampled_5m.len(), 2);
    assert_eq!(resampled_5m[0].time, 0);
    assert_eq!(resampled_5m[0].volume, 50.0);
    assert_eq!(resampled_5m[1].time, 300);
    assert_eq!(resampled_5m[1].volume, 50.0);
}

#[test]
fn test_trade_db_persistence() {
    let db = TradeDb::open(":memory:").unwrap();

    let mut rec = TradeRecord {
        trade_id: 42,
        strategy_id: 1,
        symbol: "BTCUSDT".to_string(),
        direction: Direction::Buy,
        entry_price: 50000.0,
        exit_price: 0.0,
        initial_stop_loss: 49000.0,
        current_stop_loss: 49500.0,
        take_profit: 53000.0,
        volume: 0.1,
        open_risk: 100.0,
        entry_time: 1719000000,
        exit_time: 0,
        exit_reason: ExitReason::EndOfData,
        profit: 0.0,
        currency_pnl: 0.0,
        group_id: 0,
    };

    // Insert record (open position)
    db.insert(&rec).unwrap();

    // Verify duplicate trade_id is rejected
    assert!(db.insert(&rec).is_err());

    // Fetch open positions for strategy 1
    let open_trades = db.load_open(1).unwrap();
    assert_eq!(open_trades.len(), 1);
    assert_eq!(open_trades[0].trade_id, 42);

    // Update stop
    db.update_stop(42, 49700.0).unwrap();
    let open_trades_after_update = db.load_open(1).unwrap();
    assert_eq!(open_trades_after_update[0].current_stop_loss, 49700.0);

    // Close position
    rec.exit_price = 51000.0;
    rec.exit_time = 1719003600;
    rec.exit_reason = ExitReason::TakeProfit;
    rec.profit = 1.0;
    rec.currency_pnl = 100.0;
    db.close(&rec).unwrap();

    // Position is closed, so load_open should return empty
    let open_trades_after_close = db.load_open(1).unwrap();
    assert!(open_trades_after_close.is_empty());

    // Test invalidate by trade_id
    // First insert another open position
    let rec2 = TradeRecord {
        trade_id: 43,
        strategy_id: 1,
        symbol: "BTCUSDT".to_string(),
        direction: Direction::Buy,
        entry_price: 50000.0,
        exit_price: 0.0,
        initial_stop_loss: 49000.0,
        current_stop_loss: 49500.0,
        take_profit: 53000.0,
        volume: 0.1,
        open_risk: 100.0,
        entry_time: 1719000000,
        exit_time: 0,
        exit_reason: ExitReason::EndOfData,
        profit: 0.0,
        currency_pnl: 0.0,
        group_id: 0,
    };
    db.insert(&rec2).unwrap();
    assert_eq!(db.load_open(1).unwrap().len(), 1);

    db.invalidate(Some(1), Some(43)).unwrap();
    assert_eq!(db.load_open(1).unwrap().len(), 0);

    // Test clear
    db.insert(&rec2).unwrap();
    db.clear().unwrap();
    assert_eq!(db.load_open(1).unwrap().len(), 0);
}

#[test]
fn test_indicator_cache_operations() {
    let dir = tempdir().unwrap();
    let cache = IndicatorCache::new(dir.path());

    let ohlcv_hash = "my_ohlcv_hash";
    let config_json = "my_config_json";

    // Check missing
    let missing = cache.get(ohlcv_hash, config_json).unwrap();
    assert!(missing.is_none());

    // Save columns
    let cols = vec![
        ("rsi".to_string(), vec![1.0, 2.0, 3.0]),
        ("ema".to_string(), vec![10.0, 11.0, 12.0]),
    ];
    cache.put(ohlcv_hash, config_json, &cols).unwrap();

    // Get columns
    let retrieved = cache.get(ohlcv_hash, config_json).unwrap().unwrap();
    assert_eq!(retrieved.get("rsi").unwrap(), &vec![1.0, 2.0, 3.0]);
    assert_eq!(retrieved.get("ema").unwrap(), &vec![10.0, 11.0, 12.0]);

    // Invalidate
    cache.invalidate(ohlcv_hash, config_json);
    let after_invalidate = cache.get(ohlcv_hash, config_json).unwrap();
    assert!(after_invalidate.is_none());
}

#[test]
fn test_ohlcv_cache_operations() {
    let dir = tempdir().unwrap();
    let cache = OhlcvCache::new(dir.path());

    let sym = "BTCUSDT";
    let tf = Timeframe::M15;

    // Load empty
    let empty = cache.load_all(sym, tf).unwrap();
    assert!(empty.is_empty());

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

    // Save bars
    cache.save(sym, tf, &bars).unwrap();

    // Load all
    let loaded = cache.load_all(sym, tf).unwrap();
    assert_eq!(loaded.len(), 2);
    assert_eq!(loaded[0].time, 1719000000);

    // Load range
    let loaded_range = cache.load(sym, tf, 1719000000, 1719000000).unwrap();
    assert_eq!(loaded_range.len(), 1);
    assert_eq!(loaded_range[0].time, 1719000000);

    // Append bar
    let new_bar = Bar {
        time: 1719001800,
        open: 109.0,
        high: 115.0,
        low: 108.0,
        close: 114.0,
        volume: 1200.0,
    };
    cache.append_bar(sym, tf, &new_bar).unwrap();

    let loaded_after_append = cache.load_all(sym, tf).unwrap();
    assert_eq!(loaded_after_append.len(), 3);
    assert_eq!(loaded_after_append[2].time, 1719001800);

    // Clear
    cache.clear().unwrap();
    let loaded_after_clear = cache.load_all(sym, tf).unwrap();
    assert!(loaded_after_clear.is_empty());
}

// ── OHLCV CACHE EDGE CASES ─────────────────────────────────────────────────

#[test]
fn test_ohlcv_cache_load_range_no_match() {
    let dir = tempdir().unwrap();
    let cache = OhlcvCache::new(dir.path());

    let bars = vec![
        Bar {
            time: 1000,
            open: 100.0,
            high: 105.0,
            low: 95.0,
            close: 101.0,
            volume: 100.0,
        },
        Bar {
            time: 2000,
            open: 101.0,
            high: 106.0,
            low: 96.0,
            close: 102.0,
            volume: 100.0,
        },
    ];
    cache.save("BTCUSDT", Timeframe::M1, &bars).unwrap();

    // Query range outside stored bars
    let loaded = cache.load("BTCUSDT", Timeframe::M1, 5000, 6000).unwrap();
    assert!(loaded.is_empty());
}

#[test]
fn test_ohlcv_cache_overwrite() {
    let dir = tempdir().unwrap();
    let cache = OhlcvCache::new(dir.path());

    let bars1 = vec![Bar {
        time: 1000,
        open: 100.0,
        high: 105.0,
        low: 95.0,
        close: 101.0,
        volume: 100.0,
    }];
    cache.save("BTCUSDT", Timeframe::M1, &bars1).unwrap();

    let bars2 = vec![
        Bar {
            time: 1000,
            open: 200.0,
            high: 205.0,
            low: 195.0,
            close: 201.0,
            volume: 200.0,
        },
        Bar {
            time: 2000,
            open: 201.0,
            high: 206.0,
            low: 196.0,
            close: 202.0,
            volume: 200.0,
        },
    ];
    cache.save("BTCUSDT", Timeframe::M1, &bars2).unwrap();

    let loaded = cache.load_all("BTCUSDT", Timeframe::M1).unwrap();
    assert_eq!(loaded.len(), 2);
    assert_eq!(loaded[0].open, 200.0);
}

#[test]
fn test_ohlcv_cache_different_symbols() {
    let dir = tempdir().unwrap();
    let cache = OhlcvCache::new(dir.path());

    let bars_btc = vec![Bar {
        time: 1000,
        open: 50000.0,
        high: 51000.0,
        low: 49000.0,
        close: 50500.0,
        volume: 100.0,
    }];
    let bars_eth = vec![Bar {
        time: 1000,
        open: 3000.0,
        high: 3100.0,
        low: 2900.0,
        close: 3050.0,
        volume: 200.0,
    }];

    cache.save("BTCUSDT", Timeframe::H1, &bars_btc).unwrap();
    cache.save("ETHUSDT", Timeframe::H1, &bars_eth).unwrap();

    let loaded_btc = cache.load_all("BTCUSDT", Timeframe::H1).unwrap();
    let loaded_eth = cache.load_all("ETHUSDT", Timeframe::H1).unwrap();

    assert_eq!(loaded_btc.len(), 1);
    assert_eq!(loaded_eth.len(), 1);
    assert_eq!(loaded_btc[0].open, 50000.0);
    assert_eq!(loaded_eth[0].open, 3000.0);
}

#[test]
fn test_ohlcv_cache_time_range() {
    let dir = tempdir().unwrap();
    let cache = OhlcvCache::new(dir.path());

    let sym = "BTCUSDT";
    let tf = Timeframe::M15;

    // Test empty
    let res = cache.time_range(sym, tf, 0, 9999999999).unwrap();
    assert!(res.is_none());

    let bars = vec![
        Bar {
            time: 1000,
            open: 100.0,
            high: 105.0,
            low: 95.0,
            close: 101.0,
            volume: 100.0,
        },
        Bar {
            time: 2000,
            open: 101.0,
            high: 106.0,
            low: 96.0,
            close: 102.0,
            volume: 100.0,
        },
        Bar {
            time: 3000,
            open: 102.0,
            high: 107.0,
            low: 97.0,
            close: 103.0,
            volume: 100.0,
        },
    ];
    cache.save(sym, tf, &bars).unwrap();

    // Query entire range
    let res = cache.time_range(sym, tf, 0, 4000).unwrap().unwrap();
    assert_eq!(res.0, 1000);
    assert_eq!(res.1, 3000);
    assert_eq!(res.2, 3);

    // Query subrange clamping
    let res = cache.time_range(sym, tf, 1500, 2500).unwrap().unwrap();
    assert_eq!(res.0, 2000);
    assert_eq!(res.1, 2000);
    assert_eq!(res.2, 1);

    // Query subrange out of bounds
    let res = cache.time_range(sym, tf, 4000, 5000).unwrap();
    assert!(res.is_none());
}

// ── RESAMPLE EDGE CASES ────────────────────────────────────────────────────

#[test]
fn test_resample_incomplete_excluded() {
    // 7 bars -> 1 complete 5m bucket + 2 leftover bars
    let bars = (0..7)
        .map(|i| Bar::new(i * 60, 100.0, 101.0, 99.0, 100.0, 10.0))
        .collect::<Vec<Bar>>();

    let resampled = resample(&bars, Timeframe::M5, false).unwrap();
    assert_eq!(resampled.len(), 1); // only 1 complete bucket (bars 0-4)

    let resampled_inc = resample(&bars, Timeframe::M5, true).unwrap();
    assert_eq!(resampled_inc.len(), 2); // 1 complete + 1 incomplete
}

#[test]
fn test_resample_ohlc_aggregation() {
    let bars = vec![
        Bar::new(0, 100.0, 105.0, 95.0, 102.0, 10.0),
        Bar::new(60, 102.0, 110.0, 98.0, 103.0, 20.0),
        Bar::new(120, 103.0, 108.0, 93.0, 99.0, 15.0),
        Bar::new(180, 99.0, 106.0, 97.0, 104.0, 25.0),
        Bar::new(240, 104.0, 107.0, 100.0, 105.0, 30.0),
    ];

    let resampled = resample(&bars, Timeframe::M5, true).unwrap();
    assert_eq!(resampled.len(), 1);

    let r = &resampled[0];
    assert_eq!(r.time, 0);
    assert_eq!(r.open, 100.0); // first bar's open
    assert_eq!(r.high, 110.0); // max high across all bars
    assert_eq!(r.low, 93.0); // min low across all bars
    assert_eq!(r.close, 105.0); // last bar's close
    assert_eq!(r.volume, 100.0); // sum of volumes
}

#[test]
fn test_resample_empty_bars() {
    let bars: Vec<Bar> = vec![];
    let resampled = resample(&bars, Timeframe::M5, true).unwrap();
    assert!(resampled.is_empty());
}

// ── JSON CACHE EDGE CASES ───────────────────────────────────────────────────

#[test]
fn test_json_cache_overwrite() {
    let dir = tempdir().unwrap();
    let cache = JsonCache::new(dir.path());

    let data1 = TestData {
        id: "v1".to_string(),
        val: 1.0,
    };
    cache.put("item", &data1).unwrap();

    let data2 = TestData {
        id: "v2".to_string(),
        val: 2.0,
    };
    cache.put("item", &data2).unwrap();

    let retrieved: Option<TestData> = cache.get("item").unwrap();
    assert_eq!(retrieved.unwrap().id, "v2");
}

#[test]
fn test_json_cache_multiple_items() {
    let dir = tempdir().unwrap();
    let cache = JsonCache::new(dir.path());

    cache
        .put(
            "a",
            &TestData {
                id: "a".to_string(),
                val: 1.0,
            },
        )
        .unwrap();
    cache
        .put(
            "b",
            &TestData {
                id: "b".to_string(),
                val: 2.0,
            },
        )
        .unwrap();

    let a: TestData = cache.get("a").unwrap().unwrap();
    let b: TestData = cache.get("b").unwrap().unwrap();
    assert_eq!(a.id, "a");
    assert_eq!(b.id, "b");

    // Invalidate one, other still exists
    cache.invalidate("a");
    assert!(cache.get::<TestData>("a").unwrap().is_none());
    assert!(cache.get::<TestData>("b").unwrap().is_some());
}

// ── TRADE DB EDGE CASES ────────────────────────────────────────────────────

#[test]
fn test_trade_db_multiple_strategies() {
    let db = TradeDb::open(":memory:").unwrap();

    let rec1 = TradeRecord {
        trade_id: 1,
        strategy_id: 100,
        symbol: "BTCUSDT".to_string(),
        direction: Direction::Buy,
        entry_price: 50000.0,
        exit_price: 0.0,
        initial_stop_loss: 49000.0,
        current_stop_loss: 49000.0,
        take_profit: 53000.0,
        volume: 0.1,
        open_risk: 100.0,
        entry_time: 1719000000,
        exit_time: 0,
        exit_reason: ExitReason::EndOfData,
        profit: 0.0,
        currency_pnl: 0.0,
        group_id: 0,
    };

    let rec2 = TradeRecord {
        trade_id: 2,
        strategy_id: 200,
        ..rec1.clone()
    };

    db.insert(&rec1).unwrap();
    db.insert(&rec2).unwrap();

    assert_eq!(db.load_open(100).unwrap().len(), 1);
    assert_eq!(db.load_open(200).unwrap().len(), 1);
    assert_eq!(db.load_open(999).unwrap().len(), 0);
}

#[test]
fn test_trade_db_sell_direction_roundtrip() {
    let db = TradeDb::open(":memory:").unwrap();

    let rec = TradeRecord {
        trade_id: 10,
        strategy_id: 1,
        symbol: "ETHUSDT".to_string(),
        direction: Direction::Sell,
        entry_price: 3000.0,
        exit_price: 0.0,
        initial_stop_loss: 3100.0,
        current_stop_loss: 3100.0,
        take_profit: 2800.0,
        volume: 1.0,
        open_risk: 100.0,
        entry_time: 1719000000,
        exit_time: 0,
        exit_reason: ExitReason::EndOfData,
        profit: 0.0,
        currency_pnl: 0.0,
        group_id: 5,
    };
    db.insert(&rec).unwrap();

    let loaded = db.load_open(1).unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].direction, Direction::Sell);
    assert_eq!(loaded[0].group_id, 5);
}

// ── INDICATOR CACHE MULTIPLE CONFIGS ────────────────────────────────────────

#[test]
fn test_indicator_cache_different_configs() {
    let dir = tempdir().unwrap();
    let cache = IndicatorCache::new(dir.path());

    let cols1 = vec![("rsi_14".to_string(), vec![1.0, 2.0])];
    let cols2 = vec![("rsi_7".to_string(), vec![3.0, 4.0])];

    cache.put("hash1", "config_a", &cols1).unwrap();
    cache.put("hash1", "config_b", &cols2).unwrap();

    let r1 = cache.get("hash1", "config_a").unwrap().unwrap();
    let r2 = cache.get("hash1", "config_b").unwrap().unwrap();

    assert_eq!(r1.get("rsi_14").unwrap(), &vec![1.0, 2.0]);
    assert_eq!(r2.get("rsi_7").unwrap(), &vec![3.0, 4.0]);
}
