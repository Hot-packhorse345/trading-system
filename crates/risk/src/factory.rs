use crate::{
    config::{ExitManagerConfig, StopManagerConfig, VolumeManagerConfig},
    exit::{ExitManager, StrategyExit, StrategyExitCondition, TimeExit},
    stop::{
        AtrTrail, BreakevenTrail, FixedStop, StopManager, SupertrendTrail, TslVariant1,
        TslVariant2, TslVariant3, TslVariant4,
    },
    volume::{FixedAmount, FixedPercent, TieredPercent, VolumeManager},
};
use anyhow::Result;

pub fn build_volume(cfg: &VolumeManagerConfig) -> Result<Box<dyn VolumeManager>> {
    match cfg {
        VolumeManagerConfig::FixedPercent {
            pct,
            initial_balance,
        } => Ok(Box::new(FixedPercent {
            pct: *pct,
            initial_balance: *initial_balance,
        })),
        VolumeManagerConfig::FixedAmount { amount } => {
            Ok(Box::new(FixedAmount { amount: *amount }))
        }
        VolumeManagerConfig::TieredPercent {
            tiers,
            daily_dd_limit,
        } => {
            let tier_vec = tiers
                .iter()
                .map(|t| (t.dd_min, t.dd_max, t.risk_pct))
                .collect();
            Ok(Box::new(TieredPercent {
                tiers: tier_vec,
                daily_dd_limit: *daily_dd_limit,
            }))
        }
    }
}

pub fn build_stop(cfg: &StopManagerConfig) -> Box<dyn StopManager> {
    match cfg.sm_type.as_str() {
        "fixed" => Box::new(FixedStop::default()),
        "variant1" => Box::new(TslVariant1::new(cfg.stop_distance)),
        "variant2" => Box::new(TslVariant2::new(cfg.stop_distance)),
        "variant3" => Box::new(TslVariant3::new(cfg.stop_distance, cfg.start_rr)),
        "variant4" => Box::new(TslVariant4::new(cfg.stop_distance, cfg.start_rr)),
        // For atr_trail / supertrend, `stop_distance` is the ATR multiplier and
        // `start_rr` (when >= 1) is reused as the ATR period (default 14 / 10).
        "atr_trail" => {
            let period = if cfg.start_rr >= 1.0 {
                cfg.start_rr as usize
            } else {
                14
            };
            Box::new(AtrTrail::new(cfg.stop_distance, period))
        }
        "breakeven" => Box::new(BreakevenTrail::new(cfg.start_rr, cfg.stop_distance)),
        "supertrend" => {
            let period = if cfg.start_rr >= 1.0 {
                cfg.start_rr as usize
            } else {
                10
            };
            Box::new(SupertrendTrail::new(period, cfg.stop_distance))
        }
        other => {
            tracing::warn!(sm_type=%other, "unknown stop manager type, using fixed");
            Box::new(FixedStop::default())
        }
    }
}

pub fn build_exit(cfgs: &[ExitManagerConfig]) -> Vec<Box<dyn ExitManager>> {
    cfgs.iter()
        .map(|cfg| -> Box<dyn ExitManager> {
            match cfg {
                ExitManagerConfig::Strategy { condition } => {
                    let cond = if condition == "opposite_signal" {
                        StrategyExitCondition::OppositeSignal
                    } else {
                        StrategyExitCondition::BiasFlip
                    };
                    Box::new(StrategyExit { condition: cond })
                }
                ExitManagerConfig::Time {
                    delta_time,
                    max_bars,
                    exit_hour_utc,
                } => Box::new(TimeExit {
                    delta_time: delta_time.clone(),
                    max_bars: *max_bars,
                    exit_hour_utc: *exit_hour_utc,
                }),
            }
        })
        .collect()
}
