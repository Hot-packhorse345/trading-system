use indicators::IndicatorConfig;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyConfig {
    pub name: String,
    #[serde(default)]
    pub params: HashMap<String, serde_json::Value>,
    pub indicators: Vec<IndicatorConfig>,
}
