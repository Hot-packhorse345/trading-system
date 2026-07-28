use std::collections::HashMap;
use std::fs;
use std::path::Path;
use anyhow::{Context, Result};
use serde_json::Value;
use sha2::{Digest, Sha256};
use strategy::{build_strategy, ParamKind, ParamSpec};
use crate::catalog::{SymbolMeta, AssetClass};
use crate::ledger::{TrialRecord, append_ledger};
use crate::priors::read_priors;

pub fn synthesize_grid(
    schema: &[ParamSpec],
    priors: &HashMap<String, Vec<(f64, String, String)>>,
    exclude_symbol: &str,
    exclude_date_range: (&str, &str),
) -> Result<Value> {
    let tunable_count = schema.iter().filter(|spec| spec.kind == ParamKind::Tunable).count();
    if tunable_count > 3 {
        return Err(anyhow::anyhow!("Too many tunable parameters: {} (max 3 allowed)", tunable_count));
    }

    let mut map = serde_json::Map::new();
    for spec in schema {
        match spec.kind {
            ParamKind::Tunable => {
                let mut usable_priors = Vec::new();
                if let Some(list) = priors.get(&spec.key) {
                    for (val, symbol, date_range) in list {
                        let parts: Vec<&str> = date_range.split(" to ").collect();
                        let (p_start, p_end) = if parts.len() == 2 {
                            (parts[0].trim(), parts[1].trim())
                        } else {
                            ("", "")
                        };
                        let overlaps = symbol == exclude_symbol
                            || date_ranges_overlap(p_start, p_end, exclude_date_range.0, exclude_date_range.1);
                        if !overlaps {
                            usable_priors.push(*val);
                        }
                    }
                }

                let mut center = if usable_priors.len() >= 2 {
                    median(usable_priors)
                } else {
                    spec.default.as_f64().unwrap_or(0.0)
                };

                if let Some((min_val, max_val)) = spec.safe_range {
                    center = center.max(min_val).min(max_val);
                }

                let floor = get_min_step_floor(&spec.key);
                let mut step = if floor < 1.0 { 0.5 } else { 5.0 };
                if step < floor {
                    step = floor;
                }

                let mut start = center - step;
                let mut stop = center + step;
                if let Some((min_val, max_val)) = spec.safe_range {
                    start = start.max(min_val).min(max_val);
                    stop = stop.max(min_val).min(max_val);
                }

                map.insert(
                    spec.key.clone(),
                    serde_json::json!({
                        "$sample": "range",
                        "start": start,
                        "stop": stop + (step * 0.01),
                        "step": step
                    }),
                );
            }
            ParamKind::Structural => {
                map.insert(spec.key.clone(), spec.default.clone());
            }
        }
    }

    Ok(Value::Object(map))
}

fn date_ranges_overlap(
    prior_start: &str,
    prior_end: &str,
    exclude_start: &str,
    exclude_end: &str,
) -> bool {
    if prior_start.is_empty() || prior_end.is_empty() || exclude_start.is_empty() || exclude_end.is_empty() {
        return false;
    }
    prior_start <= exclude_end && exclude_start <= prior_end
}

fn median(mut values: Vec<f64>) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mid = values.len() / 2;
    if values.len() % 2 == 0 {
        (values[mid - 1] + values[mid]) / 2.0
    } else {
        values[mid]
    }
}

fn get_min_step_floor(param_key: &str) -> f64 {
    if param_key.contains("distance")
        || param_key.contains("rr")
        || param_key.contains("multiplier")
        || param_key.contains("mult")
    {
        0.2
    } else {
        5.0
    }
}

fn calculate_hash(val: &Value) -> String {
    let serialized = serde_json::to_string(val).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(serialized.as_bytes());
    format!("{:x}", hasher.finalize())
}

pub fn find_base_template(strategy_name: &str) -> Option<Value> {
    let Ok(entries) = std::fs::read_dir("configs/templates") else {
        return None;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("json") {
            if let Ok(text) = std::fs::read_to_string(&path) {
                if let Ok(val) = serde_json::from_str::<Value>(&text) {
                    if val.get("strategy").and_then(|s| s.as_str()) == Some(strategy_name) {
                        return Some(val);
                    }
                }
            }
        }
    }
    None
}

pub fn get_default_skeleton(strategy: &str) -> Result<Value> {
    let stop_manager = serde_json::json!([
        {
            "type": "variant3",
            "stop_distance": {
                "$sample": "range",
                "start": 0.5,
                "stop": 3.05,
                "step": 0.5
            },
            "start_rr": {
                "$sample": "range",
                "start": 0.5,
                "stop": 2.05,
                "step": 0.5
            }
        }
    ]);

    let indicators = match strategy {
        "ema_cross" => serde_json::json!({
            "ema_fast": { "type": "ema", "period": 9 },
            "ema_slow": { "type": "ema", "period": 21 }
        }),
        other => return Err(anyhow::anyhow!("unknown strategy: {}", other)),
    };

    let mut strategy_parameters = serde_json::Map::new();
    let strat_obj = strategy::build_strategy(strategy)?;
    for spec in strat_obj.param_schema() {
        if !spec.key.contains('.') {
            strategy_parameters.insert(spec.key, spec.default);
        }
    }

    Ok(serde_json::json!({
        "strategy": strategy,
        "stop_manager": stop_manager,
        "strategy_parameters": Value::Object(strategy_parameters),
        "indicators": indicators,
        "metric": "profit_factor",
        "data_provider": "mt5",
        "initial_balance": 100000.0,
        "risk_percentage": 0.001,
        "pyramiding": false,
        "exit_rules": [],
        "gates": {
            "min_probe_trades": 200,
            "min_wfe": 0.5,
            "min_consistency": 0.5,
            "min_pf": 1.5,
            "min_sharpe": 0.8,
            "min_wr": 0.3,
            "max_dd_pct": 25.0,
            "min_trades": 200,
            "min_density": 40.0,
            "plateau_pass_rate": 0.6,
            "plateau_min_pf": 1.2,
            "cross_min_pf": 1.2,
            "target_is_trades": 50.0,
            "min_rounds": 8
        }
    }))
}

pub fn enumerate_candidates(
    strategy_name: &str,
    broker: &str,
    symbols: &[String],
    timeframes: &[String],
    exclude_date_range: (&str, &str),
    out_dir: &Path,
    ledger_path: &Path,
    session_id: &str,
) -> Result<Vec<(String, Value)>> {
    let catalog = crate::catalog::load_broker_catalog(broker)
        .with_context(|| format!("failed to load catalog for {}", broker))?;

    let strategy_obj = build_strategy(strategy_name)?;
    let priors = read_priors(strategy_name, "docs/strategy");

    let base_tpl = find_base_template(strategy_name)
        .unwrap_or_else(|| get_default_skeleton(strategy_name).unwrap());

    // Filter symbols to match input list if provided
    let target_symbols: Vec<SymbolMeta> = if symbols.is_empty() {
        catalog
    } else {
        catalog
            .into_iter()
            .filter(|s| symbols.contains(&s.symbol))
            .collect()
    };

    // Group symbols by asset class
    let mut asset_groups: HashMap<AssetClass, Vec<SymbolMeta>> = HashMap::new();
    for sym in target_symbols {
        asset_groups.entry(sym.asset_class.clone()).or_default().push(sym);
    }

    fs::create_dir_all(out_dir)?;

    let mut generated_candidates = Vec::new();

    // Iterate over each (asset_class, timeframe) pair
    for (asset_class, syms) in asset_groups {
        let first_symbol = &syms[0].symbol;

        // Build the combined schema containing strategy's whitelisted params AND stop manager params
        let mut full_schema = strategy_obj.param_schema();
        if !full_schema.iter().any(|spec| spec.key == "stop_distance") {
            full_schema.push(ParamSpec {
                key: "stop_distance".to_string(),
                kind: ParamKind::Tunable,
                default: serde_json::json!(1.5),
                safe_range: Some((0.5, 5.0)),
            });
        }
        if !full_schema.iter().any(|spec| spec.key == "start_rr") {
            full_schema.push(ParamSpec {
                key: "start_rr".to_string(),
                kind: ParamKind::Tunable,
                default: serde_json::json!(1.0),
                safe_range: Some((0.5, 3.0)),
            });
        }

        // Synthesize grid once per asset class using the first symbol's name to avoid circular seeding on that asset class
        let resolved_grid = synthesize_grid(
            &full_schema,
            &priors,
            first_symbol,
            exclude_date_range,
        )?;

        for tf in timeframes {
            let mut candidate = base_tpl.clone();

            // 1. Distribute stop manager parameters
            if let Some(Value::Array(sm_arr)) = candidate.get_mut("stop_manager") {
                if let Some(Value::Object(sm_obj)) = sm_arr.first_mut() {
                    if let Some(grid_val) = resolved_grid.get("stop_distance") {
                        sm_obj.insert("stop_distance".to_string(), grid_val.clone());
                    }
                    if let Some(grid_val) = resolved_grid.get("start_rr") {
                        sm_obj.insert("start_rr".to_string(), grid_val.clone());
                    }
                }
            }

            // 2. Distribute strategy parameters
            if let Some(Value::Object(sp_obj)) = candidate.get_mut("strategy_parameters") {
                for (key, val) in resolved_grid.as_object().unwrap() {
                    if key != "stop_distance" && key != "start_rr" && !key.contains('.') {
                        sp_obj.insert(key.clone(), val.clone());
                    }
                }
            }

            // 3. Distribute indicator parameters
            if let Some(Value::Object(ind_obj)) = candidate.get_mut("indicators") {
                for (key, val) in resolved_grid.as_object().unwrap() {
                    if key.contains('.') {
                        let parts: Vec<&str> = key.split('.').collect();
                        let ind_name = parts[0];
                        let param_name = parts[1];
                        if let Some(Value::Object(ind_param_obj)) = ind_obj.get_mut(ind_name) {
                            ind_param_obj.insert(param_name.to_string(), val.clone());
                        }
                    }
                }
            }

            // Set provider and other candidate properties
            if let Some(obj) = candidate.as_object_mut() {
                obj.insert("data_provider".to_string(), Value::String(broker.to_string()));
            }

            // Calculate generation_id / hash of the template
            let hash = calculate_hash(&candidate);

            // Format output filename
            let asset_class_str = format!("{:?}", asset_class).to_lowercase();
            let filename = format!("{}_{}_{}_{}.json", strategy_name, asset_class_str, tf, hash);
            let filepath = out_dir.join(&filename);

            // Write once: only if file does not exist
            if !filepath.exists() {
                let json_str = serde_json::to_string_pretty(&candidate)?;
                fs::write(&filepath, json_str)?;
            }

            // Ledger logging: Append a Verdict::Generated entry for each candidate for every target symbol under this asset class
            for s in &syms {
                let record = TrialRecord {
                    session_id: session_id.to_string(),
                    timestamp: chrono::Local::now().to_rfc3339(),
                    strategy: strategy_name.to_string(),
                    symbol: s.symbol.clone(),
                    tf: tf.clone(),
                    gates_hash: hash.clone(),
                    holdout_start: if exclude_date_range.0.is_empty() { None } else { Some(exclude_date_range.0.to_string()) },
                    holdout_end: if exclude_date_range.1.is_empty() { None } else { Some(exclude_date_range.1.to_string()) },
                    phase2_hash: None,
                    phase_reached: 0,
                    verdict: "Generated".to_string(),
                    cross_asset_status: None,
                    wfe: 0.0,
                    consistency: 0.0,
                    consistency_lcb: 0.0,
                    pf: 0.0,
                    sharpe: 0.0,
                    trades: 0,
                };
                append_ledger(ledger_path, &record)?;
            }

            generated_candidates.push((filename, candidate));
        }
    }

    Ok(generated_candidates)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_date_ranges_overlap() {
        assert!(date_ranges_overlap("2024-01-01", "2024-12-31", "2024-06-01", "2024-07-01"));
        assert!(date_ranges_overlap("2024-01-01", "2024-12-31", "2023-12-01", "2024-01-15"));
        assert!(date_ranges_overlap("2024-01-01", "2024-12-31", "2024-12-15", "2025-01-15"));

        // Exact edge overlap
        assert!(date_ranges_overlap("2024-01-01", "2024-12-31", "2024-12-31", "2025-01-01"));

        // No overlap
        assert!(!date_ranges_overlap("2024-01-01", "2024-12-31", "2025-01-01", "2025-06-01"));
        assert!(!date_ranges_overlap("2024-01-01", "2024-12-31", "2023-01-01", "2023-12-31"));
    }

    #[test]
    fn test_grid_synthesis_cap_error() {
        // Build schema with 4 Tunable params -> should error
        let schema = vec![
            ParamSpec { key: "a".into(), kind: ParamKind::Tunable, default: 1.0.into(), safe_range: None },
            ParamSpec { key: "b".into(), kind: ParamKind::Tunable, default: 2.0.into(), safe_range: None },
            ParamSpec { key: "c".into(), kind: ParamKind::Tunable, default: 3.0.into(), safe_range: None },
            ParamSpec { key: "d".into(), kind: ParamKind::Tunable, default: 4.0.into(), safe_range: None },
        ];
        let priors = HashMap::new();
        let res = synthesize_grid(&schema, &priors, "EURUSD", ("", ""));
        assert!(res.is_err());
        assert!(res.err().unwrap().to_string().contains("Too many tunable parameters"));
    }

    #[test]
    fn test_grid_synthesis_median_centering_and_floor() {
        let schema = vec![
            ParamSpec {
                key: "atr_mult".into(),
                kind: ParamKind::Tunable,
                default: 1.5.into(),
                safe_range: Some((0.5, 5.0)),
            },
            ParamSpec {
                key: "min_adx".into(),
                kind: ParamKind::Tunable,
                default: 20.0.into(),
                safe_range: Some((10.0, 50.0)),
            },
        ];

        let mut priors = HashMap::new();
        // 3 prior findings for atr_mult: 1.0, 1.8, 2.2 -> median is 1.8
        priors.insert(
            "atr_mult".to_string(),
            vec![
                (1.0, "GBPUSD".to_string(), "2024-01-01 to 2024-06-01".to_string()),
                (1.8, "GBPUSD".to_string(), "2024-06-01 to 2024-12-01".to_string()),
                (2.2, "GBPUSD".to_string(), "2025-01-01 to 2025-06-01".to_string()),
            ],
        );
        // fewer than 2 priors for min_adx -> falls back to default 20.0

        let res = synthesize_grid(&schema, &priors, "EURUSD", ("2026-01-01", "2026-12-31")).unwrap();
        let grid = res.as_object().unwrap();

        // check atr_mult
        let atr_grid = &grid["atr_mult"];
        assert!((atr_grid["start"].as_f64().unwrap() - 1.3).abs() < 1e-9); // center (1.8) - step (0.5)
        assert_eq!(atr_grid["step"].as_f64().unwrap(), 0.5);

        // check min_adx (floor is 5.0)
        let adx_grid = &grid["min_adx"];
        assert_eq!(adx_grid["start"].as_f64().unwrap(), 15.0); // center (20.0) - step (5.0)
        assert_eq!(adx_grid["step"].as_f64().unwrap(), 5.0);
    }

    #[test]
    fn test_circular_seeding_exclusion() {
        let schema = vec![
            ParamSpec {
                key: "atr_mult".into(),
                kind: ParamKind::Tunable,
                default: 1.5.into(),
                safe_range: Some((0.5, 5.0)),
            },
        ];

        let mut priors = HashMap::new();
        priors.insert(
            "atr_mult".to_string(),
            vec![
                // Overlaps symbol EURUSD -> exclude
                (1.0, "EURUSD".to_string(), "2024-01-01 to 2024-06-01".to_string()),
                // Overlaps date-range -> exclude
                (1.8, "GBPUSD".to_string(), "2026-06-01 to 2026-12-01".to_string()),
                // Clean prior -> include
                (2.2, "GBPUSD".to_string(), "2024-01-01 to 2024-12-01".to_string()),
                // Clean prior -> include
                (2.4, "GBPUSD".to_string(), "2025-01-01 to 2025-12-31".to_string()),
            ],
        );

        // Current validation on EURUSD for date range 2026-01-01 to 2026-12-31
        let res = synthesize_grid(&schema, &priors, "EURUSD", ("2026-01-01", "2026-12-31")).unwrap();
        let grid = res.as_object().unwrap();

        // 2 clean priors: 2.2 and 2.4 -> median is 2.3
        let atr_grid = &grid["atr_mult"];
        assert!((atr_grid["start"].as_f64().unwrap() - 1.8).abs() < 1e-9); // center (2.3) - step (0.5)
    }
}
