use anyhow::{anyhow, Context, Result};
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SymbolMeta {
    pub symbol: String,
    pub asset_class: AssetClass,
    pub commission_per_lot: f64,
    pub commission_percent: f64,
    pub trade_direction: String, // "both" | "long"
    pub backtest_range: String,  // e.g. "5_year"
    pub correlated_asset: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum AssetClass {
    Forex,
    Index,
    Commodity,
    Crypto,
    Unknown,
}

pub fn load_broker_catalog(broker: &str) -> Result<Vec<SymbolMeta>> {
    let broker_upper = broker.to_uppercase();
    let path = format!("docs/broker/{}_SYMBOLS.md", broker_upper);
    let mut content = String::new();
    File::open(&path)
        .with_context(|| format!("Cannot open broker catalog: {path}"))?
        .read_to_string(&mut content)?;

    parse_broker_markdown(&content)
}

pub fn parse_broker_markdown(md: &str) -> Result<Vec<SymbolMeta>> {
    let mut class_lot: HashMap<AssetClass, f64> = HashMap::new();
    let mut class_pct: HashMap<AssetClass, f64> = HashMap::new();
    let mut class_range: HashMap<AssetClass, String> = HashMap::new();
    let mut class_dir: HashMap<AssetClass, String> = HashMap::new();

    for line in md.lines() {
        if line.contains("commission_per_lot") || line.contains("commission_percent") {
            let ac = detect_asset_class_in_line(line);
            if ac != AssetClass::Unknown {
                if let Some(v) = extract_commission_value(line, "commission_per_lot") {
                    class_lot.insert(ac.clone(), v);
                }
                if let Some(v) = extract_commission_value(line, "commission_percent") {
                    class_pct.insert(ac.clone(), v);
                }
            }
        }

        if line.contains("_year") && line.contains('|') {
            let ac = detect_asset_class_in_line(line);
            if ac != AssetClass::Unknown {
                if let Some(range) = extract_recommended_range(line) {
                    class_range.entry(ac).or_insert(range);
                }
            }
        }
    }

    let default_range = |ac: &AssetClass| match ac {
        AssetClass::Forex => "5_year".to_string(),
        AssetClass::Index => "5_year".to_string(),
        AssetClass::Commodity => "4_year".to_string(),
        AssetClass::Crypto => "3_year".to_string(),
        AssetClass::Unknown => "5_year".to_string(),
    };

    class_dir.insert(AssetClass::Index, "long".to_string());

    let mut symbols: Vec<SymbolMeta> = Vec::new();
    let mut current_class = AssetClass::Unknown;

    for line in md.lines() {
        if line.starts_with("## ") {
            current_class = detect_asset_class_in_line(line);
            continue;
        }
        if line.starts_with("### ") {
            continue;
        }
        if !line.starts_with('|') {
            continue;
        }

        let sym = extract_backtick_symbol(line);
        let sym = match sym {
            Some(s) if !s.is_empty() && !s.starts_with('-') => s,
            _ => continue,
        };

        if sym.to_lowercase().contains("symbol")
            || sym.to_lowercase().contains("ticker")
            || sym.chars().all(|c| c == '-' || c == ' ')
        {
            continue;
        }

        let ac = if current_class != AssetClass::Unknown {
            current_class.clone()
        } else {
            infer_class_from_symbol(&sym)
        };

        let lot = *class_lot.get(&ac).unwrap_or(&0.0);
        let pct = *class_pct.get(&ac).unwrap_or(&0.0);
        let range = class_range
            .get(&ac)
            .cloned()
            .unwrap_or_else(|| default_range(&ac));
        let direction = class_dir
            .get(&ac)
            .cloned()
            .unwrap_or_else(|| "both".to_string());

        if symbols.iter().any(|s: &SymbolMeta| s.symbol == sym) {
            continue;
        }

        symbols.push(SymbolMeta {
            symbol: sym,
            asset_class: ac,
            commission_per_lot: lot,
            commission_percent: pct,
            trade_direction: direction,
            backtest_range: range,
            correlated_asset: None,
        });
    }

    assign_correlations(&mut symbols);

    if symbols.is_empty() {
        return Err(anyhow!(
            "No symbols found in broker catalog. Check docs/broker path and Markdown format."
        ));
    }

    Ok(symbols)
}

pub fn detect_asset_class_in_line(line: &str) -> AssetClass {
    let l = line.to_lowercase();
    if l.contains("forex") || l.contains("fx pair") || l.contains("currency pair") {
        AssetClass::Forex
    } else if l.contains("ind")
        && (l.contains("stock") || l.contains("index") || l.contains("cash"))
    {
        AssetClass::Index
    } else if l.contains("commod")
        || l.contains("metal")
        || l.contains("energy")
        || l.contains("agri")
        || l.contains("gold")
        || l.contains("oil")
    {
        AssetClass::Commodity
    } else if l.contains("crypto") || l.contains("bitcoin") || l.contains("digital") {
        AssetClass::Crypto
    } else {
        AssetClass::Unknown
    }
}

pub fn infer_class_from_symbol(sym: &str) -> AssetClass {
    if sym.ends_with(".cash") {
        AssetClass::Index
    } else if sym.starts_with("XAU")
        || sym.starts_with("XAG")
        || sym.starts_with("XPT")
        || sym.starts_with("XPD")
        || sym.ends_with("OIL")
        || sym.ends_with("OIL.cash")
        || sym == "NATGAS.cash"
    {
        AssetClass::Commodity
    } else if sym.ends_with("USD")
        && (sym.starts_with("BTC")
            || sym.starts_with("ETH")
            || sym.starts_with("XRP")
            || sym.starts_with("SOL")
            || sym.starts_with("BNB")
            || sym.starts_with("ADA")
            || sym.starts_with("DOT")
            || sym.starts_with("LTC"))
    {
        AssetClass::Crypto
    } else if sym.len() == 6 && sym.chars().all(|c| c.is_ascii_alphabetic()) {
        AssetClass::Forex
    } else {
        AssetClass::Unknown
    }
}

fn extract_backtick_symbol(line: &str) -> Option<String> {
    let parts: Vec<&str> = line.split('|').collect();
    if parts.len() < 2 {
        return None;
    }
    let first_col = parts[1];
    let start = first_col.find('`')?;
    let rest = &first_col[start + 1..];
    let end = rest.find('`')?;
    let sym = rest[..end].trim().to_string();
    if sym.is_empty() {
        None
    } else {
        Some(sym)
    }
}

fn extract_commission_value(line: &str, field: &str) -> Option<f64> {
    let key = format!("\"{field}\":");
    let pos = line.find(&key)? + key.len();
    let rest = line[pos..].trim_start();
    let end = rest.find(['`', '|', ' ', ',']).unwrap_or(rest.len());
    rest[..end].trim().parse().ok()
}

fn extract_recommended_range(line: &str) -> Option<String> {
    let cols: Vec<&str> = line.split('|').collect();
    if cols.len() < 5 {
        return None;
    }
    let candidate = cols[3].trim();
    if candidate.contains('_') && (candidate.contains("year") || candidate.contains("month")) {
        let norm = candidate
            .replace("_years", "_year")
            .replace("_months", "_month");
        Some(norm)
    } else {
        None
    }
}

pub fn assign_correlations(symbols: &mut [SymbolMeta]) {
    let idx: HashMap<String, usize> = symbols
        .iter()
        .enumerate()
        .map(|(i, s)| (s.symbol.clone(), i))
        .collect();

    let mut pairs: Vec<(usize, String)> = Vec::new();

    for (i, s) in symbols.iter().enumerate() {
        let corr = find_correlated(&s.symbol, &s.asset_class, &idx);
        if let Some(c) = corr {
            pairs.push((i, c));
        }
    }

    for (i, corr) in pairs {
        symbols[i].correlated_asset = Some(corr);
    }
}

fn find_correlated(sym: &str, ac: &AssetClass, idx: &HashMap<String, usize>) -> Option<String> {
    let explicit: &[(&str, &str)] = &[
        ("XAUUSD", "XAGUSD"),
        ("XAGUSD", "XAUUSD"),
        ("BTCUSD", "ETHUSD"),
        ("ETHUSD", "BTCUSD"),
        ("US100.cash", "US500.cash"),
        ("US500.cash", "US100.cash"),
        ("GER40.cash", "US100.cash"),
        ("UK100.cash", "GER40.cash"),
        ("GBPUSD", "EURUSD"),
        ("GBPJPY", "EURJPY"),
        ("USDJPY", "EURJPY"),
        ("AUDUSD", "NZDUSD"),
        ("NZDUSD", "AUDUSD"),
        ("USOIL.cash", "UKOIL.cash"),
        ("UKOIL.cash", "USOIL.cash"),
    ];
    for (a, b) in explicit {
        if *a == sym && idx.contains_key(*b) {
            return Some(b.to_string());
        }
    }

    match ac {
        AssetClass::Forex => {
            for other in idx.keys() {
                if other != sym
                    && other.len() == 6
                    && (other[..3] == sym[..3] || other[3..] == sym[3..])
                {
                    return Some(other.clone());
                }
            }
            None
        }
        AssetClass::Index => {
            for other in idx.keys() {
                if other != sym && other.ends_with(".cash") {
                    return Some(other.clone());
                }
            }
            None
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_broker_markdown() {
        let md = r#"
# FTMO Symbol Catalog

| Asset Class | Type | Detail | Commission |
|---|---|---|---|
| Forex | Per lot | $5.00 | `"commission_per_lot": 5.0` |
| Indices | Percent | 0.0% | `"commission_percent": 0.0` |

| Timeframe | Recommended | Alternate |
|---|---|---|
| Forex (4h) | 5_years | 3_years |
| Index (4h) | 3_years | 2_years |

## 2. Forex
| Symbol | Name |
|---|---|
| `EURUSD` | Euro / US Dollar |
| `GBPUSD` | Pound / US Dollar |

## 3. Indices
| Symbol | Name |
|---|---|
| `US100.cash` | Nasdaq |
        "#;

        let res = parse_broker_markdown(md).unwrap();
        assert_eq!(res.len(), 3);
        assert_eq!(res[0].symbol, "EURUSD");
        assert_eq!(res[0].asset_class, AssetClass::Forex);
        assert_eq!(res[0].commission_per_lot, 5.0);
        assert_eq!(res[0].backtest_range, "3_year");
        assert_eq!(res[0].correlated_asset, Some("GBPUSD".to_string()));

        assert_eq!(res[2].symbol, "US100.cash");
        assert_eq!(res[2].asset_class, AssetClass::Index);
        assert_eq!(res[2].commission_percent, 0.0);
        assert_eq!(res[2].trade_direction, "long");
    }
}
