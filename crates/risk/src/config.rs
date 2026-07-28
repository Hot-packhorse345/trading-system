use serde::{Deserialize, Serialize};

fn default_stop_distance() -> f64 {
    1.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StopManagerConfig {
    #[serde(rename = "type")]
    pub sm_type: String,
    #[serde(default = "default_stop_distance")]
    pub stop_distance: f64,
    #[serde(default)]
    pub start_rr: f64,
}

impl StopManagerConfig {
    pub fn fixed() -> Self {
        Self {
            sm_type: "fixed".into(),
            stop_distance: 0.0,
            start_rr: 0.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum VolumeManagerConfig {
    FixedPercent {
        #[serde(alias = "risk_percentage")]
        pct: f64,
        initial_balance: f64,
    },
    FixedAmount {
        amount: f64,
    },
    TieredPercent {
        #[serde(alias = "risk_tiers")]
        tiers: Vec<VolumeTier>,
        daily_dd_limit: f64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolumeTier {
    pub dd_min: f64,
    pub dd_max: f64,
    pub risk_pct: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ExitManagerConfig {
    #[serde(rename = "strategy_exit")]
    Strategy {
        #[serde(default = "default_condition")]
        condition: String,
    },
    #[serde(rename = "time_exit")]
    Time {
        delta_time: Option<String>,
        max_bars: Option<usize>,
        exit_hour_utc: Option<u32>,
    },
}

fn default_condition() -> String {
    "bias_flip".to_string()
}
