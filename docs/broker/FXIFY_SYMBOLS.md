# FXIFY Symbol Reference Guide

This document is the authoritative reference for all FXIFY-tradable symbols, their commission structures, platform tickers, and the correct `commission_percent` / `commission_per_lot` values to use in backtest and live configs. Always consult this file before setting cost parameters in any config.

> **Source:** [fxify.com/faqs/trading/](https://fxify.com/faqs/trading/) · Last verified: 2026-06-28
> **Broker partner:** FXPIG (regulated brokerage infrastructure)
> **Platforms:** MT4, MT5, DXTrade, TradingView · **Server time:** GMT+3 (DST-adjusted)
> **Data provider string for all FXIFY configs:** `"data_provider": "mt5"`

---

## 1. Account Types & Pricing Models

FXIFY offers two pricing models, selected at account checkout. This is the single most important decision affecting commission config values.

| Account Type | Spread Model | Commission | Best For |
|-------------|-------------|-----------|---------|
| **RAW** | Raw broker spread (from 0.0 on majors) | $6 per lot (FX, Metals, Indices, Commodities) | Scalpers, high-frequency traders |
| **All-In** | Wider spread, cost built in | $0 (FX, Metals, Indices, Commodities) | Intraday and swing traders |
| **Stocks (either type)** | — | 0.35% round trip | Equities CFDs only |
| **Crypto accounts** | RAW spread | $0 commission | Crypto-only accounts |

> **Critical:** FXIFY has **separate account types for Forex and Crypto**. FOREX accounts cannot trade crypto. CRYPTO accounts are exclusively for cryptocurrencies. Select the correct account type at checkout.

---

## 2. Commission Model by Asset Class

### 2.1 RAW Account (commission applies)

| Asset Class | Commission Type | Value | Config Field |
|-------------|----------------|-------|-------------|
| **Forex** | Per lot (round trip) | $6.00 | `"commission_per_lot": 6.0` |
| **Metals** | Per lot (round trip) | $6.00 | `"commission_per_lot": 6.0` |
| **Indices** | Per lot (round trip) | $6.00 | `"commission_per_lot": 6.0` |
| **Commodities** | Per lot (round trip) | $6.00 | `"commission_per_lot": 6.0` |
| **Stocks** | Percent of notional (round trip) | 0.35% | `"commission_percent": 0.0035` |
| **Crypto** | Zero commission | $0.00 | `"commission_percent": 0.0` |

### 2.2 All-In Account (commission-free)

| Asset Class | Commission Type | Value | Config Field |
|-------------|----------------|-------|-------------|
| **Forex** | Zero (built into spread) | $0.00 | `"commission_percent": 0.0` |
| **Metals** | Zero (built into spread) | $0.00 | `"commission_percent": 0.0` |
| **Indices** | Zero (built into spread) | $0.00 | `"commission_percent": 0.0` |
| **Commodities** | Zero (built into spread) | $0.00 | `"commission_percent": 0.0` |
| **Stocks** | Percent of notional (round trip) | 0.35% | `"commission_percent": 0.0035` |
| **Crypto** | Zero commission | $0.00 | `"commission_percent": 0.0` |

> **Stocks are never commission-free** regardless of account type. The 0.35% round-trip fee always applies.

### 2.3 Config Snippet Examples

```json
// RAW account — Forex / Metals / Indices / Commodities
{ "commission_per_lot": 6.0, "commission_percent": 0.0 }

// All-In account — Forex / Metals / Indices / Commodities
{ "commission_percent": 0.0 }

// Stocks (either account type)
{ "commission_percent": 0.0035, "commission_per_lot": 0.0 }

// Crypto (either account type)
{ "commission_percent": 0.0 }
```

---

## 3. Forex Pairs

**Contract size:** 100,000 units of base currency
**Ticker suffix:** `.r` on DXTrade/FXIFY platform (e.g. `EURUSD.r`). On MT4/MT5, use the base symbol without suffix.
**Sessions:** Mon 00:00 – Fri 23:00 server time
**Leverage:** 30:1 standard; 50:1 available as add-on at checkout

### 3.1 Major Pairs

| Platform Ticker | MT4/MT5 Symbol | Description | Digits |
|-----------------|---------------|-------------|--------|
| `EURUSD.r` | `EURUSD` | Euro vs US Dollar | 5 |
| `GBPUSD.r` | `GBPUSD` | British Pound vs US Dollar | 5 |
| `USDJPY.r` | `USDJPY` | US Dollar vs Japanese Yen | 3 |
| `USDCHF.r` | `USDCHF` | US Dollar vs Swiss Franc | 5 |
| `AUDUSD.r` | `AUDUSD` | Australian Dollar vs US Dollar | 5 |
| `USDCAD.r` | `USDCAD` | US Dollar vs Canadian Dollar | 5 |
| `NZDUSD.r` | `NZDUSD` | New Zealand Dollar vs US Dollar | 5 |

### 3.2 Minor / Cross Pairs

| Platform Ticker | MT4/MT5 Symbol | Description | Digits |
|-----------------|---------------|-------------|--------|
| `AUDCAD.r` | `AUDCAD` | Australian Dollar vs Canadian Dollar | 5 |
| `AUDCHF.r` | `AUDCHF` | Australian Dollar vs Swiss Franc | 5 |
| `AUDJPY.r` | `AUDJPY` | Australian Dollar vs Japanese Yen | 3 |
| `AUDNZD.r` | `AUDNZD` | Australian Dollar vs New Zealand Dollar | 5 |
| `CADCHF.r` | `CADCHF` | Canadian Dollar vs Swiss Franc | 5 |
| `CADJPY.r` | `CADJPY` | Canadian Dollar vs Japanese Yen | 3 |
| `CHFJPY.r` | `CHFJPY` | Swiss Franc vs Japanese Yen | 3 |
| `EURAUD.r` | `EURAUD` | Euro vs Australian Dollar | 5 |
| `EURCAD.r` | `EURCAD` | Euro vs Canadian Dollar | 5 |
| `EURCHF.r` | `EURCHF` | Euro vs Swiss Franc | 5 |
| `EURGBP.r` | `EURGBP` | Euro vs British Pound | 5 |
| `EURJPY.r` | `EURJPY` | Euro vs Japanese Yen | 3 |
| `EURNZD.r` | `EURNZD` | Euro vs New Zealand Dollar | 5 |
| `GBPAUD.r` | `GBPAUD` | British Pound vs Australian Dollar | 5 |
| `GBPCAD.r` | `GBPCAD` | British Pound vs Canadian Dollar | 5 |
| `GBPCHF.r` | `GBPCHF` | British Pound vs Swiss Franc | 5 |
| `GBPJPY.r` | `GBPJPY` | British Pound vs Japanese Yen | 3 |
| `GBPNZD.r` | `GBPNZD` | British Pound vs New Zealand Dollar | 5 |
| `NZDCAD.r` | `NZDCAD` | New Zealand Dollar vs Canadian Dollar | 5 |
| `NZDCHF.r` | `NZDCHF` | New Zealand Dollar vs Swiss Franc | 5 |
| `NZDJPY.r` | `NZDJPY` | New Zealand Dollar vs Japanese Yen | 3 |

### 3.3 Exotic Pairs

| Platform Ticker | MT4/MT5 Symbol | Description | Digits |
|-----------------|---------------|-------------|--------|
| `EURMXN.r` | `EURMXN` | Euro vs Mexican Peso | 5 |
| `EURTRY.r` | `EURTRY` | Euro vs Turkish Lira | 5 |
| `USDCNH.r` | `USDCNH` | US Dollar vs Chinese Renminbi (offshore) | 5 |
| `USDCZK.r` | `USDCZK` | US Dollar vs Czech Koruna | 4 |
| `USDDKK.r` | `USDDKK` | US Dollar vs Danish Krone | 5 |
| `USDHKD.r` | `USDHKD` | US Dollar vs Hong Kong Dollar | 5 |
| `USDHUF.r` | `USDHUF` | US Dollar vs Hungarian Forint | 3 |
| `USDILS.r` | `USDILS` | US Dollar vs Israeli Shekel | 5 |
| `USDMXN.r` | `USDMXN` | US Dollar vs Mexican Peso | 5 |
| `USDNOK.r` | `USDNOK` | US Dollar vs Norwegian Krone | 5 |
| `USDPLN.r` | `USDPLN` | US Dollar vs Polish Zloty | 5 |
| `USDSEK.r` | `USDSEK` | US Dollar vs Swedish Krona | 5 |
| `USDSGD.r` | `USDSGD` | US Dollar vs Singapore Dollar | 5 |
| `USDTRY.r` | `USDTRY` | US Dollar vs Turkish Lira | 5 |
| `USDZAR.r` | `USDZAR` | US Dollar vs South African Rand | 5 |

> Exotic pairs carry wider spreads and lower liquidity. Suitable for longer-term positions only. Avoid on timeframes below 1h.

---

## 4. Metals

**Contract sizes vary by instrument** — see table below.
**Commission (RAW):** $6 per lot · **Commission (All-In):** $0
**Sessions:** Mon 01:00 – Fri 23:00 server time

| Platform Ticker | MT4/MT5 Symbol | Description | Contract Size | Digits | Quote CCY |
|-----------------|---------------|-------------|--------------|--------|-----------|
| `XAUUSD.r` | `XAUUSD` | Gold vs US Dollar | 100 | 2 | USD |
| `XAGUSD.r` | `XAGUSD` | Silver vs US Dollar | 5,000 | 3 | USD |
| `XAUEUR.r` | `XAUEUR` | Gold vs Euro | 100 | 2 | EUR |
| `XPDUSD.r` | `XPDUSD` | Palladium vs US Dollar | 100 | 2 | USD |
| `XPTUSD.r` | `XPTUSD` | Platinum vs US Dollar | 100 | 2 | USD |

> **XAUUSD** is the highest-volume instrument on most prop platforms. The contract size of 100 oz (not 1000 as on some brokers) is important for position sizing calculations.

---

## 5. Indices (Stock Index CFDs)

**Contract size:** 10 units per lot for most indices (exceptions noted)
**Commission (RAW):** $6 per lot · **Commission (All-In):** $0
**Sessions:** Vary by index — see table

| Platform Ticker | MT4/MT5 Symbol | Description | Contract Size | Quote CCY | Typical Session (Server GMT+3) |
|-----------------|---------------|-------------|--------------|-----------|-------------------------------|
| `USTEC.r` | `USTEC` | Nasdaq 100 | 10 | USD | Mon–Fri 01:00–23:00 |
| `DJ30.r` | `DJ30` | Dow Jones Industrial 30 | 10 | USD | Mon–Fri 01:00–23:00 |
| `US500.r` | `US500` | S&P 500 | 100 | USD | Mon–Fri 01:00–23:00 |
| `RUS2000.r` | `RUS2000` | Russell 2000 | 100 | USD | Mon–Fri 01:00–23:00 |
| `US400.r` | `US400` | S&P Midcap 400 | 100 | USD | Mon–Fri 01:00–23:00 |
| `DE30.r` | `DE30` | DAX 40 (Germany) | 10 | EUR | Mon–Fri 02:00–23:00 |
| `UK100.r` | `UK100` | FTSE 100 (UK) | 10 | GBP | Mon–Fri 02:00–22:30 |
| `F40.r` | `F40` | CAC 40 (France) | 10 | EUR | Mon–Fri 02:00–23:00 |
| `ES35.r` | `ES35` | IBEX 35 (Spain) | 10 | EUR | Mon–Fri 09:00–19:00 |
| `STOXX50.r` | `STOXX50` | Euro Stoxx 50 | 10 | EUR | Mon–Fri 02:00–23:00 |
| `JPN225.r` | `JPN225` | Nikkei 225 (Japan) | 1,000 | JPY | Mon–Fri 02:00–11:30, 12:30–23:00 |
| `AUS200.r` | `AUS200` | ASX 200 (Australia) | 10 | AUD | Mon–Fri 02:55–09:30, 10:15–23:55 |
| `HKG50.r` | `HKG50` | Hang Seng 50 (Hong Kong) | 100 | HKD | Mon–Fri 04:15–07:00, 08:00–20:00 |
| `VIX.r` | `VIX` | CBOE Volatility Index | 1,000 | USD | Mon–Fri 01:00–23:00 |

> **Ticker naming differs from FTMO**: FXIFY uses `USTEC` (not `US100.cash`), `DJ30` (not `US30.cash`), `DE30` (not `GER40.cash`), `JPN225` (not `JP225.cash`). Always use FXIFY tickers when building FXIFY configs.
> **VIX** is a volatility index, not a directional instrument — unsuitable for trend-following strategies.

---

## 6. Commodities (Energies)

**Contract size:** 1,000 units per lot
**Commission (RAW):** $6 per lot · **Commission (All-In):** $0
**Sessions:** Mon 01:00 – Fri 23:00 server time

| Platform Ticker | MT4/MT5 Symbol | Description | Contract Size | Digits |
|-----------------|---------------|-------------|--------------|--------|
| `USOIL.r` | `USOIL` | WTI Crude Oil | 1,000 | 3 |
| `UKOIL.r` | `UKOIL` | Brent Crude Oil | 1,000 | 3 |
| `NGAS.r` | `NGAS` | Natural Gas Spot | 1,000 | 3 |

> FXIFY does not offer agriculture or precious metals futures as separate commodity instruments — those are covered under the Metals category (§4).

---

## 7. Stock CFDs

**Commission:** 0.35% round trip — applies on **both RAW and All-In** accounts
**Config field:** `"commission_percent": 0.0035`
**Sessions:** Exchange-specific; US stocks trade Mon–Fri ~15:30–22:00 server time

| Platform Ticker | Description | Contract Size |
|-----------------|-------------|--------------|
| `AAPLd` | Apple Inc. | 100 |
| `MSFTd` | Microsoft Corporation | 100 |
| `AMZNd` | Amazon.com Inc. | 100 |
| `GOOGd` | Alphabet (Google) | 100 |
| `TSLAd` | Tesla Inc. | 100 |
| `NVDAd` | Nvidia Corporation | 100 |
| `NFLXd` | Netflix | 100 |
| `MRNAd` | Moderna Inc. | 100 |
| `JPMd` | JP Morgan Chase | 100 |
| `BABAd` | Alibaba Group | 100 |
| `ABNBd` | Airbnb Inc. | 100 |
| `COINd` | Coinbase Global | 100 |
| `DISd` | Walt Disney Company | 100 |
| `HDd` | The Home Depot Inc. | 100 |
| `NKEd` | Nike Inc. | 100 |
| `PYPLd` | PayPal | 100 |
| `SBUXd` | Starbucks Corp. | 100 |
| `PTONd` | Peloton Interactive | 100 |
| `AALd` | American Airlines | 1,000 |
| `CSCOd` | Cisco Systems | 1,000 |
| `EBAYd` | eBay Inc. | 1,000 |
| `INTCd` | Intel Corporation | 1,000 |
| `ORCLd` | Oracle Corporation | 1,000 |
| `QCOMd` | Qualcomm Inc. | 1,000 |
| `UBERd` | Uber Technologies | 1,000 |
| `XOMd` | Exxon Mobil | 1,000 |
| `Fd` | Ford Motor Company | 10,000 |
| `WMTd` | Walmart Inc. | 100 |
| `FBd` | Meta Platforms (legacy `FB` ticker) | 100 |

> **Stocks are not suitable for the backtesting system** — data availability on MT5 for individual equities is inconsistent. Use only Forex, Metals, Indices, and Commodities for WFA workflows described in EDGE_DISCOVERY.md.

---

## 8. Cryptocurrency (Crypto Accounts Only)

> **IMPORTANT: Crypto is on a SEPARATE account type.** FOREX accounts cannot trade crypto. You must purchase a dedicated **Crypto Challenge** or **Crypto Instant Funded** account to trade these instruments.

**Commission:** $0 (no per-lot or percent commission on crypto accounts)
**Config field:** `"commission_percent": 0.0`
**Sessions:** 24/7 (subject to maintenance windows)
**Ticker format:** Symbol only (e.g. `BTC`, `ETH`) — no suffix on crypto accounts

### 8.1 Major Crypto

| Symbol | Name |
|--------|------|
| `BTC` | Bitcoin |
| `ETH` | Ethereum |
| `XRP` | Ripple |
| `BNB` | Binance Coin |
| `SOL` | Solana |
| `ADA` | Cardano |
| `DOGE` | Dogecoin |
| `AVAX` | Avalanche |
| `DOT` | Polkadot |
| `LTC` | Litecoin |
| `BCH` | Bitcoin Cash |
| `LINK` | Chainlink |
| `UNI` | Uniswap |
| `ATOM` | Cosmos |
| `NEAR` | NEAR Protocol |

### 8.2 DeFi & Altcoins (Full List — 83 instruments as of Oct 2025)

| Symbol | Name | Symbol | Name |
|--------|------|--------|------|
| `1INCH` | 1inch Network | `MANA` | Decentraland |
| `AAVE` | Aave | `MELANIA` | Melania Meme |
| `ALGO` | Algorand | `MTL` | Metal |
| `ALICE` | My Neighbor Alice | `NEO` | Neo |
| `ANKR` | Ankr | `NKN` | NKN |
| `APE` | ApeCoin | `OGN` | Origin Protocol |
| `AXS` | Axie Infinity | `ONE` | Harmony |
| `BAND` | Band Protocol | `QTUM` | Qtum |
| `BAT` | Basic Attention Token | `RLC` | iExec RLC |
| `BEL` | Bella Protocol | `RSR` | Reserve Rights |
| `CELR` | Celer Network | `RUNE` | THORChain |
| `CHR` | Chromia | `RVN` | Ravencoin |
| `CHZ` | Chiliz | `SAND` | The Sandbox |
| `COMP` | Compound | `SFP` | SafePal |
| `COTI` | COTI | `SHIB` | Shiba Inu |
| `CRV` | Curve DAO | `SKL` | SKALE Network |
| `DASH` | Dash | `SNX` | Synthetix |
| `DENT` | Dent | `STORJ` | Storj |
| `EGLD` | MultiversX (Elrond) | `SUSHI` | SushiSwap |
| `ENJ` | Enjin Coin | `SXP` | Solar |
| `ETC` | Ethereum Classic | `THETA` | Theta Network |
| `FIL` | Filecoin | `TRB` | Tellor |
| `FLM` | Flamingo | `TRUMP` | OFFICIAL TRUMP |
| `GRT` | The Graph | `TRX` | TRON |
| `HBAR` | Hedera | `VET` | VeChain |
| `HOT` | Holo | `XLM` | Stellar |
| `ICX` | ICON | `XMR` | Monero |
| `IOST` | IOST | `XTZ` | Tezos |
| `IOTA` | IOTA | `YFI` | yearn.finance |
| `KAVA` | Kava | `ZEC` | Zcash |
| `KNC` | Kyber Network | `ZEN` | Horizen |
| `KSM` | Kusama | `ZIL` | Zilliqa |
| `LINK` | Chainlink | `ZRX` | 0x Protocol |
| `LRC` | Loopring | | |

> Total: **83 crypto instruments** as of October 2025. List is subject to additions based on market cap rankings. Always verify the current list in your crypto account's Market Watch.

---

## 9. Commission Quick-Reference by Symbol

Use this table to copy the correct config fields directly.

| Symbol | Asset Class | RAW Account | All-In Account |
|--------|-------------|-------------|---------------|
| `EURUSD` | Forex | `"commission_per_lot": 6.0` | `"commission_percent": 0.0` |
| `GBPUSD` | Forex | `"commission_per_lot": 6.0` | `"commission_percent": 0.0` |
| `USDJPY` | Forex | `"commission_per_lot": 6.0` | `"commission_percent": 0.0` |
| `GBPJPY` | Forex | `"commission_per_lot": 6.0` | `"commission_percent": 0.0` |
| `AUDUSD` | Forex | `"commission_per_lot": 6.0` | `"commission_percent": 0.0` |
| *(all other FX pairs)* | Forex | `"commission_per_lot": 6.0` | `"commission_percent": 0.0` |
| `XAUUSD` | Metal | `"commission_per_lot": 6.0` | `"commission_percent": 0.0` |
| `XAGUSD` | Metal | `"commission_per_lot": 6.0` | `"commission_percent": 0.0` |
| *(all other metals)* | Metal | `"commission_per_lot": 6.0` | `"commission_percent": 0.0` |
| `USTEC` | Index | `"commission_per_lot": 6.0` | `"commission_percent": 0.0` |
| `DJ30` | Index | `"commission_per_lot": 6.0` | `"commission_percent": 0.0` |
| `US500` | Index | `"commission_per_lot": 6.0` | `"commission_percent": 0.0` |
| `DE30` | Index | `"commission_per_lot": 6.0` | `"commission_percent": 0.0` |
| `UK100` | Index | `"commission_per_lot": 6.0` | `"commission_percent": 0.0` |
| `JPN225` | Index | `"commission_per_lot": 6.0` | `"commission_percent": 0.0` |
| *(all other indices)* | Index | `"commission_per_lot": 6.0` | `"commission_percent": 0.0` |
| `USOIL` | Commodity | `"commission_per_lot": 6.0` | `"commission_percent": 0.0` |
| `UKOIL` | Commodity | `"commission_per_lot": 6.0` | `"commission_percent": 0.0` |
| `NGAS` | Commodity | `"commission_per_lot": 6.0` | `"commission_percent": 0.0` |
| *(any stock, e.g. `AAPLd`)* | Stock CFD | `"commission_percent": 0.0035` | `"commission_percent": 0.0035` |
| `BTC`, `ETH`, etc. | Crypto | `"commission_percent": 0.0` | `"commission_percent": 0.0` |

---

## 10. Ticker Name Differences vs FTMO

This table is critical when porting configs between brokers. Many symbols have different tickers on FXIFY vs FTMO.

| Instrument | FTMO Ticker | FXIFY Ticker | Notes |
|------------|------------|-------------|-------|
| Nasdaq 100 | `US100.cash` | `USTEC` | Different name entirely |
| Dow Jones 30 | `US30.cash` | `DJ30` | Different name |
| S&P 500 | `US500.cash` | `US500` | No `.cash` suffix on FXIFY |
| DAX 40 | `GER40.cash` | `DE30` | Different name and no suffix |
| Nikkei 225 | `JP225.cash` | `JPN225` | Different name |
| Hang Seng | `HK50.cash` | `HKG50` | Different name |
| ASX 200 | `AUS200.cash` | `AUS200` | No `.cash` suffix |
| Brent Oil | `UKOIL.cash` | `UKOIL` | No `.cash` suffix |
| WTI Oil | `USOIL.cash` | `USOIL` | No `.cash` suffix |
| Natural Gas | `NATGAS.cash` | `NGAS` | Different name |
| Russell 2000 | `US2000.cash` | `RUS2000` | Different name |
| Euro Stoxx 50 | `EU50.cash` | `STOXX50` | Different name |
| CAC 40 | `FRA40.cash` | `F40` | Different name |
| IBEX 35 | `SPN35.cash` | `ES35` | Different name |
| Gold | `XAUUSD` | `XAUUSD` | Same ✓ |
| Silver | `XAGUSD` | `XAGUSD` | Same ✓ |
| EURUSD | `EURUSD` | `EURUSD` | Same ✓ (`.r` suffix only on DXTrade) |

> **Never copy index/commodity symbol names directly between FTMO and FXIFY configs.** Always translate using this table.

---

## 11. Leverage by Asset Class

| Asset Class | Standard Leverage | With 50:1 Add-On |
|-------------|------------------|-----------------|
| Forex Majors (e.g. EURUSD, GBPUSD) | 30:1 | 50:1 |
| Forex Exotics | 30:1 | 30:1 (add-on does not apply) |
| Gold (XAUUSD) | 30:1 | 50:1 |
| Silver (XAGUSD) | 10:1 | 10:1 |
| Indices | 30:1 | 30:1 |
| Oil / Commodities | 10:1 | 10:1 |
| Stocks | 5:1 | 5:1 |
| Crypto | 2:1 | 2:1 |

> Lower leverage on non-FX instruments means margin requirements are higher. Factor this into position sizing when configuring the `volume_manager`.

---

## 12. Recommended Backtest Ranges by Symbol Type

These apply when using `"data_provider": "mt5"` against FXPIG's MT5 server. Limits are comparable to standard MT5 data availability constraints.

| Asset Class | Minimum Range | Recommended | MT5 Cap |
|-------------|--------------|-------------|---------|
| Forex (4h) | 3_years | 5_years | 6_years |
| Gold XAUUSD (4h) | 3_years | 5_years | 6_years |
| Indices (4h) | 3_years | 5_years | 6_years |
| Oil / Commodities (4h) | 2_years | 4_years | 6_years |
| Crypto (1h) | 2_years | 3_years | 6_years |

> For timeframes below 1h on any instrument, reduce ranges proportionally. For 1m strategies, the hard cap is approximately 22 months regardless of asset class.

---

## 13. Important Trading Rules & Compliance Notes

- **Account separation:** FOREX and CRYPTO accounts are strictly separate. Purchasing a FOREX account does not grant access to crypto instruments.
- **Stop-loss requirement:** FXIFY requires a stop-loss on every trade. The first and second trades placed without one are automatically closed.
- **News trading:** Allowed without restriction on all programs.
- **EAs and Martingale:** Permitted, provided strategies are unique and developed by the trader. Copy-trading from your own FXIFY accounts is allowed; provide master account statement to FXIFY if copying.
- **Weekend holding:** Allowed on all main programs (Lightning Challenge, One Phase, Two Phase variants, Three Phase). Factor gap risk into stop-loss placement.
- **Overnight swaps:** Applied on **both** RAW and All-In accounts — FXIFY does not offer swap-free (Islamic) accounts. Rates vary by instrument; check the Symbol Specification in your platform. See §14 for config mapping.
- **Total capital cap:** $400,000 across all accounts per trader/strategy simultaneously. Single account cap is $200,000.
- **Drawdown reset on withdrawal:** When a payout is requested, the Max Drawdown locks at the starting balance. Always maintain a buffer; withdrawing all profits leaves no room for any subsequent trades.
- **RAW vs All-In decision is permanent:** Pricing model is chosen at account checkout and cannot be changed after purchase. Backtest with the model you intend to trade.

---

## 14. Swap / Rollover Costs (Overnight Financing)

FXIFY does not offer swap-free accounts and confirms overnight swap fees
apply and vary by instrument (per FXIFY's own
[swap-free-accounts FAQ](https://fxify.com/faqs/all-faqs/swap-free-accounts/)
and independent broker reviews). As with FTMO, there's no static published
per-symbol table — read the live value from your platform before setting
config values:

> MT4/MT5 → Market Watch → right-click symbol → **Specification** → `Swap Long` / `Swap Short`.
> DXTrade → Symbol details panel.

```json
"swap": {
  "long_points": -7.0,
  "short_points": 2.5,
  "long_per_lot": 0.0,
  "short_per_lot": 0.0,
  "rollover_mode": "triple_wednesday",
  "wednesday_multiplier": 3.0,
  "rollover_hour_utc": 22
}
```

*(Values above are placeholders — pull the real `Swap Long`/`Swap Short`
figures for your account and symbol before running any backtest you intend
to trust.)*

**Applies to:** Forex, Metals, Indices, Commodities — applies on **both**
RAW and All-In accounts, independent of which commission model you picked
at checkout (swap is a separate cost layer from commission).
**Crypto:** FXIFY doesn't document an explicit swap exclusion for its
crypto contracts (unlike FTMO — see the FTMO guide §10) — verify in your
crypto account's platform before assuming zero.
**Stocks:** not used for backtesting per §7 (MT5 data availability is
unreliable for individual equities) — no need to configure swap for these.

**Weekend handling & rollover time:** see the general notes in
[FTMO_SYMBOLS.md §10](./FTMO_SYMBOLS.md#10-swap--rollover-costs-overnight-financing) —
the Wednesday-triple / DST-boundary caveats there apply equally to FXIFY's
MT5/MT4 infrastructure (FXPIG).

> **Reminder:** leaving `"swap"` unset silently assumes free overnight
> holding on a broker that explicitly does not offer that. Set real values
> before trusting results for any overnight-holding strategy.
