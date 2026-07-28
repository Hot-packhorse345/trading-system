use anyhow::{anyhow, Result};
use broker::{build_broker_handles, BrokerHandles};
use data::TradeDb;
use infra::{
    news::NewsBlackoutService,
    notify::{NullNotifier, TelegramNotifier},
    Notifier,
};
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, watch};
use tracing::{error, info, warn};

use crate::{
    account_reporter,
    config::{LiveConfig, LiveWorkerConfig},
    drawdown_manager,
    worker::LiveWorker,
};

pub struct LiveEngine {
    config: LiveConfig,
}

impl LiveEngine {
    pub fn new(config: LiveConfig) -> Self {
        Self { config }
    }

    pub async fn run(self) -> Result<()> {
        let workers = self.config.into_workers();
        if workers.is_empty() {
            return Err(anyhow!("no workers configured"));
        }

        let n_workers = workers.len();
        info!(n_workers=%n_workers, "live engine initialising");

        // ── Build notifier ───────────────────────────────────────────────────
        let notifier: Arc<dyn Notifier> = build_notifier();

        // ── Build broker handles per worker ──────────────────────────────────
        // Each worker gets its own handles (streams need separate subscriptions).
        let mut handles_per_worker: Vec<BrokerHandles> = Vec::new();
        for w in &workers {
            let h = build_broker_handles(&w.trade_executor)
                .map_err(|e| anyhow!("failed to build broker '{}': {e}", w.trade_executor))?;
            handles_per_worker.push(h);
        }

        // ── Shared state ─────────────────────────────────────────────────────
        let data_dir = workers[0].data_dir.join("live");
        std::fs::create_dir_all(&data_dir).map_err(|e| anyhow!("cannot create data dir: {e}"))?;

        let trade_db = Arc::new(Mutex::new(
            TradeDb::open(data_dir.join("trades.db"))
                .map_err(|e| anyhow!("cannot open trade DB: {e}"))?,
        ));

        // ── Shutdown channel ─────────────────────────────────────────────────
        let (shutdown_tx, _shutdown_rx) = watch::channel(false);

        // ── Drawdown emergency channel ────────────────────────────────────────
        let (emergency_tx, emergency_rx) = watch::channel(false);

        // ── Current-drawdown channel (fraction 0.0–1.0) for tiered sizing ─────
        let (dd_tx, dd_rx) = watch::channel(0.0_f64);

        // ── News blackout service ─────────────────────────────────────────────
        let news_dir = workers[0].data_dir.clone();
        let news_service = NewsBlackoutService::new(news_dir);
        let mut news_rxs: Vec<mpsc::UnboundedReceiver<infra::news::BlackoutNotification>> =
            Vec::new();
        for w in workers.iter() {
            let worker_id = format!("{}:{}:{}", w.strategy, w.symbol, w.timeframe);
            let rx = news_service
                .register_worker(worker_id, w.news_blackout.clone())
                .await;
            news_rxs.push(rx);
        }
        news_service.start().await?;

        // ── Validate executor homogeneity ──────────────────────────────────────
        // The drawdown monitor polls a single account (first worker's provider). If
        // workers mix executors, the DD check may read the wrong account.
        {
            let first_exec = workers[0].trade_executor.to_lowercase();
            for (i, w) in workers.iter().enumerate().skip(1) {
                if w.trade_executor.to_lowercase() != first_exec {
                    warn!(
                        worker_idx=%i,
                        executor=%w.trade_executor,
                        first=%first_exec,
                        "worker uses a different executor than the drawdown monitor's provider — DD check may be inaccurate"
                    );
                }
            }
        }

        // ── Launch drawdown monitor ───────────────────────────────────────────
        {
            let provider = handles_per_worker[0].provider.clone();
            let notifier_c = notifier.clone();
            let data_dir_c = workers[0].data_dir.clone();
            let dd_limit = extract_daily_dd_limit(&workers[0]);
            let shtdn_rx = shutdown_tx.subscribe();
            let emrg_tx = emergency_tx.clone();
            let dd_tx_c = dd_tx.clone();

            tokio::spawn(async move {
                drawdown_manager::run(
                    provider, dd_limit, emrg_tx, dd_tx_c, notifier_c, data_dir_c, shtdn_rx,
                )
                .await;
            });
        }

        // ── Launch account reporter ───────────────────────────────────────────
        {
            let provider = handles_per_worker[0].provider.clone();
            let notifier_c = notifier.clone();
            let data_dir_c = workers[0].data_dir.clone();
            let shtdn_rx = shutdown_tx.subscribe();

            tokio::spawn(async move {
                account_reporter::run(provider, notifier_c, data_dir_c, shtdn_rx).await;
            });
        }

        // ── Launch workers ────────────────────────────────────────────────────
        let mut worker_handles = Vec::new();

        for (i, (w_config, handles)) in workers.into_iter().zip(handles_per_worker).enumerate() {
            let news_rx = news_rxs.remove(0);
            let emrg_rx = emergency_rx.clone();
            let shtdn_rx = shutdown_tx.subscribe();
            let notifier_c = notifier.clone();
            let db_c = trade_db.clone();

            // ── Per-worker decay monitor (opt-in via embedded oos_distribution) ──
            let (decay_tx, decay_rx) = watch::channel(false);
            if let Some(ref oos_dist) = w_config.oos_distribution {
                if !oos_dist.is_empty() {
                    let oos_dist = oos_dist.clone();
                    let symbol_upper = w_config.symbol.to_uppercase();
                    let strategy_id = crate::worker::compute_strategy_id(
                        &w_config.strategy,
                        &symbol_upper,
                        &w_config.timeframe,
                    );
                    let worker_id =
                        format!("{}:{}:{}", w_config.strategy, symbol_upper, w_config.timeframe);
                    let db_for_decay = trade_db.clone();
                    let notifier_for_decay = notifier.clone();
                    let decay_shtdn_rx = shutdown_tx.subscribe();
                    tokio::spawn(async move {
                        crate::decay_monitor::run(
                            db_for_decay,
                            strategy_id,
                            worker_id,
                            oos_dist,
                            notifier_for_decay,
                            decay_tx,
                            decay_shtdn_rx,
                        )
                        .await;
                    });
                }
            }

            let worker = match LiveWorker::new(&w_config, &handles, notifier_c, db_c, dd_rx.clone())
            {
                Ok(w) => w,
                Err(e) => {
                    error!(err=%e, "failed to build worker {i}");
                    continue;
                }
            };

            let jh = tokio::spawn(async move {
                if let Err(e) = worker
                    .run(w_config, handles, news_rx, emrg_rx, decay_rx, shtdn_rx)
                    .await
                {
                    error!(worker_idx=%i, err=%e, "worker exited with error");
                }
            });
            worker_handles.push(jh);
        }

        // ── Await Ctrl-C / SIGTERM ────────────────────────────────────────────
        #[cfg(unix)]
        let sigterm_fut = {
            use tokio::signal::unix::{signal, SignalKind};
            async move {
                match signal(SignalKind::terminate()) {
                    Ok(mut s) => {
                        s.recv().await;
                    }
                    Err(_) => std::future::pending::<()>().await,
                }
            }
        };
        #[cfg(not(unix))]
        let sigterm_fut = std::future::pending::<()>();

        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                info!("received Ctrl-C, shutting down");
            }
            _ = sigterm_fut => {
                info!("received SIGTERM, shutting down");
            }
        }

        // ── Graceful shutdown ─────────────────────────────────────────────────
        shutdown_tx.send(true).ok();
        news_service.stop().await;

        for jh in worker_handles {
            jh.await.ok();
        }

        info!("live engine stopped");
        Ok(())
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn build_notifier() -> Arc<dyn Notifier> {
    let token = std::env::var("TELEGRAM_BOT_TOKEN").unwrap_or_default();
    let chat_id = std::env::var("TELEGRAM_CHAT_ID").unwrap_or_default();
    if !token.is_empty() && !chat_id.is_empty() {
        Arc::new(TelegramNotifier::new(token, chat_id))
    } else {
        warn!("TELEGRAM_BOT_TOKEN or TELEGRAM_CHAT_ID not set — using null notifier");
        Arc::new(NullNotifier)
    }
}

/// Extract daily drawdown limit from the first worker's risk_manager config.
fn extract_daily_dd_limit(config: &LiveWorkerConfig) -> f64 {
    config
        .risk_manager
        .get("daily_dd_limit")
        .and_then(|v| v.as_f64())
        .unwrap_or(5.0)
}

