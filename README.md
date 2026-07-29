# 🚀 Algorithmic Trading System

A high-performance algorithmic trading system written entirely in Rust. Designed for speed, safety, and reliability, it leverages native compilation, memory safety, and zero-overhead parallelism to deliver exceptional performance in both backtesting and live trading environments.

![Rust 1.75+](https://img.shields.io/badge/Rust-1.75+-CE422B?style=flat&logo=rust&logoColor=white)
![License MIT](https://img.shields.io/badge/License-MIT-green?style=flat)
![Status Active](https://img.shields.io/badge/Status-Active-brightgreen?style=flat)
![Platforms Linux/Windows](https://img.shields.io/badge/Platforms-Linux%2FWindows-blue?style=flat)

---

## 📋 Table of Contents
- [✨ Core Features](#core-features)
- [🚀 Quick Start](#quick-start)
- [🏗️ System Architecture](#system-architecture)
- [📁 Project Structure](#project-structure)
- [⚙️ Configuration Guide](#configuration-guide)
- [🎮 Usage Modes](#usage-modes)
  - [📥 Download Data (tools)](#download-data-tools)
  - [📊 Backtest Mode](#backtest-mode)
  - [📡 Live Trading](#live-trading)
  - [🔁 Walk-Forward Analysis](#walk-forward-analysis)
  - [🔍 Edge Discovery](#edge-discovery)
  - [🧰 Tools (Utilities)](#tools-utilities)
- [🖥️ Visualization & Tools](#visualization-tools)
- [⚡ Performance](#performance)
- [🛡️ Risk Management](#risk-management)
- [🧪 Testing & Validation](#testing-validation)
- [📦 Deployment](#deployment)
- [🛠️ Development](#development)
- [🤝 Contributing](#contributing)
- [⚠️ Troubleshooting](#troubleshooting)
- [📄 License](#license)
- [⚠️ Disclaimer](#disclaimer)

---

<a id="core-features"></a>
## ✨ Core Features

### Five Operating Modes
| Mode | Purpose | Speed | Best For |
|------|---------|-------|----------|
| **Backtest** | Historical simulation | 800-1200 combos/sec | Parameter optimization & strategy development |
| **Live** | Real-time execution | Sub-millisecond | Production trading with Binance or MT5 |
| **Walkforward** | Rolling in-sample/out-of-sample validation | Grid search per window | Checking a strategy generalizes before going live |
| **DiscoverEdge** | Automated end-to-end edge discovery pipeline | Multi-phase, per symbol/timeframe | Scanning a broker's catalog for statistically robust edges |
| **Tools** | Utilities & validation | N/A | Download data, workflow batches, repaint checks, inspections |

### Key Capabilities
✅ **Technical Indicators** — EMA, MACD, RSI (Extensible via the `Indicator` trait)  
✅ **Example Strategy** — `ema_cross` (Trend-following EMA crossover)  
✅ **Multiple Brokers** — Binance (REST+WebSocket), MetaTrader 5 (Windows)
✅ **Parallel Grid Search** — Rayon-powered multi-threaded backtesting with resumable state  
✅ **Live Trading** — Tokio async engine with real-time bar/tick processing  
✅ **Risk Management** — Tiered position sizing, trailing stops, daily drawdown limits  
✅ **Economic Calendar** — News blackout integration for high-impact events  
✅ **Telegram Alerts** — Real-time trade notifications and account reporting  
✅ **Multi-layer Caching** — Parquet for OHLCV, SQLite for trades/state, in-memory for indicators  
✅ **Walk-Forward Analysis** — Rolling in-sample/out-of-sample validation with consensus parameter selection  
✅ **Automated Edge Discovery** — 8-phase pipeline (repaint gate → probe → WFA → dual-metric confirmation → full backtest → plateau robustness → stress test → cross-asset validation) across an entire broker symbol catalog  

---

<a id="quick-start"></a>
## 🚀 Quick Start

### Prerequisites
| Requirement | Version | Notes |
|-------------|---------|-------|
| Rust | 1.75+ | Install: `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \| sh` |
| OpenSSL | Latest | `apt-get install libssl-dev` on Linux; bundled on macOS/Windows |
| Git | Latest | For cloning the repository |

### Installation
```bash
# 1. Clone the repository
git clone https://github.com/0xbarss/trading-system.git
cd trading-system

# 2. Build in release mode (production-optimized)
cargo build --release

# 3. Verify installation
./target/release/trading-system --help
```

### Configuration
```bash
# 1. Copy environment template
cp .env.example .env

# 2. Edit .env with your credentials
nano .env
```

**Minimum `.env` for Binance:**
```env
BINANCE_API_KEY=your_key_here
BINANCE_API_SECRET=your_secret_here
RUST_LOG=info
```

### First Trade
```bash
# 1. Download historical data (1 year, 1h timeframe)
./target/release/trading-system tools download \
  -S BTCUSDT -t 1h --from 2024-01-01 -o data/

# 2. Run a backtest (grid search over parameter combinations)
./target/release/trading-system backtest \
  --config configs/backtest/ema_cross_btcusdt.json --top 5

# 3. Live trade in paper mode (simulated)
./target/release/trading-system live \
  --config configs/live/paper_ema_cross.json
```

---

<a id="system-architecture"></a>
## 🏗️ System Architecture

```text
┌─────────────────────────────────────────────────────────────┐
│                      CLI Layer (clap)                       │
│  5 subcommands: backtest, live, walkforward,                │
│                 tools, discover-edge                        │
└────────────────────────┬────────────────────────────────────┘
                         │
┌────────────────────────▼────────────────────────────────────┐
│                    Core Types (ts_core)                     │
│  Bar, Tick, Signal, Position, Account, Indicators, Enums   │
└─────────────────────────────────────────────────────────────┘
                         │
        ┌────────────────┼───────────────┐
        │                │               │
    ┌───▼───┐     ┌─────▼──────┐  ┌─────▼─────┐
    │ Risk  │     │  Indicators │  │ Strategies│
    │Mgmt   │     │             │  │           │
    └───────┘     └─────────────┘  └───────────┘
        │                │                │
        └────────────────┼────────────────┘
                         │
        ┌────────────────┼───────────────┐
        │                │               │
    ┌───▼──────┐  ┌──────▼──────┐ ┌─────▼────┐
    │  Broker  │  │    Data     │  │  Infra   │
    │ Adapters │  │  Caching    │  │ Logging, │
    │          │  │             │  │ Alerts   │
    └──────────┘  └─────────────┘  └──────────┘
        │
    ┌───▼──────────────────────────────────────┐
    │  Binance / MT5 / Paper Trading Engines   │
    └──────────────────────────────────────────┘
```

**Core Engines:**
- **Backtest** — Rayon-parallelized grid search with state resumption
- **Live** — Tokio async engine with real-time bar/tick streams
- **Walkforward** — Rolls the backtest engine across overlapping IS/OOS windows and scores generalization
- **Edge** — Orchestrates walkforward + backtest + plateau/stress/cross-asset checks into a gated discovery pipeline

---

<a id="project-structure"></a>
## 📁 Project Structure

```text
trading-system/
 ├── Cargo.toml                  # Workspace with 11 crates
 ├── src/
 │   ├── main.rs                 # CLI entry point (backtest, live, walkforward, tools, discover-edge)
 │   └── commands/
 │       ├── backtest.rs         # `backtest` command
 │       ├── live.rs             # `live` command
 │       ├── walkforward.rs      # `walkforward` command
 │       ├── edge.rs             # `discover-edge` command
 │       └── tools/              # `tools` subcommands (download, workflow, etc.)
 ├── crates/
 │   ├── ts_core/                # Core types (Bar, Signal, Position, etc.)
 │   ├── risk/                   # Position sizing & stop-loss managers
 │   ├── indicators/             # Technical indicators (EMA, MACD, RSI)
 │   ├── strategy/                # Trading strategies (ema_cross)
 │   ├── data/                   # OHLCV cache, trade DB, state cache
 │   ├── broker/                 # Binance, MT5, Paper adapters
 │   ├── infra/                  # Logging, news calendar, alerts
 │   ├── backtest/               # Backtesting engine (Rayon)
 │   ├── live/                   # Live trading engine (Tokio)
 │   ├── walkforward/            # Rolling IS/OOS window engine & HTML/Markdown reports
 │   └── edge/                   # Automated edge-discovery pipeline (gates, plateau, stress, cross-asset)
 ├── configs/
 │   ├── backtest/               # Backtest config JSONs
 │   └── live/                   # Live trading config JSONs
 ├── docs/
 │   └── broker/                 # Symbol catalogs consumed by `discover-edge` (FTMO, FXIFY, Binance)
 ├── data/                       # Auto-generated at runtime
 │   ├── ohlcv/                  # Parquet OHLCV cache
 │   ├── indicators/             # Indicator result cache
 │   ├── results/                # CSV backtest results, edge_discovery_*.json summaries
 │   └── *.db                    # SQLite caches (state, trades)
 ├── README.md                   # This file
 └── .env.example                # Environment variables template
```

---

<a id="configuration-guide"></a>
## ⚙️ Configuration Guide

### Backtest Configuration
All configs are JSON files with full type checking via Serde.

```json
{
  "strategy": "ema_cross",
  "symbol": "BTCUSDT",
  "timeframe": "15m",
  "pyramiding": true,
  "data_provider": "binance",
  "backtest_range": "1_year",
  "initial_balance": 100000.0,
  "risk_percentage": 0.001,
  "commission_percent": 0.000325,
  "data_dir": "data",
  "output_dir": "data/results",
  "stop_manager": {
    "type": "variant2",
    "stop_distance": [0.1, 0.2, 0.3],
    "start_rr": 1.0
  },
  "strategy_parameters": {
    "fast_period": 9,
    "slow_period": [21, 50],
    "stop_pct": [0.02, 0.03],
    "tp_pct": [0.04, 0.06]
  },
  "indicators": {
    "ema_fast": { "type": "ema", "period": 9 },
    "ema_slow": { "type": "ema", "period": [21, 50] }
  }
}
```

### Grid Expansion
Backtest configs (`strategy_parameters`, `indicators`, `stop_manager`) support several syntaxes for turning one config into many parameter combinations:

| Syntax | Example | Effect |
|---|---|---|
| Plain array | `"slow_period": [21, 50]` | Cartesian-product choice — expands independently against every other array in the config |
| `#` prefix | `"#levels": [1, 2, 3]` | Fixed/literal value — kept as-is (e.g. an actual array or object parameter), never expanded into combos |
| `[group]` prefix | `"[stops]stop_pct": [0.02, 0.03]`, `"[stops]tp_pct": [0.04, 0.06]` | Linked/grouped choice — values sharing the same group name are stepped through together (index 0 with index 0, index 1 with index 1, ...) instead of being crossed, so this pair yields 2 combos, not 4 |
| `$sample` object | `{ "$sample": "range", "start": 0.1, "stop": 0.3, "step": 0.1 }` | Generates a value list from a rule instead of listing every value by hand. Supported types: `range` (`start`/`stop`/`step`), `linspace` (`start`/`stop`/`n`, evenly spaced), `log` (`start`/`stop`/`n`, log-spaced), `values` (`items`, an explicit list) |

**Example combining all three:**
```json
{
  "strategy_parameters": {
    "fast_period": 9,
    "slow_period": [21, 50],
    "[stops]stop_pct": [0.02, 0.03],
    "[stops]tp_pct": [0.04, 0.06],
    "#allowed_hours": [8, 9, 10, 11, 12, 13, 14, 15, 16],
    "atr_multiplier": { "$sample": "linspace", "start": 1.0, "stop": 3.0, "n": 5 }
  }
}
```
- `slow_period` contributes 2 choices, crossed with everything else.
- `stop_pct`/`tp_pct` are grouped, so they contribute 2 *linked* choices (not 2 × 2 = 4).
- `#allowed_hours` is fixed — every combo gets the literal array `[8, 9, ..., 16]` as-is; it isn't expanded.
- `atr_multiplier` is sampled into 5 evenly spaced values via `linspace`, each crossed with everything else.

*Total combinations = 2 (slow_period) × 2 (linked stop/tp pair) × 1 (fixed hours) × 5 (atr_multiplier) = 20 parameter sets.*

Plain (non-prefixed) nested objects are expanded recursively the same way, which is how per-indicator parameters (e.g. `"ema_slow": { "type": "ema", "period": [21, 50] }`) get their own grids.

### Live Configuration
```json
{
  "strategy": "ema_cross",
  "symbol": "BTCUSDT",
  "timeframe": "1m",
  "stop_timeframe": "timeframe",
  "data_provider": "binance",
  "bar_streamer": "binance",
  "tick_streamer": "binance",
  "trade_executor": "paper",
  "pyramiding": true,
  "stop_manager": {
    "type": "variant1",
    "stop_distance": 0.5,
    "start_rr": 1.0
  },
  "volume_manager": {
    "type": "tiered_percent",
    "initial_balance": 100000.0,
    "daily_dd_limit": 3.5,
    "tiers": [
      { "dd_min": 0.0, "dd_max": 2.0, "risk_pct": 0.006 },
      { "dd_min": 2.0, "dd_max": 4.0, "risk_pct": 0.004 },
      { "dd_min": 4.0, "dd_max": 6.0, "risk_pct": 0.002 }
    ]
  },
  "indicators": {
    "ema_fast": { "type": "ema", "period": 9 },
    "ema_slow": { "type": "ema", "period": 21 },
    "rsi": { "type": "rsi", "period": 14 }
  }
}
```

<a id="walk-forward-configuration"></a>
### Walk-Forward Configuration
`walkforward` reads the **same JSON file** as `backtest` — it reuses the grid definition to optimize each in-sample window, and layers a few extra fields on top for the window mechanics and pass/fail thresholds:

```json
{
  "is_bars": 2000,
  "oos_bars": 500,
  "step_bars": 250,
  "min_wf_efficiency": 0.5,
  "min_oos_consistency": 0.6,
  "min_oos_trades_per_round": 3,
  "min_oos_consistency_lcb": 0.5,
  "consistency_confidence_z": 1.96,
  "metric": "enhanced_score"
}
```

| Field | Default | Meaning |
|---|---|---|
| `is_bars` | `2000` | Bars in each in-sample (optimization) window |
| `oos_bars` | `500` | Bars in each out-of-sample (validation) window |
| `step_bars` | `250` | Bars the window rolls forward each round |
| `min_wf_efficiency` | `0.5` | Minimum `mean(OOS score) / mean(IS score)` to pass |
| `min_oos_consistency` | `0.6` | Minimum fraction of OOS rounds that must be profitable |
| `min_oos_trades_per_round` | `0` | Rounds with fewer OOS trades than this are excluded from scoring |
| `min_oos_consistency_lcb` | `0.0` | Minimum Wilson lower-confidence-bound on OOS consistency (stricter than the raw ratio) |
| `metric` | `"enhanced_score"` | Metric used to rank in-sample combos and score OOS performance |

<a id="edge-discovery-template"></a>
### Edge Discovery Template
`discover-edge` reads a **discovery template** — a strategy-agnostic description of what to search over — plus an optional `gates` block controlling every pass/fail threshold in the pipeline:

```json
{
  "strategy": "ema_cross",
  "data_provider": "mt5",
  "metric": "enhanced_score",
  "initial_balance": 100000.0,
  "risk_percentage": 0.001,
  "pyramiding": false,
  "stop_manager": [
    { "type": "variant2", "stop_distance": { "$sample": "range", "start": 0.1, "stop": 0.3, "step": 0.1 }, "start_rr": 1.0 }
  ],
  "strategy_parameters": {
    "fast_period": 9,
    "slow_period": { "$sample": "values", "items": [21, 34, 50] }
  },
  "indicators": {
    "ema_fast": { "type": "ema", "period": 9 },
    "ema_slow": { "type": "ema", "period": { "$sample": "values", "items": [21, 34, 50] } }
  },
  "gates": {
    "min_probe_trades": 200,
    "min_wfe": 0.5,
    "min_consistency": 0.5,
    "min_pf": 1.5,
    "min_sharpe": 0.8,
    "min_wr": 0.30,
    "max_dd_pct": 25.0,
    "min_trades": 200,
    "min_density": 40.0,
    "plateau_pass_rate": 0.60,
    "max_stress_dd_pct": 15.0,
    "min_cross_asset_correlation": 0.60,
    "require_cross_asset": false
  }
}
```

`$sample` fields describe a search space (`values`, `range`, `linspace`, or `log`) that the pipeline resolves per phase — e.g. pinned to the first value for cheap probing, expanded to a full grid for walk-forward optimization. All `gates` fields are optional and fall back to the defaults shown above (see `crates/edge/src/gates.rs`).

---

<a id="usage-modes"></a>
## 🎮 Usage Modes

<a id="download-data-tools"></a>
### 📥 Download Data (tools)
```bash
# Download 1 year of BTCUSDT 1h bars
./target/release/trading-system tools download \
  -S BTCUSDT -t 1h --from 2024-01-01 --to 2024-12-31 -o data/

# Download multiple symbols
for symbol in BTCUSDT ETHUSDT SOLUSDT; do
  ./target/release/trading-system tools download -S $symbol -t 4h --from 2024-01-01 -o data/
done
```

<a id="backtest-mode"></a>
### 📊 Backtest Mode
```bash
# Single backtest
./target/release/trading-system backtest --config configs/backtest/ema_cross.json --top 10

# Backtest with custom log level
RUST_LOG=debug ./target/release/trading-system backtest --config configs/backtest/ema_cross.json

# JSON output for log aggregators
./target/release/trading-system --json-log backtest --config configs/backtest/ema_cross.json
```

**Output Files:**
- `data/results/<strategy_name>_<timestamp>.csv` — All metrics and parameters
- `data/results/trades_<hash>.csv` — Individual trade records

<a id="live-trading"></a>
### 📡 Live Trading
```bash
# Paper trading (safe for testing)
./target/release/trading-system live --config configs/live/paper_ema_cross.json

# Live trading (real money - use with caution!)
./target/release/trading-system live --config configs/live/binance_ema_cross.json
```

**Safety Features:**
- Daily drawdown limits (configurable, stops all trading when breached)
- Economic calendar integration (blackout windows for high-impact news)
- Graceful restart with trade recovery from SQLite
- Telegram alerts for all critical events

<a id="walk-forward-analysis"></a>
### 🔁 Walk-Forward Analysis
```bash
# Roll the grid in configs/backtest/ema_cross.json across IS/OOS windows
./target/release/trading-system walkforward \
  --config configs/backtest/ema_cross.json \
  --output data/results/wf_report.html
```
The command prints a console report (per-window IS/OOS scores, walk-forward efficiency, OOS consistency, verdict) and writes an `.html` or `.md` report (based on the `--output` extension; defaults to a timestamped `.html` under the config's `output_dir`). See [Walk-Forward Configuration](#walk-forward-configuration) for the extra fields the config needs.

<a id="edge-discovery"></a>
### 🔍 Edge Discovery
```bash
# Scan every FTMO symbol at 15m/30m/1h/4h for a robust ema_cross edge
./target/release/trading-system discover-edge \
  --template configs/discovery/ema_cross_template.json \
  --broker ftmo \
  --timeframes 15m,30m,1h,4h

# Restrict to specific symbols, override the density gate, and resume a prior run
./target/release/trading-system discover-edge \
  --template configs/discovery/ema_cross_template.json \
  --broker ftmo \
  --symbols EURUSD,XAUUSD \
  --min-density 30 \
  --resume data/results/edge_discovery_20250101_120000.json
```

For every symbol/timeframe pair, `discover-edge` runs an 8-phase gate (each phase can early-exit the pair as a fail):

| Phase | Name | What it checks |
|---|---|---|
| 0 | Repaint gate | Rejects strategies whose signals repaint on historical bars |
| 1 | Probe | Minimum trade density on a fixed-parameter run (`min_probe_trades`) |
| 2 | Walk-Forward Optimization | Runs `walkforward` over the template's grid; requires it to pass |
| 3 | Dual-Metric Confirmation | Re-runs walk-forward with the alternate metric to guard against metric overfitting |
| 4 | Full-Range Backtest | Consensus parameters must clear `min_pf`, `min_sharpe`, `min_wr`, `max_dd_pct`, `min_density` over the full range |
| 5 | Plateau Robustness | Neighboring parameter combinations around the consensus point must also be profitable |
| 6 | Synthetic Stress Test | Re-runs with injected black-swan shocks; checks `max_stress_dd_pct` |
| 7 | Cross-Asset Validation | If a correlated asset is configured, verifies the edge holds there too |

Broker symbol catalogs live in `docs/broker/{FTMO,FXIFY,BINANCE}_SYMBOLS.md`; `--broker` selects which one to load (`ftmo` by default). Results (verdict, metrics, and generated config paths for every phase) are printed as a summary table and written to `data/results/edge_discovery_<timestamp>.json`, which can be passed back in via `--resume` to skip symbols already marked `Passed`/`FailedGate`.

<a id="tools-utilities"></a>
### 🧰 Tools (Utilities)
`tools` groups data review, workflow, validation, and deployment commands.

```bash
# Review cached OHLCV data
./target/release/trading-system tools review-data -S BTCUSDT -t 1h --tail 10

# Review indicator outputs
./target/release/trading-system tools indicator \
  --indicator rsi --indicator-config '{"timeperiod": 14}' \
  --symbol BTCUSDT --timeframe 1h --bars 500 --tail 20

# Review strategy signals (combo 0 from backtest config)
./target/release/trading-system tools strategy-signals \
  --config configs/backtest/ema_cross_btcusdt.json --combo 0 --bars 2000 --tail 20

# Count total parameter combinations in a backtest config
./target/release/trading-system tools count-combos --config configs/backtest/ema_cross_btcusdt.json

# Validate indicator for live trading safety
./target/release/trading-system tools repaint-check \
  --indicator rsi \
  --indicator-config '{"timeperiod": 14}' \
  --symbol BTCUSDT \
  --timeframe 1h \
  --bars 1000 \
  --test-mode both

# Validate that a full strategy (not just an indicator) doesn't repaint signals
./target/release/trading-system tools strategy-repaint-check \
  --config configs/backtest/ema_cross_btcusdt.json \
  --combo 0 \
  --bars 1000 \
  --test-mode both

# Run multiple backtests, rank by Sharpe ratio
./target/release/trading-system tools workflow \
  --configs configs/backtest/ema_cross.json configs/backtest/ema_cross_alt.json \
  --metric sharpe \
  --top 5 \
  --export-best-config \
  --best-config-dir configs/optimized/

# Render top result from a backtest CSV as a PNG image
./target/release/trading-system tools csv-snapshot data/results/ema_cross_latest.csv --rank 1

# Generate a live trading config from backtest results CSV
./target/release/trading-system tools generate-live-config \
  --csv data/results/ema_cross_latest.csv \
  --backtest-config configs/backtest/ema_cross_btcusdt.json \
  --rank 1 \
  --trade-executor paper

# Generate discover-edge templates with synthesized parameter grids for every
# symbol in a broker's catalog
./target/release/trading-system tools generate-templates \
  --strategy ema_cross \
  --broker ftmo \
  --timeframes 15m,1h \
  --data-dir data

# Package a live-trading deployment for a VPS
./target/release/trading-system tools deploy-live --config configs/live/binance_ema_cross.json
```

---

<a id="visualization-tools"></a>
## 🖥️ Visualization & Tools

### CSV Analysis
After backtesting, analyze results:
```bash
# View top 10 results sorted by Sharpe ratio
head -11 data/results/ema_cross_*.csv | tail -10

# Calculate summary statistics
awk -F',' 'NR>1 {print $5, $12}' data/results/ema_cross_*.csv | sort -k2 -rn | head -5
```

### Equity Curve from Trades
```bash
# Plot equity curve from trade CSV
gnuplot << 'EOF'
set terminal png
set output 'equity.png'
set xlabel 'Trade #'
set ylabel 'Cumulative Profit'
set title 'Equity Curve'
plot 'data/results/trades_*.csv' using 1:5 with lines
EOF
```

---

<a id="performance"></a>
## ⚡ Performance

### Benchmarks
Measured on a 16-core CPU with Rust release build:

| Operation | Combinations | Time | Speed |
|---|---|---|---|
| Single backtest | 1 | ~15 ms | 67 BT/sec |
| Grid search (100 combos) | 100 | ~800 ms | 125 BT/sec |
| Full grid (500 combos) | 500 | ~550 ms | 909 BT/sec |
| Parallel grid (1000 combos) | 1000 | ~850 ms | 1,176 BT/sec |

### Optimization Tips
```bash
# 1. Use release build (already configured)
cargo build --release

# 2. Set Rayon thread pool size
export RAYON_NUM_THREADS=8

# 3. Run on SSD for faster cache operations
# Ensure `data/` is on fast storage

# 4. Batch multiple backtests in workflow mode
# Avoid individual invocations for each config
```

---

<a id="risk-management"></a>
## 🛡️ Risk Management

### Position Sizing
| Model | Formula | Use Case |
|---|---|---|
| Fixed Percent | `risk_pct × balance` | Compounding accounts |
| Fixed Amount | Fixed `$$` per trade | Consistent risk |
| Tiered Percent | Risk ↓ as DD ↑ | Prop firm rules |

### Stop-Loss Strategies
| Strategy | Description | Best For |
|---|---|---|
| `fixed` | Static distance from entry | Conservative traders |
| `variant1` | Trail from first profit | Breakout strategies |
| `variant2` | Trail after 1:1 R:R | Mean reversion |
| `atr_trail` | ATR-based trailing | Volatile markets |
| `supertrend` | Supertrend indicator | Trend-following |

### Daily Drawdown Limits
```json
{
  "volume_manager": {
    "type": "tiered_percent",
    "daily_dd_limit": 3.5,
    "tiers": [
      { "dd_min": 0.0, "dd_max": 2.0, "risk_pct": 0.006 },
      { "dd_min": 2.0, "dd_max": 4.0, "risk_pct": 0.003 }
    ]
  }
}
```
**When daily DD hits 3.5%:**
1. All new trades are paused
2. Telegram alert sent immediately
3. Trading resumes at UTC midnight

---

<a id="testing-validation"></a>
## 🧪 Testing & Validation

### Repaint Detection
Some indicators "repaint" — past values change on new bars. Use `tools repaint-check`:

```bash
./target/release/trading-system tools repaint-check \
  --indicator ema \
  --indicator-config '{"period": 20}' \
  --symbol BTCUSDT \
  --timeframe 1m \
  --bars 500
```

**Two Test Modes:**
- **Forward Simulation** — Feed bars incrementally, check if past values change
- **Origin Shifting** — Start dataset at different points, verify consistency

The same two test modes apply to `tools strategy-repaint-check`, which checks a full strategy's generated signals (rather than a single indicator's output) for repainting.

### Out-of-Sample Validation
Beyond repaint checks, two higher-level commands validate that a strategy generalizes rather than just fitting historical noise:
- **`walkforward`** — rolls a config's grid across overlapping in-sample/out-of-sample windows and reports walk-forward efficiency and OOS consistency. See [Walk-Forward Analysis](#walk-forward-analysis).
- **`discover-edge`** — runs `walkforward` plus plateau, synthetic stress, and cross-asset checks as part of an 8-phase gate. See [Edge Discovery](#edge-discovery).

---

<a id="deployment"></a>
## 📦 Deployment

### Linux VPS Setup
```bash
# 1. Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env

# 2. Clone repository
git clone https://github.com/0xbarss/trading-system.git
cd trading-system

# 3. Build
cargo build --release

# 4. Set up environment
cp .env.example .env
nano .env  # Add API keys

# 5. Run live trading in background
nohup ./target/release/trading-system live \
  --config configs/live/binance_ema_cross.json > live.log 2>&1 &

# 6. Monitor logs
tail -f live.log
```

### Systemd Service (Optional)
```ini
# /etc/systemd/system/trading-system.service
[Unit]
Description=Trading System Live Engine
After=network.target

[Service]
WorkingDirectory=/home/user/trading-system
ExecStart=/home/user/trading-system/target/release/trading-system live --config configs/live/binance.json
Restart=on-failure
RestartSec=10
EnvironmentFile=/home/user/trading-system/.env

[Install]
WantedBy=multi-user.target
```

**Enable and start:**
```bash
sudo systemctl daemon-reload
sudo systemctl enable trading-system
sudo systemctl start trading-system
sudo systemctl status trading-system
```

---

<a id="development"></a>
## 🛠️ Development

### Adding a New Indicator
Create `crates/indicators/src/my_indicator.rs`:
```rust
use ts_core::Bar;
use crate::Indicator;

pub struct MyIndicator { period: usize }

impl Indicator for MyIndicator {
    fn compute(&self, bars: &[Bar]) -> Vec<(String, Vec<f64>)> {
        let values = vec![0.0; bars.len()];  // Replace with real logic
        vec![("my_value".to_string(), values)]
    }
}
```
Register in `crates/indicators/src/registry.rs`:
```rust
"my_indicator" => Box::new(MyIndicator::new(14))
```
Export from `crates/indicators/src/lib.rs`:
```rust
pub mod my_indicator;
```
Use in config:
```json
{ "my_ind": { "type": "my_indicator" } }
```

### Adding a New Strategy
Create `crates/strategy/src/strategies/my_strategy.rs`:
```rust
use crate::Strategy;
use ts_core::{Bar, Signal, IndicatorSet, Params};

pub struct MyStrategy;

impl Strategy for MyStrategy {
    fn generate_signals(&self, bars: &[Bar], cols: &IndicatorSet, params: &Params) -> Vec<Option<Signal>> {
        let mut signals = vec![None; bars.len()];
        // Full-series calculation here (stateful strategies can build signals across all bars)
        signals
    }
}
```
*Signals are produced via `Strategy::generate_signals`, which performs a full-series calculation each call.*

Register in `crates/strategy/src/registry.rs`  
Export from `crates/strategy/src/strategies/mod.rs`  
Use in config: `"strategy": "MyStrategy"`

### Building from Source
```bash
# Debug build (faster compile, slower runtime)
cargo build

# Release build (optimization: LTO, single codegen unit)
cargo build --release

# Run tests
cargo test

# Check for issues without building
cargo check

# Format code
cargo fmt

# Lint code
cargo clippy
```

---

<a id="troubleshooting"></a>
## ⚠️ Troubleshooting

| Issue | Cause | Solution |
|---|---|---|
| Binance 401 Unauthorized | Wrong API keys | Verify `.env`, check IP whitelist, sync clock |
| MT5 DLL not found | Wrong path on Windows | Set `MT5_DLL_PATH` in `.env` |
| Connection timeout | Network issue | Check internet, firewall, proxy settings |
| Chrono compile error | Wrong version | Workspace pins `chrono < 0.4.39` — do not override |
| No data files created | Missing permissions | Ensure `data/` directory is writable |
| Live trading not executing | Insufficient margin | Check account balance, symbol info |
| Logs not appearing | RUST_LOG not set | `export RUST_LOG=info` before running |

### Debug Logging
```bash
# Enable debug logs
RUST_LOG=debug ./target/release/trading-system backtest --config configs/backtest/ema_cross.json

# Trace level (very verbose)
RUST_LOG=trace ./target/release/trading-system live --config configs/live/paper.json

# JSON structured logs
./target/release/trading-system --json-log backtest --config configs/backtest/ema_cross.json
```

---

<a id="contributing"></a>
## 🤝 Contributing

Contributions are welcome. See [CONTRIBUTING.md](CONTRIBUTING.md) for the
development workflow, code standards, and guidelines for adding indicators,
strategies, and broker adapters.

---

<a id="license"></a>
## 📄 License

Licensed under the MIT License. See [LICENSE](LICENSE) file for details.

You are free to:
✅ Use commercially  
✅ Modify and distribute  
✅ Use privately  
✅ Include patent protection  

*Without: Liability or warranty.*

---

## 👥 Authors

**Barış Özdemir** ([@0xbarss](https://github.com/0xbarss))  
System architecture

---

<a id="disclaimer"></a>
## ⚠️ Disclaimer

**THIS SOFTWARE IS PROVIDED FOR EDUCATIONAL AND RESEARCH PURPOSES ONLY.**

### Critical Risk Warnings
🚨 **Trading involves significant risk of loss** — Past performance ≠ future results  
🚨 **NOT financial advice** — Authors are not registered financial advisors  
🚨 **Backtest ≠ Live performance** — Slippage, liquidity, and execution vary by broker  
🚨 **Prop firm rules** — Always verify strategy compliance with firm terms  

### Responsible Use Guidelines
**✅ DO:**
- Test strategies extensively in demo/paper mode first
- Start with minimal capital you can afford to lose
- Use proper risk management (1–2% per trade maximum)
- Monitor live trades initially before leaving unattended
- Keep a trading journal for continuous improvement

**❌ DON'T:**
- Risk money you cannot afford to lose
- Use aggressive leverage (>10x for beginners)
- Trade during major news events without protection
- Expect consistent profits (professionals have losing periods)
- Blindly follow backtest results without forward testing

### Final Warning
**YOU ARE SOLELY RESPONSIBLE FOR YOUR TRADING DECISIONS AND ANY FINANCIAL LOSSES INCURRED.**  
This software is not financial advice. Consult a qualified financial advisor before trading real money.

*Happy Trading! 🚀📈*  
*Built with ❤️ by algorithmic traders, for algorithmic traders*
