mod bar_stream;
mod data_provider;
mod executor;
mod tick_stream;

use anyhow::{anyhow, Result};
use hmac::{Hmac, Mac};
use reqwest::Client;
use serde_json::Value;
use sha2::Sha256;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tokio::time::Duration;
use ts_core::SymbolInfo;

/// How long a fetched `SymbolInfo` is reused before refetching. Exchange filters
/// (tick size, lot step) change rarely; this prevents fetching the heavy
/// `exchangeInfo` payload on every entry and every stop-loss update.
pub(crate) const SYMBOL_INFO_TTL: Duration = Duration::from_secs(300);

pub(crate) const LIVE_REST: &str = "https://fapi.binance.com";
pub(crate) const TEST_REST: &str = "https://testnet.binancefuture.com";
pub(crate) const LIVE_WS: &str = "wss://fstream.binance.com/ws";
pub(crate) const TEST_WS: &str = "wss://stream.binancefuture.com/ws";

#[derive(Clone)]
pub(crate) struct OpenOrderInfo {
    pub symbol: String,
    pub sl_order_id: Option<i64>,
    pub tp_order_id: Option<i64>,

    pub position_side: String,
}

#[derive(Clone)]
pub struct BinanceBroker {
    pub(crate) client: Client,
    pub(crate) api_key: String,
    pub(crate) api_secret: String,
    pub(crate) rest: String,
    pub(crate) ws: String,

    pub(crate) open_orders: Arc<Mutex<HashMap<u64, OpenOrderInfo>>>,
    pub(crate) symbol_cache: Arc<Mutex<HashMap<String, (SymbolInfo, Instant)>>>,
}

impl BinanceBroker {
    pub fn new(api_key: String, api_secret: String, testnet: bool) -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap(),
            api_key,
            api_secret,
            rest: if testnet { TEST_REST } else { LIVE_REST }.to_string(),
            ws: if testnet { TEST_WS } else { LIVE_WS }.to_string(),
            open_orders: Arc::new(Mutex::new(HashMap::new())),
            symbol_cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    // ── Shared HTTP helpers ─────────────────────────────────────────────

    pub(crate) fn ts_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
    }

    pub(crate) fn sign(&self, query: &str) -> String {
        let mut mac = Hmac::<Sha256>::new_from_slice(self.api_secret.as_bytes()).unwrap();
        mac.update(query.as_bytes());
        hex::encode(mac.finalize().into_bytes())
    }

    pub(crate) async fn get_signed(&self, path: &str, params: &str) -> Result<Value> {
        let ts = Self::ts_ms();
        let query = if params.is_empty() {
            format!("timestamp={ts}")
        } else {
            format!("{params}&timestamp={ts}")
        };
        let sig = self.sign(&query);
        let url = format!("{}{path}?{query}&signature={sig}", self.rest);
        let resp: Value = self
            .client
            .get(&url)
            .header("X-MBX-APIKEY", &self.api_key)
            .send()
            .await?
            .json()
            .await?;
        Self::check_error(&resp)?;
        Ok(resp)
    }

    pub(crate) async fn post_signed(&self, path: &str, params: &str) -> Result<Value> {
        let ts = Self::ts_ms();
        let body = format!("{params}&timestamp={ts}");
        let sig = self.sign(&body);
        let url = format!("{}{path}", self.rest);
        let resp: Value = self
            .client
            .post(&url)
            .header("X-MBX-APIKEY", &self.api_key)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(format!("{body}&signature={sig}"))
            .send()
            .await?
            .json()
            .await?;
        Self::check_error(&resp)?;
        Ok(resp)
    }

    pub(crate) async fn delete_signed(&self, path: &str, params: &str) -> Result<Value> {
        let ts = Self::ts_ms();
        let query = format!("{params}&timestamp={ts}");
        let sig = self.sign(&query);
        let url = format!("{}{path}?{query}&signature={sig}", self.rest);
        let resp: Value = self
            .client
            .delete(&url)
            .header("X-MBX-APIKEY", &self.api_key)
            .send()
            .await?
            .json()
            .await?;
        Self::check_error(&resp)?;
        Ok(resp)
    }

    fn check_error(resp: &Value) -> Result<()> {
        if let Some(code) = resp.get("code") {
            return Err(anyhow!("Binance error {}: {}", code, resp["msg"]));
        }
        Ok(())
    }

    pub(crate) fn round_price(price: f64, tick_size: f64) -> f64 {
        if tick_size <= 0.0 {
            return price;
        }
        let factor = 1.0 / tick_size;
        (price * factor).round() / factor
    }
}
