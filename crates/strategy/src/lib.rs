pub mod config;
pub mod factory;
pub mod schema;
pub mod strategies;

pub use config::StrategyConfig;
pub use factory::build_strategy;
pub use schema::{ParamKind, ParamSpec};

use std::collections::HashMap;
use ts_core::{Bar, IndicatorSet, Params, Signal};

pub trait Strategy: Send + Sync {
    fn generate_signals(
        &self,
        bars: &[Bar],
        cols: &IndicatorSet,
        params: &Params,
        ind_params: &HashMap<String, Params>,
    ) -> Vec<Option<Signal>>;

    fn param_schema(&self) -> Vec<ParamSpec>;
}
