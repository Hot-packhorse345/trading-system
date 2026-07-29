use async_trait::async_trait;
use broker::{DataProvider, Executor};
use data::JsonCache;
use infra::{news::BlackoutWindow, news::WindowEvent, NullNotifier};
use live::{drawdown_manager, formatter};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::watch;
use ts_core::{AccountInfo, Direction, ExitReason, Position, SymbolInfo, TradeRecord};

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

#[test]
fn test_formatter_simple() {
    assert_eq!(
        formatter::format_news_cleared(),
        "✅ <b>News Blackout Cleared</b>"
    );
    assert_eq!(
        formatter::exit_reason_label(ExitReason::StopLoss),
        "Stop Out"
    );
    assert_eq!(
        formatter::exit_reason_label(ExitReason::TakeProfit),
        "Take Profit"
    );
    assert_eq!(
        formatter::exit_reason_label(ExitReason::ExitRule),
        "Exit Rule"
    );
    assert_eq!(
        formatter::exit_reason_label(ExitReason::StopProfit),
        "Stop Profit"
    );
    assert_eq!(
        formatter::exit_reason_label(ExitReason::EndOfData),
        "Closed"
    );
}

#[test]
fn test_formatter_trade_open_close() {
    let pos = Position {
        trade_id: 1,
        strategy_id: 1,
        direction: Direction::Buy,
        entry_price: 1.2345,
        initial_stop_loss: 1.2000,
        current_stop_loss: 1.2100,
        take_profit: 1.3000,
        volume: 2.5,
        open_risk: 0.08625,
        entry_time: 1719000000,
        is_split_chunk: false,
        group_id: 1,
    };
    let open_msg = formatter::format_trade_open(&pos, "EURUSD");
    assert!(open_msg.contains("Trade Opened"));
    assert!(open_msg.contains("EURUSD"));

    let update_msg = formatter::format_stop_update(&pos, "EURUSD", now_secs());
    assert!(update_msg.contains("Stop Updated"));

    let rec = TradeRecord {
        trade_id: 1,
        strategy_id: 1,
        symbol: "EURUSD".to_string(),
        direction: Direction::Buy,
        entry_price: 1.2345,
        exit_price: 1.2500,
        initial_stop_loss: 1.2000,
        current_stop_loss: 1.2100,
        take_profit: 1.3000,
        volume: 2.5,
        open_risk: 0.08625,
        entry_time: 1719000000,
        exit_time: 1719003600,
        exit_reason: ExitReason::ExitRule,
        profit: 1.5,
        currency_pnl: 38.75,
        group_id: 1,
    };
    let stop_out_msg = formatter::format_stop_out(&rec);
    assert!(stop_out_msg.contains("Stop Out"));

    let tp_msg = formatter::format_take_profit(&rec);
    assert!(tp_msg.contains("Take Profit"));

    let exit_rule_msg = formatter::format_exit_rule(&rec);
    assert!(exit_rule_msg.contains("Exit Rule"));
}

#[test]
fn test_formatter_news_and_account() {
    let window = BlackoutWindow {
        start: chrono::DateTime::from_timestamp(1719000000, 0)
            .unwrap()
            .with_timezone(&chrono::Utc),
        end: chrono::DateTime::from_timestamp(1719003600, 0)
            .unwrap()
            .with_timezone(&chrono::Utc),
        events: vec![WindowEvent {
            is_custom: false,
            event_time: chrono::DateTime::from_timestamp(1719001800, 0)
                .unwrap()
                .with_timezone(&chrono::Utc),
            country: "USD".to_string(),
            impact: "High".to_string(),
            title: "Fed Interest Rate Decision".to_string(),
        }],
    };
    let blackout_msg = formatter::format_news_blackout(&window);
    assert!(blackout_msg.contains("News Blackout Active"));
    assert!(blackout_msg.contains("Fed Interest Rate Decision"));

    let acct = AccountInfo {
        balance: 100000.0,
        equity: 102500.0,
        profit: 2500.0,
        currency: "USDT".to_string(),
        margin: 500.0,
        margin_free: 102000.0,
    };
    let acct_msg = formatter::format_account_report(&acct);
    assert!(acct_msg.contains("Account Report"));
    assert!(acct_msg.contains("USDT"));
}

struct MockDataProvider {
    equity: f64,
    balance: f64,
}

#[async_trait]
impl DataProvider for MockDataProvider {
    async fn ohlcv(
        &self,
        _sym: &str,
        _tf: ts_core::Timeframe,
        _start: i64,
        _end: i64,
    ) -> anyhow::Result<Vec<ts_core::Bar>> {
        Ok(vec![])
    }

    async fn account(&self) -> anyhow::Result<AccountInfo> {
        Ok(AccountInfo {
            balance: self.balance,
            equity: self.equity,
            profit: 0.0,
            currency: "USDT".to_string(),
            margin: 0.0,
            margin_free: self.equity,
        })
    }

    async fn symbol_info(&self, sym: &str) -> anyhow::Result<SymbolInfo> {
        Ok(SymbolInfo {
            symbol: sym.to_string(),
            ask: 1.0,
            bid: 1.0,
            point: 0.01,
            tick_value: 0.01,
            lot_step: 0.01,
            min_lot: 0.01,
            max_lot: 1000.0,
            spread: 0.0,
            digits: 2,
            time: 0.0,
        })
    }
}

#[tokio::test]
async fn test_drawdown_manager_shutdown() {
    let now_nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let temp_dir = std::env::temp_dir().join(format!("test_live_{}", now_nanos));
    std::fs::create_dir_all(&temp_dir).unwrap();

    let provider = Arc::new(MockDataProvider {
        equity: 50000.0,
        balance: 50000.0,
    });
    let (emergency_tx, _emergency_rx) = watch::channel(false);
    let (dd_tx, _dd_rx) = watch::channel(0.0);
    let notifier = Arc::new(NullNotifier);
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    let run_fut = drawdown_manager::run(
        provider.clone(),
        5.0,
        emergency_tx,
        dd_tx,
        notifier,
        temp_dir.clone(),
        shutdown_rx,
    );
    let handle = tokio::spawn(run_fut);
    shutdown_tx.send(true).unwrap();
    handle.await.unwrap();

    let cache = JsonCache::new(temp_dir.join("live"));
    assert!(cache
        .get::<serde_json::Value>("daily_anchor")
        .unwrap()
        .is_some());
    std::fs::remove_dir_all(&temp_dir).ok();
}

#[tokio::test]
async fn test_drawdown_manager_rearm() {
    let now_nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let temp_dir = std::env::temp_dir().join(format!("test_live_rearm_{}", now_nanos));
    std::fs::create_dir_all(&temp_dir).unwrap();

    let cache = JsonCache::new(temp_dir.join("live"));
    let today = chrono::Utc::now()
        .date_naive()
        .format("%Y-%m-%d")
        .to_string();
    let emergency_state = serde_json::json!({
        "date": today,
        "active": true
    });
    cache.put("drawdown_emergency", &emergency_state).unwrap();

    let provider = Arc::new(MockDataProvider {
        equity: 50000.0,
        balance: 50000.0,
    });
    let (emergency_tx, emergency_rx) = watch::channel(false);
    let (dd_tx, _dd_rx) = watch::channel(0.0);
    let notifier = Arc::new(NullNotifier);
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    let run_fut = drawdown_manager::run(
        provider.clone(),
        5.0,
        emergency_tx,
        dd_tx,
        notifier,
        temp_dir.clone(),
        shutdown_rx,
    );
    let handle = tokio::spawn(run_fut);

    // Sleep a bit to allow drawdown monitor loop to read persisted state and update channel
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    assert_eq!(*emergency_rx.borrow(), true);

    shutdown_tx.send(true).unwrap();
    handle.await.unwrap();
    std::fs::remove_dir_all(&temp_dir).ok();
}

// ── ADDITIONAL LIVE TESTS FOR EDGE CASES ─────────────────────────────────────

#[test]
fn test_live_config_parsing() {
    use live::config::LiveConfig;

    let single_json = r#"{
        "strategy": "rsi_reversion",
        "symbol": "BTCUSDT",
        "timeframe": "1h",
        "risk_manager": {
            "type": "fixed_percent",
            "pct": 0.01,
            "initial_balance": 100000.0
        },
        "stop_manager": {
            "type": "fixed",
            "stop_distance": 100.0,
            "start_rr": 0.0
        },
        "indicators": {
            "rsi": {
                "type": "rsi",
                "period": 14
            }
        },
        "strategy_parameters": {
            "oversold": 30.0,
            "overbought": 70.0,
            "stop_pct": 0.02,
            "tp_pct": 0.04
        }
    }"#;
    let config = LiveConfig::from_json(single_json).unwrap();
    assert_eq!(config.workers().len(), 1);
    assert_eq!(config.workers()[0].strategy, "rsi_reversion");
    assert_eq!(config.workers()[0].symbol, "BTCUSDT");
    assert_eq!(config.workers()[0].effective_stop_timeframe(), "1h");
    assert_eq!(config.workers()[0].tick_streamer_name(), "binance");

    let array_json = r#"[
        {
            "strategy": "rsi_reversion",
            "symbol": "BTCUSDT",
            "timeframe": "1h",
            "risk_manager": {
                "type": "fixed_percent",
                "pct": 0.01,
                "initial_balance": 100000.0
            },
            "stop_manager": {
                "type": "fixed",
                "stop_distance": 10.0,
                "start_rr": 0.0
            },
            "indicators": {
                "rsi": { "type": "rsi", "period": 14 }
            },
            "strategy_parameters": {
                "oversold": 30.0,
                "overbought": 70.0
            }
        },
        {
            "strategy": "ema_cross",
            "symbol": "ETHUSDT",
            "timeframe": "15m",
            "risk_manager": {
                "type": "fixed_percent",
                "pct": 0.01,
                "initial_balance": 100000.0
            },
            "stop_manager": {
                "type": "fixed",
                "stop_distance": 10.0,
                "start_rr": 0.0
            },
            "indicators": {
                "ema_fast": { "type": "ema", "period": 9 },
                "ema_slow": { "type": "ema", "period": 21 }
            },
            "strategy_parameters": {
                "stop_pct": 0.02,
                "tp_pct": 0.04
            }
        }
    ]"#;
    let config2 = LiveConfig::from_json(array_json).unwrap();
    assert_eq!(config2.workers().len(), 2);
    assert_eq!(config2.workers()[1].strategy, "ema_cross");

    assert!(LiveConfig::from_json("[]").is_err());
    assert!(LiveConfig::from_json("{invalid").is_err());
}

#[test]
fn test_trade_managers() {
    use live::trade_manager::{GroupTradeManager, TradeManager};
    use risk::StopManager;
    use ts_core::{Bar, Direction, ExitReason, Params, Position, Signal, Tick};

    struct SimpleStopManager {
        stop: f64,
    }
    impl StopManager for SimpleStopManager {
        fn init(&mut self, _entry: f64, stop: f64, _dir: Direction) {
            self.stop = stop;
        }
        fn update(&mut self, _close: f64, _high: f64, _low: f64) {}
        fn stop(&self) -> f64 {
            self.stop
        }
        fn stopped_out(&self, _high: f64, _low: f64) -> bool {
            false
        }
    }

    struct TighterStopManager {
        stop: f64,
    }
    impl StopManager for TighterStopManager {
        fn init(&mut self, _entry: f64, stop: f64, _dir: Direction) {
            self.stop = stop;
        }
        fn update(&mut self, _close: f64, _high: f64, _low: f64) {
            self.stop = 95.0;
        }
        fn stop(&self) -> f64 {
            self.stop
        }
        fn stopped_out(&self, _high: f64, _low: f64) -> bool {
            false
        }
    }

    let pos = Position {
        trade_id: 1,
        strategy_id: 100,
        direction: Direction::Buy,
        entry_price: 100.0,
        initial_stop_loss: 90.0,
        current_stop_loss: 90.0,
        take_profit: 120.0,
        volume: 1.0,
        open_risk: 10.0,
        entry_time: 1000,
        is_split_chunk: false,
        group_id: 0,
    };

    let mut tm = TradeManager::new(
        pos,
        "BTCUSDT".to_string(),
        Box::new(SimpleStopManager { stop: 90.0 }),
        vec![],
    );
    assert!(!tm.is_closed());
    assert_eq!(tm.close_reason(), ExitReason::ExitRule);

    let safe_tick = Tick {
        symbol: "BTCUSDT".to_string(),
        bid: 105.0,
        ask: 105.0,
        last: 105.0,
        volume: 1.0,
        timestamp: 1001.0,
    };
    assert!(!tm.check_tick_stop(&safe_tick));
    assert!(!tm.is_closed());

    let stop_tick = Tick {
        symbol: "BTCUSDT".to_string(),
        bid: 89.0,
        ask: 89.0,
        last: 89.0,
        volume: 1.0,
        timestamp: 1002.0,
    };
    assert!(tm.check_tick_stop(&stop_tick));
    assert!(tm.is_closed());
    assert_eq!(tm.close_reason(), ExitReason::StopLoss);

    let mut tm_tp = TradeManager::new(
        pos,
        "BTCUSDT".to_string(),
        Box::new(SimpleStopManager { stop: 90.0 }),
        vec![],
    );
    let tp_tick = Tick {
        symbol: "BTCUSDT".to_string(),
        bid: 121.0,
        ask: 121.0,
        last: 121.0,
        volume: 1.0,
        timestamp: 1003.0,
    };
    assert!(tm_tp.check_tick_tp(&tp_tick));
    assert!(tm_tp.is_closed());
    assert_eq!(tm_tp.close_reason(), ExitReason::TakeProfit);

    // Test update_stop and check_exit_rules
    let bar = Bar::new(1719000000, 100.0, 101.0, 99.0, 100.0, 1000.0);
    let mut tm_update = TradeManager::new(
        pos,
        "BTCUSDT".to_string(),
        Box::new(TighterStopManager { stop: 90.0 }),
        vec![],
    );
    assert_eq!(tm_update.update_stop(&bar), Some(95.0));

    let cols_empty = ts_core::IndicatorSet::default();
    let params_empty = Params::default();
    let sig_empty = Signal::new(Direction::Hold, 100.0, 90.0, 120.0);
    assert!(!tm_update.check_exit_rules(0, &[bar], &cols_empty, &params_empty, &sig_empty, None));

    let pos_chunk1 = Position {
        trade_id: 2,
        group_id: 5,
        ..pos
    };
    let pos_chunk2 = Position {
        trade_id: 3,
        group_id: 5,
        ..pos
    };

    let tm1 = TradeManager::new(
        pos_chunk1,
        "BTCUSDT".to_string(),
        Box::new(SimpleStopManager { stop: 90.0 }),
        vec![],
    );
    let tm2 = TradeManager::new(
        pos_chunk2,
        "BTCUSDT".to_string(),
        Box::new(SimpleStopManager { stop: 90.0 }),
        vec![],
    );
    let mut gtm = GroupTradeManager::new(
        5,
        vec![tm1, tm2],
        Box::new(SimpleStopManager { stop: 90.0 }),
    );
    assert!(!gtm.is_fully_closed());
    assert_eq!(gtm.direction(), Some(Direction::Buy));

    let stopped_indices = gtm.check_tick_stop(&stop_tick);
    assert_eq!(stopped_indices, vec![0, 1]);

    // Recreate group for TP check since first group is now stopped out
    let tm1_tp = TradeManager::new(
        pos_chunk1,
        "BTCUSDT".to_string(),
        Box::new(SimpleStopManager { stop: 90.0 }),
        vec![],
    );
    let tm2_tp = TradeManager::new(
        pos_chunk2,
        "BTCUSDT".to_string(),
        Box::new(SimpleStopManager { stop: 90.0 }),
        vec![],
    );
    let mut gtm_tp = GroupTradeManager::new(
        5,
        vec![tm1_tp, tm2_tp],
        Box::new(SimpleStopManager { stop: 90.0 }),
    );
    let tp_indices = gtm_tp.check_tick_tp(&tp_tick);
    assert_eq!(tp_indices, vec![0, 1]);

    // Test GroupTradeManager update_stop
    let tm_gtm_up1 = TradeManager::new(
        pos_chunk1,
        "BTCUSDT".to_string(),
        Box::new(TighterStopManager { stop: 90.0 }),
        vec![],
    );
    let tm_gtm_up2 = TradeManager::new(
        pos_chunk2,
        "BTCUSDT".to_string(),
        Box::new(TighterStopManager { stop: 90.0 }),
        vec![],
    );
    let mut gtm_update = GroupTradeManager::new(
        5,
        vec![tm_gtm_up1, tm_gtm_up2],
        Box::new(TighterStopManager { stop: 90.0 }),
    );
    assert_eq!(gtm_update.update_stop(&bar), Some(95.0));

    gtm_tp.remove_chunks(vec![0]);
    assert_eq!(gtm_tp.chunks.len(), 1);
}

#[tokio::test]
async fn test_account_reporter_shutdown() {
    use live::account_reporter;
    let now_nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let temp_dir =
        std::env::temp_dir().join(format!("test_account_reporter_shutdown_{}", now_nanos));

    let provider = Arc::new(MockDataProvider {
        equity: 50000.0,
        balance: 50000.0,
    });
    let notifier = Arc::new(NullNotifier);
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    shutdown_tx.send(true).unwrap();

    account_reporter::run(provider, notifier, temp_dir.clone(), shutdown_rx).await;
    std::fs::remove_dir_all(&temp_dir).ok();
}

use std::sync::Mutex;

#[derive(Clone)]
struct MockBroker {
    balance: f64,
    equity: f64,
    bar_txs: Arc<Mutex<Vec<tokio::sync::mpsc::Sender<ts_core::Bar>>>>,
    tick_txs: Arc<Mutex<Vec<tokio::sync::mpsc::Sender<ts_core::Tick>>>>,
}

impl MockBroker {
    fn new() -> Self {
        Self {
            balance: 100000.0,
            equity: 100000.0,
            bar_txs: Arc::new(Mutex::new(Vec::new())),
            tick_txs: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

#[async_trait]
impl broker::DataProvider for MockBroker {
    async fn ohlcv(
        &self,
        _sym: &str,
        _tf: ts_core::Timeframe,
        _start: i64,
        _end: i64,
    ) -> anyhow::Result<Vec<ts_core::Bar>> {
        Ok(vec![])
    }

    async fn account(&self) -> anyhow::Result<ts_core::AccountInfo> {
        Ok(ts_core::AccountInfo {
            balance: self.balance,
            equity: self.equity,
            profit: 0.0,
            currency: "USDT".to_string(),
            margin: 0.0,
            margin_free: self.equity,
        })
    }

    async fn symbol_info(&self, sym: &str) -> anyhow::Result<ts_core::SymbolInfo> {
        Ok(ts_core::SymbolInfo {
            symbol: sym.to_string(),
            ask: 100.0,
            bid: 100.0,
            point: 0.01,
            tick_value: 0.01,
            lot_step: 0.01,
            min_lot: 0.01,
            max_lot: 100.0,
            spread: 0.0,
            digits: 2,
            time: 0.0,
        })
    }
}

#[async_trait]
impl broker::Executor for MockBroker {
    async fn open(&self, req: &broker::OrderRequest) -> anyhow::Result<ts_core::Position> {
        Ok(ts_core::Position {
            trade_id: req.trade_id,
            strategy_id: req.strategy_id,
            direction: req.direction,
            entry_price: req.entry_price,
            initial_stop_loss: req.stop_loss,
            current_stop_loss: req.stop_loss,
            take_profit: req.take_profit,
            volume: req.volume,
            open_risk: (req.entry_price - req.stop_loss).abs() * req.volume,
            entry_time: 12345,
            is_split_chunk: false,
            group_id: 0,
        })
    }

    async fn close(
        &self,
        pos: &ts_core::Position,
        _symbol: &str,
    ) -> anyhow::Result<ts_core::TradeRecord> {
        Ok(ts_core::TradeRecord::close_position(
            pos,
            "BTCUSDT",
            pos.entry_price,
            12346,
            ts_core::ExitReason::ExitRule,
        ))
    }

    async fn update_sl(
        &self,
        pos: &ts_core::Position,
        sl: f64,
    ) -> anyhow::Result<ts_core::Position> {
        let mut p = *pos;
        p.current_stop_loss = sl;
        Ok(p)
    }
}

#[async_trait]
impl broker::BarStream for MockBroker {
    async fn subscribe(
        &self,
        _sym: &str,
        _tf: ts_core::Timeframe,
        tx: tokio::sync::mpsc::Sender<ts_core::Bar>,
    ) -> anyhow::Result<()> {
        self.bar_txs.lock().unwrap().push(tx);
        Ok(())
    }
}

#[async_trait]
impl broker::TickStream for MockBroker {
    async fn subscribe(
        &self,
        _sym: &str,
        tx: tokio::sync::mpsc::Sender<ts_core::Tick>,
    ) -> anyhow::Result<()> {
        self.tick_txs.lock().unwrap().push(tx);
        Ok(())
    }
}

#[tokio::test]
async fn test_live_worker_run_loop() {
    use broker::BrokerHandles;
    use data::TradeDb;
    use live::config::LiveWorkerConfig;
    use live::worker::LiveWorker;
    use tokio::sync::{mpsc, watch};

    let now_nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let temp_db_path = std::env::temp_dir().join(format!("test_worker_loop_{}.db", now_nanos));
    let trade_db = Arc::new(Mutex::new(TradeDb::open(&temp_db_path).unwrap()));

    let worker_json = r#"{
        "strategy": "ema_cross",
        "symbol": "BTCUSDT",
        "timeframe": "1m",
        "risk_manager": {
            "type": "fixed_percent",
            "pct": 0.02,
            "initial_balance": 100000.0
        },
        "stop_manager": {
            "type": "variant1",
            "stop_distance": 0.5,
            "start_rr": 0.0
        },
        "indicators": {
            "ema_fast": {
                "type": "ema",
                "period": 9
            },
            "ema_slow": {
                "type": "ema",
                "period": 21
            }
        },
        "strategy_parameters": {
            "stop_pct": 0.02,
            "tp_pct": 0.04
        }
    }"#;

    let config: LiveWorkerConfig = serde_json::from_str(worker_json).unwrap();
    let mock_broker = Arc::new(MockBroker::new());
    let handles = BrokerHandles {
        provider: mock_broker.clone(),
        executor: mock_broker.clone(),
        streamer: mock_broker.clone(),
        tick_streamer: mock_broker.clone(),
    };
    let notifier = Arc::new(NullNotifier);
    let (_dd_tx, dd_rx) = watch::channel(0.0);

    let worker = LiveWorker::new(&config, &handles, notifier, trade_db.clone(), dd_rx).unwrap();
    let (news_tx, news_rx) = mpsc::unbounded_channel();
    let (emergency_tx, emergency_rx) = watch::channel(false);
    let (_decay_tx, decay_rx) = watch::channel(false);
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    let mock_broker_c = mock_broker.clone();
    let jh = tokio::spawn(async move {
        worker
            .run(
                config,
                handles,
                news_rx,
                emergency_rx,
                decay_rx,
                shutdown_rx,
            )
            .await
    });

    // Sleep briefly to let loop start and subscribe
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    // Retrieve sender
    let bar_sender = {
        let txs = mock_broker_c.bar_txs.lock().unwrap();
        txs.get(0).cloned()
    };

    if let Some(sender) = bar_sender.clone() {
        // Send 100 bars to trigger indicators & signals.
        // MIN_BARS = 50, so signals are only processed from bar 50 onwards.
        // We delay the EMA golden cross until after bar 50 by holding price flat
        // between bars 40-50, then starting a steep uptrend.
        for i in 0..100 {
            let price = if i < 20 {
                100.0 // flat
            } else if i < 40 {
                100.0 - (i - 20) as f64 * 2.0 // 100 → 60 (downtrend)
            } else if i < 50 {
                60.0 // flat (EMAs converge, fast < slow)
            } else {
                60.0 + (i - 50) as f64 * 2.0 // 60 → 160 (uptrend → cross)
            };
            let bar = ts_core::Bar::new(
                1719000000 + i * 60,
                price,
                price + 0.1,
                price - 0.1,
                price,
                100.0,
            );
            sender.send(bar).await.ok();
        }
    }

    // Give the event loop time to process the bars
    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    // Verify a trade has been opened in the db
    let strategy_id = {
        let mut h: u64 = 14695981039346656037;
        for b in "ema_cross:BTCUSDT:1m".bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(1099511628211);
        }
        h
    };

    {
        let db = trade_db.lock().unwrap();
        let open_trades = db.load_open(strategy_id).unwrap();
        assert!(!open_trades.is_empty(), "Expected open trades in DB");
    }

    // Now trigger a news blackout event to test the blackout close logic
    news_tx
        .send(infra::news::BlackoutNotification {
            active: true,
            window: Some(infra::news::BlackoutWindow {
                start: chrono::Utc::now(),
                end: chrono::Utc::now() + chrono::Duration::minutes(5),
                events: vec![],
            }),
        })
        .unwrap();

    // Give some time to process
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    // Verify trades are closed after news blackout
    {
        let db = trade_db.lock().unwrap();
        let open_trades = db.load_open(strategy_id).unwrap();
        assert!(
            open_trades.is_empty(),
            "Expected trades to be closed by news blackout"
        );
    }

    // Clear blackout
    news_tx
        .send(infra::news::BlackoutNotification {
            active: false,
            window: None,
        })
        .unwrap();
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    // Send more bars to open a trade now that blackout is cleared.
    // Another V-shaped series to trigger a second golden cross.
    if let Some(ref sender) = bar_sender {
        for i in 100..150 {
            let price = if i < 120 {
                160.0 - (i - 100) as f64 * 3.0 // 160 → 100 (downtrend)
            } else {
                100.0 + (i - 120) as f64 * 2.0 // 100 → 200 (uptrend → cross)
            };
            let bar = ts_core::Bar::new(
                1719000000 + i * 60,
                price,
                price + 0.1,
                price - 0.1,
                price,
                100.0,
            );
            sender.send(bar).await.ok();
        }
    }
    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    // Assert a new trade is open
    {
        let db = trade_db.lock().unwrap();
        let open_trades = db.load_open(strategy_id).unwrap();
        assert!(!open_trades.is_empty(), "Expected a new open trade");
    }

    // Trigger emergency to close positions
    emergency_tx.send(true).unwrap();
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    // Assert open trades are closed
    {
        let db = trade_db.lock().unwrap();
        let open_trades = db.load_open(strategy_id).unwrap();
        assert!(
            open_trades.is_empty(),
            "Expected trades to be closed by emergency"
        );
    }

    // Trigger shutdown
    shutdown_tx.send(true).unwrap();
    let run_res = jh.await.unwrap();
    assert!(run_res.is_ok());
    std::fs::remove_file(&temp_db_path).ok();
}

#[tokio::test]
async fn test_live_engine_run() {
    use live::config::LiveConfig;
    use live::engine::LiveEngine;

    let now_nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let temp_dir = std::env::temp_dir().join(format!("test_engine_{}", now_nanos));
    std::fs::create_dir_all(&temp_dir).unwrap();

    let json = format!(
        r#"{{
        "strategy": "ema_cross",
        "symbol": "BTCUSDT",
        "timeframe": "1h",
        "risk_manager": {{
            "type": "fixed_percent",
            "pct": 0.02,
            "initial_balance": 100000.0
        }},
        "stop_manager": {{
            "type": "fixed",
            "stop_distance": 10.0,
            "start_rr": 0.0
        }},
        "indicators": {{
            "ema_fast": {{ "type": "ema", "period": 9 }},
            "ema_slow": {{ "type": "ema", "period": 21 }}
        }},
        "strategy_parameters": {{
            "stop_pct": 0.02,
            "tp_pct": 0.04
        }},
        "trade_executor": "paper",
        "data_dir": {:?}
    }}"#,
        temp_dir.to_str().unwrap()
    );

    let config = LiveConfig::from_json(&json).unwrap();
    let engine = LiveEngine::new(config);
    let run_fut = engine.run();
    let result = tokio::time::timeout(tokio::time::Duration::from_millis(200), run_fut).await;
    assert!(result.is_err());
    std::fs::remove_dir_all(&temp_dir).ok();
}

struct DynamicDataProvider {
    equity: Mutex<f64>,
    balance: Mutex<f64>,
}

#[async_trait]
impl DataProvider for DynamicDataProvider {
    async fn ohlcv(
        &self,
        _sym: &str,
        _tf: ts_core::Timeframe,
        _start: i64,
        _end: i64,
    ) -> anyhow::Result<Vec<ts_core::Bar>> {
        Ok(vec![])
    }

    async fn account(&self) -> anyhow::Result<ts_core::AccountInfo> {
        let eq = *self.equity.lock().unwrap();
        let bal = *self.balance.lock().unwrap();
        Ok(ts_core::AccountInfo {
            balance: bal,
            equity: eq,
            profit: 0.0,
            currency: "USDT".to_string(),
            margin: 0.0,
            margin_free: eq,
        })
    }

    async fn symbol_info(&self, sym: &str) -> anyhow::Result<ts_core::SymbolInfo> {
        Ok(ts_core::SymbolInfo {
            symbol: sym.to_string(),
            ask: 1.0,
            bid: 1.0,
            point: 0.01,
            tick_value: 0.01,
            lot_step: 0.01,
            min_lot: 0.01,
            max_lot: 1000.0,
            spread: 0.0,
            digits: 2,
            time: 0.0,
        })
    }
}

#[tokio::test(start_paused = true)]
async fn test_drawdown_manager_tripped() {
    let now_nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let temp_dir = std::env::temp_dir().join(format!("test_live_dd_tripped_{}", now_nanos));
    std::fs::create_dir_all(&temp_dir).unwrap();

    let provider = Arc::new(DynamicDataProvider {
        equity: Mutex::new(50000.0),
        balance: Mutex::new(50000.0),
    });
    let (emergency_tx, emergency_rx) = watch::channel(false);
    let (dd_tx, dd_rx) = watch::channel(0.0);
    let notifier = Arc::new(NullNotifier);
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    let run_fut = drawdown_manager::run(
        provider.clone(),
        5.0,
        emergency_tx,
        dd_tx,
        notifier,
        temp_dir.clone(),
        shutdown_rx,
    );
    let handle = tokio::spawn(run_fut);

    tokio::time::advance(tokio::time::Duration::from_secs(65)).await;
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    assert_eq!(*emergency_rx.borrow(), false);
    assert_eq!(*dd_rx.borrow(), 0.0);

    {
        *provider.equity.lock().unwrap() = 45000.0;
    }
    tokio::time::advance(tokio::time::Duration::from_secs(60)).await;
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    assert_eq!(*emergency_rx.borrow(), true);
    assert_eq!(*dd_rx.borrow(), 0.1);

    shutdown_tx.send(true).unwrap();
    handle.await.unwrap();
    std::fs::remove_dir_all(&temp_dir).ok();
}

#[tokio::test(start_paused = true)]
async fn test_drawdown_manager_rollover() {
    let now_nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let temp_dir = std::env::temp_dir().join(format!("test_live_dd_rollover_{}", now_nanos));
    std::fs::create_dir_all(&temp_dir).unwrap();

    let provider = Arc::new(DynamicDataProvider {
        equity: Mutex::new(50000.0),
        balance: Mutex::new(50000.0),
    });
    let (emergency_tx, emergency_rx) = watch::channel(false);
    let (dd_tx, dd_rx) = watch::channel(0.0);
    let notifier = Arc::new(NullNotifier);
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    let cache = JsonCache::new(temp_dir.join("live"));
    let today = chrono::Utc::now()
        .date_naive()
        .format("%Y-%m-%d")
        .to_string();
    let emergency_state = serde_json::json!({
        "date": today,
        "active": true
    });
    cache.put("drawdown_emergency", &emergency_state).unwrap();

    let run_fut = drawdown_manager::run(
        provider.clone(),
        5.0,
        emergency_tx,
        dd_tx,
        notifier,
        temp_dir.clone(),
        shutdown_rx,
    );
    let handle = tokio::spawn(run_fut);

    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    assert_eq!(*emergency_rx.borrow(), true);

    let yesterday = (chrono::Utc::now().date_naive() - chrono::Duration::days(1))
        .format("%Y-%m-%d")
        .to_string();
    let old_anchor = serde_json::json!({
        "date": yesterday,
        "equity": 50000.0,
        "balance": 50000.0
    });
    cache.put("daily_anchor", &old_anchor).unwrap();

    tokio::time::advance(tokio::time::Duration::from_secs(65)).await;
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    assert_eq!(*emergency_rx.borrow(), false);
    assert_eq!(*dd_rx.borrow(), 0.0);

    shutdown_tx.send(true).unwrap();
    handle.await.unwrap();
    std::fs::remove_dir_all(&temp_dir).ok();
}

struct ErrorDataProvider;

#[async_trait]
impl DataProvider for ErrorDataProvider {
    async fn ohlcv(
        &self,
        _sym: &str,
        _tf: ts_core::Timeframe,
        _start: i64,
        _end: i64,
    ) -> anyhow::Result<Vec<ts_core::Bar>> {
        Ok(vec![])
    }

    async fn account(&self) -> anyhow::Result<AccountInfo> {
        Err(anyhow::anyhow!("Mock account error"))
    }

    async fn symbol_info(&self, sym: &str) -> anyhow::Result<SymbolInfo> {
        Ok(SymbolInfo {
            symbol: sym.to_string(),
            ask: 1.0,
            bid: 1.0,
            point: 0.01,
            tick_value: 0.01,
            lot_step: 0.01,
            min_lot: 0.01,
            max_lot: 1000.0,
            spread: 0.0,
            digits: 2,
            time: 0.0,
        })
    }
}

#[tokio::test(start_paused = true)]
async fn test_account_reporter_report() {
    use live::account_reporter;
    let now_nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let temp_dir = std::env::temp_dir().join(format!("test_account_reporter_report_{}", now_nanos));

    let provider = Arc::new(MockDataProvider {
        equity: 50000.0,
        balance: 50000.0,
    });
    let notifier = Arc::new(NullNotifier);
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    let handle = tokio::spawn(account_reporter::run(
        provider,
        notifier,
        temp_dir.clone(),
        shutdown_rx,
    ));

    // Yield control so that the loop initializes and schedules the sleep timer
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    // Fast forward virtual clock to trigger sleep
    tokio::time::advance(tokio::time::Duration::from_secs(7200)).await;
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    shutdown_tx.send(true).unwrap();
    handle.await.unwrap();
    std::fs::remove_dir_all(&temp_dir).ok();
}

#[tokio::test(start_paused = true)]
async fn test_account_reporter_error() {
    use live::account_reporter;
    let now_nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let temp_dir = std::env::temp_dir().join(format!("test_account_reporter_error_{}", now_nanos));

    let provider = Arc::new(ErrorDataProvider);
    let notifier = Arc::new(NullNotifier);
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    let handle = tokio::spawn(account_reporter::run(
        provider,
        notifier,
        temp_dir.clone(),
        shutdown_rx,
    ));

    // Yield control so that the loop initializes and schedules the sleep timer
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    // Fast forward virtual clock to trigger sleep
    tokio::time::advance(tokio::time::Duration::from_secs(7200)).await;
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    shutdown_tx.send(true).unwrap();
    handle.await.unwrap();
    std::fs::remove_dir_all(&temp_dir).ok();
}

struct CountingNotifier {
    count: Arc<std::sync::atomic::AtomicUsize>,
}

#[async_trait::async_trait]
impl infra::Notifier for CountingNotifier {
    async fn send(&self, _msg: &str) -> anyhow::Result<()> {
        self.count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }
}

/// Simulates the restart scenario: a report for the current hour bucket was
/// already persisted (e.g. by a process that reported right before a
/// redeploy), so the freshly-started reporter must skip sending a duplicate
/// when it wakes at that same hour boundary.
#[tokio::test(start_paused = true)]
async fn test_account_reporter_skips_duplicate_within_same_hour() {
    use chrono::Timelike;
    use live::account_reporter;

    let now_nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let temp_dir = std::env::temp_dir().join(format!("test_account_reporter_dedup_{}", now_nanos));

    let hour_bucket = chrono::Utc::now()
        .with_minute(0)
        .unwrap()
        .with_second(0)
        .unwrap()
        .with_nanosecond(0)
        .unwrap()
        .to_rfc3339();
    let cache = JsonCache::new(temp_dir.join("live"));
    cache
        .put(
            "account_report_last_hour",
            &serde_json::json!({ "hour_bucket": hour_bucket }),
        )
        .unwrap();

    let provider = Arc::new(MockDataProvider {
        equity: 50000.0,
        balance: 50000.0,
    });
    let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let notifier = Arc::new(CountingNotifier {
        count: count.clone(),
    });
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    let handle = tokio::spawn(account_reporter::run(
        provider,
        notifier,
        temp_dir.clone(),
        shutdown_rx,
    ));

    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    tokio::time::advance(tokio::time::Duration::from_secs(7200)).await;
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    shutdown_tx.send(true).unwrap();
    handle.await.unwrap();

    assert_eq!(
        count.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "report for an already-reported hour bucket must be skipped"
    );
    std::fs::remove_dir_all(&temp_dir).ok();
}

#[tokio::test]
async fn test_live_engine_graceful_shutdown() {
    use live::config::LiveConfig;
    use live::engine::LiveEngine;

    let now_nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let temp_dir = std::env::temp_dir().join(format!("test_engine_graceful_{}", now_nanos));
    std::fs::create_dir_all(&temp_dir).unwrap();

    let json = format!(
        r#"{{
        "strategy": "ema_cross",
        "symbol": "BTCUSDT",
        "timeframe": "1h",
        "risk_manager": {{
            "type": "fixed_percent",
            "pct": 0.02,
            "initial_balance": 100000.0
        }},
        "stop_manager": {{
            "type": "fixed",
            "stop_distance": 10.0,
            "start_rr": 0.0
        }},
        "indicators": {{
            "ema_fast": {{ "type": "ema", "period": 9 }},
            "ema_slow": {{ "type": "ema", "period": 21 }}
        }},
        "strategy_parameters": {{
            "stop_pct": 0.02,
            "tp_pct": 0.04
        }},
        "trade_executor": "paper",
        "data_dir": {:?}
    }}"#,
        temp_dir.to_str().unwrap()
    );

    let config = LiveConfig::from_json(&json).unwrap();
    let engine = LiveEngine::new(config);
    let handle = tokio::spawn(engine.run());

    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    #[cfg(unix)]
    {
        let pid = std::process::id();
        let _status = std::process::Command::new("kill")
            .arg("-15")
            .arg(pid.to_string())
            .status();
    }

    let run_res = handle.await.unwrap();
    assert!(run_res.is_ok());
    std::fs::remove_dir_all(&temp_dir).ok();
}

// ── FORMATTER EDGE CASES ────────────────────────────────────────────────────

#[test]
fn test_formatter_drawdown_emergency() {
    let msg = formatter::format_drawdown_emergency(5.5, 5.0);
    assert!(msg.contains("5.5"));
    assert!(msg.contains("5.0"));
}

#[test]
fn test_formatter_stop_profit() {
    let rec = TradeRecord {
        trade_id: 1,
        strategy_id: 1,
        symbol: "BTCUSDT".to_string(),
        direction: Direction::Buy,
        entry_price: 100.0,
        exit_price: 105.0,
        initial_stop_loss: 90.0,
        current_stop_loss: 102.0,
        take_profit: 120.0,
        volume: 1.0,
        open_risk: 10.0,
        entry_time: 1719000000,
        exit_time: 1719003600,
        exit_reason: ExitReason::StopProfit,
        profit: 0.5,
        currency_pnl: 5.0,
        group_id: 1,
    };
    let msg = formatter::format_stop_out(&rec);
    assert!(msg.contains("Stop"));
}

// ── UNIFIED ALERT FORMAT TESTS ──────────────────────────────────────────────

#[test]
fn test_formatter_unified_structure() {
    let pos = Position {
        trade_id: 1,
        strategy_id: 1,
        direction: Direction::Buy,
        entry_price: 1.23450,
        initial_stop_loss: 1.21000,
        current_stop_loss: 1.22000,
        take_profit: 1.30000,
        volume: 2.5,
        open_risk: 0.05,
        entry_time: 1719000000,
        is_split_chunk: false,
        group_id: 0,
    };
    let open_msg = formatter::format_trade_open(&pos, "EURUSD");
    // All position fields present
    assert!(open_msg.contains("Trade Opened"));
    assert!(open_msg.contains("Volume:"));
    assert!(open_msg.contains("Entry Price:"));
    assert!(open_msg.contains("Stop Loss:"));
    assert!(open_msg.contains("Take Profit:"));

    let update_msg = formatter::format_stop_update(&pos, "EURUSD", now_secs());
    // Stop update must now include same fields as open (not just the new stop)
    assert!(update_msg.contains("Stop Updated"));
    assert!(update_msg.contains("Volume:"));
    assert!(update_msg.contains("Entry Price:"));
    assert!(update_msg.contains("Stop Loss:"));
    assert!(update_msg.contains("Take Profit:"));

    let rec = TradeRecord {
        trade_id: 1,
        strategy_id: 1,
        symbol: "EURUSD".to_string(),
        direction: Direction::Buy,
        entry_price: 1.23450,
        exit_price: 1.25000,
        initial_stop_loss: 1.21000,
        current_stop_loss: 1.22000,
        take_profit: 1.30000,
        volume: 2.5,
        open_risk: 0.05,
        entry_time: 1719000000,
        exit_time: 1719003600,
        exit_reason: ExitReason::TakeProfit,
        profit: 1.5,
        currency_pnl: 38.75,
        group_id: 0,
    };
    let close_msg = formatter::format_take_profit(&rec);
    // Close message must include all position fields plus exit + P&L
    assert!(close_msg.contains("Take Profit"));
    assert!(close_msg.contains("Volume:"));
    assert!(close_msg.contains("Entry Price:"));
    assert!(close_msg.contains("Stop Loss:"));
    assert!(close_msg.contains("Take Profit:"));
    assert!(close_msg.contains("Profit:"));
    // Must NOT double-emit a leading emoji for the reason label
    assert!(
        !close_msg.starts_with("🎯 🎯"),
        "duplicate emoji at start of close msg"
    );
}

#[test]
fn test_formatter_stop_update_sell() {
    let pos = Position {
        trade_id: 2,
        strategy_id: 1,
        direction: Direction::Sell,
        entry_price: 100.0,
        initial_stop_loss: 110.0,
        current_stop_loss: 107.0,
        take_profit: 80.0,
        volume: 1.0,
        open_risk: 10.0,
        entry_time: 1719000000,
        is_split_chunk: false,
        group_id: 0,
    };
    let msg = formatter::format_stop_update(&pos, "BTCUSDT", now_secs());
    assert!(msg.contains("SELL"));
    assert!(msg.contains("Entry Price:"));
    assert!(msg.contains("107"));
}

// ── EXECUTOR CLOSE_AT_PRICE DEFAULT FALLBACK TEST ────────────────────────────

#[tokio::test]
async fn test_executor_close_at_price_default_fallback() {
    // The default Executor::close_at_price implementation must delegate to close().
    // MockBroker's close() always returns ExitReason::ExitRule.
    let broker = Arc::new(MockBroker::new());
    let pos = ts_core::Position {
        trade_id: 1,
        strategy_id: 1,
        direction: ts_core::Direction::Buy,
        entry_price: 100.0,
        initial_stop_loss: 90.0,
        current_stop_loss: 90.0,
        take_profit: 120.0,
        volume: 1.0,
        open_risk: 10.0,
        entry_time: 12345,
        is_split_chunk: false,
        group_id: 0,
    };
    // close_at_price default → calls close() → ExitReason::ExitRule
    let rec = broker
        .close_at_price(&pos, "BTCUSDT", pos.current_stop_loss)
        .await
        .unwrap();
    assert_eq!(rec.exit_reason, ts_core::ExitReason::ExitRule);
}

// ── LIVE CONFIG EDGE CASES ──────────────────────────────────────────────────

#[test]
fn test_live_config_with_all_fields() {
    use live::config::LiveConfig;

    let json = r#"{
        "strategy": "ema_cross",
        "symbol": "ETHUSDT",
        "timeframe": "5m",
        "stop_timeframe": "15m",
        "pyramiding": true,
        "max_open_positions": 3,
        "data_provider": "binance",
        "trade_executor": "paper",
        "bar_streamer": "binance",
        "tick_streamer": "binance",
        "risk_manager": {
            "type": "fixed_percent",
            "pct": 0.01,
            "initial_balance": 50000.0
        },
        "stop_manager": {
            "type": "variant2",
            "stop_distance": 0.5,
            "start_rr": 0.0
        },
        "exit_rules": [
            { "type": "strategy_exit", "condition": "opposite_signal" }
        ],
        "strategy_parameters": {
            "stop_pct": 0.02,
            "tp_pct": 0.04
        },
        "indicators": {
            "ema_fast": { "type": "ema", "period": 9 },
            "ema_slow": { "type": "ema", "period": 21 }
        }
    }"#;
    let config = LiveConfig::from_json(json).unwrap();
    assert_eq!(config.workers().len(), 1);
    let w = &config.workers()[0];
    assert_eq!(w.strategy, "ema_cross");
    assert_eq!(w.symbol, "ETHUSDT");
    assert_eq!(w.effective_stop_timeframe(), "15m");
    assert_eq!(w.tick_streamer_name(), "binance");
}

#[test]
fn test_live_config_defaults() {
    use live::config::LiveConfig;

    let json = r#"{
        "strategy": "ema_cross",
        "symbol": "BTCUSDT",
        "timeframe": "1h",
        "risk_manager": {},
        "stop_manager": {}
    }"#;
    let config = LiveConfig::from_json(json).unwrap();
    let w = &config.workers()[0];
    assert_eq!(w.effective_stop_timeframe(), "1h"); // defaults to timeframe
    assert_eq!(w.tick_streamer_name(), "binance"); // default
}

// ── TRADE MANAGER EDGE CASES ────────────────────────────────────────────────

#[test]
fn test_trade_manager_sell_position() {
    use live::trade_manager::TradeManager;

    struct SimpleStop {
        stop: f64,
    }
    impl risk::StopManager for SimpleStop {
        fn init(&mut self, _e: f64, s: f64, _d: Direction) {
            self.stop = s;
        }
        fn update(&mut self, _c: f64, _h: f64, _l: f64) {}
        fn stop(&self) -> f64 {
            self.stop
        }
        fn stopped_out(&self, _h: f64, _l: f64) -> bool {
            false
        }
    }

    let pos = Position {
        trade_id: 1,
        strategy_id: 1,
        direction: Direction::Sell,
        entry_price: 100.0,
        initial_stop_loss: 110.0,
        current_stop_loss: 110.0,
        take_profit: 80.0,
        volume: 1.0,
        open_risk: 10.0,
        entry_time: 1000,
        is_split_chunk: false,
        group_id: 0,
    };
    let mut tm = TradeManager::new(
        pos,
        "BTCUSDT".to_string(),
        Box::new(SimpleStop { stop: 110.0 }),
        vec![],
    );

    // Tick at 80.0 -> TP hit for sell (bid <= TP)
    let tp_tick = ts_core::Tick {
        symbol: "BTCUSDT".to_string(),
        bid: 79.0,
        ask: 79.5,
        last: 79.0,
        volume: 1.0,
        timestamp: 1001.0,
    };
    assert!(tm.check_tick_tp(&tp_tick));
    assert!(tm.is_closed());
    assert_eq!(tm.close_reason(), ExitReason::TakeProfit);
}

#[test]
fn test_trade_manager_sell_stop_hit() {
    use live::trade_manager::TradeManager;

    struct SimpleStop {
        stop: f64,
    }
    impl risk::StopManager for SimpleStop {
        fn init(&mut self, _e: f64, s: f64, _d: Direction) {
            self.stop = s;
        }
        fn update(&mut self, _c: f64, _h: f64, _l: f64) {}
        fn stop(&self) -> f64 {
            self.stop
        }
        fn stopped_out(&self, _h: f64, _l: f64) -> bool {
            false
        }
    }

    let pos = Position {
        trade_id: 1,
        strategy_id: 1,
        direction: Direction::Sell,
        entry_price: 100.0,
        initial_stop_loss: 110.0,
        current_stop_loss: 110.0,
        take_profit: 80.0,
        volume: 1.0,
        open_risk: 10.0,
        entry_time: 1000,
        is_split_chunk: false,
        group_id: 0,
    };
    let mut tm = TradeManager::new(
        pos,
        "BTCUSDT".to_string(),
        Box::new(SimpleStop { stop: 110.0 }),
        vec![],
    );

    // Tick at 111.0 -> SL hit for sell (ask >= SL)
    let sl_tick = ts_core::Tick {
        symbol: "BTCUSDT".to_string(),
        bid: 111.0,
        ask: 111.0,
        last: 111.0,
        volume: 1.0,
        timestamp: 1001.0,
    };
    assert!(tm.check_tick_stop(&sl_tick));
    assert!(tm.is_closed());
    assert_eq!(tm.close_reason(), ExitReason::StopLoss);
}

// ── DRAWDOWN MANAGER WITH ERROR PROVIDER ────────────────────────────────────

#[tokio::test(start_paused = true)]
async fn test_drawdown_manager_error_provider() {
    let now_nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let temp_dir = std::env::temp_dir().join(format!("test_dd_error_{}", now_nanos));
    std::fs::create_dir_all(&temp_dir).unwrap();

    let provider = Arc::new(ErrorDataProvider);
    let (emergency_tx, emergency_rx) = watch::channel(false);
    let (dd_tx, _dd_rx) = watch::channel(0.0);
    let notifier = Arc::new(NullNotifier);
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    let handle = tokio::spawn(drawdown_manager::run(
        provider,
        5.0,
        emergency_tx,
        dd_tx,
        notifier,
        temp_dir.clone(),
        shutdown_rx,
    ));

    tokio::time::advance(tokio::time::Duration::from_secs(65)).await;
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    // Should not trip emergency on error
    assert_eq!(*emergency_rx.borrow(), false);

    shutdown_tx.send(true).unwrap();
    handle.await.unwrap();
    std::fs::remove_dir_all(&temp_dir).ok();
}

// ── FORMATTER EDGE CASES (HTML escaping, negative P&L) ──────────────────────

#[test]
fn test_formatter_html_escaping_in_news() {
    // Titles containing `&`, `<`, `>` must be escaped so Telegram HTML mode
    // does not interpret them as markup.
    let window = infra::news::BlackoutWindow {
        start: chrono::DateTime::from_timestamp(1719000000, 0)
            .unwrap()
            .with_timezone(&chrono::Utc),
        end: chrono::DateTime::from_timestamp(1719003600, 0)
            .unwrap()
            .with_timezone(&chrono::Utc),
        events: vec![infra::news::WindowEvent {
            is_custom: false,
            event_time: chrono::DateTime::from_timestamp(1719001800, 0)
                .unwrap()
                .with_timezone(&chrono::Utc),
            country: "USD".to_string(),
            impact: "High".to_string(),
            title: "A&B <Report> \"Data\"".to_string(),
        }],
    };
    let msg = formatter::format_news_blackout(&window);
    assert!(msg.contains("A&amp;B"), "& must be escaped to &amp;");
    assert!(msg.contains("&lt;Report&gt;"), "< and > must be escaped");
    assert!(!msg.contains("A&B"), "raw & must not appear");
}

#[test]
fn test_formatter_negative_pnl_uses_loss_emoji() {
    let rec = TradeRecord {
        trade_id: 1,
        strategy_id: 1,
        symbol: "BTCUSDT".to_string(),
        direction: Direction::Buy,
        entry_price: 100.0,
        exit_price: 95.0,
        initial_stop_loss: 90.0,
        current_stop_loss: 90.0,
        take_profit: 120.0,
        volume: 1.0,
        open_risk: 10.0,
        entry_time: 1719000000,
        exit_time: 1719003600,
        exit_reason: ExitReason::StopLoss,
        profit: -0.5,
        currency_pnl: -5.0,
        group_id: 1,
    };
    let msg = formatter::format_stop_out(&rec);
    assert!(msg.contains("💸"), "negative P&L must use 💸 emoji");
    assert!(
        !msg.contains("💰"),
        "positive emoji must not appear for a loss"
    );
}

#[test]
fn test_formatter_positive_pnl_uses_profit_emoji() {
    let rec = TradeRecord {
        trade_id: 2,
        strategy_id: 1,
        symbol: "BTCUSDT".to_string(),
        direction: Direction::Buy,
        entry_price: 100.0,
        exit_price: 110.0,
        initial_stop_loss: 90.0,
        current_stop_loss: 90.0,
        take_profit: 120.0,
        volume: 1.0,
        open_risk: 10.0,
        entry_time: 1719000000,
        exit_time: 1719003600,
        exit_reason: ExitReason::TakeProfit,
        profit: 1.0,
        currency_pnl: 10.0,
        group_id: 1,
    };
    let msg = formatter::format_take_profit(&rec);
    assert!(msg.contains("💰"), "positive P&L must use 💰 emoji");
    assert!(!msg.contains("💸"), "loss emoji must not appear for a win");
}

// ── LIVE CONFIG EDGE CASES ──────────────────────────────────────────────────

#[test]
fn test_live_config_tick_streamer_inherits_bar_streamer() {
    use live::config::LiveConfig;
    // When tick_streamer is omitted, tick_streamer_name() must fall back to bar_streamer.
    let json = r#"{
        "strategy": "ema_cross",
        "symbol": "BTCUSDT",
        "timeframe": "1h",
        "bar_streamer": "mt5",
        "risk_manager": {},
        "stop_manager": {}
    }"#;
    let config = LiveConfig::from_json(json).unwrap();
    let w = &config.workers()[0];
    assert_eq!(w.bar_streamer, "mt5");
    assert_eq!(
        w.tick_streamer_name(),
        "mt5",
        "tick_streamer_name() must inherit bar_streamer when tick_streamer is None"
    );
}

#[test]
fn test_live_config_explicit_tick_streamer_overrides_bar_streamer() {
    use live::config::LiveConfig;
    let json = r#"{
        "strategy": "ema_cross",
        "symbol": "BTCUSDT",
        "timeframe": "1h",
        "bar_streamer": "binance",
        "tick_streamer": "mt5",
        "risk_manager": {},
        "stop_manager": {}
    }"#;
    let config = LiveConfig::from_json(json).unwrap();
    let w = &config.workers()[0];
    assert_eq!(w.bar_streamer, "binance");
    assert_eq!(
        w.tick_streamer_name(),
        "mt5",
        "explicit tick_streamer must take priority over bar_streamer"
    );
}

#[test]
fn test_live_config_max_open_positions_default_is_none() {
    use live::config::LiveConfig;
    let json = r#"{
        "strategy": "ema_cross",
        "symbol": "BTCUSDT",
        "timeframe": "1h",
        "risk_manager": {},
        "stop_manager": {}
    }"#;
    let config = LiveConfig::from_json(json).unwrap();
    assert!(
        config.workers()[0].max_open_positions.is_none(),
        "max_open_positions must default to None (unlimited)"
    );
}

#[test]
fn test_live_config_process_incomplete_bars_default_false() {
    use live::config::LiveConfig;
    let json = r#"{
        "strategy": "ema_cross",
        "symbol": "BTCUSDT",
        "timeframe": "1h",
        "risk_manager": {},
        "stop_manager": {}
    }"#;
    let config = LiveConfig::from_json(json).unwrap();
    assert!(
        !config.workers()[0].process_incomplete_bars,
        "process_incomplete_bars must default to false"
    );
}

#[test]
fn test_live_config_stop_timeframe_defaults_to_timeframe() {
    use live::config::LiveConfig;
    let json = r#"{
        "strategy": "ema_cross",
        "symbol": "BTCUSDT",
        "timeframe": "4h",
        "risk_manager": {},
        "stop_manager": {}
    }"#;
    let config = LiveConfig::from_json(json).unwrap();
    assert_eq!(
        config.workers()[0].effective_stop_timeframe(),
        "4h",
        "when stop_timeframe is absent, effective_stop_timeframe() must return timeframe"
    );

    // "stop_timeframe": "timeframe"
    let json2 = r#"{
        "strategy": "ema_cross",
        "symbol": "BTCUSDT",
        "timeframe": "4h",
        "stop_timeframe": "timeframe",
        "risk_manager": {},
        "stop_manager": {}
    }"#;
    let config2 = LiveConfig::from_json(json2).unwrap();
    assert_eq!(config2.workers()[0].effective_stop_timeframe(), "4h");

    // "stop_timeframe": "15m"
    let json3 = r#"{
        "strategy": "ema_cross",
        "symbol": "BTCUSDT",
        "timeframe": "4h",
        "stop_timeframe": "15m",
        "risk_manager": {},
        "stop_manager": {}
    }"#;
    let config3 = LiveConfig::from_json(json3).unwrap();
    assert_eq!(config3.workers()[0].effective_stop_timeframe(), "15m");

    // "stop_timeframe": ["5m"]
    let json4 = r#"{
        "strategy": "ema_cross",
        "symbol": "BTCUSDT",
        "timeframe": "4h",
        "stop_timeframe": ["5m"],
        "risk_manager": {},
        "stop_manager": {}
    }"#;
    let config4 = LiveConfig::from_json(json4).unwrap();
    assert_eq!(config4.workers()[0].effective_stop_timeframe(), "5m");

    // "stop_timeframe": ["timeframe"]
    let json5 = r#"{
        "strategy": "ema_cross",
        "symbol": "BTCUSDT",
        "timeframe": "4h",
        "stop_timeframe": ["timeframe"],
        "risk_manager": {},
        "stop_manager": {}
    }"#;
    let config5 = LiveConfig::from_json(json5).unwrap();
    assert_eq!(config5.workers()[0].effective_stop_timeframe(), "4h");
}
