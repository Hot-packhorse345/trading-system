use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "live", about = "Live trading engine (minimal)")]
struct Cli {
    /// Path to live trading config JSON.
    #[arg(short, long)]
    config: PathBuf,

    /// Emit JSON structured log output.
    #[arg(long, default_value_t = false)]
    json_log: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    let cli = Cli::parse();
    infra::init_tracing(cli.json_log);

    let json = std::fs::read_to_string(&cli.config)
        .map_err(|e| anyhow::anyhow!("cannot read config {}: {e}", cli.config.display()))?;

    let cfg = live::LiveConfig::from_json(&json)?;
    live::LiveEngine::new(cfg).run().await
}
