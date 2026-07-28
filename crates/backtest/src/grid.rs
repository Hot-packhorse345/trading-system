use crate::config::{
    resolve_stop_tf, BacktestConfig, ComboKind, GroupSpec, IndicatorDef, ResolvedCombo,
};
use anyhow::{anyhow, Result};
use risk::config::{ExitManagerConfig, StopManagerConfig};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use tracing::warn;
use ts_core::parse_timeframe;

// ═══════════════════════════════════════════════════════════════════════════════
// LAYER 1 — Space trait and combinators
// ═══════════════════════════════════════════════════════════════════════════════

pub(crate) trait Space: Send + Sync {
    fn iter(&self) -> Box<dyn Iterator<Item = HashMap<String, Value>> + '_>;
    fn count(&self) -> usize;
}

// ── 1. ChoiceSpace ────────────────────────────────────────────────────────────

struct ChoiceSpace {
    key: String,
    values: Vec<Value>,
}

impl Space for ChoiceSpace {
    fn iter(&self) -> Box<dyn Iterator<Item = HashMap<String, Value>> + '_> {
        let key = self.key.clone();
        Box::new(self.values.iter().map(move |v| {
            let mut m = HashMap::new();
            m.insert(key.clone(), v.clone());
            m
        }))
    }
    fn count(&self) -> usize {
        self.values.len()
    }
}

// ── 2. RangeSpace ─────────────────────────────────────────────────────────────

struct RangeSpace {
    key: String,
    start: f64,
    stop: f64,
    step: f64,
}

impl Space for RangeSpace {
    fn iter(&self) -> Box<dyn Iterator<Item = HashMap<String, Value>> + '_> {
        let vals = range_values(self.start, self.stop, self.step);
        let key = self.key.clone();
        Box::new(vals.into_iter().map(move |f| {
            let mut m = HashMap::new();
            m.insert(key.clone(), f64_val(f));
            m
        }))
    }
    fn count(&self) -> usize {
        range_values(self.start, self.stop, self.step).len()
    }
}

fn range_values(start: f64, stop: f64, step: f64) -> Vec<f64> {
    let mut vals = Vec::new();
    let mut v = start;
    while v < stop {
        vals.push(v);
        v += step;
    }
    vals
}

// ── 3. LogRangeSpace ──────────────────────────────────────────────────────────

struct LogRangeSpace {
    key: String,
    start: f64,
    stop: f64,
    n: usize,
}

impl Space for LogRangeSpace {
    fn iter(&self) -> Box<dyn Iterator<Item = HashMap<String, Value>> + '_> {
        let n = self.n;
        let ln_start = self.start.ln();
        let ln_stop = self.stop.ln();
        let vals: Vec<f64> = (0..n)
            .map(|i| {
                let t = i as f64 / (n - 1) as f64;
                (ln_start + t * (ln_stop - ln_start)).exp()
            })
            .filter(|f| f.is_finite())
            .collect();
        let key = self.key.clone();
        Box::new(vals.into_iter().map(move |f| {
            let mut m = HashMap::new();
            m.insert(key.clone(), f64_val(f));
            m
        }))
    }
    fn count(&self) -> usize {
        self.n
    }
}

// ── 4. LinspaceSpace ─────────────────────────────────────────────────────────

struct LinspaceSpace {
    key: String,
    start: f64,
    stop: f64,
    n: usize,
}

impl Space for LinspaceSpace {
    fn iter(&self) -> Box<dyn Iterator<Item = HashMap<String, Value>> + '_> {
        let n = self.n;
        let start = self.start;
        let stop = self.stop;
        let vals: Vec<f64> = (0..n)
            .map(|i| start + i as f64 * (stop - start) / (n - 1) as f64)
            .collect();
        let key = self.key.clone();
        Box::new(vals.into_iter().map(move |f| {
            let mut m = HashMap::new();
            m.insert(key.clone(), f64_val(f));
            m
        }))
    }
    fn count(&self) -> usize {
        self.n
    }
}

// ── 5. FixedSpace ────────────────────────────────────────────────────────────

struct FixedSpace {
    key: String,
    value: Value,
}

impl Space for FixedSpace {
    fn iter(&self) -> Box<dyn Iterator<Item = HashMap<String, Value>> + '_> {
        let mut m = HashMap::new();
        m.insert(self.key.clone(), self.value.clone());
        Box::new(std::iter::once(m))
    }
    fn count(&self) -> usize {
        1
    }
}

// ── 6. GroupSpace ─────────────────────────────────────────────────────────────

struct GroupSpace {
    name: String,
    spaces: Vec<Box<dyn Space>>,
}

impl Space for GroupSpace {
    fn iter(&self) -> Box<dyn Iterator<Item = HashMap<String, Value>> + '_> {
        if self.spaces.is_empty() {
            return Box::new(std::iter::once(HashMap::new()));
        }
        let n = self.spaces[0].count();
        for s in &self.spaces {
            assert_eq!(
                s.count(),
                n,
                "GroupSpace: all children must have equal count"
            );
        }
        let rows: Vec<Vec<HashMap<String, Value>>> =
            self.spaces.iter().map(|s| s.iter().collect()).collect();
        let name = self.name.clone();
        let merged: Vec<HashMap<String, Value>> = (0..n)
            .map(move |i| {
                let mut map = rows
                    .iter()
                    .fold(HashMap::new(), |acc, row| merge_maps(acc, row[i].clone()));
                map.insert(format!("_group_idx:{}", name), Value::Number(i.into()));
                map
            })
            .collect();
        Box::new(merged.into_iter())
    }
    fn count(&self) -> usize {
        if self.spaces.is_empty() {
            return 1;
        }
        let n = self.spaces[0].count();
        for s in &self.spaces {
            assert_eq!(
                s.count(),
                n,
                "GroupSpace: all children must have equal count"
            );
        }
        n
    }
}

// ── 7. CrossSpace ────────────────────────────────────────────────────────────

struct CrossSpace {
    spaces: Vec<Box<dyn Space>>,
}

impl Space for CrossSpace {
    fn iter(&self) -> Box<dyn Iterator<Item = HashMap<String, Value>> + '_> {
        if self.spaces.is_empty() {
            return Box::new(std::iter::once(HashMap::new()));
        }
        let children: Vec<Vec<HashMap<String, Value>>> =
            self.spaces.iter().map(|s| s.iter().collect()).collect();
        let lens: Vec<usize> = children.iter().map(|c| c.len()).collect();
        let combos: Vec<HashMap<String, Value>> = cartesian_indices(&lens)
            .map(|indices| {
                indices
                    .iter()
                    .enumerate()
                    .fold(HashMap::new(), |acc, (dim, &idx)| {
                        merge_maps(acc, children[dim][idx].clone())
                    })
            })
            .collect();
        Box::new(combos.into_iter())
    }
    fn count(&self) -> usize {
        if self.spaces.is_empty() {
            return 1;
        }
        self.spaces.iter().map(|s| s.count()).product()
    }
}

// ── NestedSpace (private, only constructed in build_space_from_map) ───────────

struct NestedSpace {
    key: String,
    inner: Box<dyn Space>,
}

impl Space for NestedSpace {
    fn iter(&self) -> Box<dyn Iterator<Item = HashMap<String, Value>> + '_> {
        let key = self.key.clone();
        let combos: Vec<HashMap<String, Value>> = self.inner.iter().collect();
        Box::new(combos.into_iter().map(move |combo| {
            let obj = Value::Object(combo.into_iter().collect());
            let mut m = HashMap::new();
            m.insert(key.clone(), obj);
            m
        }))
    }
    fn count(&self) -> usize {
        self.inner.count()
    }
}

// ── Private helpers ───────────────────────────────────────────────────────────

fn merge_maps(mut a: HashMap<String, Value>, b: HashMap<String, Value>) -> HashMap<String, Value> {
    for (k, v) in b {
        a.insert(k, v);
    }
    a
}

fn cartesian_indices(lens: &[usize]) -> impl Iterator<Item = Vec<usize>> {
    let total: usize = if lens.is_empty() {
        1
    } else {
        lens.iter().product()
    };
    let n = lens.len();
    let lens = lens.to_vec();
    (0..total).map(move |mut i| {
        if n == 0 {
            return vec![];
        }
        let mut indices = vec![0usize; n];
        for dim in (0..n).rev() {
            indices[dim] = i % lens[dim];
            i /= lens[dim];
        }
        indices
    })
}

fn f64_val(f: f64) -> Value {
    serde_json::Number::from_f64(f)
        .map(Value::Number)
        .unwrap_or(Value::Null)
}

// ── Constructors ──────────────────────────────────────────────────────────────

pub(crate) fn choice(key: impl Into<String>, values: Vec<Value>) -> Box<dyn Space> {
    Box::new(ChoiceSpace {
        key: key.into(),
        values,
    })
}

pub(crate) fn range_space(
    key: impl Into<String>,
    start: f64,
    stop: f64,
    step: f64,
) -> Box<dyn Space> {
    assert!(step > 0.0, "range_space: step must be > 0.0");
    Box::new(RangeSpace {
        key: key.into(),
        start,
        stop,
        step,
    })
}

pub(crate) fn log_range(key: impl Into<String>, start: f64, stop: f64, n: usize) -> Box<dyn Space> {
    assert!(
        start > 0.0 && stop > 0.0,
        "log_range: start and stop must be > 0.0"
    );
    assert!(n >= 2, "log_range: n must be >= 2");
    Box::new(LogRangeSpace {
        key: key.into(),
        start,
        stop,
        n,
    })
}

pub(crate) fn linspace(key: impl Into<String>, start: f64, stop: f64, n: usize) -> Box<dyn Space> {
    assert!(n >= 2, "linspace: n must be >= 2");
    Box::new(LinspaceSpace {
        key: key.into(),
        start,
        stop,
        n,
    })
}

pub(crate) fn fixed(key: impl Into<String>, value: Value) -> Box<dyn Space> {
    Box::new(FixedSpace {
        key: key.into(),
        value,
    })
}

pub(crate) fn group(name: impl Into<String>, spaces: Vec<Box<dyn Space>>) -> Box<dyn Space> {
    let name = name.into();
    if !spaces.is_empty() {
        let n = spaces[0].count();
        for s in &spaces {
            assert_eq!(s.count(), n, "group: all children must have equal count");
        }
    }
    Box::new(GroupSpace { name, spaces })
}

pub(crate) fn cross(spaces: Vec<Box<dyn Space>>) -> Box<dyn Space> {
    Box::new(CrossSpace { spaces })
}

// ═══════════════════════════════════════════════════════════════════════════════
// LAYER 2 — JSON config → Space → ResolvedCombo
// ═══════════════════════════════════════════════════════════════════════════════

pub fn generate_combos(cfg: &BacktestConfig) -> Result<Vec<ResolvedCombo>> {
    let (start, end) = cfg.date_range()?;

    let trading_hours = if let Some(ref session) = cfg.trading_session {
        Some(crate::config::parse_trading_session(session)?)
    } else {
        None
    };

    let strategy_space = build_space_from_map(&cfg.strategy_parameters)?;
    let indicator_combos = build_indicator_space(&cfg.indicators)?;
    let sm_combos = expand_stop_managers(&cfg.stop_manager)?;

    let symbols = value_to_list(&cfg.symbol);
    let timeframes = value_to_list(&cfg.timeframe);
    let stop_tfs = cfg
        .stop_timeframe
        .as_ref()
        .map(value_to_list)
        .unwrap_or_else(|| vec![Value::String("timeframe".into())]);

    let mut combos: Vec<ResolvedCombo> = Vec::new();
    let mut seen_signal_hashes: std::collections::HashSet<String> =
        std::collections::HashSet::new();

    for sym in &symbols {
        let symbol = json_str(sym);
        for tf_val in &timeframes {
            let tf_str = json_str(tf_val);
            let tf = match parse_timeframe(&tf_str) {
                Ok(t) => t,
                Err(e) => {
                    warn!("{e}");
                    continue;
                }
            };
            for stop_tf_val in &stop_tfs {
                let stop_tf = resolve_stop_tf(stop_tf_val, tf).unwrap_or(tf);
                let stop_tf_str = format!("{:?}", stop_tf);
                for mut sp in strategy_space.iter() {
                    sp.retain(|k, _| !k.starts_with("_group_idx:"));
                    for ind_list in &indicator_combos {
                        let sh = signal_hash(
                            &cfg.strategy,
                            &symbol,
                            &tf_str,
                            &stop_tf_str,
                            &sp,
                            ind_list,
                        );
                        let kind = if seen_signal_hashes.insert(sh.clone()) {
                            ComboKind::TypeA
                        } else {
                            ComboKind::TypeB
                        };
                        for sm in &sm_combos {
                            combos.push(ResolvedCombo {
                                strategy_name: cfg.strategy.clone(),
                                symbol: symbol.clone(),
                                timeframe: tf,
                                stop_timeframe: stop_tf,
                                pyramiding: cfg.pyramiding,
                                start,
                                end,
                                trading_hours: trading_hours.clone(),
                                initial_balance: cfg.initial_balance,
                                risk_percentage: cfg.risk_percentage,
                                commission_pct: cfg.commission_pct(),
                                commission_per_lot: cfg.commission_per_lot_val(),
                                swap: cfg.swap.clone(),
                                strategy_params: sp.clone(),
                                indicators: ind_list.clone(),
                                stop_manager: sm.clone(),
                                signal_hash: sh.clone(),
                                combo_hash: combo_hash(&sh, sm),
                                combo_kind: kind.clone(),
                            });
                        }
                    }
                }
            }
        }
    }
    Ok(combos)
}

/// Like `generate_combos` but produces one `GroupSpec` per unique signal
/// configuration instead of one `ResolvedCombo` per (signal × stop_manager)
/// pair.  All stop-manager variants for a signal share the same `GroupSpec`,
/// so shared data (strategy_params, indicators, symbol, …) is allocated only
/// once rather than N_stop_managers times.
pub fn generate_group_specs(cfg: &BacktestConfig) -> Result<Vec<GroupSpec>> {
    let (start, end) = cfg.date_range()?;

    let trading_hours = if let Some(ref session) = cfg.trading_session {
        Some(crate::config::parse_trading_session(session)?)
    } else {
        None
    };

    let strategy_space = build_space_from_map(&cfg.strategy_parameters)?;
    let indicator_combos = build_indicator_space(&cfg.indicators)?;
    let sm_combos = expand_stop_managers(&cfg.stop_manager)?;

    let exit_managers: Vec<ExitManagerConfig> = if cfg.exit_rules.is_empty() {
        Vec::new()
    } else {
        let rules = Value::Array(cfg.exit_rules.clone());
        serde_json::from_value(rules).unwrap_or_else(|e| {
            warn!("invalid exit_rules in config: {e}");
            Vec::new()
        })
    };

    let symbols = value_to_list(&cfg.symbol);
    let timeframes = value_to_list(&cfg.timeframe);
    let stop_tfs = cfg
        .stop_timeframe
        .as_ref()
        .map(value_to_list)
        .unwrap_or_else(|| vec![Value::String("timeframe".into())]);

    let mut groups: Vec<GroupSpec> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    for sym in &symbols {
        let symbol = json_str(sym);
        for tf_val in &timeframes {
            let tf_str = json_str(tf_val);
            let tf = match parse_timeframe(&tf_str) {
                Ok(t) => t,
                Err(e) => {
                    warn!("{e}");
                    continue;
                }
            };
            for stop_tf_val in &stop_tfs {
                let stop_tf = resolve_stop_tf(stop_tf_val, tf).unwrap_or(tf);
                let stop_tf_str = format!("{:?}", stop_tf);
                for mut sp in strategy_space.iter() {
                    sp.retain(|k, _| !k.starts_with("_group_idx:"));
                    for ind_list in &indicator_combos {
                        let sh = signal_hash(
                            &cfg.strategy,
                            &symbol,
                            &tf_str,
                            &stop_tf_str,
                            &sp,
                            ind_list,
                        );
                        let kind = if seen.insert(sh.clone()) {
                            ComboKind::TypeA
                        } else {
                            ComboKind::TypeB
                        };
                        groups.push(GroupSpec {
                            strategy_name: cfg.strategy.clone(),
                            symbol: symbol.clone(),
                            timeframe: tf,
                            stop_timeframe: stop_tf,
                            pyramiding: cfg.pyramiding,
                            start,
                            end,
                            trading_hours: trading_hours.clone(),
                            initial_balance: cfg.initial_balance,
                            risk_percentage: cfg.risk_percentage,
                            commission_pct: cfg.commission_pct(),
                            commission_per_lot: cfg.commission_per_lot_val(),
                            swap: cfg.swap.clone(),
                            strategy_params: sp.clone(),
                            indicators: ind_list.clone(),
                            signal_hash: sh,
                            combo_kind: kind,
                            stop_managers: sm_combos.clone(),
                            exit_managers: exit_managers.clone(),
                        });
                    }
                }
            }
        }
    }
    Ok(groups)
}

/// Compute the per-combo hash from a signal hash and a stop-manager definition.
/// Mirrors the private `combo_hash` used inside `generate_combos`.
pub fn combo_hash_for(signal_hash: &str, sm: &StopManagerConfig) -> String {
    combo_hash(signal_hash, sm)
}

pub fn count_combos(cfg: &BacktestConfig) -> Result<usize> {
    let strategy_count = build_space_from_map(&cfg.strategy_parameters)?.count();
    let indicator_count = build_indicator_space(&cfg.indicators)?.len();
    let sm_count = expand_stop_managers(&cfg.stop_manager)?.len();
    let symbols = value_to_list(&cfg.symbol).len();
    let timeframes = value_to_list(&cfg.timeframe).len();
    let stop_tfs = cfg
        .stop_timeframe
        .as_ref()
        .map(|v| value_to_list(v).len())
        .unwrap_or(1);
    Ok(symbols * timeframes * stop_tfs * strategy_count * indicator_count * sm_count)
}

// ── JSON parser ───────────────────────────────────────────────────────────────

fn build_space_from_map(map: &HashMap<String, Value>) -> Result<Box<dyn Space>> {
    let mut children: Vec<Box<dyn Space>> = Vec::new();
    let mut pending_groups: HashMap<String, Vec<(String, Vec<Value>)>> = HashMap::new();

    for (raw_key, raw_value) in map {
        // a. '#' prefix → FixedSpace (value kept as literal)
        if let Some(stripped) = raw_key.strip_prefix('#') {
            children.push(fixed(stripped, raw_value.clone()));
            continue;
        }

        // b. '[group]key' pattern → collect into pending group
        if let Some((group_name, key)) = parse_group_key(raw_key) {
            let values = match raw_value {
                Value::Array(arr) => arr.clone(),
                _ => return Err(anyhow!("group member '{}' must be a JSON array", raw_key)),
            };
            pending_groups
                .entry(group_name)
                .or_default()
                .push((key, values));
            continue;
        }

        // c. Object with "$sample" → sampler
        if let Value::Object(obj) = raw_value {
            if obj.contains_key("$sample") {
                children.push(resolve_sampler(raw_key, obj)?);
                continue;
            }
            // e. Non-empty Object without "$sample" → nested
            if !obj.is_empty() {
                let sub_map: HashMap<String, Value> =
                    obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                let sub_space = build_space_from_map(&sub_map)?;
                children.push(Box::new(NestedSpace {
                    key: raw_key.clone(),
                    inner: sub_space,
                }));
                continue;
            }
            // empty object → fall through to f
        }

        // d. Non-empty Array → expand elements that are objects
        if let Value::Array(arr) = raw_value {
            if !arr.is_empty() {
                let mut expanded: Vec<Value> = Vec::new();
                for elem in arr {
                    if let Value::Object(sub_obj) = elem {
                        let sub_map: HashMap<String, Value> = sub_obj
                            .iter()
                            .map(|(k, v)| (k.clone(), v.clone()))
                            .collect();
                        let sub_space = build_space_from_map(&sub_map)?;
                        for combo in sub_space.iter() {
                            expanded.push(Value::Object(combo.into_iter().collect()));
                        }
                    } else {
                        expanded.push(elem.clone());
                    }
                }
                children.push(choice(raw_key, expanded));
                continue;
            }
            // empty array → fall through to f
        }

        // f. Everything else → FixedSpace
        children.push(fixed(raw_key, raw_value.clone()));
    }

    // Emit one GroupSpace per group
    for (group_name, members) in pending_groups {
        let expected_len = members[0].1.len();
        for (key, values) in &members {
            if values.len() != expected_len {
                return Err(anyhow!(
                    "group '{}': member '{}' has {} values but expected {}",
                    group_name,
                    key,
                    values.len(),
                    expected_len
                ));
            }
        }
        let group_children: Vec<Box<dyn Space>> = members
            .into_iter()
            .map(|(key, values)| choice(key, values))
            .collect();
        children.push(group(group_name, group_children));
    }

    Ok(cross(children))
}

fn resolve_sampler(key: &str, obj: &serde_json::Map<String, Value>) -> Result<Box<dyn Space>> {
    let sample_type = obj
        .get("$sample")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("$sample must be a string"))?;

    match sample_type {
        "range" => {
            let start = obj
                .get("start")
                .and_then(|v| v.as_f64())
                .ok_or_else(|| anyhow!("$sample range: missing or invalid 'start'"))?;
            let stop = obj
                .get("stop")
                .and_then(|v| v.as_f64())
                .ok_or_else(|| anyhow!("$sample range: missing or invalid 'stop'"))?;
            let step = obj
                .get("step")
                .and_then(|v| v.as_f64())
                .ok_or_else(|| anyhow!("$sample range: missing or invalid 'step'"))?;
            if step <= 0.0 {
                return Err(anyhow!("$sample range: step must be > 0"));
            }
            Ok(range_space(key, start, stop, step))
        }
        "log" => {
            let start = obj
                .get("start")
                .and_then(|v| v.as_f64())
                .ok_or_else(|| anyhow!("$sample log: missing or invalid 'start'"))?;
            let stop = obj
                .get("stop")
                .and_then(|v| v.as_f64())
                .ok_or_else(|| anyhow!("$sample log: missing or invalid 'stop'"))?;
            let n = obj
                .get("n")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| anyhow!("$sample log: missing or invalid 'n'"))?
                as usize;
            if start <= 0.0 || stop <= 0.0 {
                return Err(anyhow!("$sample log: start and stop must be > 0"));
            }
            if n < 2 {
                return Err(anyhow!("$sample log: n must be >= 2"));
            }
            Ok(log_range(key, start, stop, n))
        }
        "linspace" => {
            let start = obj
                .get("start")
                .and_then(|v| v.as_f64())
                .ok_or_else(|| anyhow!("$sample linspace: missing or invalid 'start'"))?;
            let stop = obj
                .get("stop")
                .and_then(|v| v.as_f64())
                .ok_or_else(|| anyhow!("$sample linspace: missing or invalid 'stop'"))?;
            let n = obj
                .get("n")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| anyhow!("$sample linspace: missing or invalid 'n'"))?
                as usize;
            if n < 2 {
                return Err(anyhow!("$sample linspace: n must be >= 2"));
            }
            Ok(linspace(key, start, stop, n))
        }
        "values" => {
            let items = obj
                .get("items")
                .and_then(|v| v.as_array())
                .ok_or_else(|| anyhow!("$sample values: missing or invalid 'items'"))?;
            if items.is_empty() {
                return Err(anyhow!("$sample values: 'items' must be non-empty"));
            }
            Ok(choice(key, items.clone()))
        }
        s => Err(anyhow!("unknown $sample type: {s}")),
    }
}

// ── Indicator expansion ───────────────────────────────────────────────────────

fn build_indicator_space(indicators: &HashMap<String, Value>) -> Result<Vec<Vec<IndicatorDef>>> {
    let per_indicator: Vec<Vec<IndicatorDef>> = indicators
        .iter()
        .map(|(name, def_val)| -> Result<Vec<IndicatorDef>> {
            let obj = match def_val.as_object() {
                Some(o) => o,
                None => return Ok(vec![]),
            };
            let (ind_type, timeframe, params_map) = indicators::split_indicator_def(obj);
            let ind_type = ind_type.unwrap_or_default();
            let space = build_space_from_map(&params_map)?;
            let combos: Vec<IndicatorDef> = space
                .iter()
                .map(|mut params| {
                    let resolved_tf = params
                        .remove("timeframe")
                        .and_then(|v| v.as_str().map(|s| s.to_string()))
                        .or_else(|| timeframe.clone());
                    IndicatorDef {
                        name: name.clone(),
                        ind_type: ind_type.clone(),
                        timeframe: resolved_tf,
                        params,
                    }
                })
                .collect();
            Ok(combos)
        })
        .collect::<Result<_>>()?;

    if per_indicator.is_empty() {
        return Ok(vec![vec![]]);
    }

    let mut result: Vec<Vec<IndicatorDef>> = vec![vec![]];
    for ind_combos in per_indicator {
        if ind_combos.is_empty() {
            continue;
        }
        result = result
            .into_iter()
            .flat_map(|existing| {
                ind_combos.iter().filter_map(move |ind| {
                    for ext_ind in &existing {
                        for (k, v1) in &ind.params {
                            if k.starts_with("_group_idx:") {
                                if let Some(v2) = ext_ind.params.get(k) {
                                    if v1 != v2 {
                                        return None;
                                    }
                                }
                            }
                        }
                    }
                    let mut r = existing.clone();
                    r.push(ind.clone());
                    Some(r)
                })
            })
            .collect();
    }

    // Strip group indices metadata from final indicator definitions
    for combo in &mut result {
        for ind in combo {
            ind.params.retain(|k, _| !k.starts_with("_group_idx:"));
        }
    }

    Ok(result)
}

// ── Stop-manager expansion ────────────────────────────────────────────────────

fn expand_stop_managers(val: &Value) -> Result<Vec<StopManagerConfig>> {
    let defs = match val {
        Value::Array(arr) => arr.clone(),
        Value::Object(_) => vec![val.clone()],
        _ => return Err(anyhow!("stop_manager must be an object or array")),
    };

    let mut result = Vec::new();
    for def in &defs {
        let obj = def
            .as_object()
            .ok_or_else(|| anyhow!("stop_manager entry must be an object"))?;
        let sm_type = obj
            .get("type")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("stop_manager missing 'type'"))?
            .to_string();
        let distances = value_to_f64_list(obj.get("stop_distance").unwrap_or(&Value::Null), 1.0);
        let start_rrs = value_to_f64_list(obj.get("start_rr").unwrap_or(&Value::Null), 0.0);

        for &dist in &distances {
            match sm_type.as_str() {
                "variant3" | "variant4" | "breakeven" => {
                    for &rr in &start_rrs {
                        result.push(StopManagerConfig {
                            sm_type: sm_type.clone(),
                            stop_distance: dist,
                            start_rr: rr,
                        });
                    }
                }
                _ => {
                    result.push(StopManagerConfig {
                        sm_type: sm_type.clone(),
                        stop_distance: dist,
                        start_rr: 0.0,
                    });
                }
            }
        }
    }
    Ok(result)
}

// ── Hashing ───────────────────────────────────────────────────────────────────

pub fn canonical_value(v: &Value) -> String {
    match v {
        Value::Object(map) => {
            let mut pairs: Vec<(&String, &Value)> = map.iter().collect();
            pairs.sort_by_key(|(k, _)| k.as_str());
            let inner = pairs
                .iter()
                .map(|(k, v)| format!("{}:{}", k, canonical_value(v)))
                .collect::<Vec<_>>()
                .join(",");
            format!("{{{}}}", inner)
        }
        Value::Array(arr) => {
            let inner = arr
                .iter()
                .map(canonical_value)
                .collect::<Vec<_>>()
                .join(",");
            format!("[{}]", inner)
        }
        Value::Number(n) => {
            if let Some(f) = n.as_f64() {
                format!("{:.8}", f)
            } else {
                v.to_string()
            }
        }
        _ => v.to_string(),
    }
}

pub fn expand_sample_values(obj: &serde_json::Map<String, Value>) -> Result<Vec<Value>> {
    let sample_type = obj
        .get("$sample")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("$sample must be a string"))?;
    match sample_type {
        "range" => {
            let start = obj
                .get("start")
                .and_then(|v| v.as_f64())
                .ok_or_else(|| anyhow!("$sample range: missing or invalid 'start'"))?;
            let stop = obj
                .get("stop")
                .and_then(|v| v.as_f64())
                .ok_or_else(|| anyhow!("$sample range: missing or invalid 'stop'"))?;
            let step = obj
                .get("step")
                .and_then(|v| v.as_f64())
                .ok_or_else(|| anyhow!("$sample range: missing or invalid 'step'"))?;
            if step <= 0.0 {
                return Err(anyhow!("$sample range: step must be > 0"));
            }
            let vals = range_values(start, stop, step);
            Ok(vals.into_iter().map(f64_val).collect())
        }
        "log" => {
            let start = obj
                .get("start")
                .and_then(|v| v.as_f64())
                .ok_or_else(|| anyhow!("$sample log: missing or invalid 'start'"))?;
            let stop = obj
                .get("stop")
                .and_then(|v| v.as_f64())
                .ok_or_else(|| anyhow!("$sample log: missing or invalid 'stop'"))?;
            let n = obj
                .get("n")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| anyhow!("$sample log: missing or invalid 'n'"))?
                as usize;
            if start <= 0.0 || stop <= 0.0 {
                return Err(anyhow!("$sample log: start and stop must be > 0"));
            }
            if n < 2 {
                return Err(anyhow!("$sample log: n must be >= 2"));
            }
            let ln_start = start.ln();
            let ln_stop = stop.ln();
            let vals: Vec<f64> = (0..n)
                .map(|i| {
                    let t = i as f64 / (n - 1) as f64;
                    (ln_start + t * (ln_stop - ln_start)).exp()
                })
                .filter(|f| f.is_finite())
                .collect();
            Ok(vals.into_iter().map(f64_val).collect())
        }
        "linspace" => {
            let start = obj
                .get("start")
                .and_then(|v| v.as_f64())
                .ok_or_else(|| anyhow!("$sample linspace: missing or invalid 'start'"))?;
            let stop = obj
                .get("stop")
                .and_then(|v| v.as_f64())
                .ok_or_else(|| anyhow!("$sample linspace: missing or invalid 'stop'"))?;
            let n = obj
                .get("n")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| anyhow!("$sample linspace: missing or invalid 'n'"))?
                as usize;
            if n < 2 {
                return Err(anyhow!("$sample linspace: n must be >= 2"));
            }
            let vals: Vec<f64> = (0..n)
                .map(|i| start + i as f64 * (stop - start) / (n - 1) as f64)
                .collect();
            Ok(vals.into_iter().map(f64_val).collect())
        }
        "values" => {
            let items = obj
                .get("items")
                .and_then(|v| v.as_array())
                .ok_or_else(|| anyhow!("$sample values: missing or invalid 'items'"))?;
            if items.is_empty() {
                return Err(anyhow!("$sample values: 'items' must be non-empty"));
            }
            Ok(items.clone())
        }
        s => Err(anyhow!("unknown $sample type: {s}")),
    }
}

fn signal_hash(
    strategy: &str,
    symbol: &str,
    tf: &str,
    stop_tf: &str,
    params: &HashMap<String, Value>,
    inds: &[IndicatorDef],
) -> String {
    let mut h = Sha256::new();
    h.update(strategy.as_bytes());
    h.update(symbol.as_bytes());
    h.update(tf.as_bytes());
    h.update(stop_tf.as_bytes());
    let mut kv: Vec<_> = params.iter().collect();
    kv.sort_by_key(|(k, _)| k.as_str());
    for (k, v) in kv {
        h.update(k.as_bytes());
        h.update(canonical_value(v).as_bytes());
    }
    let mut ind_list: Vec<_> = inds.iter().collect();
    ind_list.sort_by_key(|d| d.name.as_str());
    for d in ind_list {
        h.update(d.name.as_bytes());
        h.update(d.ind_type.as_bytes());
        let mut kv2: Vec<_> = d.params.iter().collect();
        kv2.sort_by_key(|(k, _)| k.as_str());
        for (k, v) in kv2 {
            h.update(k.as_bytes());
            h.update(canonical_value(v).as_bytes());
        }
    }
    format!("{:x}", h.finalize())
}

fn combo_hash(sh: &str, sm: &StopManagerConfig) -> String {
    let mut h = Sha256::new();
    h.update(sh.as_bytes());
    h.update(sm.sm_type.as_bytes());
    h.update(format!("{:.8}", sm.stop_distance).as_bytes());
    h.update(format!("{:.8}", sm.start_rr).as_bytes());
    format!("{:x}", h.finalize())
}

// ── Private utilities ─────────────────────────────────────────────────────────

fn parse_group_key(raw: &str) -> Option<(String, String)> {
    if raw.starts_with('[') {
        if let Some(end) = raw.find(']') {
            let group = raw[1..end].to_string();
            let key = raw[end + 1..].to_string();
            if !group.is_empty() && !key.is_empty() {
                return Some((group, key));
            }
        }
    }
    None
}

fn value_to_list(v: &Value) -> Vec<Value> {
    match v {
        Value::Array(arr) => arr.clone(),
        other => vec![other.clone()],
    }
}

fn value_to_f64_list(v: &Value, default: f64) -> Vec<f64> {
    match v {
        Value::Array(arr) => arr.iter().filter_map(|x| x.as_f64()).collect(),
        Value::Number(n) => n.as_f64().map(|f| vec![f]).unwrap_or_else(|| vec![default]),
        Value::Object(obj) if obj.contains_key("$sample") => {
            expand_sample_to_f64(obj).unwrap_or_else(|_| vec![default])
        }
        _ => vec![default],
    }
}

fn expand_sample_to_f64(obj: &serde_json::Map<String, Value>) -> Result<Vec<f64>> {
    let sample_type = obj
        .get("$sample")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("$sample must be a string"))?;
    match sample_type {
        "range" => {
            let start = obj
                .get("start")
                .and_then(|v| v.as_f64())
                .ok_or_else(|| anyhow!("$sample range: missing 'start'"))?;
            let stop = obj
                .get("stop")
                .and_then(|v| v.as_f64())
                .ok_or_else(|| anyhow!("$sample range: missing 'stop'"))?;
            let step = obj
                .get("step")
                .and_then(|v| v.as_f64())
                .ok_or_else(|| anyhow!("$sample range: missing 'step'"))?;
            if step <= 0.0 {
                return Err(anyhow!("$sample range: step must be > 0"));
            }
            let mut vals = Vec::new();
            let mut v = start;
            while v < stop {
                vals.push(v);
                v += step;
            }
            Ok(vals)
        }
        "linspace" => {
            let start = obj
                .get("start")
                .and_then(|v| v.as_f64())
                .ok_or_else(|| anyhow!("$sample linspace: missing 'start'"))?;
            let stop = obj
                .get("stop")
                .and_then(|v| v.as_f64())
                .ok_or_else(|| anyhow!("$sample linspace: missing 'stop'"))?;
            let n = obj
                .get("n")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| anyhow!("$sample linspace: missing 'n'"))?
                as usize;
            if n < 2 {
                return Err(anyhow!("$sample linspace: n must be >= 2"));
            }
            Ok((0..n)
                .map(|i| start + i as f64 * (stop - start) / (n - 1) as f64)
                .collect())
        }
        "values" => {
            let items = obj
                .get("items")
                .and_then(|v| v.as_array())
                .ok_or_else(|| anyhow!("$sample values: missing 'items'"))?;
            Ok(items.iter().filter_map(|v| v.as_f64()).collect())
        }
        s => Err(anyhow!("unsupported $sample type for f64 list: {s}")),
    }
}

fn json_str(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}
