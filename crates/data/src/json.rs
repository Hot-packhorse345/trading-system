use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

pub struct JsonCache {
    root: PathBuf,
}

impl JsonCache {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        fs::create_dir_all(&root).ok();
        Self { root }
    }

    pub fn get<T: for<'de> Deserialize<'de>>(&self, name: &str) -> Result<Option<T>> {
        let path = self.root.join(format!("{}.json", name));
        if !path.exists() {
            return Ok(None);
        }
        let data = fs::read_to_string(&path)?;
        Ok(Some(serde_json::from_str(&data)?))
    }

    pub fn put<T: Serialize>(&self, name: &str, data: &T) -> Result<()> {
        let path = self.root.join(format!("{}.json", name));
        let json = serde_json::to_string_pretty(data)?;

        let tmp = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
        fs::write(&tmp, json)?;
        tmp.persist(&path)?;
        Ok(())
    }

    pub fn invalidate(&self, name: &str) {
        let path = self.root.join(format!("{}.json", name));
        if path.exists() {
            fs::remove_file(&path).ok();
        }
    }
}
