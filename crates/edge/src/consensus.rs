use crate::catalog::SymbolMeta;
use crate::template::DiscoveryTemplate;
use anyhow::{anyhow, Result};
use backtest::{canonical_value, expand_sample_values};
use serde_json::{json, Value};

/// How many steps on either side of the consensus value the micro-plateau
/// grid explores per numeric dimension (radius 2 => up to 5 values).
const PLATEAU_RADIUS: usize = 2;

/// One numeric dimension varied in the Phase 5 micro-plateau grid.
///
/// `key` is the dot-path matching `BacktestResult.config`'s column naming
/// (see `crates/backtest/src/engine.rs::build_result_config_from_group`),
/// e.g. `stop_manager.stop_distance`, `strategy_parameters.rsi_period`,
/// `indicators.rsi.period` — this lets the pipeline map a plateau-run result
/// back to its exact grid coordinate in each dimension.
#[derive(Debug, Clone)]
pub struct PlateauDimension {
    pub key: String,
    pub values: Vec<f64>,
    pub consensus_index: usize,
}

pub fn config_from_consensus(
    tpl: &DiscoveryTemplate,
    symbol: &SymbolMeta,
    tf: &str,
    consensus: &Value,
    start_str: &str,
    end_str: &str,
) -> Value {
    let mut indicators = serde_json::Map::new();
    if let Value::Object(ref ind_map) = consensus["indicators"] {
        for (name, spec) in ind_map {
            let mut spec_obj = spec.as_object().cloned().unwrap_or_default();
            if let Some(tf_val) = spec_obj.remove("timeframe") {
                spec_obj.insert("timeframe".to_string(), tf_val);
            }
            indicators.insert(name.clone(), Value::Object(spec_obj));
        }
    }

    json!({
        "strategy":            tpl.strategy,
        "symbol":              symbol.symbol,
        "timeframe":           tf,
        "stop_timeframe":      tf,
        "pyramiding":          tpl.pyramiding(),
        "backtest_start":      start_str,
        "backtest_end":        end_str,
        "data_provider":       tpl.data_provider(),
        "initial_balance":     tpl.initial_balance(),
        "risk_percentage":     tpl.risk_pct(),
        "commission_per_lot":  symbol.commission_per_lot,
        "commission_percent":  symbol.commission_percent,
        "stop_manager":        vec![consensus["stop_manager"].clone()],
        "strategy_parameters": consensus["strategy_parameters"].clone(),
        "indicators":          Value::Object(indicators),
    })
}

pub fn build_plateau_config(
    tpl: &DiscoveryTemplate,
    symbol: &SymbolMeta,
    tf: &str,
    htf: &[String],
    consensus: &Value,
    start_str: &str,
    end_str: &str,
) -> Result<(Value, Vec<PlateauDimension>)> {
    let cons_sm_type = consensus["stop_manager"]["type"]
        .as_str()
        .ok_or_else(|| anyhow!("Consensus stop_manager missing type"))?;

    let tpl_sm = tpl
        .stop_manager
        .iter()
        .find(|sm| sm["type"].as_str() == Some(cons_sm_type))
        .ok_or_else(|| {
            anyhow!(
                "Template does not contain stop_manager of type {}",
                cons_sm_type
            )
        })?;

    let mut dims: Vec<PlateauDimension> = Vec::new();

    let mut resolved_sm = tpl_sm.clone();
    let mut sm_combos = 1;
    walk_and_plateau(
        &mut resolved_sm,
        &consensus["stop_manager"],
        &mut sm_combos,
        "stop_manager",
        &mut dims,
    )?;

    let mut resolved_sp = tpl.strategy_parameters.clone();
    let mut sp_combos = 1;
    walk_and_plateau(
        &mut resolved_sp,
        &consensus["strategy_parameters"],
        &mut sp_combos,
        "strategy_parameters",
        &mut dims,
    )?;

    if let Value::Object(ref mut map) = resolved_sp {
        map.insert(
            "trade_direction".to_string(),
            Value::String(symbol.trade_direction.clone()),
        );
    }

    let mut resolved_ind = tpl.indicators.clone();
    let mut ind_combos = 1;

    resolved_ind = crate::template::expand_htf_timeframes(resolved_ind, htf);

    walk_and_plateau(
        &mut resolved_ind,
        &consensus["indicators"],
        &mut ind_combos,
        "indicators",
        &mut dims,
    )?;

    let total_combos = sm_combos * sp_combos * ind_combos;
    if total_combos > 200 {
        tracing::warn!(
            "Warning: Plateau grid has {} combinations, which is above 200. This might run slowly.",
            total_combos
        );
    }

    Ok((
        json!({
            "strategy":            tpl.strategy,
            "symbol":              symbol.symbol,
            "timeframe":           tf,
            "stop_timeframe":      tf,
            "pyramiding":          tpl.pyramiding(),
            "backtest_start":      start_str,
            "backtest_end":        end_str,
            "data_provider":       tpl.data_provider(),
            "initial_balance":     tpl.initial_balance(),
            "risk_percentage":     tpl.risk_pct(),
            "commission_per_lot":  symbol.commission_per_lot,
            "commission_percent":  symbol.commission_percent,
            "stop_manager":        vec![resolved_sm],
            "strategy_parameters": resolved_sp,
            "indicators":          resolved_ind,
        }),
        dims,
    ))
}

fn walk_and_plateau(
    tpl_val: &mut Value,
    cons_val: &Value,
    combos: &mut usize,
    path: &str,
    dims: &mut Vec<PlateauDimension>,
) -> Result<()> {
    match tpl_val {
        Value::Object(map) => {
            if map.contains_key("$sample") {
                let expanded = expand_sample_values(map)?;
                if expanded.len() <= 1 {
                    *tpl_val = cons_val.clone();
                } else {
                    if let Some(idx) = find_consensus_index(&expanded, cons_val) {
                        let is_numeric = expanded.iter().all(|x| x.is_number());
                        if is_numeric {
                            let window_start = idx.saturating_sub(PLATEAU_RADIUS);
                            let window_end = (idx + PLATEAU_RADIUS).min(expanded.len() - 1);
                            let plateau_vals: Vec<Value> =
                                expanded[window_start..=window_end].to_vec();
                            *combos *= plateau_vals.len();

                            if let Some(values_f64) = plateau_vals
                                .iter()
                                .map(|v| v.as_f64())
                                .collect::<Option<Vec<_>>>()
                            {
                                if values_f64.len() > 1 {
                                    dims.push(PlateauDimension {
                                        key: path.to_string(),
                                        values: values_f64,
                                        consensus_index: idx - window_start,
                                    });
                                }
                            }

                            *tpl_val = json!({
                                "$sample": "values",
                                "items": plateau_vals
                            });
                        } else {
                            *tpl_val = cons_val.clone();
                        }
                    } else {
                        *tpl_val = cons_val.clone();
                    }
                }
            } else {
                for (k, v) in map.iter_mut() {
                    let cons_k = if k == "[g]timeframe" { "timeframe" } else { k };
                    if let Some(cv) = cons_val.get(cons_k) {
                        let child_path = format!("{path}.{cons_k}");
                        walk_and_plateau(v, cv, combos, &child_path, dims)?;
                    } else {
                        crate::template::resolve_samples(v, None);
                    }
                }
            }
        }
        Value::Array(arr) => {
            if let Value::Array(cons_arr) = cons_val {
                for (i, v) in arr.iter_mut().enumerate() {
                    if i < cons_arr.len() {
                        walk_and_plateau(v, &cons_arr[i], combos, path, dims)?;
                    } else {
                        crate::template::resolve_samples(v, None);
                    }
                }
            } else {
                for v in arr.iter_mut() {
                    crate::template::resolve_samples(v, None);
                }
            }
        }
        _ => {}
    }
    Ok(())
}

fn find_consensus_index(expanded: &[Value], cons_val: &Value) -> Option<usize> {
    let cons_canon = canonical_value(cons_val);
    for (i, v) in expanded.iter().enumerate() {
        if canonical_value(v) == cons_canon {
            return Some(i);
        }
    }

    if let Some(cons_f) = cons_val.as_f64() {
        for (i, v) in expanded.iter().enumerate() {
            if let Some(vf) = v.as_f64() {
                if (vf - cons_f).abs() < 1e-9 {
                    return Some(i);
                }
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::AssetClass;

    #[test]
    fn test_config_from_consensus() {
        let tpl: DiscoveryTemplate = serde_json::from_value(json!({
            "strategy": "ema_cross",
            "stop_manager": [{"type": "fixed", "stop_distance": 2.0}],
            "indicators": {},
            "strategy_parameters": {}
        }))
        .unwrap();

        let sym = SymbolMeta {
            symbol: "EURUSD".to_string(),
            asset_class: AssetClass::Forex,
            commission_per_lot: 5.0,
            commission_percent: 0.0,
            trade_direction: "both".to_string(),
            backtest_range: "5_year".to_string(),
            correlated_asset: None,
        };

        let consensus = json!({
            "stop_manager": {"type": "fixed", "stop_distance": 2.5},
            "strategy_parameters": {"param_x": 10},
            "indicators": {
                "ind_1": {"type": "rsi", "timeframe": "4h", "period": 14}
            }
        });

        let cfg = config_from_consensus(&tpl, &sym, "15m", &consensus, "2020-01-01", "2025-01-01");
        assert_eq!(cfg["stop_manager"][0]["stop_distance"], json!(2.5));
        assert_eq!(cfg["strategy_parameters"]["param_x"], json!(10));
        assert_eq!(cfg["indicators"]["ind_1"]["timeframe"], json!("4h"));
    }

    #[test]
    fn test_build_plateau_config_widens_window_and_captures_dimension() {
        let tpl: DiscoveryTemplate = serde_json::from_value(json!({
            "strategy": "ema_cross",
            "stop_manager": [{
                "type": "fixed",
                "stop_distance": {"$sample": "range", "start": 1.0, "stop": 6.0, "step": 1.0},
                "start_rr": 1.0
            }],
            "indicators": {},
            "strategy_parameters": {}
        }))
        .unwrap();

        let sym = SymbolMeta {
            symbol: "EURUSD".to_string(),
            asset_class: AssetClass::Forex,
            commission_per_lot: 5.0,
            commission_percent: 0.0,
            trade_direction: "both".to_string(),
            backtest_range: "5_year".to_string(),
            correlated_asset: None,
        };

        let consensus = json!({
            "stop_manager": {"type": "fixed", "stop_distance": 3.0, "start_rr": 1.0},
            "strategy_parameters": {},
            "indicators": {}
        });

        let (cfg, dims) = build_plateau_config(
            &tpl,
            &sym,
            "15m",
            &[],
            &consensus,
            "2020-01-01",
            "2025-01-01",
        )
        .unwrap();

        let items = cfg["stop_manager"][0]["stop_distance"]["items"]
            .as_array()
            .unwrap();
        // radius 2 around consensus (3.0) within [1,2,3,4,5] covers the whole range
        assert_eq!(items.len(), 5);

        let dim = dims
            .iter()
            .find(|d| d.key == "stop_manager.stop_distance")
            .expect("stop_distance dimension should be captured");
        assert_eq!(dim.values, vec![1.0, 2.0, 3.0, 4.0, 5.0]);
        assert_eq!(dim.consensus_index, 2);
    }
}
