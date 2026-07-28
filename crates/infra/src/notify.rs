use anyhow::{anyhow, Result};
use async_trait::async_trait;
use reqwest::Client;
use tokio::time::{sleep, Duration};
use tracing::{error, warn};

#[async_trait]
pub trait Notifier: Send + Sync {
    async fn send(&self, msg: &str) -> Result<()>;
}

pub struct TelegramNotifier {
    client: Client,
    token: String,
    chat_id: String,
    max_retries: u32,
}

impl TelegramNotifier {
    pub fn new(token: impl Into<String>, chat_id: impl Into<String>) -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(10))
                .pool_idle_timeout(Duration::from_secs(90))
                .build()
                .expect("Failed to build HTTP client"),
            token: token.into(),
            chat_id: chat_id.into(),
            max_retries: 3,
        }
    }
}

#[async_trait]
impl Notifier for TelegramNotifier {
    async fn send(&self, msg: &str) -> Result<()> {
        let url = format!("https://api.telegram.org/bot{}/sendMessage", self.token);
        let payload = serde_json::json!({
            "chat_id": self.chat_id,
            "text": msg,
            "parse_mode": "HTML"
        });

        let mut retries = 0;
        loop {
            let resp = match self.client.post(&url).json(&payload).send().await {
                Ok(r) => r,
                Err(e) => {
                    error!("Telegram request failed: {e}");
                    if retries >= self.max_retries {
                        return Err(e.into());
                    }
                    sleep(Duration::from_millis(200 * (1 << retries))).await;
                    retries += 1;
                    continue;
                }
            };

            if resp.status().is_success() {
                return Ok(());
            } else {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                warn!("Telegram API error {}: {}", status, body);

                if status.as_u16() == 429 {
                    sleep(Duration::from_secs(60)).await;
                } else if retries >= self.max_retries {
                    return Err(anyhow!("Telegram API error {}: {}", status, body));
                } else {
                    sleep(Duration::from_millis(500 * (1 << retries))).await;
                    retries += 1;
                }
            }
        }
    }
}

pub struct NullNotifier;
#[async_trait]
impl Notifier for NullNotifier {
    async fn send(&self, _msg: &str) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_telegram_notifier_new_and_send() {
        let mut notifier = TelegramNotifier::new("dummy_token", "dummy_chat_id");
        // Lower max_retries to 1 for fast test execution
        notifier.max_retries = 1;

        let res = notifier.send("test message").await;
        assert!(res.is_err());
    }
}
