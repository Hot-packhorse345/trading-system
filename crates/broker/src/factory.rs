use anyhow::{anyhow, Result};
use std::sync::Arc;
use tracing::info;

use crate::{
    binance::BinanceBroker,
    paper::PaperBroker,
    traits::{BarStream, DataProvider, Executor, TickStream},
};

pub struct BrokerHandles {
    pub provider: Arc<dyn DataProvider>,
    pub executor: Arc<dyn Executor>,
    pub streamer: Arc<dyn BarStream>,
    pub tick_streamer: Arc<dyn TickStream>,
}

/// Build all four broker trait handles with strict credential validation.
/// Intended for live trading where execution access is required.
pub fn build_broker_handles(name: &str) -> Result<BrokerHandles> {
    match name.to_lowercase().as_str() {
        "paper" => build_paper(),
        "binance" => build_binance(),
        #[cfg(target_os = "windows")]
        "mt5" => build_mt5(),
        #[cfg(not(target_os = "windows"))]
        "mt5" => Err(anyhow!("mt5 broker is only supported on Windows builds")),
        other => Err(anyhow!("unknown broker: '{other}'")),
    }
}

/// Build a data provider only, with lenient credential handling.
/// Intended for read-only tools where API keys are optional (public endpoints).
pub fn build_data_provider(name: &str) -> Result<Box<dyn DataProvider>> {
    match name.to_lowercase().as_str() {
        "paper" => Ok(Box::new(PaperBroker::from_env())),
        "binance" => {
            let testnet = std::env::var("BINANCE_TESTNET")
                .map(|v| v.to_lowercase() == "true")
                .unwrap_or(false);
            let api_key = std::env::var("BINANCE_API_KEY").unwrap_or_default();
            let api_secret = std::env::var("BINANCE_API_SECRET").unwrap_or_default();
            Ok(Box::new(BinanceBroker::new(api_key, api_secret, testnet)))
        }
        #[cfg(target_os = "windows")]
        "mt5" => {
            let login = std::env::var("MT5_LOGIN")
                .map_err(|_| anyhow!("MT5_LOGIN not set"))?
                .parse::<i64>()
                .map_err(|_| anyhow!("MT5_LOGIN must be numeric"))?;
            let password =
                std::env::var("MT5_PASSWORD").map_err(|_| anyhow!("MT5_PASSWORD not set"))?;
            let server = std::env::var("MT5_SERVER").map_err(|_| anyhow!("MT5_SERVER not set"))?;
            Ok(Box::new(crate::Mt5Client::connect(
                login, &password, &server,
            )?))
        }
        #[cfg(not(target_os = "windows"))]
        "mt5" => Err(anyhow!("mt5 broker is only supported on Windows builds")),
        other => Err(anyhow!("unknown broker: '{other}'")),
    }
}

fn build_paper() -> Result<BrokerHandles> {
    let b = Arc::new(PaperBroker::from_env());
    Ok(BrokerHandles {
        provider: b.clone(),
        executor: b.clone(),
        streamer: b.clone(),
        tick_streamer: b,
    })
}

fn build_binance() -> Result<BrokerHandles> {
    let api_key =
        std::env::var("BINANCE_API_KEY").map_err(|_| anyhow!("BINANCE_API_KEY not set"))?;
    let api_secret =
        std::env::var("BINANCE_API_SECRET").map_err(|_| anyhow!("BINANCE_API_SECRET not set"))?;
    let testnet = std::env::var("BINANCE_TESTNET")
        .map(|v| v.to_lowercase() == "true")
        .unwrap_or(false);
    info!(testnet=%testnet, "building Binance broker");
    let b = Arc::new(BinanceBroker::new(api_key, api_secret, testnet));
    Ok(BrokerHandles {
        provider: b.clone(),
        executor: b.clone(),
        streamer: b.clone(),
        tick_streamer: b,
    })
}

#[cfg(target_os = "windows")]
fn build_mt5() -> Result<BrokerHandles> {
    let login = std::env::var("MT5_LOGIN")
        .map_err(|_| anyhow!("MT5_LOGIN not set"))?
        .parse::<i64>()
        .map_err(|_| anyhow!("MT5_LOGIN must be numeric"))?;
    let password = std::env::var("MT5_PASSWORD").map_err(|_| anyhow!("MT5_PASSWORD not set"))?;
    let server = std::env::var("MT5_SERVER").map_err(|_| anyhow!("MT5_SERVER not set"))?;
    let client = Arc::new(crate::Mt5Client::connect(login, &password, &server)?);
    Ok(BrokerHandles {
        provider: client.clone(),
        executor: client.clone(),
        streamer: client.clone(),
        tick_streamer: client,
    })
}
