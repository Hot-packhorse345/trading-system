use anyhow::{anyhow, Result};
use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveDateTime, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{mpsc, watch, Mutex};
use tracing::{debug, error, info, warn};

const CALENDAR_URL: &str = "https://nfs.faireconomy.media/ff_calendar_thisweek.json";
const CALENDAR_CACHE_NAME: &str = "economic_calendar";

// ── Config ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewsConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,

    #[serde(default = "default_currencies")]
    pub currencies: Vec<String>,

    #[serde(default = "default_impacts")]
    pub impacts: Vec<String>,

    #[serde(default = "default_30")]
    pub before_min: i64,

    #[serde(default = "default_30")]
    pub after_min: i64,

    #[serde(default = "default_30")]
    pub merge_threshold_min: i64,

    #[serde(default = "default_true")]
    pub friday_night_enabled: bool,

    #[serde(default = "default_friday_start")]
    pub friday_night_start: String,

    #[serde(default = "default_friday_end")]
    pub friday_night_end: String,

    /// `"short"` (default) — blackout ends at friday_night_end on Friday.
    /// `"weekend"` — blackout runs until Monday 00:00 UTC (entire weekend).
    #[serde(default = "default_friday_mode")]
    pub friday_night_mode: String,
}

fn default_true() -> bool {
    true
}
fn default_30() -> i64 {
    30
}
fn default_currencies() -> Vec<String> {
    vec!["USD".into()]
}
fn default_impacts() -> Vec<String> {
    vec!["High".into()]
}
fn default_friday_start() -> String {
    "20:30".to_string()
}
fn default_friday_end() -> String {
    "21:00".to_string()
}
fn default_friday_mode() -> String {
    "short".to_string()
}

impl Default for NewsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            currencies: default_currencies(),
            impacts: default_impacts(),
            before_min: 30,
            after_min: 30,
            merge_threshold_min: 30,
            friday_night_enabled: true,
            friday_night_start: default_friday_start(),
            friday_night_end: default_friday_end(),
            friday_night_mode: default_friday_mode(),
        }
    }
}

// ── Raw API event ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RawEvent {
    #[serde(default)]
    title: String,
    #[serde(default)]
    country: String,
    #[serde(default)]
    date: String,
    #[serde(default)]
    time: String,
    #[serde(default)]
    impact: String,
}

// ── Public types ──────────────────────────────────────────────────────────────

/// A parsed economic calendar event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewsEvent {
    pub title: String,
    pub country: String,
    pub impact: String,
    pub datetime: DateTime<Utc>,
}

/// An event within a merged blackout window.
#[derive(Debug, Clone)]
pub struct WindowEvent {
    pub is_custom: bool,
    pub event_time: DateTime<Utc>,
    pub country: String,
    pub impact: String,
    pub title: String,
}

/// A merged blackout window containing one or more events.
#[derive(Debug, Clone)]
pub struct BlackoutWindow {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub events: Vec<WindowEvent>,
}

impl BlackoutWindow {
    pub fn remaining_minutes(&self) -> i64 {
        (self.end - Utc::now()).num_seconds().max(0) / 60
    }

    pub fn duration_minutes(&self) -> i64 {
        (self.end - self.start).num_seconds() / 60
    }
}

/// Event sent to each worker when blackout state changes.
#[derive(Debug, Clone)]
pub struct BlackoutNotification {
    pub active: bool,
    pub window: Option<BlackoutWindow>,
}

// ── Internal worker state ─────────────────────────────────────────────────────

struct WorkerState {
    config: NewsConfig,
    sender: mpsc::UnboundedSender<BlackoutNotification>,
    in_blackout: bool,
}

// ── Service inner state ───────────────────────────────────────────────────────

struct ServiceInner {
    workers: HashMap<String, WorkerState>,
    blackout_windows: Vec<BlackoutWindow>,
    last_fetch_date: Option<NaiveDate>,
    data_dir: PathBuf,
    client: Client,
}

/// Download the economic calendar. Standalone (no `&self`) so it can run without
/// holding the service lock.
async fn download_calendar(client: &Client) -> Result<Vec<RawEvent>> {
    let resp = client
        .get(CALENDAR_URL)
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await?
        .error_for_status()?
        .json::<Vec<serde_json::Value>>()
        .await?;

    let events: Vec<RawEvent> = resp
        .into_iter()
        .filter_map(|v| serde_json::from_value(v).ok())
        .collect();

    info!(count=%events.len(), "downloaded calendar events");
    Ok(events)
}

/// Refresh blackout windows: snapshot state under the lock, perform the network
/// download WITHOUT the lock, then merge under the lock. Avoids holding the
/// service mutex across a 30s network call (which would otherwise stall the 15s
/// blackout check loop and worker registration).
async fn fetch_and_merge(inner: &Arc<Mutex<ServiceInner>>) {
    let today = Utc::now().date_naive();
    if inner.lock().await.last_fetch_date == Some(today) {
        debug!("calendar already fetched today");
        return;
    }
    let client = inner.lock().await.client.clone();

    let raw = match download_calendar(&client).await {
        Ok(r) => {
            inner.lock().await.save_cache(&r);
            r
        }
        Err(e) => {
            error!(err=%e, "failed to download calendar");
            match inner.lock().await.load_cache() {
                Some(cached) => {
                    warn!("using cached calendar data");
                    cached
                }
                None => {
                    error!("no cached calendar data available");
                    return;
                }
            }
        }
    };

    inner.lock().await.merge_raw(raw, today);
}

impl ServiceInner {
    fn new(data_dir: PathBuf) -> Self {
        Self {
            workers: HashMap::new(),
            blackout_windows: Vec::new(),
            last_fetch_date: None,
            data_dir,
            client: Client::new(),
        }
    }

    // ── JSON cache ────────────────────────────────────────────────────────────

    fn cache_path(&self) -> PathBuf {
        self.data_dir.join(format!("{CALENDAR_CACHE_NAME}.json"))
    }

    fn save_cache(&self, events: &[RawEvent]) {
        if let Ok(json) = serde_json::to_string_pretty(events) {
            std::fs::create_dir_all(&self.data_dir).ok();
            if let Err(e) = std::fs::write(self.cache_path(), json) {
                warn!(err=%e, "failed to save calendar cache");
            }
        }
    }

    fn load_cache(&self) -> Option<Vec<RawEvent>> {
        let path = self.cache_path();
        if !path.exists() {
            return None;
        }
        let data = std::fs::read_to_string(&path).ok()?;
        serde_json::from_str(&data).ok()
    }

    // ── Merge (in-memory) ──────────────────────────────────────────────────────

    /// Build and store blackout windows from already-downloaded raw events. Runs
    /// entirely in memory, so (unlike the network download) it is safe to call
    /// while holding the service lock.
    fn merge_raw(&mut self, raw: Vec<RawEvent>, today: NaiveDate) {
        // Aggregate config from all enabled workers (union of currencies/impacts,
        // max of timing params).
        let mut all_currencies: HashSet<String> = HashSet::new();
        let mut all_impacts: HashSet<String> = HashSet::new();
        let mut max_before = 0i64;
        let mut max_after = 0i64;
        let mut max_merge = 0i64;

        for ws in self.workers.values() {
            if !ws.config.enabled {
                continue;
            }
            all_currencies.extend(ws.config.currencies.iter().cloned());
            all_impacts.extend(ws.config.impacts.iter().cloned());
            max_before = max_before.max(ws.config.before_min);
            max_after = max_after.max(ws.config.after_min);
            max_merge = max_merge.max(ws.config.merge_threshold_min);
        }

        if all_currencies.is_empty() {
            all_currencies.insert("USD".into());
        }
        if all_impacts.is_empty() {
            all_impacts.insert("High".into());
        }

        // Parse events within the next 24 hours.
        let now = Utc::now();
        let cutoff = now + Duration::hours(24);

        let before_min = if max_before > 0 { max_before } else { 30 };
        let after_min = if max_after > 0 { max_after } else { 30 };

        let mut individual: Vec<(DateTime<Utc>, DateTime<Utc>, WindowEvent)> = Vec::new();

        for raw_ev in &raw {
            // Prefer a timezone-aware timestamp when the feed provides one
            // (e.g. "2024-06-10T12:30:00-04:00"); converting from its real offset
            // to UTC. Only fall back to interpreting a naive "date + time" as UTC.
            let event_dt = if let Ok(dt) = DateTime::parse_from_rfc3339(raw_ev.date.trim()) {
                dt.with_timezone(&Utc)
            } else {
                let time_str = if raw_ev.time.is_empty() {
                    "12:00am".to_string()
                } else {
                    raw_ev.time.clone()
                };
                let dt_str = format!("{} {}", raw_ev.date, time_str);
                match NaiveDateTime::parse_from_str(&dt_str, "%m-%d-%Y %I:%M%p")
                    .or_else(|_| NaiveDateTime::parse_from_str(&dt_str, "%Y-%m-%d %I:%M%p"))
                    .ok()
                {
                    Some(d) => DateTime::<Utc>::from_naive_utc_and_offset(d, Utc),
                    None => continue,
                }
            };

            if event_dt < now || event_dt > cutoff {
                continue;
            }

            let matches_currency = all_currencies
                .iter()
                .any(|c| c.eq_ignore_ascii_case(&raw_ev.country) || c.eq_ignore_ascii_case("All"));
            let matches_impact = all_impacts
                .iter()
                .any(|i| i.eq_ignore_ascii_case(&raw_ev.impact));

            if matches_currency && matches_impact {
                let start = event_dt - Duration::minutes(before_min);
                let end = event_dt + Duration::minutes(after_min);
                individual.push((
                    start,
                    end,
                    WindowEvent {
                        is_custom: false,
                        event_time: event_dt,
                        country: raw_ev.country.clone(),
                        impact: raw_ev.impact.clone(),
                        title: raw_ev.title.clone(),
                    },
                ));
            }
        }

        if !individual.is_empty() {
            info!(count=%individual.len(), "economic event windows built");
        }

        // Add Friday night window for each worker that has it enabled.
        let mut friday_added = false;
        let worker_cfgs: Vec<NewsConfig> = self
            .workers
            .values()
            .filter(|ws| ws.config.enabled && ws.config.friday_night_enabled)
            .map(|ws| ws.config.clone())
            .collect();

        for cfg in &worker_cfgs {
            if let Ok((start, end)) = next_friday_window(
                &cfg.friday_night_start,
                &cfg.friday_night_end,
                &cfg.friday_night_mode,
            ) {
                let mode_label = if cfg.friday_night_mode.eq_ignore_ascii_case("weekend") {
                    "Weekend Blackout".to_string()
                } else {
                    format!(
                        "Friday Night Blackout ({}-{})",
                        cfg.friday_night_start, cfg.friday_night_end
                    )
                };
                let title = mode_label;
                individual.push((
                    start,
                    end,
                    WindowEvent {
                        is_custom: true,
                        event_time: start,
                        country: "Global".to_string(),
                        impact: "High".to_string(),
                        title,
                    },
                ));
                friday_added = true;
            }
        }
        if friday_added {
            info!("added Friday night blackout window(s)");
        }

        // Sort by start time then merge overlapping/close windows.
        individual.sort_by_key(|(start, _, _)| *start);

        let merge_gap = Duration::minutes(if max_merge > 0 { max_merge } else { 30 });
        let mut merged: Vec<BlackoutWindow> = Vec::new();

        for (start, end, event) in individual {
            match merged.last_mut() {
                Some(last) if (start - last.end) <= merge_gap => {
                    if end > last.end {
                        last.end = end;
                    }
                    last.events.push(event);
                }
                _ => merged.push(BlackoutWindow {
                    start,
                    end,
                    events: vec![event],
                }),
            }
        }

        self.blackout_windows = merged;
        self.last_fetch_date = Some(today);
        info!(windows=%self.blackout_windows.len(), "merged blackout windows");
    }

    // ── Blackout checks ───────────────────────────────────────────────────────

    /// Check all workers and send BlackoutNotification when state changes.
    fn check_and_notify_workers(&mut self) {
        if self.blackout_windows.is_empty() {
            return;
        }

        // Collect changes first to avoid borrow conflicts.
        let changes: Vec<(String, bool, Option<BlackoutWindow>)> = self
            .workers
            .iter()
            .filter(|(_, ws)| ws.config.enabled)
            .filter_map(|(id, ws)| {
                let is_active = is_blackout_for_config(&ws.config, &self.blackout_windows);
                if is_active != ws.in_blackout {
                    let window = if is_active {
                        current_window_for_config(&ws.config, &self.blackout_windows)
                    } else {
                        None
                    };
                    Some((id.clone(), is_active, window))
                } else {
                    None
                }
            })
            .collect();

        for (worker_id, is_active, window) in changes {
            if let Some(ws) = self.workers.get_mut(&worker_id) {
                ws.in_blackout = is_active;
                if is_active {
                    let end_str = window
                        .as_ref()
                        .map(|w| w.end.format("%H:%M UTC").to_string())
                        .unwrap_or_else(|| "N/A".to_string());
                    warn!(worker=%worker_id, until=%end_str, "ENTERING news blackout");
                } else {
                    info!(worker=%worker_id, "EXITING news blackout");
                }
                ws.sender
                    .send(BlackoutNotification {
                        active: is_active,
                        window,
                    })
                    .ok();
            }
        }
    }
}

// ── Per-worker blackout helpers ───────────────────────────────────────────────

/// Only custom (Friday night) events are filtered per-worker; regular
/// economic events affect all workers equally.
fn is_blackout_for_config(config: &NewsConfig, windows: &[BlackoutWindow]) -> bool {
    let now = Utc::now();
    for window in windows {
        if window.start <= now && now <= window.end {
            for event in &window.events {
                if event.is_custom {
                    if config.friday_night_enabled {
                        return true;
                    }
                } else {
                    let currency_match = config.currencies.iter().any(|c| {
                        c.eq_ignore_ascii_case(&event.country) || c.eq_ignore_ascii_case("All")
                    });
                    let impact_match = config
                        .impacts
                        .iter()
                        .any(|i| i.eq_ignore_ascii_case(&event.impact));
                    if currency_match && impact_match {
                        return true;
                    }
                }
            }
        }
    }
    false
}

fn current_window_for_config(
    config: &NewsConfig,
    windows: &[BlackoutWindow],
) -> Option<BlackoutWindow> {
    let now = Utc::now();
    for window in windows {
        if window.start <= now && now <= window.end {
            let matching: Vec<WindowEvent> = window
                .events
                .iter()
                .filter(|e| {
                    if e.is_custom {
                        config.friday_night_enabled
                    } else {
                        let currency_match = config.currencies.iter().any(|c| {
                            c.eq_ignore_ascii_case(&e.country) || c.eq_ignore_ascii_case("All")
                        });
                        let impact_match = config
                            .impacts
                            .iter()
                            .any(|i| i.eq_ignore_ascii_case(&e.impact));
                        currency_match && impact_match
                    }
                })
                .cloned()
                .collect();

            if !matching.is_empty() {
                return Some(BlackoutWindow {
                    start: window.start,
                    end: window.end,
                    events: matching,
                });
            }
        }
    }
    None
}

// ── Friday night window ───────────────────────────────────────────────────────

/// Compute the next Friday night blackout window.
/// - `mode = "short"`: end = friday_night_end time on that same Friday
/// - `mode = "weekend"`: end = Monday 00:00 UTC (entire weekend blackout)
fn next_friday_window(
    start_str: &str,
    end_str: &str,
    mode: &str,
) -> Result<(DateTime<Utc>, DateTime<Utc>)> {
    let (sh, sm) = parse_time(start_str)?;

    let now = Utc::now();
    // Mon=0 .. Fri=4 .. Sun=6
    let weekday_num = now.weekday().num_days_from_monday() as i64;
    let days_ahead = (4 - weekday_num).rem_euclid(7);

    let friday_date = (now + Duration::days(days_ahead)).date_naive();
    let start = DateTime::<Utc>::from_naive_utc_and_offset(
        friday_date.and_hms_opt(sh, sm, 0).unwrap(),
        Utc,
    );

    // If today is Friday (days_ahead == 0) but the window already started, use next Friday.
    let start = if days_ahead == 0 && now >= start {
        let next = (now + Duration::days(7)).date_naive();
        DateTime::<Utc>::from_naive_utc_and_offset(next.and_hms_opt(sh, sm, 0).unwrap(), Utc)
    } else {
        start
    };

    let end = if mode.eq_ignore_ascii_case("weekend") {
        // End at Monday 00:00 UTC (3 days after Friday)
        let monday_date = (start + Duration::days(3)).date_naive();
        DateTime::<Utc>::from_naive_utc_and_offset(monday_date.and_hms_opt(0, 0, 0).unwrap(), Utc)
    } else {
        // "short" mode: end at friday_night_end on same Friday
        let (eh, em) = parse_time(end_str)?;
        DateTime::<Utc>::from_naive_utc_and_offset(
            start.date_naive().and_hms_opt(eh, em, 0).unwrap(),
            Utc,
        )
    };

    Ok((start, end))
}

// ── NewsBlackoutService ───────────────────────────────────────────────────────

/// Manages news blackout for multiple live workers.
/// NewsBlackoutService with a daily fetch loop and a 15-second check loop.
pub struct NewsBlackoutService {
    inner: Arc<Mutex<ServiceInner>>,
    shutdown_tx: watch::Sender<bool>,
}

impl NewsBlackoutService {
    pub fn new(data_dir: PathBuf) -> Self {
        let (shutdown_tx, _) = watch::channel(false);
        Self {
            inner: Arc::new(Mutex::new(ServiceInner::new(data_dir))),
            shutdown_tx,
        }
    }

    /// Register a worker and return a receiver for BlackoutNotifications.
    pub async fn register_worker(
        &self,
        worker_id: String,
        config: NewsConfig,
    ) -> mpsc::UnboundedReceiver<BlackoutNotification> {
        let (tx, rx) = mpsc::unbounded_channel();
        let enabled = config.enabled;
        let currencies = config.currencies.clone();
        self.inner.lock().await.workers.insert(
            worker_id.clone(),
            WorkerState {
                config,
                sender: tx,
                in_blackout: false,
            },
        );
        debug!(
            worker=%worker_id,
            enabled=%enabled,
            currencies=?currencies,
            "registered worker"
        );
        rx
    }

    /// Start the daily-fetch and 15-second-check background loops.
    pub async fn start(&self) -> Result<()> {
        {
            let inner = self.inner.lock().await;
            if inner.workers.is_empty() {
                warn!("no workers registered — news blackout service not starting");
                return Ok(());
            }
            if !inner.workers.values().any(|w| w.config.enabled) {
                info!("all worker news blackout configs disabled");
                return Ok(());
            }
        }

        // Initial fetch.
        fetch_and_merge(&self.inner).await;

        // Daily fetch loop — sleeps until next midnight UTC.
        {
            let inner_arc = self.inner.clone();
            let mut shutdown = self.shutdown_tx.subscribe();
            tokio::spawn(async move {
                loop {
                    let now = Utc::now();
                    let next_midnight = (now + Duration::days(1))
                        .date_naive()
                        .and_hms_opt(0, 0, 0)
                        .unwrap()
                        .and_utc();
                    let sleep_secs = (next_midnight - now).num_seconds().max(1) as u64;

                    tokio::select! {
                        _ = tokio::time::sleep(tokio::time::Duration::from_secs(sleep_secs)) => {
                            fetch_and_merge(&inner_arc).await;
                        }
                        _ = shutdown.changed() => break,
                    }
                }
            });
        }

        // Blackout check loop — fires every 15 seconds.
        {
            let inner_arc = self.inner.clone();
            let mut shutdown = self.shutdown_tx.subscribe();
            tokio::spawn(async move {
                loop {
                    tokio::select! {
                        _ = tokio::time::sleep(tokio::time::Duration::from_secs(15)) => {
                            inner_arc.lock().await.check_and_notify_workers();
                        }
                        _ = shutdown.changed() => break,
                    }
                }
            });
        }

        info!("news blackout service started");
        Ok(())
    }

    /// Stop all background loops.
    pub async fn stop(&self) {
        info!("stopping news blackout service");
        self.shutdown_tx.send(true).ok();
    }

    /// Return upcoming blackout windows within the next `hours` hours.
    pub async fn get_upcoming_blackouts(&self, hours: u32) -> Vec<BlackoutWindow> {
        let inner = self.inner.lock().await;
        let now = Utc::now();
        let cutoff = now + Duration::hours(hours as i64);
        inner
            .blackout_windows
            .iter()
            .filter(|w| w.start >= now && w.start <= cutoff)
            .cloned()
            .collect()
    }
}

// ── Utilities ─────────────────────────────────────────────────────────────────

fn parse_time(s: &str) -> Result<(u32, u32)> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() == 2 {
        Ok((parts[0].parse()?, parts[1].parse()?))
    } else {
        Err(anyhow!("invalid time format: {s}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_time() {
        assert_eq!(parse_time("20:30").unwrap(), (20, 30));
        assert_eq!(parse_time("00:00").unwrap(), (0, 0));
        assert!(parse_time("2030").is_err());
        assert!(parse_time("abc:def").is_err());
    }

    #[test]
    fn test_next_friday_window() {
        let (start, end) = next_friday_window("20:30", "21:00", "short").unwrap();
        assert!(start < end);
        assert_eq!((end - start).num_minutes(), 30);

        let (w_start, w_end) = next_friday_window("20:30", "21:00", "weekend").unwrap();
        assert!(w_start < w_end);
        assert!((w_end - w_start).num_hours() > 48);
    }

    #[test]
    fn test_is_blackout_for_config() {
        let config = NewsConfig {
            enabled: true,
            currencies: vec!["USD".to_string()],
            impacts: vec!["High".to_string()],
            before_min: 30,
            after_min: 30,
            merge_threshold_min: 30,
            friday_night_enabled: false,
            friday_night_start: "20:30".to_string(),
            friday_night_end: "21:00".to_string(),
            friday_night_mode: "short".to_string(),
        };

        let now = Utc::now();
        let window = BlackoutWindow {
            start: now - Duration::minutes(15),
            end: now + Duration::minutes(15),
            events: vec![WindowEvent {
                is_custom: false,
                event_time: now,
                country: "USD".to_string(),
                impact: "High".to_string(),
                title: "Test Event".to_string(),
            }],
        };

        assert!(is_blackout_for_config(&config, &[window.clone()]));

        let mismatch_config = NewsConfig {
            currencies: vec!["EUR".to_string()],
            ..config.clone()
        };
        assert!(!is_blackout_for_config(&mismatch_config, &[window.clone()]));

        let friday_config = NewsConfig {
            friday_night_enabled: true,
            ..config.clone()
        };
        let custom_window = BlackoutWindow {
            start: now - Duration::minutes(15),
            end: now + Duration::minutes(15),
            events: vec![WindowEvent {
                is_custom: true,
                event_time: now,
                country: "Global".to_string(),
                impact: "High".to_string(),
                title: "Friday Night Blackout".to_string(),
            }],
        };
        assert!(is_blackout_for_config(
            &friday_config,
            &[custom_window.clone()]
        ));
        assert!(!is_blackout_for_config(&config, &[custom_window.clone()]));
    }

    #[test]
    fn test_merge_raw_events() {
        let mut inner = ServiceInner::new(PathBuf::from("/tmp"));
        let today = Utc::now().date_naive();

        let now = Utc::now();
        let raw_events = vec![
            RawEvent {
                title: "FOMC".to_string(),
                country: "USD".to_string(),
                date: (now + Duration::minutes(60)).to_rfc3339(),
                time: String::new(),
                impact: "High".to_string(),
            },
            RawEvent {
                title: "CPI".to_string(),
                country: "USD".to_string(),
                date: (now + Duration::minutes(90)).to_rfc3339(),
                time: String::new(),
                impact: "High".to_string(),
            },
        ];

        inner.merge_raw(raw_events, today);

        assert_eq!(inner.blackout_windows.len(), 1);
        let merged_window = &inner.blackout_windows[0];
        assert_eq!(merged_window.events.len(), 2);
        assert_eq!(merged_window.duration_minutes(), 90);
    }

    #[test]
    fn test_cache_save_and_load() {
        let now_nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp_dir = std::env::temp_dir().join(format!("test_cache_{}", now_nanos));
        std::fs::create_dir_all(&temp_dir).unwrap();

        let inner = ServiceInner::new(temp_dir.clone());
        let raw_events = vec![RawEvent {
            title: "FOMC".to_string(),
            country: "USD".to_string(),
            date: "2026-06-22T20:30:00Z".to_string(),
            time: String::new(),
            impact: "High".to_string(),
        }];

        inner.save_cache(&raw_events);
        let loaded = inner.load_cache().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].title, "FOMC");

        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_service_inner_check_and_notify() {
        let mut inner = ServiceInner::new(PathBuf::from("/tmp"));
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let config = NewsConfig {
            enabled: true,
            currencies: vec!["USD".to_string()],
            impacts: vec!["High".to_string()],
            ..Default::default()
        };
        inner.workers.insert(
            "worker_1".to_string(),
            WorkerState {
                config,
                sender: tx,
                in_blackout: false,
            },
        );

        let now = Utc::now();
        inner.blackout_windows = vec![BlackoutWindow {
            start: now - Duration::minutes(5),
            end: now + Duration::minutes(5),
            events: vec![WindowEvent {
                is_custom: false,
                event_time: now,
                country: "USD".to_string(),
                impact: "High".to_string(),
                title: "FOMC".to_string(),
            }],
        }];

        inner.check_and_notify_workers();
        assert!(inner.workers.get("worker_1").unwrap().in_blackout);
        let notification = rx.try_recv().unwrap();
        assert!(notification.active);

        // Move blackout window to the past to test exit notification
        inner.blackout_windows[0].start = now - Duration::minutes(15);
        inner.blackout_windows[0].end = now - Duration::minutes(5);

        inner.check_and_notify_workers();
        assert!(!inner.workers.get("worker_1").unwrap().in_blackout);
        let notification_exit = rx.try_recv().unwrap();
        assert!(!notification_exit.active);
    }

    #[tokio::test]
    async fn test_news_blackout_service() {
        let service = NewsBlackoutService::new(PathBuf::from("/tmp"));

        let config = NewsConfig {
            enabled: true,
            currencies: vec!["USD".to_string()],
            impacts: vec!["High".to_string()],
            ..Default::default()
        };

        let _rx = service
            .register_worker("worker_1".to_string(), config)
            .await;

        {
            let mut inner = service.inner.lock().await;
            inner.last_fetch_date = Some(Utc::now().date_naive());

            let now = Utc::now();
            inner.blackout_windows = vec![
                BlackoutWindow {
                    start: now - Duration::minutes(10),
                    end: now + Duration::minutes(10),
                    events: vec![WindowEvent {
                        is_custom: false,
                        event_time: now,
                        country: "USD".to_string(),
                        impact: "High".to_string(),
                        title: "FOMC".to_string(),
                    }],
                },
                BlackoutWindow {
                    start: now + Duration::minutes(30),
                    end: now + Duration::minutes(60),
                    events: vec![WindowEvent {
                        is_custom: false,
                        event_time: now + Duration::minutes(45),
                        country: "USD".to_string(),
                        impact: "High".to_string(),
                        title: "Upcoming FOMC".to_string(),
                    }],
                },
            ];
        }

        assert!(service.start().await.is_ok());

        let upcoming = service.get_upcoming_blackouts(2).await;
        assert_eq!(upcoming.len(), 1);
        assert_eq!(upcoming[0].events[0].title, "Upcoming FOMC");

        service.stop().await;
    }
}
