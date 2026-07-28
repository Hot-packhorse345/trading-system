use crate::{
    strategies::{
        ema_cross::EmaCross
    },
    Strategy,
};
use anyhow::{anyhow, Result};

pub fn build_strategy(name: &str) -> Result<Box<dyn Strategy>> {
    match name {
        "ema_cross" => Ok(Box::new(EmaCross)),
        other => Err(anyhow!("unknown strategy: '{other}'")),
    }
}
