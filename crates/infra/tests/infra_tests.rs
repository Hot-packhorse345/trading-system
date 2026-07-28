use infra::{init_tracing, BlackoutWindow, Notifier, NullNotifier};

#[tokio::test]
async fn test_null_notifier() {
    let n = NullNotifier;
    assert!(n.send("hello test").await.is_ok());
}

#[test]
fn test_init_tracing() {
    // Tracing subscriber can only be initialized once per process safely.
    // Calling it might fail if already initialized in another test, but calling it should not panic.
    let _ = init_tracing(false);
}

#[test]
fn test_blackout_window_helpers() {
    let start = chrono::DateTime::from_timestamp(1719003000, 0).unwrap();
    let end = chrono::DateTime::from_timestamp(1719004200, 0).unwrap();
    let window = BlackoutWindow {
        start,
        end,
        events: vec![],
    };

    // Duration is end_time - start_time: 1200 seconds = 20 minutes
    assert_eq!(window.duration_minutes(), 20);
}

// ── NEWS CONFIG DEFAULTS ────────────────────────────────────────────────────

#[test]
fn test_news_config_defaults() {
    let cfg = infra::news::NewsConfig::default();
    assert!(cfg.enabled);
    assert_eq!(cfg.before_min, 30);
    assert_eq!(cfg.after_min, 30);
    assert_eq!(cfg.merge_threshold_min, 30);
    assert_eq!(cfg.currencies, vec!["USD"]);
    assert_eq!(cfg.impacts, vec!["High"]);
    assert!(cfg.friday_night_enabled);
    assert_eq!(cfg.friday_night_start, "20:30");
    assert_eq!(cfg.friday_night_end, "21:00");
    assert_eq!(cfg.friday_night_mode, "short");
}

#[test]
fn test_news_config_deserialization_partial() {
    let json = r#"{ "enabled": false, "currencies": ["EUR", "GBP"], "before_min": 15 }"#;
    let cfg: infra::news::NewsConfig = serde_json::from_str(json).unwrap();
    assert!(!cfg.enabled);
    assert_eq!(cfg.currencies, vec!["EUR", "GBP"]);
    assert_eq!(cfg.before_min, 15);
    assert_eq!(cfg.after_min, 30); // default
    assert_eq!(cfg.impacts, vec!["High"]); // default
}

#[test]
fn test_news_config_deserialization_empty() {
    let cfg: infra::news::NewsConfig = serde_json::from_str("{}").unwrap();
    assert!(cfg.enabled);
    assert_eq!(cfg.currencies, vec!["USD"]);
}

// ── BLACKOUT WINDOW EDGE CASES ──────────────────────────────────────────────

#[test]
fn test_blackout_window_remaining_minutes() {
    let now = chrono::Utc::now();
    let window = BlackoutWindow {
        start: now - chrono::Duration::minutes(10),
        end: now + chrono::Duration::minutes(20),
        events: vec![],
    };
    let remaining = window.remaining_minutes();
    // Should be approximately 20, allow 1 minute tolerance
    assert!(
        remaining >= 19 && remaining <= 21,
        "remaining: {}",
        remaining
    );
}

#[test]
fn test_blackout_window_expired() {
    let past = chrono::Utc::now() - chrono::Duration::hours(2);
    let window = BlackoutWindow {
        start: past,
        end: past + chrono::Duration::minutes(30),
        events: vec![],
    };
    assert!(window.remaining_minutes() <= 0);
}

#[test]
fn test_blackout_window_with_events() {
    let start = chrono::DateTime::from_timestamp(1719000000, 0)
        .unwrap()
        .with_timezone(&chrono::Utc);
    let end = chrono::DateTime::from_timestamp(1719003600, 0)
        .unwrap()
        .with_timezone(&chrono::Utc);
    let window = BlackoutWindow {
        start,
        end,
        events: vec![
            infra::news::WindowEvent {
                is_custom: false,
                event_time: start + chrono::Duration::minutes(15),
                country: "USD".to_string(),
                impact: "High".to_string(),
                title: "NFP".to_string(),
            },
            infra::news::WindowEvent {
                is_custom: true,
                event_time: start + chrono::Duration::minutes(45),
                country: "EUR".to_string(),
                impact: "Medium".to_string(),
                title: "ECB Speech".to_string(),
            },
        ],
    };
    assert_eq!(window.events.len(), 2);
    assert_eq!(window.duration_minutes(), 60);
}

// ── BLACKOUT NOTIFICATION ───────────────────────────────────────────────────

#[test]
fn test_blackout_notification_active() {
    let notif = infra::news::BlackoutNotification {
        active: true,
        window: Some(BlackoutWindow {
            start: chrono::Utc::now(),
            end: chrono::Utc::now() + chrono::Duration::minutes(30),
            events: vec![],
        }),
    };
    assert!(notif.active);
    assert!(notif.window.is_some());
}

#[test]
fn test_blackout_notification_cleared() {
    let notif = infra::news::BlackoutNotification {
        active: false,
        window: None,
    };
    assert!(!notif.active);
    assert!(notif.window.is_none());
}

// ── NOTIFIER TESTS ──────────────────────────────────────────────────────────

#[tokio::test]
async fn test_null_notifier_multiple_sends() {
    let n = NullNotifier;
    assert!(n.send("message 1").await.is_ok());
    assert!(n.send("message 2").await.is_ok());
    assert!(n.send("").await.is_ok()); // empty message
}

#[test]
fn test_telegram_notifier_creation() {
    let _notifier = infra::TelegramNotifier::new("test_token_123", "test_chat_456");
}

// ── INIT TRACING SAFETY ────────────────────────────────────────────────────

#[test]
fn test_init_tracing_json_mode() {
    // Should not panic even if already initialized
    let _ = init_tracing(true);
}

// ── NEWS BLACKOUT SERVICE CREATION ──────────────────────────────────────────

#[tokio::test]
async fn test_news_blackout_service_lifecycle() {
    let temp_dir = std::env::temp_dir().join(format!(
        "test_news_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&temp_dir).unwrap();

    let service = infra::news::NewsBlackoutService::new(temp_dir.clone());

    let cfg = infra::news::NewsConfig {
        enabled: false, // disabled to avoid HTTP calls
        ..Default::default()
    };

    let _rx = service.register_worker("worker1".to_string(), cfg).await;
    service.stop().await;

    std::fs::remove_dir_all(&temp_dir).ok();
}
