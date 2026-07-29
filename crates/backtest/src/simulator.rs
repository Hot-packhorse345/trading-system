use serde_json::Value;
use std::collections::HashMap;

use ts_core::{
    AccountInfo, Bar, Direction, ExitReason, IndicatorSet, Params, Position, Signal, SymbolInfo,
    Timeframe, TradeRecord,
};

use risk::{
    any_should_exit, build_stop, config::StopManagerConfig, volume::VolumeManager, ExitManager,
    StopManager,
};

use crate::config::ResolvedCombo;
use crate::cost_model::{CostModel, DynamicSpreadCostModel};
use crate::metrics::{compute_metrics, Metrics};
use crate::swap::SwapConfig;

// ─────────────────────────────────────────────────────────────────────────────
// Params
// ─────────────────────────────────────────────────────────────────────────────

pub struct SimParams<'a> {
    pub symbol: &'a str,
    pub timeframe: Timeframe,
    pub stop_timeframe: Timeframe,
    pub pyramiding: bool,
    pub initial_balance: f64,
    pub risk_pct: f64,
    pub commission_pct: f64,
    pub commission_per_lot: f64,
    pub swap: SwapConfig,
    pub trading_hours: Option<crate::config::TradingHoursConfig>,
    pub stop_manager: &'a StopManagerConfig,
    pub strategy_params: &'a HashMap<String, Value>,
    pub collect_trades: bool,
}

impl<'a> From<&'a ResolvedCombo> for SimParams<'a> {
    fn from(c: &'a ResolvedCombo) -> Self {
        Self {
            symbol: &c.symbol,
            timeframe: c.timeframe,
            stop_timeframe: c.stop_timeframe,
            pyramiding: c.pyramiding,
            initial_balance: c.initial_balance,
            risk_pct: c.risk_percentage,
            commission_pct: c.commission_pct,
            commission_per_lot: c.commission_per_lot,
            swap: c.swap.clone(),
            trading_hours: c.trading_hours.clone(),
            stop_manager: &c.stop_manager,
            strategy_params: &c.strategy_params,
            collect_trades: true,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Result
// ─────────────────────────────────────────────────────────────────────────────

pub struct SimResult {
    pub trades: Vec<TradeRecord>,
    pub metrics: Metrics,
}

// ─────────────────────────────────────────────────────────────────────────────
// Internal pending order
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Clone)]
struct PendingOrder {
    signal: Signal,
}

// ─────────────────────────────────────────────────────────────────────────────
// Entry execution helper
// ─────────────────────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn execute_entry(
    sig: Signal,
    bar: &Bar,
    account: &mut AccountInfo,
    open_trades: &mut Vec<(Position, Box<dyn StopManager>)>,
    trade_counter: &mut u64,
    open_long: &mut usize,
    open_short: &mut usize,
    params: &SimParams,
    symbol_info: &SymbolInfo,
    vol_mgr: &dyn VolumeManager,
    cost_model: &dyn CostModel,
) {
    if !sig.is_valid() {
        return;
    }

    let entry_price = cost_model.entry_price(sig.direction, bar);

    let gapped_past_sl = match sig.direction {
        Direction::Buy => entry_price <= sig.stop_loss,
        Direction::Sell => entry_price >= sig.stop_loss,
        Direction::Hold => false,
    };
    if gapped_past_sl {
        return;
    }

    let volume = vol_mgr.position_size(
        account,
        symbol_info,
        entry_price,
        sig.stop_loss,
        account.equity / params.initial_balance,
    );

    if volume <= 0.0 {
        return;
    }

    *trade_counter += 1;

    let pos = Position {
        trade_id: *trade_counter,
        strategy_id: 0,
        direction: sig.direction,
        entry_price,
        initial_stop_loss: sig.stop_loss,
        current_stop_loss: sig.stop_loss,
        take_profit: sig.take_profit,
        volume,
        open_risk: (entry_price - sig.stop_loss).abs() * volume,
        entry_time: bar.time,
        is_split_chunk: false,
        group_id: 0, // TODO: Split Chunk Feature is not implemented!!!
    };

    let mut sm = build_stop(params.stop_manager);
    sm.init(entry_price, sig.stop_loss, sig.direction);

    match sig.direction {
        Direction::Buy => *open_long += 1,
        Direction::Sell => *open_short += 1,
        Direction::Hold => {}
    }

    open_trades.push((pos, sm));
}

// ─────────────────────────────────────────────────────────────────────────────
// Main simulation
// ─────────────────────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub fn simulate(
    params: &SimParams<'_>,
    bars: &[Bar],
    stop_bars: &[Bar],
    cols: &IndicatorSet,
    signals: &[Option<Signal>],
    symbol_info: &SymbolInfo,
    vol_mgr: &dyn VolumeManager,
    exit_mgrs: &[Box<dyn ExitManager>],
) -> SimResult {
    let mut account = AccountInfo {
        balance: params.initial_balance,
        equity: params.initial_balance,
        margin_free: params.initial_balance,
        ..Default::default()
    };

    let mut cost_model = DynamicSpreadCostModel::new(symbol_info);

    let strategy_params = Params(params.strategy_params.clone());

    let hold = Signal::new(Direction::Hold, 0.0, 0.0, 0.0);

    let mut open_trades: Vec<(Position, Box<dyn StopManager>)> = Vec::new();
    let mut trades: Vec<TradeRecord> = Vec::new();

    let mut pending: Option<PendingOrder> = None;

    let mut trade_counter = 0u64;
    let mut open_long = 0usize;
    let mut open_short = 0usize;

    let mut prev_sig_dir: Option<Direction> = None;
    let timeframe_sec = params.timeframe.seconds();
    let mut stop_idx = 0;

    // Pre-allocate reusable buffers — avoids per-bar heap allocation in the hot loop.
    let sub_bar_capacity =
        (params.timeframe.seconds() / params.stop_timeframe.seconds()).max(1) as usize;
    let mut sub_bars: Vec<Bar> = Vec::with_capacity(sub_bar_capacity);
    let mut still_open: Vec<(Position, Box<dyn StopManager>)> = Vec::new();

    for i in 0..bars.len() {
        let bar = &bars[i];
        let sig_opt = signals[i];
        let sig_ref = sig_opt.as_ref().unwrap_or(&hold);

        // Find sub-bars in stop_bars
        sub_bars.clear();
        while stop_idx < stop_bars.len() && stop_bars[stop_idx].time < bar.time {
            stop_idx += 1;
        }
        let mut temp_idx = stop_idx;
        while temp_idx < stop_bars.len() && stop_bars[temp_idx].time < bar.time + timeframe_sec {
            sub_bars.push(stop_bars[temp_idx]);
            temp_idx += 1;
        }
        if sub_bars.is_empty() {
            sub_bars.push(*bar);
        }

        // ─────────────────────────────────────────────
        // 1. ENTRY PHASE (NEXT-BAR EXECUTION)
        // ─────────────────────────────────────────────

        if let Some(order) = pending.take() {
            execute_entry(
                order.signal,
                bar,
                &mut account,
                &mut open_trades,
                &mut trade_counter,
                &mut open_long,
                &mut open_short,
                params,
                symbol_info,
                vol_mgr,
                &cost_model,
            );
        }

        cost_model.update(bar);

        // ─────────────────────────────────────────────
        // 2. RISK + EXIT PHASE
        // ─────────────────────────────────────────────

        still_open.clear();

        for (mut pos, mut sm) in open_trades.drain(..) {
            let mut closed = false;
            let mut exit_price = 0.0;
            let mut reason = ExitReason::EndOfData;
            let mut exit_time = bar.time;

            for sb in &sub_bars {
                let stop_level = sm.stop();
                let stopped = sm.stopped_out(sb.high, sb.low);
                let tp_hit = pos.tp_hit(sb.high, sb.low);

                sm.update(sb.close, sb.high, sb.low);
                pos.current_stop_loss = sm.stop();

                if stopped {
                    let fill_ref = match pos.direction {
                        Direction::Buy if sb.open < stop_level => sb.open, // gapped down through stop
                        Direction::Sell if sb.open > stop_level => sb.open, // gapped up through stop
                        _ => stop_level,
                    };

                    exit_price = cost_model.exit_price(pos.direction, fill_ref);

                    reason = match pos.direction {
                        Direction::Buy => {
                            if exit_price > pos.entry_price {
                                ExitReason::StopProfit
                            } else {
                                ExitReason::StopLoss
                            }
                        }
                        Direction::Sell => {
                            if exit_price < pos.entry_price {
                                ExitReason::StopProfit
                            } else {
                                ExitReason::StopLoss
                            }
                        }
                        Direction::Hold => ExitReason::StopLoss,
                    };

                    exit_time = sb.time;
                    closed = true;
                    break;
                } else if tp_hit {
                    let tp_level = pos.take_profit;
                    let fill_ref = match pos.direction {
                        Direction::Buy if sb.open > tp_level => sb.open, // gapped above TP
                        Direction::Sell if sb.open < tp_level => sb.open, // gapped below TP
                        _ => tp_level,
                    };

                    exit_price = cost_model.exit_price(pos.direction, fill_ref);

                    reason = ExitReason::TakeProfit;
                    exit_time = sb.time;
                    closed = true;
                    break;
                }
            }

            if !closed {
                let rule_exit = any_should_exit(
                    exit_mgrs,
                    &pos,
                    i,
                    bars,
                    cols,
                    &strategy_params,
                    sig_ref,
                    prev_sig_dir,
                );

                if rule_exit {
                    exit_price = cost_model.exit_price(pos.direction, bar.close);
                    reason = ExitReason::ExitRule;
                    exit_time = bar.time;
                    closed = true;
                }
            }

            if closed {
                let rec =
                    TradeRecord::close_position(&pos, params.symbol, exit_price, exit_time, reason);

                account.balance += rec.pnl();

                match pos.direction {
                    Direction::Buy => open_long = open_long.saturating_sub(1),
                    Direction::Sell => open_short = open_short.saturating_sub(1),
                    _ => {}
                }

                trades.push(rec);
            } else {
                still_open.push((pos, sm));
            }
        }

        std::mem::swap(&mut open_trades, &mut still_open);

        // ─────────────────────────────────────────────
        // 3. SIGNAL PHASE (QUEUE ENTRY)
        // ─────────────────────────────────────────────

        if let Some(sig) = sig_opt {
            if sig.is_valid() {
                let mut can_enter = if params.pyramiding {
                    sig.direction != Direction::Hold
                } else {
                    match sig.direction {
                        Direction::Buy => open_long == 0,
                        Direction::Sell => open_short == 0,
                        _ => false,
                    }
                };

                if can_enter {
                    if let Some(ref th) = params.trading_hours {
                        if !th.is_allowed(bar.time) {
                            can_enter = false;
                        }
                    }
                }

                if can_enter {
                    pending = Some(PendingOrder { signal: sig });
                }
            }

            if sig.direction != Direction::Hold {
                prev_sig_dir = Some(sig.direction);
            }
        }

        // ─────────────────────────────────────────────
        // 4. EQUITY TRACKING
        // ─────────────────────────────────────────────

        let unrealised: f64 = open_trades
            .iter()
            .map(|(pos, _)| match pos.direction {
                Direction::Buy => (bar.close - pos.entry_price) * pos.volume,
                Direction::Sell => (pos.entry_price - bar.close) * pos.volume,
                Direction::Hold => 0.0,
            })
            .sum();

        account.equity = account.balance + unrealised;
        account.margin_free = account.equity;
    }

    // ─────────────────────────────────────────────────────────────────────────
    // FINAL LIQUIDATION
    // ─────────────────────────────────────────────────────────────────────────

    if let Some(last) = bars.last() {
        for (pos, _) in open_trades.drain(..) {
            let ep = cost_model.exit_price(pos.direction, last.close);

            let rec = TradeRecord::close_position(
                &pos,
                params.symbol,
                ep,
                last.time,
                ExitReason::EndOfData,
            );

            account.balance += rec.pnl();
            trades.push(rec);
        }
    }

    let start_ts = bars.first().map(|b| b.time).unwrap_or(0);
    let end_ts = bars.last().map(|b| b.time).unwrap_or(0);

    let metrics = compute_metrics(
        &trades,
        start_ts,
        end_ts,
        params.initial_balance,
        params.risk_pct,
        params.commission_pct,
        params.commission_per_lot,
        &params.swap,
        symbol_info.tick_value,
    );

    SimResult {
        trades: if params.collect_trades {
            trades
        } else {
            Vec::new()
        },
        metrics,
    }
}
