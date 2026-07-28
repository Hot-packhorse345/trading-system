use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Params(pub HashMap<String, Value>);

impl Params {
    pub fn f64(&self, key: &str) -> Option<f64> {
        self.0.get(key)?.as_f64()
    }
    pub fn f64_or(&self, key: &str, d: f64) -> f64 {
        self.f64(key).unwrap_or(d)
    }
    pub fn u64(&self, key: &str) -> Option<u64> {
        self.0.get(key)?.as_u64()
    }
    pub fn u64_or(&self, key: &str, d: u64) -> u64 {
        self.u64(key).unwrap_or(d)
    }
    pub fn bool_or(&self, key: &str, d: bool) -> bool {
        self.0.get(key).and_then(|v| v.as_bool()).unwrap_or(d)
    }
    pub fn str_or<'a>(&'a self, key: &str, d: &'a str) -> &'a str {
        self.0.get(key).and_then(|v| v.as_str()).unwrap_or(d)
    }
}

#[derive(Debug, Clone, Default)]
pub struct IndicatorSet {
    columns: HashMap<String, Vec<f64>>,
}

impl IndicatorSet {
    pub fn insert(&mut self, name: impl Into<String>, values: Vec<f64>) {
        self.columns.insert(name.into(), values);
    }

    pub fn get(&self, name: &str) -> &[f64] {
        self.columns
            .get(name)
            .unwrap_or_else(|| panic!("IndicatorSet: missing column '{name}'"))
            .as_slice()
    }

    pub fn try_get(&self, name: &str) -> Option<&[f64]> {
        self.columns.get(name).map(|v| v.as_slice())
    }

    pub fn contains(&self, name: &str) -> bool {
        self.columns.contains_key(name)
    }

    pub fn column_names(&self) -> impl Iterator<Item = &str> {
        self.columns.keys().map(String::as_str)
    }

    pub fn into_columns(self) -> HashMap<String, Vec<f64>> {
        self.columns
    }

    pub fn slice(&self, range: std::ops::Range<usize>) -> Self {
        let mut sliced = HashMap::new();
        for (k, v) in &self.columns {
            let start = range.start.min(v.len());
            let end = range.end.min(v.len());
            sliced.insert(k.clone(), v[start..end].to_vec());
        }
        Self { columns: sliced }
    }
}
