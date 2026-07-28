use crate::{math, Indicator};
use ts_core::Bar;

pub struct Macd {
    pub fast: usize,
    pub slow: usize,
    pub signal: usize,
}

impl Macd {
    pub fn new(fast: usize, slow: usize, signal: usize) -> Self {
        Self { fast, slow, signal }
    }
}

impl Indicator for Macd {
    fn compute(&self, bars: &[Bar]) -> Vec<(String, Vec<f64>)> {
        let c: Vec<f64> = bars.iter().map(|b| b.close).collect();
        let fe = math::ema(&c, self.fast);
        let se = math::ema(&c, self.slow);
        let n = c.len();

        let mut ml = vec![f64::NAN; n];
        for i in 0..n {
            if !fe[i].is_nan() && !se[i].is_nan() {
                ml[i] = fe[i] - se[i];
            }
        }

        // Seed signal EMA from the first `signal` valid MACD values (index slow-1 onward).
        let sl = math::ema_from(&ml, self.signal, self.slow - 1);

        let mut hist = vec![f64::NAN; n];
        for i in 0..n {
            if !ml[i].is_nan() && !sl[i].is_nan() {
                hist[i] = ml[i] - sl[i];
            }
        }

        vec![
            ("macd".into(), ml),
            ("macd_signal".into(), sl),
            ("macd_hist".into(), hist),
        ]
    }
}
