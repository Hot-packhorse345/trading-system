# FTMO Symbol Reference Guide

This document is the authoritative reference for all FTMO-tradable symbols, their commission structures, MT5 ticker names, and the correct `commission_percent` / `commission_per_lot` values to use in backtest and live configs. Always consult this file before setting cost parameters in any config.

> **Source:** [ftmo.com/en/symbols/](https://ftmo.com/en/symbols/) · Last verified: 2026-06-28  
> **Always check [Trading Updates](https://ftmo.com/en/trading-updates/) for the latest changes before trading.**  
> **Platform:** MT4, MT5, cTrader, DXtrade · **Server time:** GMT+2 (+DST)  
> **Data provider string for all FTMO configs:** `"data_provider": "mt5"`

---

## 1. Commission Model by Asset Class

This table maps directly to the config fields `commission_per_lot` and `commission_percent`. Use exactly one of the two — never both.

| Asset Class | Commission Type | Value | Config Field | Notes |
|-------------|----------------|-------|-------------|-------|
| **Forex** | Per lot (round trip) | $5.00 | `"commission_per_lot": 5.0` | Applied per standard lot (100,000 units). Applies to all FX pairs. |
| **Indices** | Zero commission | $0.00 | `"commission_percent": 0.0` | Confirmed zero commission on all `.cash` index CFDs (effective Aug 2025). |
| **Crypto** | Percent of notional | 0.0325% per side | `"commission_percent": 0.000325` | Applied per side; 0.065% round trip. Effective 28 Jul 2025. |
| **Commodities** | Percent of notional | 0.0007% | `"commission_percent": 0.000007` | Applies to metals, energies, and agriculture CFDs. |

### Config Snippet Examples

```json
// Forex
{ "commission_per_lot": 5.0, "commission_percent": 0.0 }

// Indices
{ "commission_percent": 0.0, "commission_per_lot": 0.0 }

// Crypto
{ "commission_percent": 0.000325, "commission_per_lot": 0.0 }

// Commodities
{ "commission_percent": 0.000007, "commission_per_lot": 0.0 }
```

---

## 2. Forex Pairs

**Commission:** `commission_per_lot: 5.0` (per standard lot, round trip)  
**Contract size:** 100,000 units of base currency  
**Max order size:** 50 lots per order (MT4/MT5)  
**Sessions:** Mon 00:00 – Fri 23:00 server time (GMT+2 +DST)

### 2.1 Major Pairs

| MT5 Symbol | Description | Typical Use |
|------------|-------------|-------------|
| `EURUSD` | Euro / US Dollar | Most liquid pair; 25%+ of global FX volume |
| `USDJPY` | US Dollar / Japanese Yen | Second most traded; rate-sensitive |
| `GBPUSD` | British Pound / US Dollar | High volatility, trending |
| `USDCHF` | US Dollar / Swiss Franc | Safe-haven pair |
| `AUDUSD` | Australian Dollar / US Dollar | Commodity-correlated |
| `USDCAD` | US Dollar / Canadian Dollar | Oil-correlated |
| `NZDUSD` | New Zealand Dollar / US Dollar | Risk-on proxy |

### 2.2 Minor / Cross Pairs

| MT5 Symbol | Description |
|------------|-------------|
| `EURGBP` | Euro / British Pound |
| `EURJPY` | Euro / Japanese Yen |
| `EURCHF` | Euro / Swiss Franc |
| `EURAUD` | Euro / Australian Dollar |
| `EURCAD` | Euro / Canadian Dollar |
| `EURNZD` | Euro / New Zealand Dollar |
| `GBPJPY` | British Pound / Japanese Yen — high volatility |
| `GBPCHF` | British Pound / Swiss Franc |
| `GBPAUD` | British Pound / Australian Dollar |
| `GBPCAD` | British Pound / Canadian Dollar |
| `GBPNZD` | British Pound / New Zealand Dollar |
| `AUDJPY` | Australian Dollar / Japanese Yen |
| `AUDCAD` | Australian Dollar / Canadian Dollar |
| `AUDCHF` | Australian Dollar / Swiss Franc |
| `AUDNZD` | Australian Dollar / New Zealand Dollar |
| `CADJPY` | Canadian Dollar / Japanese Yen |
| `CADCHF` | Canadian Dollar / Swiss Franc |
| `CHFJPY` | Swiss Franc / Japanese Yen |
| `NZDJPY` | New Zealand Dollar / Japanese Yen |
| `NZDCAD` | New Zealand Dollar / Canadian Dollar |
| `NZDCHF` | New Zealand Dollar / Swiss Franc |

### 2.3 Exotic Pairs

Available but with wider spreads and lower liquidity. Suitable for longer-term positions only.

| MT5 Symbol | Description |
|------------|-------------|
| `USDSEK` | US Dollar / Swedish Krona |
| `USDNOK` | US Dollar / Norwegian Krone |
| `USDDKK` | US Dollar / Danish Krone |
| `USDPLN` | US Dollar / Polish Zloty |
| `USDHUF` | US Dollar / Hungarian Forint |
| `USDCZK` | US Dollar / Czech Koruna |
| `USDSGD` | US Dollar / Singapore Dollar |
| `USDHKD` | US Dollar / Hong Kong Dollar |
| `USDTRY` | US Dollar / Turkish Lira |
| `USDZAR` | US Dollar / South African Rand |
| `USDMXN` | US Dollar / Mexican Peso |
| `EURSEK` | Euro / Swedish Krona |
| `EURNOK` | Euro / Norwegian Krone |
| `EURPLN` | Euro / Polish Zloty |
| `EURHUF` | Euro / Hungarian Forint |
| `EURTRY` | Euro / Turkish Lira |
| `GBPSGD` | British Pound / Singapore Dollar |
| `GBPNOK` | British Pound / Norwegian Krone |
| `GBPSEK` | British Pound / Swedish Krone |

> **Note:** Exotic pair availability may vary by platform. Always verify the symbol exists in MT5 Market Watch before configuring.

---

## 3. Indices (Stock Index CFDs)

**Commission:** Zero (`commission_percent: 0.0`) — confirmed effective August 2025  
**Contract size:** 1 contract per lot (standardised across all `.cash` symbols since Oct 2022)  
**Suffix convention:** All index CFDs use the `.cash` suffix on MT4/MT5 (e.g. `US100.cash`)

| MT5 Symbol | Index Name | Exchange / Region | Trading Hours (Server GMT+2) |
|------------|-----------|-------------------|------------------------------|
| `US100.cash` | Nasdaq 100 | USA | Mon–Fri 01:00–23:00 |
| `US30.cash` | Dow Jones Industrial Average | USA | Mon–Fri 01:00–23:00 |
| `US500.cash` | S&P 500 | USA | Mon–Fri 01:00–23:00 |
| `US2000.cash` | Russell 2000 | USA | Mon–Fri 01:00–23:00 |
| `GER40.cash` | DAX 40 | Germany | Mon–Fri 02:00–23:00 |
| `UK100.cash` | FTSE 100 | United Kingdom | Mon–Fri 02:00–22:30 |
| `EU50.cash` | Euro Stoxx 50 | Europe | Mon–Fri 02:00–23:00 |
| `FRA40.cash` | CAC 40 | France | Mon–Fri 02:00–23:00 |
| `SPN35.cash` | IBEX 35 | Spain | Mon–Fri 09:00–19:00 |
| `JP225.cash` | Nikkei 225 | Japan | Mon–Fri 02:00–11:30, 12:30–23:00 |
| `AUS200.cash` | ASX 200 | Australia | Mon–Fri 02:55–09:30, 10:15–23:55 |
| `HK50.cash` | Hang Seng 50 | Hong Kong | Mon–Fri 04:15–07:00, 08:00–11:30, 13:00–20:00 |
| `N25.cash` | AEX 25 | Netherlands | Mon–Fri 09:00–17:30 |
| `DXY.cash` | US Dollar Index | USA | Mon–Fri 01:00–23:00 |

> **Holiday closures and early closes happen frequently** — always check [Trading Updates](https://ftmo.com/en/trading-updates/) before the session.  
> **DXY.cash commission:** 0.001% of notional per round trip (special case — not zero).

---

## 4. Commodities

**Commission:** `commission_percent: 0.000007` (0.0007% of notional)

### 4.1 Metals

| MT5 Symbol | Description | Sessions (Server GMT+2) |
|------------|-------------|------------------------|
| `XAUUSD` | Gold / US Dollar — most popular FTMO instrument (24%+ of trades) | Mon–Fri 01:00–23:00 |
| `XAGUSD` | Silver / US Dollar | Mon–Fri 01:00–23:00 |
| `XPDUSD` | Palladium / US Dollar | Mon–Fri 01:00–23:00 |
| `XPTUSD` | Platinum / US Dollar | Mon–Fri 01:00–23:00 |
| `XAUEUR` | Gold / Euro | Mon–Fri 01:00–23:00 |
| `XAGEUR` | Silver / Euro | Mon–Fri 01:00–23:00 |
| `XAUGBP` | Gold / British Pound | Mon–Fri 01:00–23:00 |

### 4.2 Energies

| MT5 Symbol | Description | Sessions (Server GMT+2) |
|------------|-------------|------------------------|
| `USOIL.cash` | WTI Crude Oil CFD | Mon–Fri 01:00–23:00 |
| `UKOIL.cash` | Brent Crude Oil CFD | Mon–Fri 03:00–23:00 |
| `NATGAS.cash` | Natural Gas CFD | Mon–Fri 01:00–23:00 |
| `HEATOIL.c` | Heating Oil CFD | Mon–Fri 01:00–23:00 |

### 4.3 Agriculture

| MT5 Symbol | Description | Sessions (Server GMT+2) |
|------------|-------------|------------------------|
| `COCOA.c` | Cocoa CFD | Mon–Fri 11:45–20:30 |
| `COFFEE.c` | Coffee CFD | Mon–Fri 11:30–20:30 |
| `SUGAR.c` | Sugar CFD | Mon–Fri 10:30–20:00 |
| `COTTON.c` | Cotton CFD | Mon–Fri 15:00–21:20 |

> **Agriculture symbols** have limited sessions and late opens; verify hours before configuring.

---

## 5. Cryptocurrencies

**Commission:** `commission_percent: 0.000325` (0.0325% per side; 0.065% round trip)  
**Model:** Spot-based CFDs — no expiry, no rollover cost  
**Weekend trading:** Available but hours may vary due to maintenance. Check [Trading Updates](https://ftmo.com/en/trading-updates/).  
**Effective:** 28 July 2025 (new spread model + commission structure)
**Swap:** None — FTMO's crypto CFDs are spot-based with no rollover cost, so leave `"swap"` unset (all-zero default) for crypto symbols.

### 5.1 Major Crypto

| MT5 Symbol | Description | New Contract Size | Max Volume/Trade |
|------------|-------------|-------------------|-----------------|
| `BTCUSD` | Bitcoin / US Dollar | 1 | 5 lots |
| `ETHUSD` | Ethereum / US Dollar | 10 | 5 lots |
| `XRPUSD` | Ripple / US Dollar | 10,000 | 5 lots |
| `LTCUSD` | Litecoin / US Dollar | 100 | 5 lots |
| `ADAUSD` | Cardano / US Dollar | 100,000 | 5 lots |
| `DOTUSD` | Polkadot / US Dollar | 10,000 | 5 lots |
| `DOGEUSD` | Dogecoin / US Dollar | 100,000 | 1 lot |
| `XMRUSD` | Monero / US Dollar | 100 | 1 lot |
| `NEOUSD` | Neo / US Dollar | 1,000 | 1 lot |
| `DASHUSD` | Dash / US Dollar | 1,000 | 5 lots |

### 5.2 Altcoins (Added July 2025 — 22 new instruments)

| MT5 Symbol | Description |
|------------|-------------|
| `SOLUSD` | Solana / US Dollar |
| `BNBUSD` | Binance Coin / US Dollar |
| `XLMUSD` | Stellar / US Dollar |
| `AAVEUSD` | AAVE / US Dollar |
| `LINKUSD` | Chainlink / US Dollar |
| `AVAXUSD` | Avalanche / US Dollar |
| `MATICUSD` | Polygon / US Dollar |
| `UNIUSD` | Uniswap / US Dollar |
| `ATOMUSD` | Cosmos / US Dollar |
| `ALGOUSD` | Algorand / US Dollar |
| `FILUSD` | Filecoin / US Dollar |
| `TRXUSD` | TRON / US Dollar |
| `ETCUSD` | Ethereum Classic / US Dollar |
| `ICPUSD` | Internet Computer / US Dollar |
| `NEARUSD` | NEAR Protocol / US Dollar |
| `APTUSD` | Aptos / US Dollar |
| `ARBUSD` | Arbitrum / US Dollar |
| `OPUSD` | Optimism / US Dollar |
| `SUIUSD` | Sui / US Dollar |
| `INJUSD` | Injective / US Dollar |
| `GRTUSD` | The Graph / US Dollar |
| `COMPUSD` | Compound / US Dollar |

> Total crypto offering as of July 2025: **32 instruments**. New symbols may be added; verify full list at [ftmo.com/en/symbols/](https://ftmo.com/en/symbols/).

---

## 6. Config Quick-Reference by Symbol

Use this table to copy-paste the correct cost fields directly into any config.

| Symbol | Asset Class | Config Fields |
|--------|-------------|--------------|
| `EURUSD` | Forex | `"commission_per_lot": 5.0` |
| `GBPUSD` | Forex | `"commission_per_lot": 5.0` |
| `USDJPY` | Forex | `"commission_per_lot": 5.0` |
| `GBPJPY` | Forex | `"commission_per_lot": 5.0` |
| `AUDUSD` | Forex | `"commission_per_lot": 5.0` |
| `USDCAD` | Forex | `"commission_per_lot": 5.0` |
| `USDCHF` | Forex | `"commission_per_lot": 5.0` |
| `NZDUSD` | Forex | `"commission_per_lot": 5.0` |
| *(all other FX pairs)* | Forex | `"commission_per_lot": 5.0` |
| `US100.cash` | Index | `"commission_percent": 0.0` |
| `US30.cash` | Index | `"commission_percent": 0.0` |
| `US500.cash` | Index | `"commission_percent": 0.0` |
| `GER40.cash` | Index | `"commission_percent": 0.0` |
| `UK100.cash` | Index | `"commission_percent": 0.0` |
| `JP225.cash` | Index | `"commission_percent": 0.0` |
| `AUS200.cash` | Index | `"commission_percent": 0.0` |
| `HK50.cash` | Index | `"commission_percent": 0.0` |
| *(all other `.cash` indices)* | Index | `"commission_percent": 0.0` |
| `DXY.cash` | Dollar Index | `"commission_percent": 0.00001` *(0.001% round trip — special case)* |
| `XAUUSD` | Commodity | `"commission_percent": 0.000007` |
| `XAGUSD` | Commodity | `"commission_percent": 0.000007` |
| `USOIL.cash` | Commodity | `"commission_percent": 0.000007` |
| `UKOIL.cash` | Commodity | `"commission_percent": 0.000007` |
| `NATGAS.cash` | Commodity | `"commission_percent": 0.000007` |
| *(all other commodities)* | Commodity | `"commission_percent": 0.000007` |
| `BTCUSD` | Crypto | `"commission_percent": 0.000325` |
| `ETHUSD` | Crypto | `"commission_percent": 0.000325` |
| `XRPUSD` | Crypto | `"commission_percent": 0.000325` |
| *(all other crypto)* | Crypto | `"commission_percent": 0.000325` |

---

## 7. MT5 Data Availability Limits

From CONFIG_DESIGN_GUIDE §6 — limits that apply when `"data_provider": "mt5"`:

| Timeframe | Calendar Limit | Notes |
|-----------|---------------|-------|
| 1m | ~22 months | Hard MT5 chart buffer limit |
| 5m–30m | ~6–7 years | Set `backtest_range` ≤ `"6_years"` |
| 1h–4h | ~6–7 years | Set `backtest_range` ≤ `"6_years"` |
| 1d+ | ~6–7 years | Calendar limit before bar cap |

Never set `backtest_range` beyond these limits for `"data_provider": "mt5"` — the broker silently returns truncated data with no error.

---

## 8. Important Trading Rules & Compliance Notes

- **Max order size (Forex, MT4/MT5):** 50 lots per individual order. Place multiple orders for larger positions.
- **Overnight swaps:** Variable and updated regularly. Check symbol specifications in MT5 before holding overnight. FTMO is not responsible for swap changes affecting results. See §10 for how to map these into the `"swap"` config block.
- **Holiday and early close events:** Occur several times per month. Always check [Trading Updates](https://ftmo.com/en/trading-updates/) before Friday close and around major holidays.
- **Crypto weekend maintenance:** Crypto trading hours may vary on weekends due to scheduled maintenance. Verify before holding positions into the weekend.
- **Forbidden practices:** Latency arbitrage, quote manipulation, and coordinated account trading are prohibited and void results.
- **News trading:** Allowed on standard accounts. High-impact news events may cause price spikes, widened spreads, and slippage
- **Swing accounts:** Available with no daily drawdown limit but tighter weekly limits. Symbol availability same as standard accounts.

---

## 9. Recommended Backtest Ranges by Symbol Type

These are recommended minimum and preferred ranges when using `"data_provider": "mt5"`, derived from the MT5 data limits and the asset class volatility profiles in EDGE_DISCOVERY.md.

| Asset Class | Minimum Range | Recommended | MT5 Cap |
|-------------|--------------|-------------|---------|
| Forex (4h) | 3_years | 5_years | 6_years |
| Indices (4h) | 3_years | 5_years | 6_years |
| Gold / XAUUSD (4h) | 3_years | 5_years | 6_years |
| Crypto (1h) | 2_years | 3_years | 6_years |
| Commodities / Energy (4h) | 2_years | 4_years | 6_years |

> For Forex and Gold on timeframes below 1h, reduce ranges proportionally. For 1m strategies, the hard cap is ~22 months regardless of asset class.

---

## 10. Swap / Rollover Costs (Overnight Financing)

Unlike commission, FTMO does **not** publish a static per-symbol swap table.
Swap points are adjusted regularly based on interest rate differentials and
dividend adjustments, and FTMO explicitly disclaims responsibility for
results affected by swap changes. **Always read the live value from the MT5
terminal before setting cost parameters:**

> MT5 → Market Watch → right-click symbol → **Specification** → `Swap Long` / `Swap Short` (in points).

That maps directly onto the new `"swap"` config block (points mode — the
most direct MT5 mapping, converted to $ automatically via the symbol's
`tick_value`):

```json
"swap": {
  "long_points": -6.25,
  "short_points": 3.00,
  "long_per_lot": 0.0,
  "short_per_lot": 0.0,
  "rollover_mode": "triple_wednesday",
  "wednesday_multiplier": 3.0,
  "rollover_hour_utc": 22
}
```

*(`-6.25` / `3.00` above are FTMO's own illustrative EURUSD figures from
["What is a swap and for whom is it important?"](https://ftmo.com/en/blog/what-is-a-swap-and-for-whom-is-it-important/)
— an educational example, **not** a live rate. Replace with whatever your
MT5 terminal shows for the symbol you're configuring.)*

**Applies to:** Forex, Metals, Indices, Commodities — anything held as a
leveraged CFD.
**Does not apply to:** Crypto CFDs (see §5 — spot-based, no rollover).

**Weekend handling:** FTMO's own blog confirms the standard triple-swap
Wednesday→Thursday convention for FX (most instruments settle T+2, so a
Wednesday-night rollover has to cover Thursday, Saturday, *and* Sunday) —
their example shows a EURUSD long position going from 6.25 points/night to
18.75 points on the Wednesday rollover, exactly 3×. Set
`"rollover_mode": "triple_wednesday"` for the most realistic modeling of FX
and metals. FTMO's docs don't specify a per-asset-class triple day, and some
MT5 brokers apply the weekly triple charge to indices/commodities on Friday
instead of Wednesday — verify in the MT5 contract specification rather than
assuming Wednesday applies uniformly. If you can't confirm,
`"rollover_mode": "every_night"` (the system default) is a conservative
fallback that never under-counts the weekend cost.

**Rollover time caveat:** the standard forex rollover instant is 17:00 New
York time — 21:00 UTC during US daylight saving (mid-March to early
November) and 22:00 UTC the rest of the year. `rollover_hour_utc` is a fixed
UTC hour and does not auto-adjust for the DST switch, so trades near the
boundary during DST months will be off by up to an hour against the real
MT5 rollover. This rarely changes which *day* gets charged, but if exact
timing across a DST transition matters for your test, use
`rollover_hour_utc: 21` for spring/summer date ranges and `22` for
autumn/winter ones.

> **Reminder:** leaving `"swap"` unset (the default) silently assumes free
> overnight holding. For any strategy that holds forex/metals/indices/
> commodities positions overnight — most swing and position strategies —
> set real values from MT5 before trusting backtest results, exactly as you
> would for `commission_per_lot`.
