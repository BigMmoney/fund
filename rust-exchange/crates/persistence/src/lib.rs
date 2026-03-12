use anyhow::Result;
use parking_lot::Mutex;
use serde::{de::DeserializeOwned, Serialize};
use std::fs::{create_dir_all, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::marker::PhantomData;
use std::path::{Path, PathBuf};

pub trait WalStore<T>: Send + Sync
where
    T: Clone + Serialize + DeserializeOwned + Send + Sync + 'static,
{
    fn append(&self, record: &T) -> Result<()>;
    fn entries(&self) -> Result<Vec<T>>;
}

#[derive(Debug, Default)]
pub struct InMemoryWal<T>
where
    T: Clone + Serialize + DeserializeOwned + Send + Sync + 'static,
{
    entries: Mutex<Vec<T>>,
}

impl<T> InMemoryWal<T>
where
    T: Clone + Serialize + DeserializeOwned + Send + Sync + 'static,
{
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(Vec::new()),
        }
    }
}

impl<T> WalStore<T> for InMemoryWal<T>
where
    T: Clone + Serialize + DeserializeOwned + Send + Sync + 'static,
{
    fn append(&self, record: &T) -> Result<()> {
        self.entries.lock().push(record.clone());
        Ok(())
    }

    fn entries(&self) -> Result<Vec<T>> {
        Ok(self.entries.lock().clone())
    }
}

#[derive(Debug)]
pub struct JsonlFileWal<T>
where
    T: Clone + Serialize + DeserializeOwned + Send + Sync + 'static,
{
    path: PathBuf,
    write_lock: Mutex<()>,
    _marker: PhantomData<T>,
}

impl<T> JsonlFileWal<T>
where
    T: Clone + Serialize + DeserializeOwned + Send + Sync + 'static,
{
    pub fn new(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                create_dir_all(parent)?;
            }
        }
        if !Path::new(&path).exists() {
            File::create(&path)?;
        }
        Ok(Self {
            path,
            write_lock: Mutex::new(()),
            _marker: PhantomData,
        })
    }
}

impl<T> WalStore<T> for JsonlFileWal<T>
where
    T: Clone + Serialize + DeserializeOwned + Send + Sync + 'static,
{
    fn append(&self, record: &T) -> Result<()> {
        let _guard = self.write_lock.lock();
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        let line = serde_json::to_string(record)?;
        file.write_all(line.as_bytes())?;
        file.write_all(b"\n")?;
        file.flush()?;
        Ok(())
    }

    fn entries(&self) -> Result<Vec<T>> {
        let _guard = self.write_lock.lock();
        let file = OpenOptions::new().read(true).open(&self.path)?;
        let reader = BufReader::new(file);
        let mut entries = Vec::new();
        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            entries.push(serde_json::from_str(&line)?);
        }
        Ok(entries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn in_memory_wal_round_trips_entries() {
        let wal = InMemoryWal::<String>::new();
        wal.append(&"entry-1".to_string()).unwrap();
        wal.append(&"entry-2".to_string()).unwrap();

        assert_eq!(
            wal.entries().unwrap(),
            vec!["entry-1".to_string(), "entry-2".to_string()]
        );
    }

    #[test]
    fn jsonl_file_wal_round_trips_entries() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("rust_exchange_wal_{unique}.jsonl"));

        let wal = JsonlFileWal::<String>::new(&path).unwrap();
        wal.append(&"entry-a".to_string()).unwrap();
        wal.append(&"entry-b".to_string()).unwrap();

        assert_eq!(
            wal.entries().unwrap(),
            vec!["entry-a".to_string(), "entry-b".to_string()]
        );

        std::fs::remove_file(path).unwrap();
    }
}
