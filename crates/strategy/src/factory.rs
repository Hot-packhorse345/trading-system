use crate::{
    strategies::{ema_cross::EmaCross, rsi_reversion::RsiReversion},
    Strategy,
};
use anyhow::{anyhow, Result};

pub fn build_strategy(name: &str) -> Result<Box<dyn Strategy>> {
    match name {
        "ema_cross" => Ok(Box::new(EmaCross)),
        "rsi_reversion" => Ok(Box::new(RsiReversion)),
        other => Err(anyhow!("unknown strategy: '{other}'")),
    }
}
