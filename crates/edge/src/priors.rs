use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

pub fn read_priors(strategy: &str, doc_dir: &str) -> HashMap<String, Vec<(f64, String, String)>> {
    let mut priors: HashMap<String, Vec<(f64, String, String)>> = HashMap::new();
    let doc_path = Path::new(doc_dir).join(format!("{}.md", strategy));
    if !doc_path.exists() {
        return priors;
    }

    let Ok(content) = fs::read_to_string(&doc_path) else {
        return priors;
    };

    // Find the section ## Walkforward Parameter Discoveries
    let section_headers = [
        "## Walkforward Parameter Discoveries",
        "## 3. Walkforward Parameter Discoveries",
    ];
    let mut section_idx = None;
    for header in &section_headers {
        if let Some(idx) = content.find(header) {
            section_idx = Some(idx + header.len());
            break;
        }
    }

    let Some(idx) = section_idx else {
        return priors;
    };

    let discoveries_content = &content[idx..];

    // Split discoveries by subheadings "### 3."
    let parts = discoveries_content.split("### 3.");
    for part in parts {
        if part.trim().is_empty() {
            continue;
        }

        // Extract symbol from the header line (first line of the part)
        let Some(first_line) = part.lines().next() else {
            continue;
        };
        let tokens: Vec<&str> = first_line.split_whitespace().collect();
        if tokens.len() < 2 {
            continue;
        }
        // Since we split on "### 3.", tokens might look like: ["1", "BTCUSDT", "1h", "Walk-Forward", ...]
        // So the symbol is at index 1
        let symbol = tokens[1].to_string();

        // Extract date range / holdout period
        // Look for: "Holdout Period: **[start] to [end]**"
        let mut date_range = String::new();
        if let Some(h_idx) = part.find("Holdout Period: **") {
            let remain = &part[h_idx + "Holdout Period: **".len()..];
            if let Some(end_stars) = remain.find("**") {
                date_range = remain[..end_stars].trim().to_string();
            }
        }

        // Extract JSON code block
        let mut json_str = String::new();
        if let Some(block_start) = part.find("```json") {
            let remain = &part[block_start + "```json".len()..];
            if let Some(block_end) = remain.find("```") {
                json_str = remain[..block_end].trim().to_string();
            }
        }

        if json_str.is_empty() {
            continue;
        }

        let Ok(json_val): Result<Value, _> = serde_json::from_str(&json_str) else {
            continue;
        };

        // Flatten the json_val into key-value pairs
        let mut flat_params: Vec<(String, f64)> = Vec::new();

        // 1. stop_manager
        if let Some(sm_val) = json_val.get("stop_manager") {
            let process_sm_obj = |obj: &serde_json::Map<String, Value>,
                                  flat: &mut Vec<(String, f64)>| {
                for key in &["stop_distance", "start_rr", "multiplier"] {
                    if let Some(val) = obj.get(*key).and_then(|v| v.as_f64()) {
                        flat.push((key.to_string(), val));
                    }
                }
            };
            match sm_val {
                Value::Array(arr) => {
                    for item in arr {
                        if let Value::Object(obj) = item {
                            process_sm_obj(obj, &mut flat_params);
                        }
                    }
                }
                Value::Object(obj) => {
                    process_sm_obj(obj, &mut flat_params);
                }
                _ => {}
            }
        }

        // 2. strategy_parameters
        if let Some(Value::Object(sp_obj)) = json_val.get("strategy_parameters") {
            for (key, val) in sp_obj {
                if let Some(v) = val.as_f64() {
                    flat_params.push((key.clone(), v));
                }
            }
        }

        // 3. indicators
        if let Some(Value::Object(ind_obj)) = json_val.get("indicators") {
            for (ind_name, ind_val) in ind_obj {
                if let Value::Object(ind_params) = ind_val {
                    for (param_key, param_val) in ind_params {
                        if let Some(v) = param_val.as_f64() {
                            flat_params.push((format!("{}.{}", ind_name, param_key), v));
                        }
                    }
                }
            }
        }

        // Add to priors HashMap
        for (key, val) in flat_params {
            priors
                .entry(key)
                .or_default()
                .push((val, symbol.clone(), date_range.clone()));
        }
    }

    priors
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::SystemTime;

    fn get_temp_dir() -> std::path::PathBuf {
        let timestamp = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("test_priors_{}", timestamp));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn test_read_priors_missing_file() {
        let dir = get_temp_dir();
        let priors = read_priors("nonexistent", dir.to_str().unwrap());
        assert!(priors.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }
}
