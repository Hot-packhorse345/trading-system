use anyhow::Result;
use backtest::config::BacktestConfig;
use backtest::count_combos;
use clap::Subcommand;
use std::path::PathBuf;

pub mod csv_snapshot;
pub mod deploy_live;
pub mod download;
pub mod generate_live_config;
pub mod generate_templates;
pub mod repaint;
pub mod repaint_common;
pub mod review_data;
pub mod review_indicator;
pub mod review_strategy_signals;
pub mod strategy_repaint;
pub mod utils;
pub mod workflow;

#[derive(Subcommand, Debug)]
pub enum ToolsCommand {
    /// Download historical OHLCV data
    Download {
        #[arg(short = 'S', long)]
        symbol: String,
        #[arg(short, long, default_value = "1h")]
        timeframe: String,
        #[arg(long)]
        from: String,
        #[arg(long, default_value = "now")]
        to: String,
        #[arg(short, long, default_value = "data")]
        output: PathBuf,
        #[arg(long, default_value = "binance")]
        broker: String,
    },

    /// Review cached OHLCV data (summary + tail)
    ReviewData {
        #[arg(short = 'S', long)]
        symbol: String,
        #[arg(short, long, default_value = "1h")]
        timeframe: String,
        #[arg(long)]
        from: Option<String>,
        #[arg(long)]
        to: Option<String>,
        #[arg(long, default_value_t = 20)]
        tail: usize,
        #[arg(short, long, default_value = "data")]
        data_dir: PathBuf,
    },

    /// Check whether an indicator repaints on historical bars
    RepaintCheck {
        #[arg(long, conflicts_with = "indicator")]
        config: Option<PathBuf>,
        #[arg(long, conflicts_with = "config")]
        indicator: Option<String>,
        #[arg(long, default_value = "")]
        indicator_config: String,
        #[arg(long, default_value = "BTCUSDT")]
        symbol: String,
        #[arg(long, default_value = "1h")]
        timeframe: String,
        #[arg(long, default_value_t = 500)]
        bars: usize,
        #[arg(long, default_value_t = 0)]
        warmup: usize,
        #[arg(long, default_value_t = 10)]
        shift_step: usize,
        #[arg(long, default_value = "both")]
        test_mode: String,
        #[arg(long)]
        export: Option<PathBuf>,
        #[arg(long, default_value = "data")]
        data_dir: PathBuf,
        #[arg(long, default_value_t = false)]
        verbose: bool,
    },

    /// Run multiple backtests sequentially, analyse results, optionally export best config
    Workflow {
        #[arg(long, num_args = 1.., conflicts_with = "workflow_file")]
        configs: Vec<PathBuf>,
        #[arg(long, conflicts_with = "configs")]
        workflow_file: Option<PathBuf>,
        #[arg(long, default_value = "enhanced_score")]
        metric: String,
        #[arg(long, default_value_t = 10)]
        top: usize,
        #[arg(long, default_value_t = 1)]
        rank: usize,
        #[arg(long, default_value_t = false)]
        ascending: bool,
        #[arg(long, default_value_t = false)]
        stop_on_fail: bool,
        #[arg(long, default_value_t = false)]
        no_alert: bool,
        #[arg(long, default_value_t = false)]
        cleanup: bool,
        #[arg(long, default_value_t = false)]
        export_best_config: bool,
        #[arg(long, default_value_t = 1)]
        best_config_rank: usize,
        #[arg(long)]
        best_config_dir: Option<PathBuf>,
        #[arg(long, num_args = 1.., default_values = ["strategy_parameters.", "indicators.", "stop_manager."])]
        best_config_param_prefixes: Vec<String>,
    },

    /// Calculate indicator outputs and review tail values
    Indicator {
        #[arg(long, conflicts_with = "indicator")]
        config: Option<PathBuf>,
        #[arg(long, conflicts_with = "config")]
        indicator: Option<String>,
        #[arg(long, default_value = "")]
        indicator_config: String,
        #[arg(long, default_value = "BTCUSDT")]
        symbol: String,
        #[arg(long, default_value = "1h")]
        timeframe: String,
        #[arg(long, default_value_t = 500)]
        bars: usize,
        #[arg(long, default_value_t = 25)]
        tail: usize,
        #[arg(long, default_value = "data")]
        data_dir: PathBuf,
    },

    /// Calculate strategy signals and review tail outputs
    StrategySignals {
        #[arg(long)]
        config: PathBuf,
        #[arg(long, default_value_t = 0)]
        combo: usize,
        #[arg(long, default_value_t = 0)]
        bars: usize,
        #[arg(long, default_value_t = 25)]
        tail: usize,
        #[arg(long)]
        data_dir: Option<PathBuf>,
        #[arg(long)]
        export: Option<PathBuf>,
        #[arg(long)]
        export_trades: Option<PathBuf>,
    },

    /// Check whether a strategy repaints signals on historical bars
    StrategyRepaintCheck {
        #[arg(long)]
        config: PathBuf,
        #[arg(long, default_value_t = 0)]
        combo: usize,
        #[arg(long, default_value_t = 500)]
        bars: usize,
        #[arg(long, default_value_t = 0)]
        warmup: usize,
        #[arg(long, default_value_t = 10)]
        shift_step: usize,
        #[arg(long, default_value = "both")]
        test_mode: String,
        #[arg(long)]
        export: Option<PathBuf>,
        #[arg(long)]
        data_dir: Option<PathBuf>,
        #[arg(long, default_value_t = false)]
        verbose: bool,
    },

    /// Package a minimal live-trading deployment for a VPS
    DeployLive {
        #[arg(short, long)]
        config: PathBuf,
        #[arg(long, default_value = "dist/live")]
        out: PathBuf,
    },

    /// Count the total number of parameter combinations in a backtest config
    CountCombos {
        #[arg(short, long)]
        config: PathBuf,
    },

    /// Render a single result from a backtest CSV as a PNG image
    CsvSnapshot {
        file: PathBuf,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long, default_value_t = 1)]
        rank: usize,
    },

    /// Generate a live trading config from a backtest results CSV
    GenerateLiveConfig {
        #[arg(long)]
        csv: PathBuf,
        #[arg(long)]
        backtest_config: Option<PathBuf>,
        #[arg(long, default_value_t = 1)]
        rank: usize,
        #[arg(long)]
        metric: Option<String>,
        #[arg(long, default_value_t = false)]
        ascending: bool,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long, default_value = "binance")]
        trade_executor: String,
        #[arg(long, default_value = "binance")]
        bar_streamer: String,
        #[arg(long, default_value = "binance")]
        tick_streamer: String,
        #[arg(long)]
        risk_manager: Option<String>,
    },

    /// Generate DiscoveryTemplate JSON files with synthesized parameter grids
    GenerateTemplates {
        #[arg(long)]
        strategy: String,
        #[arg(long)]
        broker: String,
        #[arg(long, num_args = 1.., default_values = ["..."])]
        symbols: Vec<String>,
        #[arg(long, use_value_delimiter = true, default_values = ["1h", "4h"])]
        timeframes: Vec<String>,
        #[arg(long)]
        asset_class: Option<String>,
        #[arg(long, default_value = "data")]
        data_dir: PathBuf,
        #[arg(long)]
        exclude_holdout_start: Option<String>,
        #[arg(long)]
        exclude_holdout_end: Option<String>,
    },
}

pub async fn run_cmd(cmd: ToolsCommand) -> Result<()> {
    match cmd {
        ToolsCommand::Download {
            symbol,
            timeframe,
            from,
            to,
            output,
            broker,
        } => download::run(symbol, timeframe, from, to, output, broker).await,
        ToolsCommand::ReviewData {
            symbol,
            timeframe,
            from,
            to,
            tail,
            data_dir,
        } => review_data::run(symbol, timeframe, from, to, tail, data_dir),
        ToolsCommand::RepaintCheck {
            config,
            indicator,
            indicator_config,
            symbol,
            timeframe,
            bars,
            warmup,
            shift_step,
            test_mode,
            export,
            data_dir,
            verbose,
        } => {
            repaint::run_cmd(
                config,
                indicator,
                if indicator_config.is_empty() {
                    None
                } else {
                    Some(indicator_config)
                },
                symbol,
                timeframe,
                bars,
                warmup,
                shift_step,
                test_mode,
                export,
                data_dir,
                verbose,
            )
            .await
        }
        ToolsCommand::Workflow {
            configs,
            workflow_file,
            metric,
            top,
            rank,
            ascending,
            stop_on_fail,
            no_alert,
            cleanup,
            export_best_config,
            best_config_rank,
            best_config_dir,
            best_config_param_prefixes,
        } => {
            workflow::run_cmd(
                configs,
                workflow_file,
                metric,
                top,
                rank,
                ascending,
                stop_on_fail,
                no_alert,
                cleanup,
                export_best_config,
                best_config_rank,
                best_config_dir,
                best_config_param_prefixes,
            )
            .await
        }
        ToolsCommand::Indicator {
            config,
            indicator,
            indicator_config,
            symbol,
            timeframe,
            bars,
            tail,
            data_dir,
        } => {
            review_indicator::run(
                config,
                indicator,
                indicator_config,
                symbol,
                timeframe,
                bars,
                tail,
                data_dir,
            )
            .await
        }
        ToolsCommand::StrategySignals {
            config,
            combo,
            bars,
            tail,
            data_dir,
            export,
            export_trades,
        } => {
            review_strategy_signals::run(config, combo, bars, tail, data_dir, export, export_trades)
                .await
        }
        ToolsCommand::StrategyRepaintCheck {
            config,
            combo,
            bars,
            warmup,
            shift_step,
            test_mode,
            export,
            data_dir,
            verbose,
        } => {
            strategy_repaint::run_cmd(
                config, combo, bars, warmup, shift_step, test_mode, export, data_dir, verbose,
            )
            .await
        }
        ToolsCommand::DeployLive { config, out } => deploy_live::run(config, out).await,
        ToolsCommand::CountCombos { config } => {
            let text = std::fs::read_to_string(&config)?;
            let cfg: BacktestConfig = serde_json::from_str(&text)?;
            println!("{}", count_combos(&cfg)?);
            Ok(())
        }
        ToolsCommand::CsvSnapshot { file, out, rank } => csv_snapshot::run(file, out, rank),
        ToolsCommand::GenerateLiveConfig {
            csv,
            backtest_config,
            rank,
            metric,
            ascending,
            out,
            trade_executor,
            bar_streamer,
            tick_streamer,
            risk_manager,
        } => generate_live_config::run(
            csv,
            backtest_config,
            rank,
            metric,
            ascending,
            out,
            trade_executor,
            bar_streamer,
            tick_streamer,
            risk_manager,
        ),
        ToolsCommand::GenerateTemplates {
            strategy,
            broker,
            symbols,
            timeframes,
            asset_class,
            data_dir,
            exclude_holdout_start,
            exclude_holdout_end,
        } => {
            generate_templates::run(
                strategy,
                broker,
                symbols,
                timeframes,
                asset_class,
                data_dir,
                exclude_holdout_start,
                exclude_holdout_end,
            )
            .await
        }
    }
}
