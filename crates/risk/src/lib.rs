pub mod config;
pub mod exit;
pub mod factory;
pub mod stop;
pub mod volume;

pub use config::{ExitManagerConfig, StopManagerConfig, VolumeManagerConfig, VolumeTier};
pub use exit::{any_should_exit, ExitManager, StrategyExit, TimeExit};
pub use factory::{build_exit, build_stop, build_volume};
pub use stop::{
    advance_stop, AtrTrail, BreakevenTrail, FixedStop, StopManager, SupertrendTrail, TslVariant1,
    TslVariant2, TslVariant3, TslVariant4,
};
pub use volume::{FixedAmount, FixedPercent, TieredPercent, VolumeManager};
