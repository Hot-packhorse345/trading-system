use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("indicator error: {0}")]
    Indicator(String),
    #[error("strategy error: {0}")]
    Strategy(String),
    #[error("broker error: {0}")]
    Broker(String),
    #[error("data error: {0}")]
    Data(String),
    #[error("config error: {0}")]
    Config(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}
