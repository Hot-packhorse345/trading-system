use anyhow::Result;
use arrow::array::{ArrayRef, Float64Array};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::{RecordBatch, RecordBatchReader};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use parquet::arrow::ArrowWriter;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

pub struct IndicatorCache {
    root: PathBuf,
}

impl IndicatorCache {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        fs::create_dir_all(&root).ok();
        Self { root }
    }

    fn key(ohlcv_hash: &str, config_json: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(ohlcv_hash.as_bytes());
        hasher.update(config_json.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    pub fn get(
        &self,
        ohlcv_hash: &str,
        config_json: &str,
    ) -> Result<Option<HashMap<String, Vec<f64>>>> {
        let path = self
            .root
            .join(format!("{}.parquet", Self::key(ohlcv_hash, config_json)));
        if !path.exists() {
            return Ok(None);
        }

        let file = fs::File::open(&path)?;
        let reader = ParquetRecordBatchReaderBuilder::try_new(file)?.build()?;
        let schema = reader.schema();
        let mut values: HashMap<String, Vec<f64>> = schema
            .fields()
            .iter()
            .map(|f| (f.name().clone(), Vec::new()))
            .collect();
        for batch in reader {
            let b = batch?;
            for (idx, field) in b.schema().fields().iter().enumerate() {
                // A corrupted or schema-mismatched cache entry is treated as a
                // miss (return None) rather than panicking on a failed downcast.
                let Some(col) = b.column(idx).as_any().downcast_ref::<Float64Array>() else {
                    return Ok(None);
                };
                if let Some(dst) = values.get_mut(field.name()) {
                    dst.extend(col.values().iter().copied());
                }
            }
        }
        Ok(Some(values))
    }

    pub fn put(
        &self,
        ohlcv_hash: &str,
        config_json: &str,
        columns: &[(String, Vec<f64>)],
    ) -> Result<()> {
        let path = self
            .root
            .join(format!("{}.parquet", Self::key(ohlcv_hash, config_json)));
        let fields: Vec<Field> = columns
            .iter()
            .map(|(name, _)| Field::new(name, DataType::Float64, true))
            .collect();
        let schema = Arc::new(Schema::new(fields));
        let arrays: Vec<ArrayRef> = columns
            .iter()
            .map(|(_, values)| {
                Arc::new(Float64Array::from_iter_values(values.iter().copied())) as ArrayRef
            })
            .collect();
        let batch = RecordBatch::try_new(schema.clone(), arrays)?;

        let tmp = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
        let mut writer = ArrowWriter::try_new(tmp.as_file(), schema, None)?;
        writer.write(&batch)?;
        writer.close()?;
        if path.exists() {
            fs::remove_file(&path).ok();
        }
        tmp.persist(&path)?;
        Ok(())
    }

    pub fn invalidate(&self, ohlcv_hash: &str, config_json: &str) {
        let path = self
            .root
            .join(format!("{}.parquet", Self::key(ohlcv_hash, config_json)));
        if path.exists() {
            fs::remove_file(&path).ok();
        }
    }
}
