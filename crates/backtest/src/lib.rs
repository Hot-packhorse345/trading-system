pub mod config;
pub mod cost_model;
pub mod engine;
pub mod grid;
pub mod metrics;
pub mod report;
pub mod result;
pub mod simulator;
pub mod swap;
pub mod top_heap;

pub use config::{BacktestConfig, GroupSpec, ResolvedCombo};
pub use cost_model::{CostModel, DynamicSpreadCostModel};
pub use engine::run;
pub use grid::{canonical_value, combo_hash_for, count_combos, expand_sample_values};
pub use metrics::Metrics;
pub use report::{write_csv, write_trades_csv};
pub use result::BacktestResult;
pub use swap::{RolloverMode, SwapConfig};
