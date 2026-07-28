use anyhow::{Context, Result};
use arrow::array::{Float64Array, Int64Array};
use arrow::datatypes::{DataType, Field, Schema, SchemaRef};
use arrow::record_batch::RecordBatch;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use parquet::arrow::ArrowWriter;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use ts_core::{Bar, Timeframe};

static OHLCV_SCHEMA: OnceLock<SchemaRef> = OnceLock::new();

fn ohlcv_schema() -> SchemaRef {
    OHLCV_SCHEMA
        .get_or_init(|| {
            Arc::new(Schema::new(vec![
                Field::new("time", DataType::Int64, false),
                Field::new("open", DataType::Float64, false),
                Field::new("high", DataType::Float64, false),
                Field::new("low", DataType::Float64, false),
                Field::new("close", DataType::Float64, false),
                Field::new("volume", DataType::Float64, false),
            ]))
        })
        .clone()
}

pub struct OhlcvCache {
    root: PathBuf,
    locks: Arc<dashmap::DashMap<String, ()>>,
}

impl OhlcvCache {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        fs::create_dir_all(&root).ok();
        Self {
            root,
            locks: Arc::new(dashmap::DashMap::new()),
        }
    }

    fn cache_path(&self, sym: &str, tf: Timeframe) -> PathBuf {
        self.root.join(format!("{}_{}.parquet", sym, tf.to_str()))
    }

    /// Read every bar from a parquet file. Does NOT take the per-symbol lock, so
    /// it can be called by both `load` and `save` while the lock is already held
    /// (taking the same `DashMap` entry twice would otherwise deadlock).
    fn read_all_unlocked(path: &std::path::Path) -> Result<Vec<Bar>> {
        if !path.exists() {
            return Ok(Vec::new());
        }
        let file = fs::File::open(path).context("open parquet")?;
        let reader = ParquetRecordBatchReaderBuilder::try_new(file)?
            .build()
            .context("build parquet reader")?;

        let mut bars = Vec::new();
        for batch in reader {
            let b = batch.context("read batch")?;
            // Defensive downcasts: a schema mismatch (old/corrupted file) returns
            // an error instead of panicking via unwrap().
            let times = downcast_i64(&b, 0)?;
            let opens = downcast_f64(&b, 1)?;
            let highs = downcast_f64(&b, 2)?;
            let lows = downcast_f64(&b, 3)?;
            let closes = downcast_f64(&b, 4)?;
            let volumes = downcast_f64(&b, 5)?;

            for i in 0..b.num_rows() {
                bars.push(Bar::new(
                    times.value(i),
                    opens.value(i),
                    highs.value(i),
                    lows.value(i),
                    closes.value(i),
                    volumes.value(i),
                ));
            }
        }
        Ok(bars)
    }

    pub fn load(&self, sym: &str, tf: Timeframe, start: i64, end: i64) -> Result<Vec<Bar>> {
        let path = self.cache_path(sym, tf);
        if !path.exists() {
            return Ok(Vec::new());
        }

        let _guard = self.locks.entry(format!("{}_{}", sym, tf)).or_insert(());
        let mut bars: Vec<Bar> = Self::read_all_unlocked(&path)?
            .into_iter()
            .filter(|b| b.time >= start && b.time <= end)
            .collect();
        bars.sort_by_key(|b| b.time);
        Ok(bars)
    }

    pub fn time_range(
        &self,
        sym: &str,
        tf: Timeframe,
        start: i64,
        end: i64,
    ) -> Result<Option<(i64 /*first*/, i64 /*last*/, usize /*count*/)>> {
        let bars = self.load(sym, tf, start, end)?;
        if bars.is_empty() {
            Ok(None)
        } else {
            let first = bars.first().unwrap().time;
            let last = bars.last().unwrap().time;
            Ok(Some((first, last, bars.len())))
        }
    }

    pub fn load_all(&self, sym: &str, tf: Timeframe) -> Result<Vec<Bar>> {
        self.load(sym, tf, i64::MIN, i64::MAX)
    }

    /// Persist `bars`, MERGING with any already-cached bars for this symbol/tf
    /// (union by timestamp; the newly supplied bar wins on a tie). Previously
    /// this overwrote the whole file, so backtesting a different date range
    /// silently discarded all other cached data and forced repeated downloads.
    pub fn save(&self, sym: &str, tf: Timeframe, bars: &[Bar]) -> Result<()> {
        if bars.is_empty() {
            return Ok(());
        }
        let path = self.cache_path(sym, tf);
        let _guard = self.locks.entry(format!("{}_{}", sym, tf)).or_insert(());

        let mut merged: std::collections::BTreeMap<i64, Bar> = Self::read_all_unlocked(&path)?
            .into_iter()
            .map(|b| (b.time, b))
            .collect();
        for b in bars {
            merged.insert(b.time, *b);
        }
        let bars: Vec<Bar> = merged.into_values().collect();

        let schema = ohlcv_schema();
        let times = Int64Array::from_iter_values(bars.iter().map(|b| b.time));
        let opens = Float64Array::from_iter_values(bars.iter().map(|b| b.open));
        let highs = Float64Array::from_iter_values(bars.iter().map(|b| b.high));
        let lows = Float64Array::from_iter_values(bars.iter().map(|b| b.low));
        let closes = Float64Array::from_iter_values(bars.iter().map(|b| b.close));
        let volumes = Float64Array::from_iter_values(bars.iter().map(|b| b.volume));

        let batch = RecordBatch::try_new(
            schema.clone(),
            vec![
                Arc::new(times),
                Arc::new(opens),
                Arc::new(highs),
                Arc::new(lows),
                Arc::new(closes),
                Arc::new(volumes),
            ],
        )
        .context("create record batch")?;

        let tmp = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
        let mut writer = ArrowWriter::try_new(tmp.as_file(), schema, None)?;
        writer.write(&batch)?;
        writer.close().context("close parquet writer")?;
        tmp.persist(&path).context("persist parquet file")?;
        Ok(())
    }

    pub fn append_bar(&self, sym: &str, tf: Timeframe, bar: &Bar) -> Result<()> {
        // `save` now merges by timestamp, so a single-bar save is an upsert.
        self.save(sym, tf, std::slice::from_ref(bar))
    }

    pub fn clear(&self) -> Result<()> {
        for entry in fs::read_dir(&self.root)? {
            let path = entry?.path();
            if path.extension().is_some_and(|e| e == "parquet") {
                fs::remove_file(&path).ok();
            }
        }
        Ok(())
    }
}

fn downcast_f64(b: &RecordBatch, idx: usize) -> Result<&Float64Array> {
    b.column(idx)
        .as_any()
        .downcast_ref::<Float64Array>()
        .context("OHLCV parquet column is not Float64 (schema mismatch / corrupted cache)")
}

fn downcast_i64(b: &RecordBatch, idx: usize) -> Result<&Int64Array> {
    b.column(idx)
        .as_any()
        .downcast_ref::<Int64Array>()
        .context("OHLCV parquet column is not Int64 (schema mismatch / corrupted cache)")
}
