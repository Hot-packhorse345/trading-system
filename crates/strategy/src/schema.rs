use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ParamKind {
    Structural,
    Tunable,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParamSpec {
    pub key: String,
    pub kind: ParamKind,
    pub default: serde_json::Value,
    pub safe_range: Option<(f64, f64)>, // for Tunable numeric params only
}

#[cfg(test)]
mod tests {
    use crate::factory::build_strategy;
    use std::fs;

    #[test]
    fn test_schema_defaults_match_code() {
        let strategies = vec![
            "ema_cross"
        ];
        for name in strategies {
            let strategy = build_strategy(name).unwrap();
            let schema = strategy.param_schema();
            let file_path = format!("src/strategies/{}.rs", name);
            let content = fs::read_to_string(&file_path)
                .or_else(|_| fs::read_to_string(format!("crates/strategy/src/strategies/{}.rs", name)))
                .expect(&format!("Could not read strategy file {}", file_path));

            for spec in schema {
                let key = &spec.key;
                if key.contains('.') {
                    // Indicator period parameter, e.g. "rsi.timeperiod" -> unwrap_or(14)
                    let expected_default = match &spec.default {
                        serde_json::Value::Number(num) => num.as_u64().unwrap(),
                        _ => panic!("Expected indicator period default to be a number"),
                    };
                    let pattern1 = format!("unwrap_or({})", expected_default);
                    assert!(
                        content.contains(&pattern1),
                        "Strategy {} has schema key {} with default {} but code does not contain '{}'",
                        name, key, expected_default, pattern1
                    );
                } else {
                    // Strategy parameter
                    match &spec.default {
                        serde_json::Value::Number(num) => {
                            let val = num.as_f64().unwrap();
                            let pattern1 = format!("f64_or(\"{}\", {})", key, val);
                            let pattern2 = format!("f64_or(\"{}\", {:.1})", key, val);
                            assert!(
                                content.contains(&pattern1) || content.contains(&pattern2),
                                "Strategy {} has schema key {} with default {} but code does not contain '{}' or '{}'",
                                name, key, val, pattern1, pattern2
                            );
                        }
                        serde_json::Value::String(s) => {
                            let pattern = format!("str_or(\"{}\", \"{}\")", key, s);
                            assert!(
                                content.contains(&pattern),
                                "Strategy {} has schema key {} with default '{}' but code does not contain '{}'",
                                name, key, s, pattern
                            );
                        }
                        serde_json::Value::Bool(b) => {
                            let pattern = format!("bool_or(\"{}\", {})", key, b);
                            assert!(
                                content.contains(&pattern),
                                "Strategy {} has schema key {} with default {} but code does not contain '{}'",
                                name, key, b, pattern
                            );
                        }
                        _ => panic!("Unsupported default type in schema test"),
                    }
                }
            }
        }
    }
}
