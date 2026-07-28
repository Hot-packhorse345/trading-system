use anyhow::Result;
use rusqlite::{params, Connection};
use std::path::Path;
use tracing::debug;
use ts_core::{Direction, ExitReason, TradeRecord};

pub struct TradeDb {
    conn: Connection,
}

impl TradeDb {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;

        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS trades (
                trade_id          INTEGER PRIMARY KEY,
                strategy_id       INTEGER NOT NULL DEFAULT 0,
                symbol            TEXT    NOT NULL DEFAULT '',
                group_id          INTEGER NOT NULL DEFAULT 0,
                direction         TEXT    NOT NULL,
                volume            REAL    NOT NULL,
                open_risk         REAL    NOT NULL DEFAULT 0,
                entry_price       REAL    NOT NULL,
                exit_price        REAL    NOT NULL DEFAULT 0,
                initial_stop_loss REAL    NOT NULL,
                current_stop_loss REAL    NOT NULL,
                take_profit       REAL    NOT NULL,
                entry_time        INTEGER NOT NULL,
                exit_time         INTEGER NOT NULL DEFAULT 0,
                exit_reason       TEXT    NOT NULL DEFAULT '',
                profit            REAL    NOT NULL DEFAULT 0,
                currency_pnl     REAL    NOT NULL DEFAULT 0,
                closed            INTEGER NOT NULL DEFAULT 0
            );
        ",
        )?;
        // Migrate existing DBs that lack the new column (no-op if already present).
        conn.execute_batch("ALTER TABLE trades ADD COLUMN currency_pnl REAL NOT NULL DEFAULT 0;")
            .ok();
        Ok(Self { conn })
    }

    pub fn insert(&self, t: &TradeRecord) -> Result<()> {
        // Plain INSERT (not INSERT OR REPLACE): a colliding trade_id must surface
        // as an error rather than silently overwriting an existing open position.
        self.conn.execute(
            "INSERT INTO trades
            (trade_id, strategy_id, symbol, group_id, direction, volume, open_risk,
             entry_price, initial_stop_loss, current_stop_loss, take_profit, entry_time)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                t.trade_id as i64,
                t.strategy_id as i64,
                &t.symbol,
                t.group_id as i64,
                format!("{:?}", t.direction),
                t.volume,
                t.open_risk,
                t.entry_price,
                t.initial_stop_loss,
                t.current_stop_loss,
                t.take_profit,
                t.entry_time
            ],
        )?;
        Ok(())
    }

    pub fn update_stop(&self, trade_id: u64, sl: f64) -> Result<()> {
        self.conn.execute(
            "UPDATE trades SET current_stop_loss=?1 WHERE trade_id=?2",
            params![sl, trade_id as i64],
        )?;
        Ok(())
    }

    pub fn close(&self, t: &TradeRecord) -> Result<()> {
        self.conn.execute(
            "UPDATE trades SET exit_price=?1, exit_time=?2, exit_reason=?3,
             profit=?4, currency_pnl=?5, current_stop_loss=?6, closed=1 WHERE trade_id=?7",
            params![
                t.exit_price,
                t.exit_time,
                format!("{:?}", t.exit_reason),
                t.profit,
                t.currency_pnl,
                t.current_stop_loss,
                t.trade_id as i64
            ],
        )?;
        Ok(())
    }

    pub fn load_open(&self, strategy_id: u64) -> Result<Vec<TradeRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT trade_id, strategy_id, symbol, group_id, direction, volume, open_risk,
                    entry_price, exit_price, initial_stop_loss, current_stop_loss, take_profit,
                    entry_time, exit_time, exit_reason, profit
             FROM trades WHERE closed=0 AND strategy_id=?1",
        )?;

        let records: Vec<_> = stmt
            .query_map(params![strategy_id as i64], map_trade_row)?
            .filter_map(|r| r.ok())
            .collect();

        debug!(
            "Recovered {} open trades for strategy {}",
            records.len(),
            strategy_id
        );
        Ok(records)
    }

    /// Closed trades for `strategy_id` with `exit_time >= since_ts` (unix
    /// seconds). Used by the live decay monitor to compute a rolling
    /// performance window comparable to the WFA OOS distribution.
    pub fn load_closed_since(&self, strategy_id: u64, since_ts: i64) -> Result<Vec<TradeRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT trade_id, strategy_id, symbol, group_id, direction, volume, open_risk,
                    entry_price, exit_price, initial_stop_loss, current_stop_loss, take_profit,
                    entry_time, exit_time, exit_reason, profit
             FROM trades WHERE closed=1 AND strategy_id=?1 AND exit_time>=?2",
        )?;

        let records: Vec<_> = stmt
            .query_map(params![strategy_id as i64, since_ts], map_trade_row)?
            .filter_map(|r| r.ok())
            .collect();
        Ok(records)
    }

    pub fn clear(&self) -> Result<()> {
        self.conn.execute("DELETE FROM trades", [])?;
        debug!("Cleared trade database");
        Ok(())
    }

    pub fn invalidate(&self, strategy_id: Option<u64>, trade_id: Option<u64>) -> Result<()> {
        match (strategy_id, trade_id) {
            (Some(s), Some(t)) => {
                self.conn.execute(
                    "DELETE FROM trades WHERE strategy_id=?1 AND trade_id=?2",
                    params![s as i64, t as i64],
                )?;
            }
            (Some(s), None) => {
                self.conn
                    .execute("DELETE FROM trades WHERE strategy_id=?1", params![s as i64])?;
            }
            _ => {
                self.conn.execute("DELETE FROM trades", [])?;
            }
        }
        Ok(())
    }
}

/// Shared row → `TradeRecord` mapping for the fixed 16-column select used by
/// both `load_open` and `load_closed_since`.
fn map_trade_row(row: &rusqlite::Row) -> rusqlite::Result<TradeRecord> {
    let dir_str: String = row.get(4)?;
    let er_str: String = row.get(14)?;

    // Never silently default an unrecognised direction to Buy — that would
    // recover a short as a long and invert all stop logic. Skip the row
    // (filtered out by the caller) and let it remain managed by the exchange SL.
    let direction = match parse_dir(&dir_str) {
        Some(d) => d,
        None => {
            tracing::error!(direction=%dir_str, trade_id=row.get::<_, i64>(0)?,
            "unknown direction in trade DB — skipping recovery of this row");
            return Err(rusqlite::Error::InvalidColumnType(
                4,
                "direction".into(),
                rusqlite::types::Type::Text,
            ));
        }
    };

    Ok(TradeRecord {
        trade_id: row.get::<_, i64>(0)? as u64,
        strategy_id: row.get::<_, i64>(1)? as u64,
        symbol: row.get(2)?,
        group_id: row.get::<_, i64>(3)? as u64,
        direction,
        volume: row.get(5)?,
        open_risk: row.get(6)?,
        entry_price: row.get(7)?,
        exit_price: row.get(8)?,
        initial_stop_loss: row.get(9)?,
        current_stop_loss: row.get(10)?,
        take_profit: row.get(11)?,
        entry_time: row.get(12)?,
        exit_time: row.get(13)?,
        exit_reason: parse_exit(&er_str),
        profit: row.get(15)?,
        currency_pnl: 0.0,
    })
}

fn parse_dir(s: &str) -> Option<Direction> {
    match s {
        "Buy" => Some(Direction::Buy),
        "Sell" => Some(Direction::Sell),
        "Hold" => Some(Direction::Hold),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open_rec(trade_id: u64, dir: Direction) -> TradeRecord {
        TradeRecord {
            trade_id,
            strategy_id: 7,
            symbol: "BTCUSDT".into(),
            direction: dir,
            entry_price: 100.0,
            exit_price: 0.0,
            initial_stop_loss: 95.0,
            current_stop_loss: 95.0,
            take_profit: 0.0,
            volume: 1.0,
            open_risk: 5.0,
            entry_time: 1,
            exit_time: 0,
            exit_reason: ExitReason::EndOfData,
            profit: 0.0,
            currency_pnl: 0.0,
            group_id: 0,
        }
    }

    #[test]
    fn direction_round_trips_and_is_never_defaulted() {
        let db = TradeDb::open(":memory:").unwrap();
        db.insert(&open_rec(1, Direction::Sell)).unwrap();
        db.insert(&open_rec(2, Direction::Buy)).unwrap();

        let mut open = db.load_open(7).unwrap();
        open.sort_by_key(|r| r.trade_id);
        assert_eq!(open.len(), 2);
        assert_eq!(open[0].direction, Direction::Sell); // not silently flipped to Buy
        assert_eq!(open[1].direction, Direction::Buy);
        assert_eq!(open[0].initial_stop_loss, 95.0);
    }

    #[test]
    fn duplicate_trade_id_is_rejected_not_overwritten() {
        let db = TradeDb::open(":memory:").unwrap();
        db.insert(&open_rec(1, Direction::Buy)).unwrap();
        // Same primary key must error rather than silently replace the open row.
        assert!(db.insert(&open_rec(1, Direction::Sell)).is_err());
        let open = db.load_open(7).unwrap();
        assert_eq!(open.len(), 1);
        assert_eq!(open[0].direction, Direction::Buy);
    }

    #[test]
    fn load_closed_since_filters_by_strategy_and_exit_time() {
        let db = TradeDb::open(":memory:").unwrap();

        let mut old_trade = open_rec(1, Direction::Buy);
        old_trade.entry_time = 100;
        db.insert(&old_trade).unwrap();
        old_trade.exit_price = 110.0;
        old_trade.exit_time = 200; // before the "since" cutoff
        db.close(&old_trade).unwrap();

        let mut recent_trade = open_rec(2, Direction::Sell);
        recent_trade.entry_time = 1_000;
        db.insert(&recent_trade).unwrap();
        recent_trade.exit_price = 90.0;
        recent_trade.exit_time = 2_000; // after the "since" cutoff
        db.close(&recent_trade).unwrap();

        let mut still_open = open_rec(3, Direction::Buy);
        still_open.entry_time = 1_500;
        db.insert(&still_open).unwrap();

        let closed = db.load_closed_since(7, 1_000).unwrap();
        assert_eq!(closed.len(), 1);
        assert_eq!(closed[0].trade_id, 2);
        assert_eq!(closed[0].exit_price, 90.0);
    }
}

fn parse_exit(s: &str) -> ExitReason {
    match s {
        "TakeProfit" => ExitReason::TakeProfit,
        "StopProfit" => ExitReason::StopProfit,
        "ExitRule" => ExitReason::ExitRule,
        "EndOfData" => ExitReason::EndOfData,
        _ => ExitReason::StopLoss,
    }
}
