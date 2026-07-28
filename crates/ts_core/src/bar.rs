use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Bar {
    pub time: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
}

impl Bar {
    pub fn new(time: i64, open: f64, high: f64, low: f64, close: f64, volume: f64) -> Self {
        Self {
            time,
            open,
            high,
            low,
            close,
            volume,
        }
    }

    pub fn true_range(&self, prev_close: f64) -> f64 {
        let hl = self.high - self.low;
        let hc = (self.high - prev_close).abs();
        let lc = (self.low - prev_close).abs();
        hl.max(hc).max(lc)
    }

    pub fn mid(&self) -> f64 {
        (self.high + self.low) / 2.0
    }
    pub fn typical(&self) -> f64 {
        (self.high + self.low + self.close) / 3.0
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tick {
    pub symbol: String,
    pub bid: f64,
    pub ask: f64,
    pub last: f64,
    pub volume: f64,
    pub timestamp: f64,
}

pub struct CircularBuffer {
    inner: Vec<Bar>,
    capacity: usize,
    write: usize,
    len: usize,
}

impl CircularBuffer {
    pub fn new(capacity: usize) -> Self {
        // A zero capacity would make `push`'s modulo arithmetic divide by zero.
        let capacity = capacity.max(1);
        Self {
            inner: Vec::with_capacity(capacity),
            capacity,
            write: 0,
            len: 0,
        }
    }

    pub fn push(&mut self, bar: Bar) {
        if self.len < self.capacity {
            self.inner.push(bar);
            self.len += 1;
        } else {
            self.inner[self.write] = bar;
        }
        self.write = (self.write + 1) % self.capacity;
    }

    pub fn len(&self) -> usize {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn as_vec(&self) -> Vec<Bar> {
        if self.len < self.capacity {
            self.inner.clone()
        } else {
            (0..self.capacity)
                .map(|i| self.inner[(self.write + i) % self.capacity])
                .collect()
        }
    }
}
