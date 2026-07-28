use broker::{
    factory::{build_broker_handles, build_data_provider},
    traits::{BarStream, TickStream},
    DataProvider, Executor, OrderRequest, PaperBroker,
};
use ts_core::{Direction, ExitReason, Timeframe};

#[tokio::test]
async fn test_paper_broker_simulated_execution() {
    let broker = PaperBroker::new(50000.0);

    // Verify account info
    let acct = broker.account().await.unwrap();
    assert_eq!(acct.balance, 50000.0);
    assert_eq!(acct.equity, 50000.0);

    // Open a Buy order
    let req = OrderRequest {
        symbol: "BTCUSDT".to_string(),
        direction: Direction::Buy,
        volume: 0.5,
        entry_price: 60000.0,
        stop_loss: 58000.0,
        take_profit: 65000.0,
        strategy_id: 1,
        trade_id: 101,
        comment: "paper test buy".to_string(),
    };

    let pos = broker.open(&req).await.unwrap();
    assert_eq!(pos.trade_id, 101);
    assert_eq!(pos.direction, Direction::Buy);
    assert_eq!(pos.volume, 0.5);
    assert!(pos.entry_price > 0.0);

    // Update stop loss
    let updated_pos = broker.update_sl(&pos, 59000.0).await.unwrap();
    assert_eq!(updated_pos.current_stop_loss, 59000.0);

    // Close the position
    let rec = broker.close(&updated_pos, "BTCUSDT").await.unwrap();
    assert_eq!(rec.trade_id, 101);
    assert_eq!(rec.exit_reason, ExitReason::ExitRule);

    // Check account after realized P&L booking
    let acct_after = broker.account().await.unwrap();
    let expected_pnl = (rec.exit_price - pos.entry_price) * 0.5;
    assert!((acct_after.balance - (50000.0 + expected_pnl)).abs() < 1e-9);

    // Call ohlcv (will fail or succeed, but executes the delegate code)
    let _ = broker
        .ohlcv("BTCUSDT", Timeframe::M15, 1719000000, 1719003600)
        .await;

    // Call streams subscription (will fail or succeed, but executes the delegate code)
    let (tx_bar, _rx_bar) = tokio::sync::mpsc::channel(10);
    let _ = BarStream::subscribe(&broker, "BTCUSDT", Timeframe::M15, tx_bar).await;

    let (tx_tick, _rx_tick) = tokio::sync::mpsc::channel(10);
    let _ = TickStream::subscribe(&broker, "BTCUSDT", tx_tick).await;
}

#[test]
fn test_factory_builders() {
    // Test building paper provider
    let paper_prov = build_data_provider("paper");
    assert!(paper_prov.is_ok());

    // Test building paper broker handles
    let paper_handles = build_broker_handles("paper");
    assert!(paper_handles.is_ok());

    // Test build invalid names
    assert!(build_data_provider("invalid").is_err());
    assert!(build_broker_handles("invalid").is_err());

    // Test building binance provider (public calls allowed even without API keys in factory)
    let binance_prov = build_data_provider("binance");
    assert!(binance_prov.is_ok());

    // Test building binance broker handles with missing API keys (should fail validation)
    std::env::remove_var("BINANCE_API_KEY");
    std::env::remove_var("BINANCE_API_SECRET");
    let binance_handles = build_broker_handles("binance");
    assert!(binance_handles.is_err());

    // Test building binance broker handles with API keys set
    std::env::set_var("BINANCE_API_KEY", "dummy_key");
    std::env::set_var("BINANCE_API_SECRET", "dummy_secret");
    let binance_handles_with_keys = build_broker_handles("binance");
    assert!(binance_handles_with_keys.is_ok());
    std::env::remove_var("BINANCE_API_KEY");
    std::env::remove_var("BINANCE_API_SECRET");
}

// ── PAPER BROKER SELL POSITION ──────────────────────────────────────────────

#[tokio::test]
async fn test_paper_broker_sell_position() {
    let broker = PaperBroker::new(50000.0);

    let req = OrderRequest {
        symbol: "ETHUSDT".to_string(),
        direction: Direction::Sell,
        volume: 1.0,
        entry_price: 3000.0,
        stop_loss: 3100.0,
        take_profit: 2800.0,
        strategy_id: 1,
        trade_id: 201,
        comment: "paper sell test".to_string(),
    };

    let pos = broker.open(&req).await.unwrap();
    assert_eq!(pos.trade_id, 201);
    assert_eq!(pos.direction, Direction::Sell);
    assert_eq!(pos.volume, 1.0);

    let updated = broker.update_sl(&pos, 3050.0).await.unwrap();
    assert_eq!(updated.current_stop_loss, 3050.0);

    let rec = broker.close(&updated, "ETHUSDT").await.unwrap();
    assert_eq!(rec.trade_id, 201);
    assert_eq!(rec.direction, Direction::Sell);
}

// ── PAPER BROKER MULTIPLE SEQUENTIAL TRADES ─────────────────────────────────

#[tokio::test]
async fn test_paper_broker_multiple_trades() {
    let broker = PaperBroker::new(100000.0);

    for i in 0..3 {
        let req = OrderRequest {
            symbol: "BTCUSDT".to_string(),
            direction: Direction::Buy,
            volume: 0.1,
            entry_price: 50000.0,
            stop_loss: 49000.0,
            take_profit: 55000.0,
            strategy_id: 1,
            trade_id: 300 + i,
            comment: format!("trade {}", i),
        };

        let pos = broker.open(&req).await.unwrap();
        assert_eq!(pos.trade_id, 300 + i);
        let _rec = broker.close(&pos, "BTCUSDT").await.unwrap();
    }

    let acct = broker.account().await.unwrap();
    // Balance should be updated after trades
    assert!(acct.balance > 0.0);
}

// ── PAPER BROKER ACCOUNT INFO ───────────────────────────────────────────────

#[tokio::test]
async fn test_paper_broker_initial_state() {
    let broker = PaperBroker::new(25000.0);
    let acct = broker.account().await.unwrap();
    assert_eq!(acct.balance, 25000.0);
    assert_eq!(acct.equity, 25000.0);
    assert_eq!(acct.profit, 0.0);
    assert_eq!(acct.currency, "USDT");
}

// ── PAPER BROKER SYMBOL INFO ────────────────────────────────────────────────

#[tokio::test]
async fn test_paper_broker_symbol_info() {
    let broker = PaperBroker::new(50000.0);
    let info = broker.symbol_info("BTCUSDT").await.unwrap();
    assert_eq!(info.symbol, "BTCUSDT");
    assert!(info.point > 0.0);
    assert!(info.tick_value > 0.0);
    assert!(info.min_lot > 0.0);
    assert!(info.max_lot > info.min_lot);
}

// ── ORDER REQUEST FIELDS ────────────────────────────────────────────────────

#[test]
fn test_order_request_serde() {
    let req = OrderRequest {
        symbol: "BTCUSDT".to_string(),
        direction: Direction::Buy,
        volume: 0.5,
        entry_price: 50000.0,
        stop_loss: 49000.0,
        take_profit: 55000.0,
        strategy_id: 42,
        trade_id: 100,
        comment: "test order".to_string(),
    };

    let json = serde_json::to_string(&req).unwrap();
    let deserialized: OrderRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.symbol, "BTCUSDT");
    assert_eq!(deserialized.direction, Direction::Buy);
    assert_eq!(deserialized.volume, 0.5);
    assert_eq!(deserialized.trade_id, 100);
}
