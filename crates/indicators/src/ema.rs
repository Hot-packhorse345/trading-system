use crate::{math, Indicator};
use ts_core::Bar;
pub struct Ema {
    pub period: usize,
}
pub struct Sma {
    pub period: usize,
}
pub struct Wma {
    pub period: usize,
}
impl Indicator for Ema {
    fn compute(&self, bars: &[Bar]) -> Vec<(String, Vec<f64>)> {
        let c: Vec<f64> = bars.iter().map(|b| b.close).collect();
        vec![(format!("ema_{}", self.period), math::ema(&c, self.period))]
    }
}
impl Indicator for Sma {
    fn compute(&self, bars: &[Bar]) -> Vec<(String, Vec<f64>)> {
        let c: Vec<f64> = bars.iter().map(|b| b.close).collect();
        vec![(format!("sma_{}", self.period), math::sma(&c, self.period))]
    }
}
impl Indicator for Wma {
    fn compute(&self, bars: &[Bar]) -> Vec<(String, Vec<f64>)> {
        let c: Vec<f64> = bars.iter().map(|b| b.close).collect();
        vec![(format!("wma_{}", self.period), math::wma(&c, self.period))]
    }
}
