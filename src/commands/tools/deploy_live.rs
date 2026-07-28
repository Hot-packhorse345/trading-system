use anyhow::{Context, Result};
use live::LiveConfig;
use std::path::{Path, PathBuf};
use tracing::info;

const BINARY_NAME: &str = "live";

pub async fn run(config: PathBuf, out: PathBuf) -> Result<()> {
    let config_json = std::fs::read_to_string(&config)
        .with_context(|| format!("cannot read config {}", config.display()))?;

    let cfg = LiveConfig::from_json(&config_json).context("invalid live config")?;
    let workers = cfg.workers();

    let uses_binance = workers.iter().any(|w| {
        let dp = w.data_provider.to_lowercase();
        let te = w.trade_executor.to_lowercase();
        dp == "binance" || te == "binance"
    });
    let uses_mt5 = workers.iter().any(|w| {
        let dp = w.data_provider.to_lowercase();
        let te = w.trade_executor.to_lowercase();
        dp == "mt5" || te == "mt5"
    });
    let uses_telegram = workers.iter().any(|w| {
        w.alert_channels
            .iter()
            .any(|c| c.to_lowercase() == "telegram")
    });

    info!(
        config=%config.display(),
        out=%out.display(),
        workers=workers.len(),
        "packaging live binary"
    );

    build_live_binary()?;

    if out.exists() {
        std::fs::remove_dir_all(&out)
            .with_context(|| format!("cannot clear output dir {}", out.display()))?;
    }
    std::fs::create_dir_all(&out)
        .with_context(|| format!("cannot create output dir {}", out.display()))?;

    // Binary
    let binary_src = PathBuf::from(format!("target/release/{BINARY_NAME}"));
    anyhow::ensure!(
        binary_src.is_file(),
        "binary not found after build: {} — make sure `cargo build --release --bin {BINARY_NAME}` ran successfully",
        binary_src.display()
    );
    let binary_dst = out.join(BINARY_NAME);
    std::fs::copy(&binary_src, &binary_dst)
        .with_context(|| format!("cannot copy binary to {}", binary_dst.display()))?;
    #[cfg(unix)]
    set_executable(&binary_dst)?;

    // Config
    let config_dst = out.join("config.json");
    std::fs::copy(&config, &config_dst)
        .with_context(|| format!("cannot copy config to {}", config_dst.display()))?;

    // data/ directory placeholder
    std::fs::create_dir_all(out.join("data"))?;

    // run.sh
    let run_sh = out.join("run.sh");
    std::fs::write(&run_sh, gen_run_sh())?;
    #[cfg(unix)]
    set_executable(&run_sh)?;

    // systemd unit
    std::fs::write(out.join("live.service"), gen_systemd_service())?;

    // .env.example
    std::fs::write(
        out.join(".env.example"),
        gen_env_example(uses_binance, uses_mt5, uses_telegram),
    )?;

    // README.md
    std::fs::write(
        out.join("README.md"),
        gen_readme(&config, workers, uses_binance, uses_mt5),
    )?;

    let total = count_files(&out);

    println!();
    println!("{}", "=".repeat(62));
    println!("  Deployment package ready!");
    println!("  Output  : {}", out.display());
    println!("  Binary  : {BINARY_NAME}  (stripped release build)");
    println!("  Workers : {}", workers.len());
    println!("  Config  : config.json  (read at runtime, not baked in)");
    println!("  Files   : {total}");
    println!();
    println!("  Upload to VPS:");
    println!(
        "    rsync -avz {out}/ user@vps:/opt/live/",
        out = out.display()
    );
    println!("    ssh user@vps \\");
    println!("      \"cd /opt/live && cp .env.example .env && nano .env && ./run.sh\"");
    println!("{}", "=".repeat(62));
    println!();

    info!("deploy-live done — {} files in {}", total, out.display());
    Ok(())
}

fn build_live_binary() -> Result<()> {
    println!("Building release binary (`cargo build --release --bin {BINARY_NAME}`)…");
    let status = std::process::Command::new("cargo")
        .args(["build", "--release", "--bin", BINARY_NAME])
        .status()
        .context("failed to spawn cargo — is it in PATH?")?;
    anyhow::ensure!(
        status.success(),
        "cargo build --release --bin {BINARY_NAME} failed (exit code {})",
        status.code().unwrap_or(-1)
    );
    Ok(())
}

#[cfg(unix)]
fn set_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path)
        .with_context(|| format!("cannot stat {}", path.display()))?
        .permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms)
        .with_context(|| format!("cannot chmod {}", path.display()))?;
    Ok(())
}

fn count_files(dir: &Path) -> usize {
    std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| {
                    if e.path().is_dir() {
                        count_files(&e.path())
                    } else {
                        1
                    }
                })
                .sum()
        })
        .unwrap_or(0)
}

fn gen_run_sh() -> &'static str {
    r#"#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

if [[ -f ".env" ]]; then
    set -o allexport
    source .env
    set +o allexport
fi

exec ./live --config config.json "$@"
"#
}

fn gen_systemd_service() -> &'static str {
    r#"[Unit]
Description=Live Trading Engine
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
WorkingDirectory=/opt/live
ExecStart=/opt/live/live --config config.json
Restart=on-failure
RestartSec=10
EnvironmentFile=/opt/live/.env

[Install]
WantedBy=multi-user.target
"#
}

fn gen_env_example(uses_binance: bool, uses_mt5: bool, uses_telegram: bool) -> String {
    let mut lines: Vec<String> = vec![
        "# Copy to .env and fill in your values.".into(),
        "# Lines starting with # are comments and are ignored.".into(),
        String::new(),
    ];

    if uses_telegram {
        lines.push("# ── Telegram ─────────────────────────────────────────────────".into());
        lines.push("TELEGRAM_BOT_TOKEN=".into());
        lines.push("TELEGRAM_CHAT_ID=".into());
        lines.push(String::new());
    }

    if uses_binance {
        lines.push("# ── Binance ──────────────────────────────────────────────────".into());
        lines.push("BINANCE_API_KEY=".into());
        lines.push("BINANCE_API_SECRET=".into());
        lines.push(String::new());
    }

    if uses_mt5 {
        lines.push("# ── MetaTrader 5 ─────────────────────────────────────────────".into());
        lines.push("MT5_LOGIN=".into());
        lines.push("MT5_PASSWORD=".into());
        lines.push("MT5_SERVER=".into());
        lines.push(
            r"MT5_DLL_PATH=C:\Program Files\MetaTrader 5\MQL5\Libraries\mt5_bridge.dll".into(),
        );
        lines.push(String::new());
    }

    lines.push("# ── Logging ──────────────────────────────────────────────────".into());
    lines.push("RUST_LOG=info".into());
    lines.push("# JSON_LOG=0  # set to 1 for structured JSON log output".into());

    lines.join("\n") + "\n"
}

fn gen_readme(
    config_path: &Path,
    workers: &[live::LiveWorkerConfig],
    uses_binance: bool,
    uses_mt5: bool,
) -> String {
    let dedup = |f: &dyn Fn(&live::LiveWorkerConfig) -> String| -> String {
        let mut seen = std::collections::HashSet::new();
        workers
            .iter()
            .filter_map(|w| {
                let v = f(w);
                if seen.insert(v.clone()) {
                    Some(v)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join(", ")
    };

    let strategy_str = dedup(&|w| w.strategy.clone());
    let symbol_str = dedup(&|w| w.symbol.clone());
    let adapter_str = dedup(&|w| w.trade_executor.clone());

    let worker_rows = workers
        .iter()
        .map(|w| {
            let stop_tf_tmp;
            let stop_tf = match &w.stop_timeframe {
                Some(v) => match v.as_str() {
                    Some(s) => s,
                    None => {
                        stop_tf_tmp = v.to_string();
                        &stop_tf_tmp
                    }
                },
                None => "-",
            };
            format!(
                "| {} | {} | {} | {} | {} |",
                w.strategy, w.symbol, w.timeframe, stop_tf, w.trade_executor
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let creds_note = if uses_binance || uses_mt5 {
        "Fill in your broker credentials in `.env` before running."
    } else {
        "No real broker credentials required (paper mode)."
    };

    format!(
        r#"# Live Trading Engine — Deployment

Generated from `{config}` by `trading-system tools deploy-live`.

| Component | Value |
|-----------|-------|
| Strategy  | `{strategy_str}` |
| Symbol    | `{symbol_str}` |
| Executor  | `{adapter_str}` |
| Workers   | {n} |

## Workers

| Strategy | Symbol | Timeframe | Stop TF | Executor |
|----------|--------|-----------|---------|----------|
{worker_rows}

## Package contents

```
.
├── live              ← live-only binary (minimal, no backtest / tool code)
├── config.json       ← trading config (read at runtime)
├── run.sh            ← convenience startup script
├── live.service      ← systemd unit for auto-start on boot
├── .env.example      ← env-var template (copy to .env)
└── data/             ← OHLCV cache and trade DB (created at runtime)
```

## VPS setup

```bash
# 1. Upload the package
rsync -avz ./ user@vps:/opt/live/

# 2. SSH in and configure
ssh user@vps
cd /opt/live
chmod +x run.sh live
cp .env.example .env
nano .env          # fill in secrets

# 3. Run manually to verify
./run.sh

# 4. (Optional) Install as a systemd service
sudo cp live.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable live
sudo systemctl start  live
journalctl -u live -f
```

## Notes

- {creds_note}
- `config.json` is read at startup — update it on the VPS and restart the service to change settings.
- `data/` is created automatically on first run; it holds local bar caches and the open-positions DB.
- To redeploy: re-run `deploy-live` locally and `rsync` again; then `sudo systemctl restart live`.
"#,
        config = config_path.display(),
        strategy_str = strategy_str,
        symbol_str = symbol_str,
        adapter_str = adapter_str,
        n = workers.len(),
        worker_rows = worker_rows,
        creds_note = creds_note,
    )
}
