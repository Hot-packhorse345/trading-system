use risk::{
    advance_stop, any_should_exit, build_exit, build_stop, build_volume, BreakevenTrail,
    ExitManager, ExitManagerConfig, FixedAmount, FixedPercent, FixedStop, StopManager,
    StopManagerConfig, StrategyExit, SupertrendTrail, TieredPercent, TimeExit, TslVariant1,
    TslVariant2, TslVariant3, TslVariant4, VolumeManager, VolumeManagerConfig, VolumeTier,
};
use ts_core::{AccountInfo, Bar, Direction, IndicatorSet, Params, Position, Signal, SymbolInfo};

// Helper to construct SymbolInfo
fn make_symbol(min_lot: f64, lot_step: f64) -> SymbolInfo {
    SymbolInfo {
        symbol: "BTCUSDT".to_string(),
        ask: 50000.0,
        bid: 50000.0,
        point: 0.1,
        tick_value: 0.1,
        lot_step,
        min_lot,
        max_lot: 1000.0,
        spread: 0.0,
        digits: 1,
        time: 0.0,
    }
}

// Helper to construct AccountInfo
fn make_account(balance: f64) -> AccountInfo {
    AccountInfo {
        balance,
        equity: balance,
        profit: 0.0,
        currency: "USD".to_string(),
        margin: 0.0,
        margin_free: balance,
    }
}

// ── VOLUME MANAGER TESTS ─────────────────────────────────────────────────────

#[test]
fn test_fixed_amount() {
    let vm = FixedAmount { amount: 100.0 };
    let sym = make_symbol(0.01, 0.01);
    let acct = make_account(10000.0);

    // Risk points = |50000 - 49000| = 1000.0
    // formula: amount / (risk_pts / point * tick_value)
    // 100 / (1000 / 0.1 * 0.1) = 100 / 1000 = 0.1 lot
    let size = vm.position_size(&acct, &sym, 50000.0, 49000.0, 0.0);
    assert_eq!(size, 0.1);

    // Edge case: zero risk points
    let size_zero = vm.position_size(&acct, &sym, 50000.0, 50000.0, 0.0);
    assert_eq!(size_zero, sym.min_lot);
}

#[test]
fn test_fixed_percent() {
    let vm = FixedPercent {
        pct: 0.01,
        initial_balance: 10000.0,
    }; // 100.0 risk
    let sym = make_symbol(0.01, 0.01);
    let acct = make_account(10000.0);

    // 1000 risk points -> 0.1 lot
    let size = vm.position_size(&acct, &sym, 50000.0, 49000.0, 0.0);
    assert_eq!(size, 0.1);

    // Edge case: zero risk points
    let size_zero = vm.position_size(&acct, &sym, 50000.0, 50000.0, 0.0);
    assert_eq!(size_zero, sym.min_lot);
}

#[test]
fn test_tiered_percent() {
    // Tiers of (dd_min_pct, dd_max_pct, risk_pct)
    let vm = TieredPercent {
        tiers: vec![
            (0.0, 2.0, 0.01),  // 0% to 2% DD -> 1% risk
            (2.0, 4.0, 0.005), // 2% to 4% DD -> 0.5% risk
        ],
        daily_dd_limit: 4.0,
    };
    let sym = make_symbol(0.001, 0.001);
    let acct = make_account(10000.0);

    // Case 1: Drawdown = 1.0% (Tier 1 -> 1% risk = $100)
    // Risk pts = 1000 -> 0.1 lot
    let size1 = vm.position_size(&acct, &sym, 50000.0, 49000.0, 0.01);
    assert_eq!(size1, 0.1);

    // Case 2: Drawdown = 3.0% (Tier 2 -> 0.5% risk = $50)
    // Risk pts = 1000 -> 0.05 lot
    let size2 = vm.position_size(&acct, &sym, 50000.0, 49000.0, 0.03);
    assert_eq!(size2, 0.05);

    // Case 3: Drawdown = 5.0% (Hits limit of 4.0%) -> 0.0 lot
    let size3 = vm.position_size(&acct, &sym, 50000.0, 49000.0, 0.05);
    assert_eq!(size3, 0.0);
}

// ── STOP MANAGER TESTS ───────────────────────────────────────────────────────

#[test]
fn test_fixed_stop() {
    let mut sm = FixedStop::default();
    sm.init(100.0, 95.0, Direction::Buy);

    assert_eq!(sm.stop(), 95.0);

    // update does nothing
    sm.update(105.0, 106.0, 104.0);
    assert_eq!(sm.stop(), 95.0);

    // stopped out checks
    assert!(sm.stopped_out(100.0, 95.0));
    assert!(sm.stopped_out(100.0, 94.0));
    assert!(!sm.stopped_out(100.0, 96.0));
}

#[test]
fn test_tsl_variant_1() {
    let mut sm = TslVariant1::new(0.5); // steps of 0.5 R
    sm.init(100.0, 90.0, Direction::Buy); // risk = 10.0

    assert_eq!(sm.stop(), 90.0);

    // Profit = 4.0 (0.4 R) -> no update (under 0.5 R threshold)
    sm.update(104.0, 104.0, 104.0);
    assert_eq!(sm.stop(), 90.0);

    // Profit = 6.0 (0.6 R) -> next_r = 1.0 + 1.0 = 2.0?
    // let profit_r = profit_size / stop_size;
    // let next_r = (profit_r / self.stop_distance).floor() + 1.0;
    // For profit_r = 0.6, next_r = (0.6 / 0.5).floor() + 1.0 = 1.0 + 1.0 = 2.0.
    // step_r = next_r - 1.0 = 1.0.
    // offset = step_r * stop_distance * stop_size = 1.0 * 0.5 * 10.0 = 5.0.
    // new_stop = initial_sl + offset = 90.0 + 5.0 = 95.0.
    sm.update(106.0, 106.0, 106.0);
    assert_eq!(sm.stop(), 95.0);

    // Try to update with lower price -> stop should not loosen
    sm.update(98.0, 98.0, 98.0);
    assert_eq!(sm.stop(), 95.0);
}

#[test]
fn test_tsl_variant_2() {
    let mut sm = TslVariant2::new(0.5);
    sm.init(100.0, 90.0, Direction::Buy); // risk = 10.0

    // profit_r = 0.4 (< 0.5) -> no update
    sm.update(104.0, 104.0, 104.0);
    assert_eq!(sm.stop(), 90.0);

    // profit_r = 0.6 -> target_r = 0.5. Stop moves to entry + 0.5*risk = 105.0.
    sm.update(106.0, 106.0, 106.0);
    assert_eq!(sm.stop(), 105.0);

    // profit_r = 1.1 -> k = (1.1-0.5)/0.5 = 1.0 -> target_r = 0.5 + 0.5 = 1.0. Stop to entry + 1.0*risk = 110.0.
    sm.update(111.0, 111.0, 111.0);
    assert_eq!(sm.stop(), 110.0);
}

#[test]
fn test_tsl_variant_3() {
    let mut sm = TslVariant3::new(0.5, 1.0); // start trailing at 1.0 RR
    sm.init(100.0, 90.0, Direction::Buy);

    // profit_r = 0.8 (< 1.0 start_rr) -> no update
    sm.update(108.0, 108.0, 108.0);
    assert_eq!(sm.stop(), 90.0);

    // profit_r = 1.2 -> steps_over = (1.2-1.0)/0.5 = 0.0.
    // target_rr = 1.0. stop_rr = target_rr - 0.5 = 0.5.
    // stop = entry + 0.5 * risk = 105.0.
    sm.update(112.0, 112.0, 112.0);
    assert_eq!(sm.stop(), 105.0);
}

#[test]
fn test_tsl_variant_4() {
    let mut sm = TslVariant4::new(0.5, 1.0);
    sm.init(100.0, 90.0, Direction::Buy);

    // profit_r = 1.2 -> target_rr = 1.0. stop = entry + 1.0 * risk = 110.0.
    sm.update(112.0, 112.0, 112.0);
    assert_eq!(sm.stop(), 110.0);
}

#[test]
fn test_breakeven_trail() {
    let mut sm = BreakevenTrail::new(1.0, 2.0); // BE at 1.0 RR, trail distance = 2.0
    sm.init(100.0, 90.0, Direction::Buy);

    // R = 0.8 (< 1.0 BE) -> no update
    sm.update(108.0, 108.0, 108.0);
    assert_eq!(sm.stop(), 90.0);

    // R = 1.2 (>= 1.0 BE) -> moves to entry (100.0)
    sm.update(112.0, 112.0, 112.0);
    assert_eq!(sm.stop(), 100.0);

    // Once at BE, trails close by distance 2.0. Close = 115.0 -> stop = 113.0
    sm.update(115.0, 115.0, 115.0);
    assert_eq!(sm.stop(), 113.0);
}

#[test]
fn test_supertrend_trail() {
    let mut sm = SupertrendTrail::new(3, 1.5);
    sm.init(100.0, 95.0, Direction::Buy);

    // Feed rising bars to seed WilderAtr
    let bars = [
        (100.0, 101.0, 99.0),
        (102.0, 103.0, 101.0),
        (104.0, 105.0, 103.0),
        (106.0, 107.0, 105.0),
    ];
    for &(c, h, l) in &bars {
        sm.update(c, h, l);
    }
    // ATR should be seeded, stop should have moved but must remain below close
    assert!(sm.stop() > 95.0);
    assert!(sm.stop() < 106.0);
}

#[test]
fn test_advance_stop() {
    let mut sm = FixedStop::default();
    let mut pos = Position {
        trade_id: 1,
        strategy_id: 1,
        direction: Direction::Buy,
        entry_price: 100.0,
        initial_stop_loss: 90.0,
        current_stop_loss: 90.0,
        take_profit: 150.0,
        volume: 1.0,
        open_risk: 10.0,
        entry_time: 1234,
        is_split_chunk: false,
        group_id: 0,
    };

    // Fixed stop doesn't move -> advance_stop returns None, position unchanged
    let res = advance_stop(&mut sm, &mut pos, 105.0, 106.0, 104.0);
    assert!(res.is_none());
    assert_eq!(pos.current_stop_loss, 90.0);

    // Let's use a moving trailing stop
    let mut sm2 = TslVariant1::new(0.5);
    sm2.init(100.0, 90.0, Direction::Buy);

    // Should update to 95.0 when price reaches 106.0
    let res2 = advance_stop(&mut sm2, &mut pos, 106.0, 106.0, 106.0);
    assert_eq!(res2, Some(95.0));
    assert_eq!(pos.current_stop_loss, 95.0);
}

// ── EXIT MANAGER TESTS ───────────────────────────────────────────────────────

#[test]
fn test_strategy_exit() {
    let pos = Position {
        trade_id: 1,
        strategy_id: 1,
        direction: Direction::Buy,
        entry_price: 100.0,
        initial_stop_loss: 90.0,
        current_stop_loss: 90.0,
        take_profit: 150.0,
        volume: 1.0,
        open_risk: 10.0,
        entry_time: 1234,
        is_split_chunk: false,
        group_id: 0,
    };

    let bars = [Bar::new(1000, 100.0, 100.0, 100.0, 100.0, 1.0)];
    let cols = IndicatorSet::default();
    let params = Params::default();

    // ── Opposite Signal ──
    let exit_opp = StrategyExit {
        condition: risk::exit::StrategyExitCondition::OppositeSignal,
    };
    let sig_buy = Signal::new(Direction::Buy, 100.0, 90.0, 0.0);
    let sig_sell = Signal::new(Direction::Sell, 100.0, 110.0, 0.0);
    let sig_hold = Signal::new(Direction::Hold, 100.0, 0.0, 0.0);

    assert!(exit_opp.should_exit(&pos, 0, &bars, &cols, &params, &sig_sell, None));
    assert!(!exit_opp.should_exit(&pos, 0, &bars, &cols, &params, &sig_buy, None));
    assert!(!exit_opp.should_exit(&pos, 0, &bars, &cols, &params, &sig_hold, None));

    // ── Bias Flip ──
    let exit_flip = StrategyExit {
        condition: risk::exit::StrategyExitCondition::BiasFlip,
    };
    // prev signal matches pos direction (Buy), current signal changes (Sell or Hold)
    assert!(exit_flip.should_exit(
        &pos,
        0,
        &bars,
        &cols,
        &params,
        &sig_sell,
        Some(Direction::Buy)
    ));
    assert!(exit_flip.should_exit(
        &pos,
        0,
        &bars,
        &cols,
        &params,
        &sig_hold,
        Some(Direction::Buy)
    ));
    // prev signal does not match pos direction
    assert!(!exit_flip.should_exit(
        &pos,
        0,
        &bars,
        &cols,
        &params,
        &sig_sell,
        Some(Direction::Sell)
    ));
}

#[test]
fn test_time_exit() {
    let pos = Position {
        trade_id: 1,
        strategy_id: 1,
        direction: Direction::Buy,
        entry_price: 100.0,
        initial_stop_loss: 90.0,
        current_stop_loss: 90.0,
        take_profit: 150.0,
        volume: 1.0,
        open_risk: 10.0,
        entry_time: 1000, // enter at timestamp 1000
        is_split_chunk: false,
        group_id: 0,
    };
    let cols = IndicatorSet::default();
    let params = Params::default();
    let sig = Signal::new(Direction::Hold, 100.0, 0.0, 0.0);

    // ── delta_time ──
    // limit = 2 minutes = 120 seconds
    let te_delta = TimeExit {
        delta_time: Some("2_minute".to_string()),
        max_bars: None,
        exit_hour_utc: None,
    };

    // Elapsed = 1100 - 1000 = 100 (< 120) -> no exit
    let bars1 = [Bar::new(1100, 100.0, 100.0, 100.0, 100.0, 1.0)];
    assert!(!te_delta.should_exit(&pos, 0, &bars1, &cols, &params, &sig, None));

    // Elapsed = 1120 - 1000 = 120 (>= 120) -> exit
    let bars2 = [Bar::new(1120, 100.0, 100.0, 100.0, 100.0, 1.0)];
    assert!(te_delta.should_exit(&pos, 0, &bars2, &cols, &params, &sig, None));

    // ── max_bars ──
    let te_bars = TimeExit {
        delta_time: None,
        max_bars: Some(2),
        exit_hour_utc: None,
    };
    // entry_ts = 1000. entry bar is at index 0. Current bar at index 1 -> elapsed bars = 1 (< 2)
    let bars3 = [
        Bar::new(1000, 100.0, 100.0, 100.0, 100.0, 1.0),
        Bar::new(1010, 100.0, 100.0, 100.0, 100.0, 1.0),
    ];
    assert!(!te_bars.should_exit(&pos, 1, &bars3, &cols, &params, &sig, None));

    // Current bar at index 2 -> elapsed bars = 2 (>= 2) -> exit
    let bars4 = [
        Bar::new(1000, 100.0, 100.0, 100.0, 100.0, 1.0),
        Bar::new(1010, 100.0, 100.0, 100.0, 100.0, 1.0),
        Bar::new(1020, 100.0, 100.0, 100.0, 100.0, 1.0),
    ];
    assert!(te_bars.should_exit(&pos, 2, &bars4, &cols, &params, &sig, None));

    // ── exit_hour_utc ──
    // exit_hour_utc = 17:00 (17 * 3600 = 61200 seconds into the day)
    let te_hour = TimeExit {
        delta_time: None,
        max_bars: None,
        exit_hour_utc: Some(17),
    };

    // Bar time = 17:05 (61500 secs into day) -> >= 17 -> exit
    let bars5 = [Bar::new(61500, 100.0, 100.0, 100.0, 100.0, 1.0)];
    assert!(te_hour.should_exit(&pos, 0, &bars5, &cols, &params, &sig, None));

    // Bar time = 16:59 (61140 secs into day) -> < 17 -> no exit
    let bars6 = [Bar::new(61140, 100.0, 100.0, 100.0, 100.0, 1.0)];
    assert!(!te_hour.should_exit(&pos, 0, &bars6, &cols, &params, &sig, None));
}

#[test]
fn test_any_should_exit() {
    let pos = Position {
        trade_id: 1,
        strategy_id: 1,
        direction: Direction::Buy,
        entry_price: 100.0,
        initial_stop_loss: 90.0,
        current_stop_loss: 90.0,
        take_profit: 150.0,
        volume: 1.0,
        open_risk: 10.0,
        entry_time: 1000,
        is_split_chunk: false,
        group_id: 0,
    };
    let bars = [Bar::new(1100, 100.0, 100.0, 100.0, 100.0, 1.0)];
    let cols = IndicatorSet::default();
    let params = Params::default();
    let sig = Signal::new(Direction::Hold, 100.0, 0.0, 0.0);

    let te1 = Box::new(TimeExit {
        delta_time: Some("2_minute".to_string()),
        max_bars: None,
        exit_hour_utc: None,
    }) as Box<dyn ExitManager>;
    let te2 = Box::new(TimeExit {
        delta_time: Some("1_minute".to_string()),
        max_bars: None,
        exit_hour_utc: None,
    }) as Box<dyn ExitManager>;

    // te2 (1m = 60s) triggers exit (since elapsed=100s >= 60s), te1 (2m = 120s) does not. `any_should_exit` should return true.
    let managers = vec![te1, te2];
    assert!(any_should_exit(
        &managers, &pos, 0, &bars, &cols, &params, &sig, None
    ));
}

// ── FACTORY TESTS ────────────────────────────────────────────────────────────

#[test]
fn test_build_volume_factory() {
    let cfg1 = VolumeManagerConfig::FixedPercent {
        pct: 0.02,
        initial_balance: 50000.0,
    };
    let vm1 = build_volume(&cfg1).unwrap();
    let sym = make_symbol(0.01, 0.01);
    let acct = make_account(10000.0);
    let size1 = vm1.position_size(&acct, &sym, 50000.0, 49000.0, 0.0);
    assert_eq!(size1, 1.0);

    let cfg2 = VolumeManagerConfig::FixedAmount { amount: 200.0 };
    let vm2 = build_volume(&cfg2).unwrap();
    let size2 = vm2.position_size(&acct, &sym, 50000.0, 49000.0, 0.0);
    assert_eq!(size2, 0.2);

    let cfg3 = VolumeManagerConfig::TieredPercent {
        tiers: vec![VolumeTier {
            dd_min: 0.0,
            dd_max: 2.0,
            risk_pct: 0.01,
        }],
        daily_dd_limit: 5.0,
    };
    let vm3 = build_volume(&cfg3).unwrap();
    let size3 = vm3.position_size(&acct, &sym, 50000.0, 49000.0, 0.01);
    assert_eq!(size3, 0.1);
}

#[test]
fn test_build_stop_factory() {
    let cfg1 = StopManagerConfig {
        sm_type: "fixed".to_string(),
        stop_distance: 0.0,
        start_rr: 0.0,
    };
    assert_eq!(build_stop(&cfg1).stop(), 0.0);

    let cfg2 = StopManagerConfig {
        sm_type: "variant1".to_string(),
        stop_distance: 0.5,
        start_rr: 0.0,
    };
    assert_eq!(build_stop(&cfg2).stop(), 0.0);

    let cfg3 = StopManagerConfig {
        sm_type: "variant2".to_string(),
        stop_distance: 0.5,
        start_rr: 0.0,
    };
    assert_eq!(build_stop(&cfg3).stop(), 0.0);

    let cfg4 = StopManagerConfig {
        sm_type: "variant3".to_string(),
        stop_distance: 0.5,
        start_rr: 1.0,
    };
    assert_eq!(build_stop(&cfg4).stop(), 0.0);

    let cfg5 = StopManagerConfig {
        sm_type: "variant4".to_string(),
        stop_distance: 0.5,
        start_rr: 1.0,
    };
    assert_eq!(build_stop(&cfg5).stop(), 0.0);

    let cfg6 = StopManagerConfig {
        sm_type: "atr_trail".to_string(),
        stop_distance: 1.5,
        start_rr: 14.0,
    };
    assert_eq!(build_stop(&cfg6).stop(), 0.0);

    let cfg6_fallback = StopManagerConfig {
        sm_type: "atr_trail".to_string(),
        stop_distance: 1.5,
        start_rr: 0.0,
    };
    assert_eq!(build_stop(&cfg6_fallback).stop(), 0.0);

    let cfg7 = StopManagerConfig {
        sm_type: "breakeven".to_string(),
        stop_distance: 2.0,
        start_rr: 1.0,
    };
    assert_eq!(build_stop(&cfg7).stop(), 0.0);

    let cfg8 = StopManagerConfig {
        sm_type: "supertrend".to_string(),
        stop_distance: 2.0,
        start_rr: 10.0,
    };
    assert_eq!(build_stop(&cfg8).stop(), 0.0);

    let cfg8_fallback = StopManagerConfig {
        sm_type: "supertrend".to_string(),
        stop_distance: 2.0,
        start_rr: 0.0,
    };
    assert_eq!(build_stop(&cfg8_fallback).stop(), 0.0);

    let cfg_fallback = StopManagerConfig {
        sm_type: "invalid_type".to_string(),
        stop_distance: 0.0,
        start_rr: 0.0,
    };
    assert_eq!(build_stop(&cfg_fallback).stop(), 0.0);
}

#[test]
fn test_build_exit_factory() {
    let cfgs = vec![
        ExitManagerConfig::Strategy {
            condition: "opposite_signal".to_string(),
        },
        ExitManagerConfig::Strategy {
            condition: "bias_flip".to_string(),
        },
        ExitManagerConfig::Time {
            delta_time: Some("1_hour".to_string()),
            max_bars: Some(5),
            exit_hour_utc: Some(17),
        },
    ];
    let ex_mgrs = build_exit(&cfgs);
    assert_eq!(ex_mgrs.len(), 3);
}

// ── SELL DIRECTION STOP MANAGER TESTS ───────────────────────────────────────

#[test]
fn test_fixed_stop_sell() {
    let mut sm = FixedStop::default();
    sm.init(100.0, 110.0, Direction::Sell);

    assert_eq!(sm.stop(), 110.0);
    sm.update(95.0, 96.0, 94.0);
    assert_eq!(sm.stop(), 110.0);

    assert!(sm.stopped_out(110.0, 105.0));
    assert!(!sm.stopped_out(109.0, 105.0));
}

#[test]
fn test_tsl_variant_1_sell() {
    let mut sm = TslVariant1::new(0.5);
    sm.init(100.0, 110.0, Direction::Sell);

    // Price drops from 100 to 94 -> profit_r = (100-94)/10 = 0.6 R
    sm.update(94.0, 94.0, 94.0);
    assert_eq!(sm.stop(), 105.0); // initial_sl(110) - offset(5) = 105

    // Stop never loosens
    sm.update(99.0, 99.0, 99.0);
    assert_eq!(sm.stop(), 105.0);
}

#[test]
fn test_tsl_variant_2_sell() {
    let mut sm = TslVariant2::new(0.5);
    sm.init(100.0, 110.0, Direction::Sell);

    sm.update(94.0, 94.0, 94.0); // profit_r = 0.6 -> target_r = 0.5
                                 // For sell: stop = entry - 0.5*risk = 100 - 0.5*10 = 95
    assert_eq!(sm.stop(), 95.0);
}

#[test]
fn test_tsl_variant_3_sell() {
    let mut sm = TslVariant3::new(0.5, 1.0);
    sm.init(100.0, 110.0, Direction::Sell);

    // Not enough profit yet
    sm.update(98.0, 98.0, 98.0); // 0.2 R < 1.0 start_rr
    assert_eq!(sm.stop(), 110.0);

    // 1.2 R profit -> trailing kicks in
    sm.update(88.0, 88.0, 88.0);
    assert!(sm.stop() < 110.0);
}

#[test]
fn test_tsl_variant_4_sell() {
    let mut sm = TslVariant4::new(0.5, 1.0);
    sm.init(100.0, 110.0, Direction::Sell);

    sm.update(88.0, 88.0, 88.0); // 1.2 R -> target_rr = 1.0
                                 // For sell: stop = entry - 1.0 * risk = 100 - 10 = 90
    assert_eq!(sm.stop(), 90.0);
}

#[test]
fn test_breakeven_trail_sell() {
    let mut sm = BreakevenTrail::new(1.0, 2.0);
    sm.init(100.0, 110.0, Direction::Sell);

    sm.update(98.0, 98.0, 98.0); // 0.2 R < 1.0 BE
    assert_eq!(sm.stop(), 110.0);

    sm.update(88.0, 88.0, 88.0); // 1.2 R >= 1.0 BE -> move to entry
    assert_eq!(sm.stop(), 100.0);

    sm.update(85.0, 85.0, 85.0); // trail: close(85) + distance(2) = 87
    assert_eq!(sm.stop(), 87.0);
}

// ── VOLUME MANAGER EDGE CASES ───────────────────────────────────────────────

#[test]
fn test_fixed_percent_respects_initial_balance() {
    let vm = FixedPercent {
        pct: 0.01,
        initial_balance: 5000.0,
    }; // $50 risk
    let sym = make_symbol(0.01, 0.01);
    let acct = make_account(100000.0); // equity doesn't matter for FixedPercent

    let size = vm.position_size(&acct, &sym, 50000.0, 49000.0, 0.0);
    // risk_amount = 5000 * 0.01 = 50
    // risk_pts = 1000 / 0.1 * 0.1 = 1000
    // lot = 50 / 1000 = 0.05
    assert_eq!(size, 0.05);
}

#[test]
fn test_tiered_percent_no_matching_tier() {
    let vm = TieredPercent {
        tiers: vec![(0.0, 2.0, 0.01)],
        daily_dd_limit: 5.0,
    };
    let sym = make_symbol(0.01, 0.01);
    let acct = make_account(10000.0);

    // DD = 3% is outside the only tier (0-2%)
    let size = vm.position_size(&acct, &sym, 50000.0, 49000.0, 0.03);
    assert_eq!(size, 0.0);
}

// ── ADVANCE_STOP SELL ───────────────────────────────────────────────────────

#[test]
fn test_advance_stop_sell() {
    let mut sm = TslVariant1::new(0.5);
    sm.init(100.0, 110.0, Direction::Sell);

    let mut pos = Position {
        trade_id: 1,
        strategy_id: 1,
        direction: Direction::Sell,
        entry_price: 100.0,
        initial_stop_loss: 110.0,
        current_stop_loss: 110.0,
        take_profit: 80.0,
        volume: 1.0,
        open_risk: 10.0,
        entry_time: 1234,
        is_split_chunk: false,
        group_id: 0,
    };

    // Price drops to 94 -> 0.6 R profit
    let res = advance_stop(&mut sm, &mut pos, 94.0, 94.0, 94.0);
    assert_eq!(res, Some(105.0));
    assert_eq!(pos.current_stop_loss, 105.0);
}

// ── STOPPED_OUT TESTS ───────────────────────────────────────────────────────

#[test]
fn test_tsl_stopped_out_sell() {
    let mut sm = TslVariant1::new(0.5);
    sm.init(100.0, 110.0, Direction::Sell);

    // Bar high touches stop
    assert!(sm.stopped_out(110.0, 105.0));
    assert!(sm.stopped_out(115.0, 108.0));
    assert!(!sm.stopped_out(109.9, 105.0));
}

#[test]
fn test_tsl_stopped_out_buy() {
    let mut sm = TslVariant1::new(0.5);
    sm.init(100.0, 90.0, Direction::Buy);

    // Bar low touches stop
    assert!(sm.stopped_out(105.0, 90.0));
    assert!(sm.stopped_out(105.0, 88.0));
    assert!(!sm.stopped_out(105.0, 90.1));
}

// ── TIME EXIT EDGE CASES ────────────────────────────────────────────────────

#[test]
fn test_time_exit_combined() {
    let pos = Position {
        trade_id: 1,
        strategy_id: 1,
        direction: Direction::Buy,
        entry_price: 100.0,
        initial_stop_loss: 90.0,
        current_stop_loss: 90.0,
        take_profit: 150.0,
        volume: 1.0,
        open_risk: 10.0,
        entry_time: 1000,
        is_split_chunk: false,
        group_id: 0,
    };
    let cols = IndicatorSet::default();
    let params = Params::default();
    let sig = Signal::new(Direction::Hold, 100.0, 0.0, 0.0);

    // Both delta_time and max_bars: whichever fires first
    let te = TimeExit {
        delta_time: Some("10_minute".to_string()),
        max_bars: Some(3),
        exit_hour_utc: None,
    };

    // After 2 bars but under 10 min -> max_bars not hit yet
    let bars2 = [
        Bar::new(1000, 100.0, 100.0, 100.0, 100.0, 1.0),
        Bar::new(1060, 100.0, 100.0, 100.0, 100.0, 1.0),
        Bar::new(1120, 100.0, 100.0, 100.0, 100.0, 1.0),
    ];
    assert!(!te.should_exit(&pos, 2, &bars2, &cols, &params, &sig, None));

    // After 3 bars -> max_bars triggers
    let bars3 = [
        Bar::new(1000, 100.0, 100.0, 100.0, 100.0, 1.0),
        Bar::new(1060, 100.0, 100.0, 100.0, 100.0, 1.0),
        Bar::new(1120, 100.0, 100.0, 100.0, 100.0, 1.0),
        Bar::new(1180, 100.0, 100.0, 100.0, 100.0, 1.0),
    ];
    assert!(te.should_exit(&pos, 3, &bars3, &cols, &params, &sig, None));
}

// ── STRATEGY EXIT WITH SELL POSITION ────────────────────────────────────────

#[test]
fn test_strategy_exit_sell_position() {
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
        entry_time: 1234,
        is_split_chunk: false,
        group_id: 0,
    };
    let bars = [Bar::new(1000, 100.0, 100.0, 100.0, 100.0, 1.0)];
    let cols = IndicatorSet::default();
    let params = Params::default();

    let exit_opp = StrategyExit {
        condition: risk::exit::StrategyExitCondition::OppositeSignal,
    };

    // Buy signal is opposite to Sell position -> should exit
    let sig_buy = Signal::new(Direction::Buy, 100.0, 90.0, 0.0);
    assert!(exit_opp.should_exit(&pos, 0, &bars, &cols, &params, &sig_buy, None));

    // Sell signal is same direction -> no exit
    let sig_sell = Signal::new(Direction::Sell, 100.0, 110.0, 0.0);
    assert!(!exit_opp.should_exit(&pos, 0, &bars, &cols, &params, &sig_sell, None));
}

// ── SUPERTREND TRAIL SELL ───────────────────────────────────────────────────

#[test]
fn test_supertrend_trail_sell() {
    let mut sm = SupertrendTrail::new(3, 1.5);
    sm.init(100.0, 105.0, Direction::Sell);

    // Feed falling bars
    for &(c, h, l) in &[
        (100.0, 101.0, 99.0),
        (98.0, 99.0, 97.0),
        (96.0, 97.0, 95.0),
        (94.0, 95.0, 93.0),
    ] {
        sm.update(c, h, l);
    }
    // Stop should have moved down from 105 but remain above current close
    assert!(sm.stop() < 105.0);
    assert!(sm.stop() > 94.0);
}

// ── EXIT MANAGER FACTORY EDGE CASES ─────────────────────────────────────────

#[test]
fn test_build_exit_empty() {
    let cfgs: Vec<ExitManagerConfig> = vec![];
    let ex_mgrs = build_exit(&cfgs);
    assert!(ex_mgrs.is_empty());
}

#[test]
fn test_any_should_exit_empty_managers() {
    let pos = Position {
        trade_id: 1,
        strategy_id: 1,
        direction: Direction::Buy,
        entry_price: 100.0,
        initial_stop_loss: 90.0,
        current_stop_loss: 90.0,
        take_profit: 150.0,
        volume: 1.0,
        open_risk: 10.0,
        entry_time: 1000,
        is_split_chunk: false,
        group_id: 0,
    };
    let bars = [Bar::new(1100, 100.0, 100.0, 100.0, 100.0, 1.0)];
    let cols = IndicatorSet::default();
    let params = Params::default();
    let sig = Signal::new(Direction::Hold, 100.0, 0.0, 0.0);

    let managers: Vec<Box<dyn ExitManager>> = vec![];
    assert!(!any_should_exit(
        &managers, &pos, 0, &bars, &cols, &params, &sig, None
    ));
}
