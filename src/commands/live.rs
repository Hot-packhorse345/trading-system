use anyhow::Result;
use std::path::PathBuf;

pub async fn run_cmd(config: PathBuf) -> Result<()> {
    let json = std::fs::read_to_string(&config)
        .map_err(|e| anyhow::anyhow!("cannot read config {}: {e}", config.display()))?;
    let cfg = live::LiveConfig::from_json(&json)?;
    live::LiveEngine::new(cfg).run().await
}
