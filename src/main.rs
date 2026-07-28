use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod commands;

#[derive(Parser, Debug)]
#[command(
    name = "trading-system",
    version = "0.1.0",
    about = "High-performance trading system"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,

    #[arg(long, default_value_t = false)]
    json_log: bool,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Run a backtest (parallel grid search)
    Backtest {
        #[arg(short, long)]
        config: PathBuf,
        #[arg(long, default_value_t = 10)]
        top: usize,
        #[arg(long, default_value = "enhanced_score")]
        metric: String,
    },

    /// Run the live trading engine
    Live {
        #[arg(short, long)]
        config: PathBuf,
    },

    /// Run a walk-forward analysis (rolling in-sample/out-of-sample)
    Walkforward {
        #[arg(short, long)]
        config: PathBuf,
        #[arg(short, long)]
        output: Option<PathBuf>,
    },

    /// Utility tools (download, workflow, data review, validation, deploy live)
    Tools {
        #[command(subcommand)]
        command: commands::tools::ToolsCommand,
    },

    /// Automatically discover trading edges by running WFA, plateau tests, etc.
    DiscoverEdge {
        #[arg(short = 't', long)]
        template: String,
        #[arg(short = 'b', long, default_value = "ftmo")]
        broker: String,
        #[arg(short = 's', long, value_delimiter = ',')]
        symbols: Option<Vec<String>>,
        #[arg(short = 'T', long, value_delimiter = ',', default_values = ["15m", "30m", "1h", "4h"])]
        timeframes: Vec<String>,
        #[arg(long)]
        min_density: Option<f64>,
        #[arg(long)]
        resume: Option<PathBuf>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    let cli = Cli::parse();
    infra::init_tracing(cli.json_log);

    match cli.command {
        Command::Backtest {
            config,
            top,
            metric,
        } => commands::backtest::run_cmd(config, top, Some(metric)).await?,

        Command::Live { config } => commands::live::run_cmd(config).await?,

        Command::Walkforward { config, output } => {
            commands::walkforward::run_cmd(config, output).await?
        }

        Command::Tools { command } => commands::tools::run_cmd(command).await?,

        Command::DiscoverEdge {
            template,
            broker,
            symbols,
            timeframes,
            min_density,
            resume,
        } => {
            commands::edge::run(
                template,
                broker,
                symbols,
                timeframes,
                min_density,
                resume,
            )
            .await?
        }
    }

    Ok(())
}
