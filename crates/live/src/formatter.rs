use chrono::DateTime;
use infra::news::BlackoutWindow;
use ts_core::{AccountInfo, Direction, ExitReason, Position, TradeRecord};

const BOT_NAME: &str = "Trading Bot";

fn dir_parts(dir: Direction) -> (&'static str, &'static str) {
    match dir {
        Direction::Buy => ("🟢", "BUY"),
        Direction::Sell => ("🔴", "SELL"),
        _ => ("⚪", "HOLD"),
    }
}

fn format_exit_time(unix_secs: i64) -> String {
    DateTime::from_timestamp(unix_secs, 0)
        .map(|dt| dt.format("%H:%M:%S").to_string())
        .unwrap_or_default()
}

pub fn format_trade_open(pos: &Position, symbol: &str) -> String {
    let (dir_emoji, dir_str) = dir_parts(pos.direction);
    let timestamp = format_exit_time(pos.entry_time);
    format!(
        "💼 <b>{BOT_NAME} - Trade Opened</b>\n\
         📊 <b>{symbol}</b> {dir_emoji} <b>{dir_str}</b>\n\
         🎫 <b>Ticket:</b> <code>{}</code>\n\
         📦 <b>Volume:</b> <code>{:.3}</code>\n\
         🏷️ <b>Entry Price:</b> <code>{:.5}</code>\n\
         🛡️ <b>Stop Loss:</b> <code>{:.5}</code>\n\
         🎯 <b>Take Profit:</b> <code>{:.5}</code>\n\
         ⏰ <b>Timestamp:</b> <code>{timestamp}</code>",
        pos.trade_id, pos.volume, pos.entry_price, pos.current_stop_loss, pos.take_profit
    )
}

pub fn format_stop_update(pos: &Position, symbol: &str, time: i64) -> String {
    let (dir_emoji, dir_str) = dir_parts(pos.direction);
    let timestamp = format_exit_time(time);
    let locked_r = pos.r_multiple(pos.current_stop_loss);
    let profit_emoji = if locked_r >= 0.0 { "💰" } else { "💸" };
    format!(
        "🔄 <b>{BOT_NAME} - Stop Updated</b>\n\
         📊 <b>{symbol}</b> {dir_emoji} <b>{dir_str}</b>\n\
         🎫 <b>Ticket:</b> <code>{}</code>\n\
         📦 <b>Volume:</b> <code>{:.3}</code>\n\
         🏷️ <b>Entry Price:</b> <code>{:.5}</code>\n\
         🛡️ <b>Stop Loss:</b> <code>{:.5}</code>\n\
         🎯 <b>Take Profit:</b> <code>{:.5}</code>\n\
         {profit_emoji} <b>Locked Profit:</b> <code>{locked_r:.2}R</code>\n\
         ⏰ <b>Timestamp:</b> <code>{timestamp}</code>",
        pos.trade_id, pos.volume, pos.entry_price, pos.current_stop_loss, pos.take_profit
    )
}

fn format_trade_close(rec: &TradeRecord, emoji: &str, alert_type: &str) -> String {
    let (dir_emoji, dir_str) = dir_parts(rec.direction);
    let profit_emoji = if rec.currency_pnl >= 0.0 {
        "💰"
    } else {
        "💸"
    };
    let timestamp = format_exit_time(rec.exit_time);
    format!(
        "{emoji} <b>{BOT_NAME} - {alert_type}</b>\n\
         📊 <b>{}</b> {dir_emoji} <b>{dir_str}</b>\n\
         🎫 <b>Ticket:</b> <code>{}</code>\n\
         📦 <b>Volume:</b> <code>{:.3}</code>\n\
         🏷️ <b>Entry Price:</b> <code>{:.5}</code>\n\
         🛡️ <b>Stop Loss:</b> <code>{:.5}</code>\n\
         🎯 <b>Take Profit:</b> <code>{:.5}</code>\n\
         {profit_emoji} <b>Profit:</b> <code>{:.2}</code>  (<code>{:.2}R</code>)\n\
         ⏰ <b>Timestamp:</b> <code>{timestamp}</code>",
        rec.symbol,
        rec.trade_id,
        rec.volume,
        rec.entry_price,
        rec.current_stop_loss,
        rec.take_profit,
        rec.currency_pnl,
        rec.profit
    )
}

pub fn format_stop_out(rec: &TradeRecord) -> String {
    format_trade_close(rec, "🚨", "Stop Out")
}

pub fn format_take_profit(rec: &TradeRecord) -> String {
    format_trade_close(rec, "🎯", "Take Profit")
}

pub fn format_exit_rule(rec: &TradeRecord) -> String {
    format_trade_close(rec, "🎯", "Exit Rule")
}

/// Escape characters that would break Telegram's HTML parse_mode. Applied to
/// free-text that originates from the external economic-calendar feed.
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

pub fn format_news_blackout(window: &BlackoutWindow) -> String {
    let events_str: Vec<String> = window
        .events
        .iter()
        .map(|e| {
            format!(
                "  • <b>{} {}</b> — {}",
                html_escape(&e.impact),
                html_escape(&e.country),
                html_escape(&e.title)
            )
        })
        .collect();
    format!(
        "🔕 <b>News Blackout Active</b>\n\
         🕐 <b>Until:</b> <code>{}</code>\n{}",
        window.end.format("%H:%M UTC"),
        events_str.join("\n")
    )
}

pub fn format_news_cleared() -> String {
    "✅ <b>News Blackout Cleared</b>".to_string()
}

pub fn format_drawdown_emergency(dd_pct: f64, limit_pct: f64) -> String {
    format!(
        "🆘 <b>Drawdown Limit Reached</b>\n\
         📉 <b>Current:</b> <code>{:.2}%</code>\n\
         🚫 <b>Limit:</b> <code>{:.2}%</code>\n\
         ⚠️ All new trades suspended for today.",
        dd_pct, limit_pct
    )
}

pub fn format_decay_halt(
    worker_id: &str,
    rolling_expectancy: f64,
    threshold: f64,
    trade_count: usize,
) -> String {
    format!(
        "🧟 <b>Strategy Decay Detected</b>\n\
         🔖 <b>Worker:</b> <code>{worker_id}</code>\n\
         📉 <b>Rolling Expectancy (30d):</b> <code>{rolling_expectancy:.3}R</code> over <code>{trade_count}</code> trades\n\
         🚫 <b>OOS 10th-percentile threshold:</b> <code>{threshold:.3}R</code>\n\
         ⚠️ New entries suspended — live performance has dropped below the walk-forward OOS distribution."
    )
}

pub fn format_decay_cleared(worker_id: &str) -> String {
    format!("✅ <b>Strategy Decay Cleared</b>\n🔖 <b>Worker:</b> <code>{worker_id}</code>")
}

pub fn format_account_report(info: &AccountInfo) -> String {
    let profit_emoji = if info.profit >= 0.0 { "💰" } else { "💸" };
    format!(
        "📊 <b>Account Report</b>\n\
         💵 <b>Balance:</b> <code>{:.2} {}</code>\n\
         📈 <b>Equity:</b> <code>{:.2}</code>\n\
         {} <b>P&amp;L:</b> <code>{:.2}</code>",
        info.balance, info.currency, info.equity, profit_emoji, info.profit
    )
}

pub fn exit_reason_label(reason: ExitReason) -> &'static str {
    match reason {
        ExitReason::StopLoss => "Stop Out",
        ExitReason::TakeProfit => "Take Profit",
        ExitReason::ExitRule => "Exit Rule",
        ExitReason::StopProfit => "Stop Profit",
        ExitReason::EndOfData => "Closed",
    }
}
