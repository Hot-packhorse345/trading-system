use crate::error::CoreError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Buy,
    Sell,
    Hold,
}

impl Direction {
    pub fn opposite(self) -> Self {
        match self {
            Direction::Buy => Direction::Sell,
            Direction::Sell => Direction::Buy,
            Direction::Hold => Direction::Hold,
        }
    }
    pub fn is_long(self) -> bool {
        self == Direction::Buy
    }
    pub fn is_short(self) -> bool {
        self == Direction::Sell
    }
    pub fn is_hold(self) -> bool {
        self == Direction::Hold
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExitReason {
    StopLoss,
    StopProfit,
    TakeProfit,
    ExitRule,
    EndOfData,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Timeframe {
    M1,
    M2,
    M3,
    M5,
    M6,
    M10,
    M12,
    M15,
    M20,
    M30,
    H1,
    H2,
    H3,
    H4,
    H6,
    H8,
    H12,
    D1,
    W1,
    MN1,
    TICK,
}

impl Timeframe {
    pub fn seconds(self) -> i64 {
        self.try_seconds()
            .expect("TICK timeframe has no fixed bar duration — use try_seconds()")
    }

    pub fn try_seconds(self) -> Option<i64> {
        match self {
            Timeframe::M1 => Some(60),
            Timeframe::M2 => Some(120),
            Timeframe::M3 => Some(180),
            Timeframe::M5 => Some(300),
            Timeframe::M6 => Some(360),
            Timeframe::M10 => Some(600),
            Timeframe::M12 => Some(720),
            Timeframe::M15 => Some(900),
            Timeframe::M20 => Some(1200),
            Timeframe::M30 => Some(1_800),
            Timeframe::H1 => Some(3_600),
            Timeframe::H2 => Some(7_200),
            Timeframe::H3 => Some(10_800),
            Timeframe::H4 => Some(14_400),
            Timeframe::H6 => Some(21_600),
            Timeframe::H8 => Some(28_800),
            Timeframe::H12 => Some(43_200),
            Timeframe::D1 => Some(86_400),
            Timeframe::W1 => Some(604_800),
            Timeframe::MN1 => Some(2_592_000),
            Timeframe::TICK => None,
        }
    }

    pub fn to_str(self) -> &'static str {
        match self {
            Timeframe::M1 => "M1",
            Timeframe::M2 => "M2",
            Timeframe::M3 => "M3",
            Timeframe::M5 => "M5",
            Timeframe::M6 => "M6",
            Timeframe::M10 => "M10",
            Timeframe::M12 => "M12",
            Timeframe::M15 => "M15",
            Timeframe::M20 => "M20",
            Timeframe::M30 => "M30",
            Timeframe::H1 => "H1",
            Timeframe::H2 => "H2",
            Timeframe::H3 => "H3",
            Timeframe::H4 => "H4",
            Timeframe::H6 => "H6",
            Timeframe::H8 => "H8",
            Timeframe::H12 => "H12",
            Timeframe::D1 => "D1",
            Timeframe::W1 => "W1",
            Timeframe::MN1 => "MN1",
            Timeframe::TICK => "TICK",
        }
    }

    pub fn to_binance(self) -> &'static str {
        match self {
            Timeframe::M1 => "1m",
            Timeframe::M2 => "2m",
            Timeframe::M3 => "3m",
            Timeframe::M5 => "5m",
            Timeframe::M6 => "6m",
            Timeframe::M10 => "10m",
            Timeframe::M12 => "12m",
            Timeframe::M15 => "15m",
            Timeframe::M20 => "20m",
            Timeframe::M30 => "30m",
            Timeframe::H1 => "1h",
            Timeframe::H2 => "2h",
            Timeframe::H3 => "3h",
            Timeframe::H4 => "4h",
            Timeframe::H6 => "6h",
            Timeframe::H8 => "8h",
            Timeframe::H12 => "12h",
            Timeframe::D1 => "1d",
            Timeframe::W1 => "1w",
            Timeframe::MN1 => "1M",
            Timeframe::TICK => "1s",
        }
    }
}

impl std::fmt::Display for Timeframe {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_str())
    }
}

impl std::str::FromStr for Timeframe {
    type Err = CoreError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        parse_timeframe(s)
    }
}

pub fn parse_timeframe(s: &str) -> Result<Timeframe, CoreError> {
    match s {
        "1m" | "M1" => Ok(Timeframe::M1),
        "2m" | "M2" => Ok(Timeframe::M2),
        "3m" | "M3" => Ok(Timeframe::M3),
        "5m" | "M5" => Ok(Timeframe::M5),
        "6m" | "M6" => Ok(Timeframe::M6),
        "10m" | "M10" => Ok(Timeframe::M10),
        "12m" | "M12" => Ok(Timeframe::M12),
        "15m" | "M15" => Ok(Timeframe::M15),
        "20m" | "M20" => Ok(Timeframe::M20),
        "30m" | "M30" => Ok(Timeframe::M30),
        "1h" | "H1" => Ok(Timeframe::H1),
        "2h" | "H2" => Ok(Timeframe::H2),
        "3h" | "H3" => Ok(Timeframe::H3),
        "4h" | "H4" => Ok(Timeframe::H4),
        "6h" | "H6" => Ok(Timeframe::H6),
        "8h" | "H8" => Ok(Timeframe::H8),
        "12h" | "H12" => Ok(Timeframe::H12),
        "1d" | "D1" => Ok(Timeframe::D1),
        "1w" | "W1" => Ok(Timeframe::W1),
        "1M" | "MN1" => Ok(Timeframe::MN1),
        other => Err(CoreError::Config(format!("unknown timeframe: {other}"))),
    }
}
