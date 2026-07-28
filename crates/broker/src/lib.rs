pub mod binance;
pub mod factory;
pub mod paper;
pub mod traits;

#[cfg(target_os = "windows")]
pub mod mt5;

pub use binance::BinanceBroker;
pub use factory::{build_broker_handles, build_data_provider, BrokerHandles};
pub use paper::PaperBroker;
pub use traits::{BarStream, DataProvider, Executor, OrderRequest, TickStream};

#[cfg(target_os = "windows")]
pub use mt5::Mt5Client;
