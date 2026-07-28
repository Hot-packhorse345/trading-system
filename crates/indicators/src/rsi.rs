use crate::Indicator;
use ts_core::Bar;

pub struct Rsi {
    pub period: usize,
    pub col: String,
}

impl Rsi {
    pub fn new(period: usize) -> Self {
        Self {
            period,
            col: format!("rsi_{period}"),
        }
    }
}

impl Indicator for Rsi {
    fn compute(&self, bars: &[Bar]) -> Vec<(String, Vec<f64>)> {
        let closes: Vec<f64> = bars.iter().map(|b| b.close).collect();
        let n = closes.len();

        let mut rsi = vec![f64::NAN; n];

        if n <= self.period {
            return vec![(self.col.clone(), rsi)];
        }

        // Price changes
        let mut gains = vec![0.0; n];
        let mut losses = vec![0.0; n];

        for i in 1..n {
            let diff = closes[i] - closes[i - 1];

            if diff > 0.0 {
                gains[i] = diff;
            } else {
                losses[i] = -diff;
            }
        }

        // TA-Lib seed:
        // SMA of first `period` gains/losses.
        let mut avg_gain = gains[1..=self.period].iter().sum::<f64>() / self.period as f64;

        let mut avg_loss = losses[1..=self.period].iter().sum::<f64>() / self.period as f64;

        // First RSI value at index = period
        rsi[self.period] = match (avg_gain, avg_loss) {
            (0.0, 0.0) => 0.0,
            (_, 0.0) => 100.0,
            (0.0, _) => 0.0,
            _ => {
                let rs = avg_gain / avg_loss;
                100.0 - (100.0 / (1.0 + rs))
            }
        };

        // Wilder smoothing
        let p = self.period as f64;

        for i in (self.period + 1)..n {
            avg_gain = ((p - 1.0) * avg_gain + gains[i]) / p;
            avg_loss = ((p - 1.0) * avg_loss + losses[i]) / p;

            rsi[i] = match (avg_gain, avg_loss) {
                (0.0, 0.0) => 0.0,
                (_, 0.0) => 100.0,
                (0.0, _) => 0.0,
                _ => {
                    let rs = avg_gain / avg_loss;
                    100.0 - (100.0 / (1.0 + rs))
                }
            };
        }

        vec![(self.col.clone(), rsi)]
    }
}
