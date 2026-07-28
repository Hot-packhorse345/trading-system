# Binance Symbol Reference Guide

This document is the authoritative reference for all Binance-tradable cryptocurrency symbols (specifically USD-M Futures), their commission structures, API ticker names, and the correct `commission_percent` values to use in backtest and live configs. Always consult this file before setting cost parameters in any config.

> **Source:** [binance.com/en/fee/futureFee](https://www.binance.com/en/fee/futureFee) · Last verified: 2026-06-29  
> **Always check [Binance Announcements](https://www.binance.com/en/support/announcement) for the latest trading updates and promotional rates.**  
> **Platform:** Binance REST API & WebSockets · **Server time:** UTC  
> **Data provider string for all Binance configs:** `"data_provider": "binance"`

---

## 1. Commission Model by Contract Type & VIP Level

This table maps directly to the config field `commission_percent`. Use only `commission_percent` for Binance configs; `commission_per_lot` should always be set to `0.0` or omitted.

| Contract Type & VIP Level | Maker Fee | Taker Fee | Recommended Backtest Config | Notes |
|---------------------------|-----------|-----------|----------------------------|-------|
| **USDT-M Futures (VIP 0)** | 0.0200% | 0.0500% | `"commission_percent": 0.000325` | Base rate for standard accounts. Backtest uses a blended `0.000325` (0.0325%) to account for slippage and taker executions. |
| **USDT-M Futures (10% BNB Discount)** | 0.0180% | 0.0450% | `"commission_percent": 0.000315` | Discount applied automatically if paying fees using BNB in the Futures wallet. |
| **USDC-M Futures (VIP 0)** | 0.0200% | 0.0500% | `"commission_percent": 0.000325` | Standard non-promotional rate for USDC-margined futures contracts. |
| **USDC-M Futures (Promo Rate)** | 0.0000% | 0.0400% | `"commission_percent": 0.000200` | Active USDC promo rate. Zero maker fees. Recommended blended backtest rate is `0.0002` (0.02%). |
| **USDC-M Futures (Promo + BNB Discount)** | 0.0000% | 0.0360% | `"commission_percent": 0.000180` | USDC promotional rate with an additional 10% BNB taker fee discount applied. |

### Config Snippet Examples

```json
// Standard USDT-M Futures (Conservative backtest default)
{ "commission_percent": 0.000325, "commission_per_lot": 0.0 }

// USDC-M Futures under active promotional rates (Blended)
{ "commission_percent": 0.000200, "commission_per_lot": 0.0 }

// USDT-M Futures Taker-only (Stress testing execution)
{ "commission_percent": 0.000500, "commission_per_lot": 0.0 }
```

---

## 2. Forex Pairs

**Not Supported:** Binance is a cryptocurrency-only trading exchange and does not offer traditional Forex pair markets. For Forex trading strategies, consult the [FTMO](./FTMO_SYMBOLS.md) or [FXIFY](./FXIFY_SYMBOLS.md) reference guides.

---

## 3. Indices (Stock Index CFDs)

**Not Supported:** Stock Index CFDs are not tradable on the Binance platform. For Stock Index trading strategies, consult the [FTMO](./FTMO_SYMBOLS.md) or [FXIFY](./FXIFY_SYMBOLS.md) reference guides.

---

## 4. Commodities

**Not Supported:** Commodities such as Metals, Energies, and Agriculture CFDs are not available on Binance. For Commodity trading strategies, consult the [FTMO](./FTMO_SYMBOLS.md) or [FXIFY](./FXIFY_SYMBOLS.md) reference guides.

---

## 5. Cryptocurrencies

**Commission:** `commission_percent: 0.000325` (Standard USDT-M blended)  
**Model:** USD-Margined Futures (USDT and USDC settled) — Perpetual and Delivery contracts  
**Sessions:** 24/7/365 (Continuous trading, no weekend market gaps)

### 5.1 Major USDT-Margined Futures (USDT-Settled)

| Binance Symbol | Description | Contract Unit | Sizing Precision | Max Leverage |
|----------------|-------------|---------------|------------------|--------------|
| `BTCUSDT` | Bitcoin / USDT | 1 BTC | 3 decimals | 125x |
| `ETHUSDT` | Ethereum / USDT | 1 ETH | 2 decimals | 100x |
| `SOLUSDT` | Solana / USDT | 1 SOL | 2 decimals | 75x |
| `BNBUSDT` | Binance Coin / USDT | 1 BNB | 2 decimals | 75x |
| `XRPUSDT` | Ripple / USDT | 1 XRP | 1 decimal | 75x |
| `ADAUSDT` | Cardano / USDT | 1 ADA | 0 decimals | 75x |
| `DOGEUSDT` | Dogecoin / USDT | 1 DOGE | 0 decimals | 75x |
| `LTCUSDT` | Litecoin / USDT | 1 LTC | 3 decimals | 75x |
| `LINKUSDT` | Chainlink / USDT | 1 LINK | 2 decimals | 75x |
| `AVAXUSDT` | Avalanche / USDT | 1 AVAX | 2 decimals | 75x |
| `DOTUSDT` | Polkadot / USDT | 1 DOT | 1 decimal | 75x |
| `NEARUSDT` | NEAR Protocol / USDT | 1 NEAR | 1 decimal | 75x |
| `TRXUSDT` | TRON / USDT | 1 TRX | 0 decimals | 75x |
| `ETCUSDT` | Ethereum Classic / USDT | 1 ETC | 2 decimals | 75x |
| `BCHUSDT` | Bitcoin Cash / USDT | 1 BCH | 3 decimals | 75x |

### 5.2 Major USDC-Margined Futures (USDC-Settled)

These contracts use the `.USDC` underlying settlement asset and are eligible for maker promotional fee exemptions.

| Binance Symbol | Description | Contract Unit | Sizing Precision | Max Leverage |
|----------------|-------------|---------------|------------------|--------------|
| `BTCUSDC` | Bitcoin / USDC | 1 BTC | 3 decimals | 125x |
| `ETHUSDC` | Ethereum / USDC | 1 ETH | 2 decimals | 100x |
| `SOLUSDC` | Solana / USDC | 1 SOL | 2 decimals | 75x |
| `XRPUSDC` | Ripple / USDC | 1 XRP | 1 decimal | 75x |
| `BNBUSDC` | Binance Coin / USDC | 1 BNB | 2 decimals | 75x |
| `DOGEUSDC` | Dogecoin / USDC | 1 DOGE | 0 decimals | 75x |
| `ADAUSDC` | Cardano / USDC | 1 ADA | 0 decimals | 75x |
| `AVAXUSDC` | Avalanche / USDC | 1 AVAX | 2 decimals | 75x |

---

## 6. Config Quick-Reference by Symbol

Use this table to copy-paste the correct cost fields directly into any config.

| Symbol | Asset Class | Config Fields |
|--------|-------------|--------------|
| `BTCUSDT` | Crypto (USDT-M) | `"commission_percent": 0.000325` |
| `ETHUSDT` | Crypto (USDT-M) | `"commission_percent": 0.000325` |
| `SOLUSDT` | Crypto (USDT-M) | `"commission_percent": 0.000325` |
| `BNBUSDT` | Crypto (USDT-M) | `"commission_percent": 0.000325` |
| `XRPUSDT` | Crypto (USDT-M) | `"commission_percent": 0.000325` |
| `ADAUSDT` | Crypto (USDT-M) | `"commission_percent": 0.000325` |
| `DOGEUSDT` | Crypto (USDT-M) | `"commission_percent": 0.000325` |
| `LTCUSDT` | Crypto (USDT-M) | `"commission_percent": 0.000325` |
| `LINKUSDT` | Crypto (USDT-M) | `"commission_percent": 0.000325` |
| `AVAXUSDT` | Crypto (USDT-M) | `"commission_percent": 0.000325` |
| `BTCUSDC` | Crypto (USDC-M) | `"commission_percent": 0.000200` *(Promo blended)* |
| `ETHUSDC` | Crypto (USDC-M) | `"commission_percent": 0.000200` *(Promo blended)* |
| `SOLUSDC` | Crypto (USDC-M) | `"commission_percent": 0.000200` *(Promo blended)* |
| *(all other USDT pairs)* | Crypto (USDT-M) | `"commission_percent": 0.000325` |
| *(all other USDC pairs)* | Crypto (USDC-M) | `"commission_percent": 0.000200` |

---

## 7. Binance Data Availability Limits

Binance provides high-density historical data through its API endpoints with **no artificial calendar limit or bar buffer ceilings** like those found on MT5. The limit is determined solely by the listing date of the specific futures contract.

| Timeframe | Calendar Limit | Notes |
|-----------|---------------|-------|
| 1m | No practical limit | Fetchable back to listing date (e.g., late 2019 for BTCUSDT). |
| 5m–30m | No practical limit | Fetchable back to listing date. |
| 1h–4h | No practical limit | Fetchable back to listing date. |
| 1d+ | No practical limit | Fetchable back to listing date. |

> **Warning on API Limits:** While historical data has no calendar limit, the Binance API imposes strict **Request Weight Limits** (typically 1,200 to 2,400 per minute). Exceeding these limits results in an HTTP 429 error and temporary IP bans. The codebase protects against this by caching fetched symbols locally (`SYMBOL_INFO_TTL` = 5 minutes).

---

## 8. Important Trading Rules & Compliance Notes

- **Funding Rates:** Perpetual contracts do not have an expiration date but settle funding fees every 4 or 8 hours depending on the premium index. Backtests still do not model funding fees — the new `"swap"` config (see FTMO_SYMBOLS.md §10) is a *daily* rollover model built for MT5-style CFD brokers and is the wrong shape for a rate that resets multiple times a day and can flip sign; leave `"swap"` unset for Binance. Continue using a slightly conservative `commission_percent` to offset funding drag, or track `funding_rate_history` (Binance API) separately if you need to model it precisely.
- **Minimum Order Notional Size:** Orders must meet a minimum value requirement (usually 5 to 20 USDT/USDC depending on the contract). Orders falling below this limit will be rejected by the executor.
- **Leverage Tiers:** Maximum leverage is determined by position size. Increasing your volume will automatically shift your account into higher margin requirement brackets, lowering maximum leverage.
- **BNB Fee Deduction:** To activate the 10% trading fee discount, you must hold BNB in your Futures wallet and enable the discount feature in your Binance account settings.
- **Continuous Operations:** Maintenance windows are rare and usually occur dynamically without full API downtime, but always monitor WebSocket connection status events.

---

## 9. Recommended Backtest Ranges by Symbol Type

These are recommended minimum and preferred ranges derived from crypto volatility profiles and cycle lengths.

| Asset Class | Minimum Range | Recommended | Notes |
|-------------|--------------|-------------|-------|
| BTC / ETH (1h/4h) | 3_years | 5_years | Captures full multi-year crypto bull, bear, and consolidation cycles. |
| Altcoins (SOL, AVAX) (1h/4h) | 1_year | 3_years | Protects against structural changes/listing date limitations on newer assets. |
| Intraday (1m–15m) | 6_months | 1_year | Sufficient bar density to achieve statistical significance (200+ trades). |

---

## 10. Swap Config — Not Applicable

Leave `"swap"` unset for all Binance configs (it defaults to a no-op). It
was built for daily MT5-style rollover (see
[FTMO_SYMBOLS.md §10](./FTMO_SYMBOLS.md#10-swap--rollover-costs-overnight-financing))
and doesn't fit perpetual funding's 4–8h cadence or sign-flipping behavior.
If funding drag matters for a specific strategy's backtest fidelity, the
more accurate approach is pulling `fundingRate` history from
`GET /fapi/v1/fundingRate` and applying it directly against position
duration rather than approximating it through the swap model.
