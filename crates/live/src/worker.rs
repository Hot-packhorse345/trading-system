use anyhow::{anyhow, Result};
use broker::{BrokerHandles, DataProvider, Executor, OrderRequest};
use data::TradeDb;
use indicators::{build_indicator_set, split_indicator_def, TfIndicator};
use infra::{news::BlackoutNotification, Notifier};
use risk::{
    build_exit, build_stop, build_volume,
    config::{ExitManagerConfig, StopManagerConfig, VolumeManagerConfig},
    VolumeManager,
};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Once};
use std::time::{SystemTime, UNIX_EPOCH};
use strategy::{build_strategy, Strategy};
use tokio::sync::{mpsc, watch};
use tracing::{error, info, warn};
use ts_core::{
    parse_timeframe, Bar, CircularBuffer, Direction, ExitReason, IndicatorSet, Params, Position,
    Signal, Tick, Timeframe, TradeRecord,
};

use crate::{
    config::LiveWorkerConfig,
    formatter,
    trade_manager::{GroupTradeManager, TradeManager},
};

// Minimum bars in the buffer before generating signals.
const MIN_BARS: usize = 50;
// Channel capacity for bar and tick streams.
const CHAN_CAP: usize = 512;

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

fn stable_hash(s: &str) -> u64 {
    let mut h: u64 = 14695981039346656037;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(1099511628211);
    }
    h
}

pub(crate) fn compute_strategy_id(strategy: &str, symbol: &str, timeframe: &str) -> u64 {
    stable_hash(&format!("{strategy}:{symbol}:{timeframe}"))
}

/// Monotonic, collision-free trade id. Seeded once from the wall clock (so ids
/// stay unique across process restarts) then strictly incremented. The previous
/// microsecond-timestamp scheme could return identical ids for orders created in
/// the same microsecond, which collided on the DB primary key.
static TRADE_ID: AtomicU64 = AtomicU64::new(0);
fn next_trade_id() -> u64 {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_micros() as u64;
        TRADE_ID.store(seed, Ordering::SeqCst);
    });
    TRADE_ID.fetch_add(1, Ordering::SeqCst)
}

fn split_volume(volume: f64, min_lot: f64, max_lot: f64, lot_step: f64) -> Vec<f64> {
    let mut chunks = Vec::new();
    let mut remaining = volume;
    while remaining >= min_lot {
        let chunk = remaining.min(max_lot);
        let chunk = ((chunk / lot_step).floor() * lot_step * 1e8).round() / 1e8;
        if chunk < min_lot {
            break;
        }
        chunks.push(chunk);
        remaining = ((remaining - chunk) * 1e8).round() / 1e8;
    }
    chunks
}

pub struct LiveWorker {
    worker_id: String,
    strategy_id: u64,
    symbol: String,
    timeframe: Timeframe,
    stop_timeframe: Timeframe,
    same_tf: bool,
    pyramiding: bool,
    max_open_positions: Option<usize>,
    /// Whether to split an order into multiple chunks. Disabled for Binance
    /// USDⓈ-M futures, which net same-side quantity into a single position
    /// (per-chunk `closePosition=true` stops would conflict / close everything).
    split_allowed: bool,

    strategy: Box<dyn Strategy>,
    ind_defs: Vec<TfIndicator>,
    params: Params,
    ind_params: HashMap<String, Params>,
    vol_manager: Box<dyn VolumeManager>,
    stop_cfg: StopManagerConfig,
    exit_cfgs: Vec<ExitManagerConfig>,

    executor: Arc<dyn Executor>,
    data_provider: Arc<dyn DataProvider>,
    notifier: Arc<dyn Notifier>,
    trade_db: Arc<Mutex<TradeDb>>,
    /// Current daily drawdown as a fraction (0.0–1.0), published by the drawdown
    /// monitor. Fed into tiered position sizing so risk actually de-escalates in
    /// drawdown (previously hardcoded to 0.0).
    current_dd: watch::Receiver<f64>,

    buffer: CircularBuffer,
    solo_trades: Vec<TradeManager>,
    group_trades: Vec<GroupTradeManager>,
    blackout_active: bool,
    emergency_active: bool,
    decay_active: bool,
    prev_signal_dir: Option<Direction>,

    long_count: usize,
    short_count: usize,
}

impl LiveWorker {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        config: &LiveWorkerConfig,
        handles: &BrokerHandles,
        notifier: Arc<dyn Notifier>,
        trade_db: Arc<Mutex<TradeDb>>,
        current_dd: watch::Receiver<f64>,
    ) -> Result<Self> {
        let symbol = config.symbol.to_uppercase();
        let timeframe = parse_timeframe(&config.timeframe)
            .map_err(|_| anyhow!("invalid timeframe: {}", config.timeframe))?;
        let stop_timeframe = parse_timeframe(&config.effective_stop_timeframe()).map_err(|_| {
            anyhow!(
                "invalid stop_timeframe: {}",
                config.effective_stop_timeframe()
            )
        })?;

        let strategy_id = compute_strategy_id(&config.strategy, &symbol, &config.timeframe);
        let worker_id = format!("{}:{}:{}", config.strategy, symbol, config.timeframe);

        let strategy = build_strategy(&config.strategy.to_lowercase())?;

        // Build indicator definitions, PRESERVING any `timeframe` override so the
        // shared multi-timeframe builder produces the same `*_htf` columns as the
        // backtest engine.
        let ind_defs: Vec<TfIndicator> = config
            .indicators
            .iter()
            .filter_map(|(name, val)| {
                let obj = val.as_object()?;
                let (kind, timeframe, params) = split_indicator_def(obj);
                Some(TfIndicator {
                    name: name.clone(),
                    ind_type: kind?,
                    timeframe,
                    params,
                })
            })
            .collect();

        // Binance USDⓈ-M futures net same-side quantity into one position, so
        // splitting an order into chunks (each with its own closePosition stop)
        // is unsafe there; allow it only for non-Binance executors.
        let split_allowed = !config.trade_executor.eq_ignore_ascii_case("binance");

        let ind_params: HashMap<String, Params> = config
            .indicators
            .iter()
            .filter_map(|(name, val)| {
                let obj = val.as_object()?;
                let p: HashMap<String, Value> = obj
                    .iter()
                    .filter(|(k, _)| *k != "type")
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                Some((name.clone(), Params(p)))
            })
            .collect();

        let params = Params(config.strategy_parameters.clone());
        let vol_cfg = serde_json::from_value::<VolumeManagerConfig>(config.risk_manager.clone())
            .map_err(|e| anyhow!("invalid risk_manager config: {e}"))?;
        let vol_manager = build_volume(&vol_cfg)?;

        let stop_cfg = serde_json::from_value::<StopManagerConfig>(config.stop_manager.clone())
            .map_err(|e| anyhow!("invalid stop_manager config: {e}"))?;

        let exit_cfgs: Vec<ExitManagerConfig> = if config.exit_rules.is_empty() {
            Vec::new()
        } else {
            let rules = Value::Array(config.exit_rules.clone());
            serde_json::from_value(rules).map_err(|e| anyhow!("invalid exit_rules: {e}"))?
        };

        let same_tf = timeframe == stop_timeframe;

        Ok(Self {
            worker_id,
            strategy_id,
            symbol,
            timeframe,
            stop_timeframe,
            same_tf,
            pyramiding: config.pyramiding,
            max_open_positions: config.max_open_positions,
            split_allowed,
            strategy,
            ind_defs,
            params,
            ind_params,
            vol_manager,
            stop_cfg,
            exit_cfgs,
            executor: handles.executor.clone(),
            data_provider: handles.provider.clone(),
            notifier,
            trade_db,
            current_dd,
            buffer: CircularBuffer::new(config.max_historical_bars.max(MIN_BARS + 50)),
            solo_trades: Vec::new(),
            group_trades: Vec::new(),
            blackout_active: false,
            emergency_active: false,
            decay_active: false,
            prev_signal_dir: None,
            long_count: 0,
            short_count: 0,
        })
    }

    // ── Startup ───────────────────────────────────────────────────────────────

    async fn seed_bars(&mut self, config: &LiveWorkerConfig) -> Result<()> {
        let end = now_secs();
        let start = if let Some(ref date) = config.start_date {
            ts_core::parse_iso_date(date)?
        } else {
            let tf_secs = self.timeframe.seconds();
            end - tf_secs * config.max_historical_bars as i64
        };

        let bars = self
            .data_provider
            .ohlcv(&self.symbol, self.timeframe, start, end)
            .await?;

        for bar in bars {
            self.buffer.push(bar);
        }
        info!(worker=%self.worker_id, bars=%self.buffer.len(), "seeded historical bars");
        Ok(())
    }

    async fn recover_open_positions(&mut self, _config: &LiveWorkerConfig) {
        let records = {
            let db = self.trade_db.lock().unwrap();
            db.load_open(self.strategy_id).unwrap_or_default()
        };

        if records.is_empty() {
            return;
        }

        info!(worker=%self.worker_id, count=%records.len(), "recovering open positions");

        for rec in records {
            // Initialise the trailing manager from the ORIGINAL stop, not the
            // already-trailed current stop, so its risk baseline (entry - initial_sl)
            // is correct after a restart. The position's current_stop_loss is still
            // restored below, and advance_stop only ever tightens, so it cannot loosen.
            let mut sm = build_stop(&self.stop_cfg);
            sm.init(rec.entry_price, rec.initial_stop_loss, rec.direction);

            let exit_managers = build_exit(&self.exit_cfgs);

            let pos = Position {
                trade_id: rec.trade_id,
                strategy_id: rec.strategy_id,
                direction: rec.direction,
                entry_price: rec.entry_price,
                initial_stop_loss: rec.initial_stop_loss,
                current_stop_loss: rec.current_stop_loss,
                take_profit: rec.take_profit,
                volume: rec.volume,
                open_risk: rec.open_risk,
                entry_time: rec.entry_time,
                is_split_chunk: rec.group_id != 0,
                group_id: rec.group_id,
            };

            match rec.direction {
                Direction::Buy => self.long_count += 1,
                Direction::Sell => self.short_count += 1,
                _ => {}
            }

            if rec.group_id == 0 {
                let tm = TradeManager::new(pos, self.symbol.clone(), sm, exit_managers);
                self.solo_trades.push(tm);
            } else {
                // Find existing group or create new one.
                let group_idx = self
                    .group_trades
                    .iter()
                    .position(|g| g.group_id == rec.group_id);
                if let Some(idx) = group_idx {
                    let chunk_sm = build_stop(&self.stop_cfg);
                    let tm = TradeManager::new(pos, self.symbol.clone(), chunk_sm, Vec::new());
                    self.group_trades[idx].chunks.push(tm);
                } else {
                    let authority_sm = sm;
                    let chunk_sm = build_stop(&self.stop_cfg);
                    let tm = TradeManager::new(pos, self.symbol.clone(), chunk_sm, exit_managers);
                    let group = GroupTradeManager::new(rec.group_id, vec![tm], authority_sm);
                    self.group_trades.push(group);
                }
            }
        }
    }

    // ── Main run loop ─────────────────────────────────────────────────────────

    pub async fn run(
        mut self,
        config: LiveWorkerConfig,
        handles: BrokerHandles,
        mut news_rx: mpsc::UnboundedReceiver<BlackoutNotification>,
        mut emergency_rx: watch::Receiver<bool>,
        mut decay_rx: watch::Receiver<bool>,
        mut shutdown_rx: watch::Receiver<bool>,
    ) -> Result<()> {
        if let Err(e) = self.seed_bars(&config).await {
            warn!(worker=%self.worker_id, err=%e, "could not seed bars, starting empty");
        }
        self.recover_open_positions(&config).await;

        // Subscribe to bar streams.
        let (strategy_bar_tx, mut strategy_bar_rx) = mpsc::channel::<Bar>(CHAN_CAP);
        handles
            .streamer
            .subscribe(&self.symbol, self.timeframe, strategy_bar_tx)
            .await
            .map_err(|e| anyhow!("bar stream subscribe failed: {e}"))?;

        // Subscribe to stop TF if different from strategy TF.
        let mut stop_bar_rx: Option<mpsc::Receiver<Bar>> = None;
        if !self.same_tf {
            let (tx, rx) = mpsc::channel::<Bar>(CHAN_CAP);
            handles
                .streamer
                .subscribe(&self.symbol, self.stop_timeframe, tx)
                .await
                .map_err(|e| anyhow!("stop bar stream subscribe failed: {e}"))?;
            stop_bar_rx = Some(rx);
        }

        // Subscribe to tick stream.
        let (tick_tx, mut tick_rx) = mpsc::channel::<Tick>(CHAN_CAP);
        handles
            .tick_streamer
            .subscribe(&self.symbol, tick_tx)
            .await
            .map_err(|e| anyhow!("tick stream subscribe failed: {e}"))?;

        info!(worker=%self.worker_id, "worker started");

        loop {
            let stop_bar_future = async {
                match stop_bar_rx {
                    Some(ref mut rx) => rx.recv().await,
                    None => std::future::pending().await,
                }
            };

            tokio::select! {
                Some(bar) = strategy_bar_rx.recv() => {
                    self.on_strategy_bar(&bar).await;
                    if self.same_tf {
                        self.on_stop_bar(&bar).await;
                    }
                }
                Some(bar) = stop_bar_future => {
                    self.on_stop_bar(&bar).await;
                }
                Some(tick) = tick_rx.recv() => {
                    self.on_tick(&tick).await;
                }
                Some(notif) = news_rx.recv() => {
                    self.on_news_blackout(notif).await;
                }
                Ok(_) = emergency_rx.changed() => {
                    let is_emergency = *emergency_rx.borrow();
                    if is_emergency && !self.emergency_active {
                        self.emergency_active = true;
                        warn!(worker=%self.worker_id, "drawdown emergency — closing all positions");
                        self.close_all_positions(ExitReason::ExitRule).await;
                    } else if !is_emergency {
                        self.emergency_active = false;
                        info!(worker=%self.worker_id, "drawdown emergency cleared (new day)");
                    }
                }
                Ok(_) = decay_rx.changed() => {
                    self.decay_active = *decay_rx.borrow();
                }
                _ = shutdown_rx.changed() => {
                    if *shutdown_rx.borrow() {
                        info!(worker=%self.worker_id, "shutdown signal received");
                        break;
                    }
                }
            }
        }

        info!(worker=%self.worker_id, "worker stopped");
        Ok(())
    }

    // ── Event handlers ────────────────────────────────────────────────────────

    async fn on_tick(&mut self, tick: &Tick) {
        // Solo positions.
        let mut to_close: Vec<(usize, ExitReason)> = Vec::new();
        for (i, tm) in self.solo_trades.iter_mut().enumerate() {
            if tm.check_tick_stop(tick) {
                to_close.push((i, ExitReason::StopLoss));
            } else if tm.check_tick_tp(tick) {
                to_close.push((i, ExitReason::TakeProfit));
            }
        }
        // Process in reverse order to preserve indices.
        // SL/TP is already on the server — no executor.close() needed.
        for (i, reason) in to_close.into_iter().rev() {
            self.close_solo(i, reason, true).await;
        }

        // Group positions — collect closures first to avoid double-borrow.
        let mut group_closures: Vec<(usize, usize, ExitReason)> = Vec::new();
        for (gi, group) in self.group_trades.iter_mut().enumerate() {
            let stopped = group.check_tick_stop(tick);
            let tp_hit = group.check_tick_tp(tick);
            let all_closed: Vec<usize> = stopped
                .iter()
                .chain(tp_hit.iter())
                .copied()
                .collect::<std::collections::HashSet<_>>()
                .into_iter()
                .collect();
            for &ci in &all_closed {
                if ci < group.chunks.len() {
                    let reason = group.chunks[ci].close_reason();
                    group_closures.push((gi, ci, reason));
                }
            }
        }
        // Process closures (needs &mut self, so cannot be inside iter_mut loop).
        for (gi, ci, reason) in group_closures {
            self.close_group_chunk_at(gi, ci, reason, true).await;
        }
        // Remove fully-closed chunks and empty groups.
        let mut groups_to_remove: Vec<usize> = Vec::new();
        for (gi, group) in self.group_trades.iter_mut().enumerate() {
            group.chunks.retain(|c| !c.stopped_out && !c.tp_hit);
            if group.is_fully_closed() {
                groups_to_remove.push(gi);
            }
        }
        for gi in groups_to_remove.into_iter().rev() {
            self.group_trades.swap_remove(gi);
        }
    }

    async fn on_stop_bar(&mut self, bar: &Bar) {
        // Update stops for solo trades.
        let mut updates: Vec<(usize, f64)> = Vec::new();
        for (i, tm) in self.solo_trades.iter_mut().enumerate() {
            if let Some(new_sl) = tm.update_stop(bar) {
                updates.push((i, new_sl));
            }
        }
        for (i, new_sl) in updates {
            self.push_sl_update_solo(i, new_sl).await;
        }

        // Update stops for group trades.
        let n_groups = self.group_trades.len();
        for gi in 0..n_groups {
            if let Some(new_sl) = self.group_trades[gi].update_stop(bar) {
                self.push_sl_update_group(gi, new_sl).await;
            }
        }
    }

    async fn on_strategy_bar(&mut self, bar: &Bar) {
        self.buffer.push(*bar);

        if self.buffer.len() < MIN_BARS {
            return;
        }
        if self.blackout_active || self.emergency_active || self.decay_active {
            return;
        }

        let bars = self.buffer.as_vec();
        let cols = match build_indicator_set(&self.ind_defs, &bars, None) {
            Ok(c) => c,
            Err(e) => {
                error!(worker=%self.worker_id, err=%e, "indicator compute failed");
                return;
            }
        };

        let signals = self
            .strategy
            .generate_signals(&bars, &cols, &self.params, &self.ind_params);
        let last_sig = match signals.last().and_then(|s| s.as_ref()) {
            Some(s) => *s,
            None => return, // keep prev_signal_dir (last non-Hold) for BiasFlip
        };

        // Check exit rules for existing positions.
        let bar_idx = bars.len().saturating_sub(1);
        self.check_and_apply_exit_rules(bar_idx, &bars, &cols, &last_sig)
            .await;

        // Consider opening a new position.
        if last_sig.is_valid() {
            self.consider_entry(&last_sig).await;
        }

        // Persist the most recent non-Hold direction (don't reset on Hold) so the
        // BiasFlip exit retains the prior bias across neutral bars.
        if last_sig.direction != Direction::Hold {
            self.prev_signal_dir = Some(last_sig.direction);
        }
    }

    async fn on_news_blackout(&mut self, notif: BlackoutNotification) {
        self.blackout_active = notif.active;
        if notif.active {
            warn!(worker=%self.worker_id, "news blackout entering — closing all positions");
            if let Some(ref window) = notif.window {
                let msg = formatter::format_news_blackout(window);
                self.send_alert(&msg).await;
            }
            self.close_all_positions(ExitReason::ExitRule).await;
        } else {
            info!(worker=%self.worker_id, "news blackout cleared");
            self.send_alert(&formatter::format_news_cleared()).await;
        }
    }

    // ── Entry logic ───────────────────────────────────────────────────────────

    async fn consider_entry(&mut self, sig: &Signal) {
        if sig.direction == Direction::Hold {
            return;
        }

        // Hard cap on concurrent open positions (safety bound for pyramiding).
        if let Some(cap) = self.max_open_positions {
            if self.long_count + self.short_count >= cap {
                warn!(worker=%self.worker_id, cap=%cap, "max_open_positions reached, skipping entry");
                return;
            }
        }

        // Pyramiding check.
        if !self.pyramiding {
            let open_in_dir = match sig.direction {
                Direction::Buy => self.long_count > 0,
                Direction::Sell => self.short_count > 0,
                _ => false,
            };
            if open_in_dir {
                return;
            }
        }

        // Fetch account + symbol info in parallel — both are read-only and independent.
        let provider = self.data_provider.clone();
        let sym_clone = self.symbol.clone();
        let (account_res, sym_info_res) = tokio::join!(
            self.data_provider.account(),
            provider.symbol_info(&sym_clone),
        );
        let account = match account_res {
            Ok(a) => a,
            Err(e) => {
                error!(err=%e, "failed to fetch account for entry");
                return;
            }
        };
        let sym_info = match sym_info_res {
            Ok(s) => s,
            Err(e) => {
                error!(err=%e, "failed to fetch symbol info for entry");
                return;
            }
        };

        // Current drawdown (fraction) published by the drawdown monitor — drives
        // tiered de-risking. Previously hardcoded to 0.0, which disabled it.
        let dd_pct = *self.current_dd.borrow();

        let volume = self.vol_manager.position_size(
            &account,
            &sym_info,
            sig.entry_price,
            sig.stop_loss,
            dd_pct,
        );
        let volume = if self.split_allowed {
            let steps = (volume / sym_info.lot_step).round();
            let rounded = steps * sym_info.lot_step;
            rounded.max(sym_info.min_lot)
        } else {
            sym_info.round_lot(volume)
        };
        if volume < sym_info.min_lot {
            warn!(worker=%self.worker_id, volume=%volume, "computed volume below min_lot, skipping entry");
            return;
        }

        let chunks = if self.split_allowed {
            split_volume(
                volume,
                sym_info.min_lot,
                sym_info.max_lot,
                sym_info.lot_step,
            )
        } else {
            // Single netted position (e.g. Binance futures); round_lot already
            // clamped to [min_lot, max_lot].
            vec![volume]
        };
        let is_grouped = chunks.len() > 1;
        let group_id = if is_grouped { next_trade_id() } else { 0 };

        let mut positions: Vec<Position> = Vec::new();
        for (ci, &chunk_vol) in chunks.iter().enumerate() {
            let trade_id = next_trade_id();
            let req = OrderRequest {
                symbol: self.symbol.clone(),
                direction: sig.direction,
                volume: chunk_vol,
                entry_price: sig.entry_price,
                stop_loss: sig.stop_loss,
                take_profit: sig.take_profit,
                strategy_id: self.strategy_id,
                trade_id,
                comment: format!("{} chunk {}", self.worker_id, ci),
            };

            match self.executor.open(&req).await {
                Ok(mut pos) => {
                    pos.is_split_chunk = is_grouped;
                    pos.group_id = group_id;

                    if let Ok(db) = self.trade_db.lock() {
                        let insert_rec =
                            ts_core::TradeRecord::from_open_position(&pos, &self.symbol);
                        db.insert(&insert_rec).ok();
                    }

                    let msg = formatter::format_trade_open(&pos, &self.symbol);
                    self.send_alert(&msg).await;
                    positions.push(pos);
                }
                Err(e) => {
                    error!(worker=%self.worker_id, err=%e, "failed to open position chunk {ci}");
                }
            }
        }

        if positions.is_empty() {
            return;
        }

        match sig.direction {
            Direction::Buy => self.long_count += positions.len(),
            Direction::Sell => self.short_count += positions.len(),
            _ => {}
        }

        if is_grouped && positions.len() > 1 {
            let mut authority_sm = build_stop(&self.stop_cfg);
            authority_sm.init(
                positions[0].entry_price,
                positions[0].current_stop_loss,
                positions[0].direction,
            );

            let chunks: Vec<TradeManager> = positions
                .into_iter()
                .map(|pos| {
                    let mut sm = build_stop(&self.stop_cfg);
                    sm.init(pos.entry_price, pos.current_stop_loss, pos.direction);
                    TradeManager::new(pos, self.symbol.clone(), sm, Vec::new())
                })
                .collect();

            let group = GroupTradeManager::new(group_id, chunks, authority_sm);
            self.group_trades.push(group);
        } else if let Some(pos) = positions.into_iter().next() {
            let mut sm = build_stop(&self.stop_cfg);
            sm.init(pos.entry_price, pos.current_stop_loss, pos.direction);
            let exit_managers = build_exit(&self.exit_cfgs);
            let tm = TradeManager::new(pos, self.symbol.clone(), sm, exit_managers);
            self.solo_trades.push(tm);
        }
    }

    // ── Exit rule checking ────────────────────────────────────────────────────

    async fn check_and_apply_exit_rules(
        &mut self,
        bar_idx: usize,
        bars: &[Bar],
        cols: &IndicatorSet,
        signal: &Signal,
    ) {
        let prev = self.prev_signal_dir;
        let mut exit_indices: Vec<usize> = Vec::new();

        for (i, tm) in self.solo_trades.iter().enumerate() {
            if tm.check_exit_rules(bar_idx, bars, cols, &self.params, signal, prev) {
                exit_indices.push(i);
            }
        }
        for i in exit_indices.into_iter().rev() {
            self.close_solo(i, ExitReason::ExitRule, false).await;
        }

        // Exit rules for groups: close entire group if the first chunk triggers.
        let mut groups_to_close: Vec<usize> = Vec::new();
        for (gi, group) in self.group_trades.iter().enumerate() {
            if let Some(tm) = group.chunks.first() {
                if tm.check_exit_rules(bar_idx, bars, cols, &self.params, signal, prev) {
                    groups_to_close.push(gi);
                }
            }
        }
        for gi in groups_to_close.into_iter().rev() {
            self.close_group_all(gi, ExitReason::ExitRule).await;
        }
    }

    // ── Close helpers ─────────────────────────────────────────────────────────

    async fn close_solo(&mut self, idx: usize, reason: ExitReason, broker_closed: bool) {
        if idx >= self.solo_trades.len() {
            return;
        }
        let tm = &self.solo_trades[idx];
        let pos = tm.position;

        if broker_closed {
            // Broker already executed the SL/TP — just record locally and alert.
            match reason {
                ExitReason::StopLoss | ExitReason::StopProfit => {
                    warn!(worker=%self.worker_id, trade_id=%pos.trade_id, sl=%pos.current_stop_loss, "stop out")
                }
                ExitReason::TakeProfit => {
                    info!(worker=%self.worker_id, trade_id=%pos.trade_id, tp=%pos.take_profit, "take profit hit")
                }
                _ => {}
            }
            let rec = TradeRecord::close_position(
                &pos,
                &self.symbol,
                pos.current_stop_loss,
                now_secs(),
                reason,
            );
            if let Ok(db) = self.trade_db.lock() {
                db.close(&rec).ok();
            }
            let msg = self.format_close(&rec);
            self.send_alert(&msg).await;

            match pos.direction {
                Direction::Buy => self.long_count = self.long_count.saturating_sub(1),
                Direction::Sell => self.short_count = self.short_count.saturating_sub(1),
                _ => {}
            }
            self.solo_trades.swap_remove(idx);
            return;
        }

        let close_result = if reason == ExitReason::StopLoss {
            self.executor
                .close_at_price(&pos, &self.symbol, pos.current_stop_loss)
                .await
        } else {
            self.executor.close(&pos, &self.symbol).await
        };
        match close_result {
            Ok(mut rec) => {
                rec.exit_reason = reason;
                if let Ok(db) = self.trade_db.lock() {
                    db.close(&rec).ok();
                }
                let msg = self.format_close(&rec);
                self.send_alert(&msg).await;

                match pos.direction {
                    Direction::Buy => self.long_count = self.long_count.saturating_sub(1),
                    Direction::Sell => self.short_count = self.short_count.saturating_sub(1),
                    _ => {}
                }
            }
            Err(e) => {
                error!(worker=%self.worker_id, err=%e, "failed to close solo position — reconciling");
                match self
                    .executor
                    .position_qty(&self.symbol, pos.direction)
                    .await
                {
                    Ok(qty) if qty < pos.volume * 0.01 => {
                        warn!(worker=%self.worker_id, "exchange position flat — treating as closed");
                        if let Ok(db) = self.trade_db.lock() {
                            let rec = TradeRecord::close_position(
                                &pos,
                                &self.symbol,
                                pos.current_stop_loss,
                                now_secs(),
                                reason,
                            );
                            db.close(&rec).ok();
                        }
                        match pos.direction {
                            Direction::Buy => self.long_count = self.long_count.saturating_sub(1),
                            Direction::Sell => {
                                self.short_count = self.short_count.saturating_sub(1)
                            }
                            _ => {}
                        }
                    }
                    _ => {
                        error!(worker=%self.worker_id, "exchange still has open position — keeping in local state");
                    }
                }
            }
        }
        self.solo_trades.swap_remove(idx);
    }

    async fn close_group_chunk_at(
        &mut self,
        gi: usize,
        ci: usize,
        reason: ExitReason,
        broker_closed: bool,
    ) {
        if gi >= self.group_trades.len() {
            return;
        }
        if ci >= self.group_trades[gi].chunks.len() {
            return;
        }
        let pos = self.group_trades[gi].chunks[ci].position;

        if broker_closed {
            match reason {
                ExitReason::StopLoss | ExitReason::StopProfit => {
                    warn!(worker=%self.worker_id, trade_id=%pos.trade_id, sl=%pos.current_stop_loss, "stop out (group chunk)")
                }
                ExitReason::TakeProfit => {
                    info!(worker=%self.worker_id, trade_id=%pos.trade_id, tp=%pos.take_profit, "take profit hit (group chunk)")
                }
                _ => {}
            }
            let rec = TradeRecord::close_position(
                &pos,
                &self.symbol,
                pos.current_stop_loss,
                now_secs(),
                reason,
            );
            if let Ok(db) = self.trade_db.lock() {
                db.close(&rec).ok();
            }
            let msg = self.format_close(&rec);
            self.send_alert(&msg).await;

            match pos.direction {
                Direction::Buy => self.long_count = self.long_count.saturating_sub(1),
                Direction::Sell => self.short_count = self.short_count.saturating_sub(1),
                _ => {}
            }
            return;
        }

        let close_result = if reason == ExitReason::StopLoss {
            self.executor
                .close_at_price(&pos, &self.symbol, pos.current_stop_loss)
                .await
        } else {
            self.executor.close(&pos, &self.symbol).await
        };
        match close_result {
            Ok(mut rec) => {
                rec.exit_reason = reason;
                if let Ok(db) = self.trade_db.lock() {
                    db.close(&rec).ok();
                }
                let msg = self.format_close(&rec);
                self.send_alert(&msg).await;

                match pos.direction {
                    Direction::Buy => self.long_count = self.long_count.saturating_sub(1),
                    Direction::Sell => self.short_count = self.short_count.saturating_sub(1),
                    _ => {}
                }
            }
            Err(e) => {
                error!(worker=%self.worker_id, err=%e, "failed to close group chunk — reconciling");
                match self
                    .executor
                    .position_qty(&self.symbol, pos.direction)
                    .await
                {
                    Ok(qty) if qty < pos.volume * 0.01 => {
                        warn!(worker=%self.worker_id, "exchange position flat — treating chunk as closed");
                        if let Ok(db) = self.trade_db.lock() {
                            let rec = TradeRecord::close_position(
                                &pos,
                                &self.symbol,
                                pos.current_stop_loss,
                                now_secs(),
                                reason,
                            );
                            db.close(&rec).ok();
                        }
                        match pos.direction {
                            Direction::Buy => self.long_count = self.long_count.saturating_sub(1),
                            Direction::Sell => {
                                self.short_count = self.short_count.saturating_sub(1)
                            }
                            _ => {}
                        }
                    }
                    _ => {
                        error!(worker=%self.worker_id, "exchange still has open position — keeping chunk in local state");
                    }
                }
            }
        }
    }

    async fn close_group_all(&mut self, gi: usize, reason: ExitReason) {
        if gi >= self.group_trades.len() {
            return;
        }
        let n_chunks = self.group_trades[gi].chunks.len();
        for ci in (0..n_chunks).rev() {
            self.close_group_chunk_at(gi, ci, reason, false).await;
        }
        self.group_trades.swap_remove(gi);
    }

    async fn close_all_positions(&mut self, reason: ExitReason) {
        let solo_count = self.solo_trades.len();
        for i in (0..solo_count).rev() {
            self.close_solo(i, reason, false).await;
        }
        let group_count = self.group_trades.len();
        for gi in (0..group_count).rev() {
            self.close_group_all(gi, reason).await;
        }
    }

    // ── SL update push ────────────────────────────────────────────────────────

    async fn push_sl_update_solo(&mut self, idx: usize, new_sl: f64) {
        if idx >= self.solo_trades.len() {
            return;
        }
        let pos = self.solo_trades[idx].position;

        match self.executor.update_sl(&pos, new_sl).await {
            Ok(updated) => {
                self.solo_trades[idx].position = updated;
                if let Ok(db) = self.trade_db.lock() {
                    db.update_stop(updated.trade_id, new_sl).ok();
                }
                let msg = formatter::format_stop_update(&updated, &self.symbol, now_secs());
                self.send_alert(&msg).await;
            }
            Err(e) => {
                error!(worker=%self.worker_id, err=%e, "failed to update SL for solo trade; closing position");
                self.close_solo(idx, ExitReason::ExitRule, false).await;
            }
        }
    }

    async fn push_sl_update_group(&mut self, gi: usize, new_sl: f64) {
        if gi >= self.group_trades.len() {
            return;
        }
        let n_chunks = self.group_trades[gi].chunks.len();
        let mut failed: Vec<usize> = Vec::new();

        for ci in 0..n_chunks {
            let pos = self.group_trades[gi].chunks[ci].position;
            match self.executor.update_sl(&pos, new_sl).await {
                Ok(updated) => {
                    self.group_trades[gi].chunks[ci].position = updated;
                    if let Ok(db) = self.trade_db.lock() {
                        db.update_stop(updated.trade_id, new_sl).ok();
                    }
                    let msg = formatter::format_stop_update(&updated, &self.symbol, now_secs());
                    self.send_alert(&msg).await;
                }
                Err(e) => {
                    error!(worker=%self.worker_id, err=%e, "failed to update SL for group chunk {ci}; will close");
                    failed.push(ci);
                }
            }
        }

        // Close any chunks that failed SL update.
        for ci in failed.into_iter().rev() {
            self.close_group_chunk_at(gi, ci, ExitReason::ExitRule, false)
                .await;
            self.group_trades[gi].chunks.swap_remove(ci);
        }
        if self.group_trades[gi].is_fully_closed() {
            self.group_trades.swap_remove(gi);
        }
    }

    // ── Utilities ─────────────────────────────────────────────────────────────

    fn format_close(&self, rec: &TradeRecord) -> String {
        match rec.exit_reason {
            ExitReason::StopLoss => formatter::format_stop_out(rec),
            ExitReason::TakeProfit => formatter::format_take_profit(rec),
            _ => formatter::format_exit_rule(rec),
        }
    }

    /// Fire-and-forget alert. The send is spawned so a slow or rate-limited
    /// notifier (e.g. a Telegram 429 that sleeps 60s) can never block the worker's
    /// event loop and stall tick/stop processing.
    async fn send_alert(&self, msg: &str) {
        let notifier = self.notifier.clone();
        let worker_id = self.worker_id.clone();
        let msg = msg.to_string();
        tokio::spawn(async move {
            if let Err(e) = notifier.send(&msg).await {
                error!(worker=%worker_id, err=%e, "failed to send alert");
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use broker::{BarStream, DataProvider, Executor, OrderRequest, TickStream};
    use infra::NullNotifier;
    use std::sync::{Arc, Mutex};
    use tokio::sync::watch;
    use ts_core::{
        AccountInfo, Bar, ExitReason, Position, SymbolInfo, Tick, Timeframe, TradeRecord,
    };

    struct MockBroker {
        balance: f64,
        equity: f64,
    }

    #[async_trait]
    impl DataProvider for MockBroker {
        async fn ohlcv(
            &self,
            _sym: &str,
            _tf: Timeframe,
            _start: i64,
            _end: i64,
        ) -> anyhow::Result<Vec<Bar>> {
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
    impl Executor for MockBroker {
        async fn open(&self, req: &OrderRequest) -> anyhow::Result<Position> {
            Ok(Position {
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
        async fn close(&self, pos: &Position, _symbol: &str) -> anyhow::Result<TradeRecord> {
            Ok(TradeRecord::close_position(
                pos,
                "BTCUSDT",
                pos.entry_price,
                12346,
                ExitReason::ExitRule,
            ))
        }
        async fn update_sl(&self, pos: &Position, sl: f64) -> anyhow::Result<Position> {
            let mut p = *pos;
            p.current_stop_loss = sl;
            Ok(p)
        }
    }

    #[async_trait]
    impl BarStream for MockBroker {
        async fn subscribe(
            &self,
            _sym: &str,
            _tf: Timeframe,
            _tx: tokio::sync::mpsc::Sender<Bar>,
        ) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[async_trait]
    impl TickStream for MockBroker {
        async fn subscribe(
            &self,
            _sym: &str,
            _tx: tokio::sync::mpsc::Sender<Tick>,
        ) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_live_worker_lifecycle() {
        use data::TradeDb;

        let now_nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp_db_path = std::env::temp_dir().join(format!("test_worker_db_{}.db", now_nanos));
        let trade_db = Arc::new(Mutex::new(TradeDb::open(&temp_db_path).unwrap()));

        let worker_json = r#"{
            "strategy": "rsi_reversion",
            "symbol": "BTCUSDT",
            "timeframe": "1h",
            "risk_manager": {
                "type": "fixed_percent",
                "pct": 0.02,
                "initial_balance": 100000.0
            },
            "stop_manager": {
                "type": "fixed",
                "stop_distance": 10.0,
                "start_rr": 0.0
            },
            "indicators": {
                "rsi_14": {
                    "type": "rsi",
                    "period": 14
                }
            }
        }"#;
        let config: LiveWorkerConfig = serde_json::from_str(worker_json).unwrap();

        let mock_broker = Arc::new(MockBroker {
            balance: 100000.0,
            equity: 100000.0,
        });
        let handles = broker::BrokerHandles {
            provider: mock_broker.clone(),
            executor: mock_broker.clone(),
            streamer: mock_broker.clone(),
            tick_streamer: mock_broker.clone(),
        };

        let notifier = Arc::new(NullNotifier);
        let (_dd_tx, dd_rx) = watch::channel(0.0);

        let mut worker = LiveWorker::new(&config, &handles, notifier, trade_db, dd_rx).unwrap();

        let res = worker.seed_bars(&config).await;
        assert!(res.is_ok());

        worker.recover_open_positions(&config).await;

        let blackout = infra::news::BlackoutNotification {
            active: true,
            window: None,
        };
        worker.on_news_blackout(blackout).await;
        assert!(worker.blackout_active);

        let clear_blackout = infra::news::BlackoutNotification {
            active: false,
            window: None,
        };
        worker.on_news_blackout(clear_blackout).await;
        assert!(!worker.blackout_active);

        std::fs::remove_file(&temp_db_path).ok();
    }

    #[tokio::test]
    async fn test_worker_details() {
        use data::TradeDb;

        let now_nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp_db_path =
            std::env::temp_dir().join(format!("test_worker_details_{}.db", now_nanos));
        let trade_db = Arc::new(Mutex::new(TradeDb::open(&temp_db_path).unwrap()));

        let worker_json = r#"{
            "strategy": "ema_cross",
            "symbol": "BTCUSDT",
            "timeframe": "1h",
            "risk_manager": {
                "type": "fixed_amount",
                "amount": 1.0
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
        }"#;
        let config: LiveWorkerConfig = serde_json::from_str(worker_json).unwrap();

        let mock_broker = Arc::new(MockBroker {
            balance: 100000.0,
            equity: 100000.0,
        });
        let handles = broker::BrokerHandles {
            provider: mock_broker.clone(),
            executor: mock_broker.clone(),
            streamer: mock_broker.clone(),
            tick_streamer: mock_broker.clone(),
        };

        let notifier = Arc::new(NullNotifier);
        let (_dd_tx, dd_rx) = watch::channel(0.0);

        let mut worker = LiveWorker::new(&config, &handles, notifier, trade_db, dd_rx).unwrap();

        // 1. Test consider_entry
        let sig = ts_core::Signal::new(Direction::Buy, 100.0, 90.0, 120.0);
        worker.consider_entry(&sig).await;
        assert_eq!(worker.solo_trades.len(), 1);
        assert_eq!(worker.long_count, 1);

        // 2. Test on_tick (no stop/tp hit)
        let safe_tick = Tick {
            symbol: "BTCUSDT".to_string(),
            bid: 100.0,
            ask: 100.0,
            last: 100.0,
            volume: 1.0,
            timestamp: 1000.0,
        };
        worker.on_tick(&safe_tick).await;
        assert_eq!(worker.solo_trades.len(), 1);

        // 3. Test on_tick (stop hit)
        let stop_tick = Tick {
            symbol: "BTCUSDT".to_string(),
            bid: 89.0,
            ask: 89.0,
            last: 89.0,
            volume: 1.0,
            timestamp: 1001.0,
        };
        worker.on_tick(&stop_tick).await;
        assert_eq!(worker.solo_trades.len(), 0);
        assert_eq!(worker.long_count, 0);

        // 4. Test exit rules checking
        let sig_hold = ts_core::Signal::new(Direction::Hold, 100.0, 90.0, 120.0);
        worker.consider_entry(&sig).await; // open again
        assert_eq!(worker.solo_trades.len(), 1);

        let bar = Bar::new(1719000000, 100.0, 101.0, 99.0, 100.0, 1000.0);
        worker
            .check_and_apply_exit_rules(0, &[bar], &ts_core::IndicatorSet::default(), &sig_hold)
            .await;
        assert_eq!(worker.solo_trades.len(), 1);

        // 5. Test close_all_positions
        worker.close_all_positions(ExitReason::ExitRule).await;
        assert_eq!(worker.solo_trades.len(), 0);

        std::fs::remove_file(&temp_db_path).ok();
    }

    #[tokio::test]
    async fn test_worker_group_trades() {
        use data::TradeDb;

        let now_nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp_db_path =
            std::env::temp_dir().join(format!("test_worker_groups_{}.db", now_nanos));
        let trade_db = Arc::new(Mutex::new(TradeDb::open(&temp_db_path).unwrap()));

        let worker_json = r#"{
            "strategy": "rsi_reversion",
            "symbol": "BTCUSDT",
            "timeframe": "1h",
            "trade_executor": "paper",
            "risk_manager": {
                "type": "fixed_amount",
                "amount": 2500.0
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
                "overbought": 70.0,
                "stop_pct": 0.02,
                "tp_pct": 0.04
            }
        }"#;
        let config: LiveWorkerConfig = serde_json::from_str(worker_json).unwrap();

        // max_lot is 100.0, so volume of 250.0 will be split into: 100.0, 100.0, 50.0 (3 chunks)
        let mock_broker = Arc::new(MockBroker {
            balance: 100000.0,
            equity: 100000.0,
        });
        let handles = broker::BrokerHandles {
            provider: mock_broker.clone(),
            executor: mock_broker.clone(),
            streamer: mock_broker.clone(),
            tick_streamer: mock_broker.clone(),
        };

        let notifier = Arc::new(NullNotifier);
        let (_dd_tx, dd_rx) = watch::channel(0.0);

        let mut worker = LiveWorker::new(&config, &handles, notifier, trade_db, dd_rx).unwrap();

        // Assert split allowed is true (non-Binance trade executor defaults to split_allowed = true)
        assert!(worker.split_allowed);

        // Consider entry (Buy)
        let sig = ts_core::Signal::new(Direction::Buy, 100.0, 90.0, 120.0);
        worker.consider_entry(&sig).await;

        // Since it was split, we should have group_trades populated and 3 positions
        assert_eq!(worker.group_trades.len(), 1);
        assert_eq!(worker.group_trades[0].chunks.len(), 3);
        assert_eq!(worker.long_count, 3);

        // Test check tick stop on group
        let stop_tick = Tick {
            symbol: "BTCUSDT".to_string(),
            bid: 89.0,
            ask: 89.0,
            last: 89.0,
            volume: 1.0,
            timestamp: 1001.0,
        };
        worker.on_tick(&stop_tick).await;

        assert_eq!(worker.group_trades.len(), 0);
        assert_eq!(worker.long_count, 0);

        std::fs::remove_file(&temp_db_path).ok();
    }
}
