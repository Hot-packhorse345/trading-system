use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use strategy::{build_strategy, ParamSpec, ParamKind};
use edge::catalog::{load_broker_catalog, AssetClass};
use edge::priors::read_priors;
use edge::generator::enumerate_candidates;

pub async fn run(
    strategy: String,
    broker: String,
    symbols: Vec<String>,
    timeframes: Vec<String>,
    asset_class: Option<String>,
    data_dir: PathBuf,
    exclude_holdout_start: Option<String>,
    exclude_holdout_end: Option<String>,
) -> Result<()> {
    // 1. Normalize inputs
    let mut normalized_symbols = Vec::new();
    for sym in symbols {
        for s in sym.split(',') {
            let trimmed = s.trim();
            if !trimmed.is_empty() && trimmed != "..." {
                normalized_symbols.push(trimmed.to_string());
            }
        }
    }

    let mut normalized_timeframes = Vec::new();
    for tf in timeframes {
        for t in tf.split(',') {
            let trimmed = t.trim();
            if !trimmed.is_empty() {
                normalized_timeframes.push(trimmed.to_string());
            }
        }
    }
    if normalized_timeframes.is_empty() {
        normalized_timeframes = vec!["1h".to_string(), "4h".to_string()];
    }

    let target_asset_class = asset_class.as_ref().map(|ac| match ac.to_lowercase().as_str() {
        "forex" => AssetClass::Forex,
        "index" | "indices" => AssetClass::Index,
        "commodity" | "commodities" | "metals" => AssetClass::Commodity,
        "crypto" => AssetClass::Crypto,
        _ => AssetClass::Unknown,
    });

    // 2. Filter symbols by asset class
    let catalog = load_broker_catalog(&broker)
        .with_context(|| format!("failed to load catalog for {}", broker))?;
    
    let mut filtered_symbols = Vec::new();
    let mut asset_classes_present = std::collections::HashSet::new();
    for item in &catalog {
        if !normalized_symbols.is_empty() && !normalized_symbols.contains(&item.symbol) {
            continue;
        }
        if let Some(ref tac) = target_asset_class {
            if &item.asset_class != tac {
                continue;
            }
        }
        filtered_symbols.push(item.symbol.clone());
        asset_classes_present.insert(item.asset_class.clone());
    }

    if filtered_symbols.is_empty() {
        println!("No matching symbols found for broker {} (asset class: {:?})", broker, asset_class);
        return Ok(());
    }

    // 3. Generate templates
    let out_dir = Path::new("configs/templates/autogen");
    let ledger_path = data_dir.join("results/trials_ledger.jsonl");
    let session_id = format!("gen_{}", chrono::Local::now().timestamp());
    let ex_start = exclude_holdout_start.clone().unwrap_or_default();
    let ex_end = exclude_holdout_end.clone().unwrap_or_default();

    println!("Generating templates for strategy '{}'...", strategy);
    let candidates = enumerate_candidates(
        &strategy,
        &broker,
        &filtered_symbols,
        &normalized_timeframes,
        (&ex_start, &ex_end),
        out_dir,
        &ledger_path,
        &session_id,
    )?;

    println!("Generated {} template files in {}", candidates.len(), out_dir.display());

    // 4. Print summary table
    let strategy_obj = build_strategy(&strategy)?;
    let priors = read_priors(&strategy, "docs/strategy");

    // Gather tunable params
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

    let tunable_specs: Vec<&ParamSpec> = full_schema
        .iter()
        .filter(|s| s.kind == ParamKind::Tunable)
        .collect();

    println!("\n╔═════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════╗");
    println!("║                                           TEMPLATE GENERATION SUMMARY                                                   ║");
    println!("╠═════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════╣");
    println!("║ {:<15} | {:<5} | {:<40} | {:<50} ║", "Asset Class", "TF", "Tunable Params", "Grid-Center Source (Param: Source)");
    println!("╠═════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════╣");

    for ac in asset_classes_present {
        // Find a symbol in this asset class to check circular seeding exclusion
        let symbol_in_class = catalog
            .iter()
            .find(|item| item.asset_class == ac)
            .map(|item| item.symbol.as_str())
            .unwrap_or("");

        let mut sources = Vec::new();
        let mut tunable_keys = Vec::new();

        for spec in &tunable_specs {
            tunable_keys.push(spec.key.as_str());

            let mut usable_count = 0;
            if let Some(list) = priors.get(&spec.key) {
                for (_, symbol, date_range) in list {
                    let parts: Vec<&str> = date_range.split(" to ").collect();
                    let (p_start, p_end) = if parts.len() == 2 {
                        (parts[0].trim(), parts[1].trim())
                    } else {
                        ("", "")
                    };
                    let overlaps = symbol == symbol_in_class
                        || ( !ex_start.is_empty() && !ex_end.is_empty() && p_start <= ex_end.as_str() && ex_start.as_str() <= p_end );
                    if !overlaps {
                        usable_count += 1;
                    }
                }
            }

            let source = if usable_count >= 2 { "prior" } else { "default" };
            sources.push(format!("{}: {}", spec.key, source));
        }

        let tunable_params_str = tunable_keys.join(", ");
        let sources_str = sources.join(", ");

        let short_tunable = if tunable_params_str.len() > 40 {
            format!("{}...", &tunable_params_str[..37])
        } else {
            tunable_params_str
        };
        let short_sources = if sources_str.len() > 50 {
            format!("{}...", &sources_str[..47])
        } else {
            sources_str
        };

        for tf in &normalized_timeframes {
            println!(
                "║ {:<15} | {:<5} | {:<40} | {:<50} ║",
                format!("{:?}", ac),
                tf,
                short_tunable,
                short_sources
            );
        }
    }
    println!("╚═════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════╝\n");

    Ok(())
}
