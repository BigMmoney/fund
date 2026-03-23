//! Write-ahead log (WAL) persistence layer.
//!
//! Provides `WalStore<T>` trait with in-memory and JSONL file-based implementations.
//! File WAL entries use CRC-32 checksums for integrity verification. Supports
//! automatic rotation at configurable thresholds and group-commit batching for
//! throughput optimisation.

use anyhow::{bail, Result};
use parking_lot::Mutex;
use serde::{de::DeserializeOwned, Serialize};
use std::fs::{create_dir_all, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Controls how corrupt WAL entries are handled during recovery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WalRecoveryMode {
    /// Abort on the first corrupt entry (default, strict).
    Strict,
    /// Log corrupt entries and skip them, returning all valid records.
    BestEffort,
}

/// Append-only store for write-ahead log records.
///
/// Guarantees: entries preserve insertion order; `append` is atomic per-record.
pub trait WalStore<T>: Send + Sync
where
    T: Clone + Serialize + DeserializeOwned + Send + Sync + 'static,
{
    fn append(&self, record: &T) -> Result<()>;
    fn entries(&self) -> Result<Vec<T>>;
    /// Number of entries appended since creation (for rotation decisions).
    fn len(&self) -> u64 {
        0
    }

    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Force durability flush (fsync). No-op for in-memory stores.
    fn sync(&self) -> Result<()> {
        Ok(())
    }
}

/// CRC-32 using industry-standard `crc32fast` crate for WAL entry integrity.
fn crc32(data: &[u8]) -> u32 {
    crc32fast::hash(data)
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

    fn len(&self) -> u64 {
        self.entries.lock().len() as u64
    }
}

#[derive(Debug)]
pub struct JsonlFileWal<T>
where
    T: Clone + Serialize + DeserializeOwned + Send + Sync + 'static,
{
    path: PathBuf,
    write_lock: Mutex<()>,
    append_count: AtomicU64,
    /// Maximum entries before automatic rotation (0 = disabled).
    max_entries: u64,
    /// Group-commit batch size. 0 = fsync every append (default, safest).
    group_commit_size: u64,
    /// Writes since last fsync (for group-commit batching).
    pending_syncs: AtomicU64,
    _marker: PhantomData<T>,
}

impl<T> JsonlFileWal<T>
where
    T: Clone + Serialize + DeserializeOwned + Send + Sync + 'static,
{
    pub fn new(path: impl Into<PathBuf>) -> Result<Self> {
        Self::with_rotation(path, 0)
    }

    pub fn with_rotation(path: impl Into<PathBuf>, max_entries: u64) -> Result<Self> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                create_dir_all(parent)?;
            }
        }
        if !Path::new(&path).exists() {
            File::create(&path)?;
        }
        // Count existing entries for rotation tracking.
        let initial_count = {
            let file = OpenOptions::new().read(true).open(&path)?;
            BufReader::new(file)
                .lines()
                .map_while(Result::ok)
                .filter(|line| !line.trim().is_empty())
                .count() as u64
        };
        Ok(Self {
            path,
            write_lock: Mutex::new(()),
            append_count: AtomicU64::new(initial_count),
            max_entries,
            group_commit_size: 0,
            pending_syncs: AtomicU64::new(0),
            _marker: PhantomData,
        })
    }

    /// Enable group-commit: fsync is deferred until `group_commit_size` appends
    /// accumulate, amortising the per-append durability cost. A value of 0
    /// (the default) syncs every append for maximum safety.
    pub fn with_group_commit(mut self, size: u64) -> Self {
        self.group_commit_size = size;
        self
    }

    /// Load entries with the specified recovery mode.
    ///
    /// In `BestEffort` mode, CRC-mismatched or malformed entries are skipped
    /// (with a tracing::error log) instead of aborting the entire load.
    pub fn entries_with_recovery(&self, mode: WalRecoveryMode) -> Result<Vec<T>> {
        let _guard = self.write_lock.lock();
        let file = OpenOptions::new().read(true).open(&self.path)?;
        let reader = BufReader::new(file);
        let mut entries = Vec::new();
        let mut skipped = 0u64;
        for (lineno, line) in reader.lines().enumerate() {
            let line = match line {
                Ok(l) => l,
                Err(e) => {
                    if mode == WalRecoveryMode::BestEffort {
                        tracing::error!(line = lineno + 1, error = %e, "WAL: skipping unreadable line");
                        skipped += 1;
                        continue;
                    }
                    return Err(e.into());
                }
            };
            if line.trim().is_empty() {
                continue;
            }
            let json_str = if let Some((crc_hex, payload)) = line.split_once('\t') {
                let expected = u32::from_str_radix(crc_hex, 16).unwrap_or(0);
                let actual = crc32(payload.as_bytes());
                if expected != actual {
                    if mode == WalRecoveryMode::BestEffort {
                        tracing::error!(
                            line = lineno + 1,
                            expected = format!("{expected:08x}"),
                            actual = format!("{actual:08x}"),
                            "WAL: CRC mismatch — skipping corrupt entry"
                        );
                        skipped += 1;
                        continue;
                    }
                    bail!(
                        "WAL CRC mismatch at line {}: expected {expected:08x}, got {actual:08x}",
                        lineno + 1
                    );
                }
                payload
            } else {
                &line
            };
            match serde_json::from_str(json_str) {
                Ok(entry) => entries.push(entry),
                Err(e) => {
                    if mode == WalRecoveryMode::BestEffort {
                        tracing::error!(line = lineno + 1, error = %e, "WAL: skipping malformed JSON entry");
                        skipped += 1;
                        continue;
                    }
                    return Err(e.into());
                }
            }
        }
        if skipped > 0 {
            tracing::warn!(
                skipped,
                total = entries.len() + skipped as usize,
                recovered = entries.len(),
                "WAL: best-effort recovery completed with skipped entries"
            );
        }
        Ok(entries)
    }

    /// Rotate the WAL file: rename current to `.bak.<timestamp>` and create a fresh file.
    pub fn rotate(&self) -> Result<()> {
        let _guard = self.write_lock.lock();
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let bak = self.path.with_extension(format!("bak.{ts}"));
        std::fs::rename(&self.path, &bak)?;
        File::create(&self.path)?;
        self.append_count.store(0, Ordering::Release);
        Ok(())
    }
}

impl<T> WalStore<T> for JsonlFileWal<T>
where
    T: Clone + Serialize + DeserializeOwned + Send + Sync + 'static,
{
    fn append(&self, record: &T) -> Result<()> {
        let _guard = self.write_lock.lock();

        // Auto-rotate if threshold reached.
        let count = self.append_count.load(Ordering::Acquire);
        if self.max_entries > 0 && count >= self.max_entries {
            drop(_guard);
            self.rotate()?;
            return self.append(record);
        }

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        let json = serde_json::to_string(record)?;
        let checksum = crc32(json.as_bytes());
        // Format: CRC32_HEX<TAB>JSON\n
        write!(file, "{checksum:08x}\t")?;
        file.write_all(json.as_bytes())?;
        file.write_all(b"\n")?;
        file.flush()?;
        // Group-commit: batch fsync calls to amortise disk latency.
        let pending = self.pending_syncs.fetch_add(1, Ordering::AcqRel) + 1;
        if self.group_commit_size == 0 || pending >= self.group_commit_size {
            file.sync_all()?; // fsync — guarantees durability on OS crash
            self.pending_syncs.store(0, Ordering::Release);
            self.append_count.fetch_add(pending, Ordering::Release);
        }
        Ok(())
    }

    fn entries(&self) -> Result<Vec<T>> {
        let _guard = self.write_lock.lock();
        let file = OpenOptions::new().read(true).open(&self.path)?;
        let reader = BufReader::new(file);
        let mut entries = Vec::new();
        for (lineno, line) in reader.lines().enumerate() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            // Support both legacy (bare JSON) and CRC-prefixed format.
            let json_str = if let Some((crc_hex, payload)) = line.split_once('\t') {
                let expected = u32::from_str_radix(crc_hex, 16).unwrap_or(0);
                let actual = crc32(payload.as_bytes());
                if expected != actual {
                    bail!(
                        "WAL CRC mismatch at line {}: expected {expected:08x}, got {actual:08x}",
                        lineno + 1
                    );
                }
                payload
            } else {
                // Legacy line without CRC — accept for backward compatibility.
                &line
            };
            entries.push(serde_json::from_str(json_str)?);
        }
        Ok(entries)
    }

    fn len(&self) -> u64 {
        self.append_count.load(Ordering::Acquire)
    }

    fn sync(&self) -> Result<()> {
        let _guard = self.write_lock.lock();
        if self.pending_syncs.load(Ordering::Acquire) > 0 {
            let file = OpenOptions::new().write(true).open(&self.path)?;
            file.sync_all()?;
            self.pending_syncs.store(0, Ordering::Release);
        }
        Ok(())
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
        assert_eq!(wal.len(), 2);
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
        assert_eq!(wal.len(), 2);

        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn crc32_detects_corruption() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("rust_exchange_wal_crc_{unique}.jsonl"));

        let wal = JsonlFileWal::<String>::new(&path).unwrap();
        wal.append(&"good-data".to_string()).unwrap();

        // Corrupt the JSON payload while keeping CRC intact.
        let content = std::fs::read_to_string(&path).unwrap();
        let corrupted = content.replace("good-data", "baad-data");
        std::fs::write(&path, corrupted).unwrap();

        let wal2 = JsonlFileWal::<String>::new(&path).unwrap();
        assert!(wal2.entries().is_err(), "should detect CRC mismatch");

        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn wal_rotation_creates_backup() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("rust_exchange_wal_rot_{unique}.jsonl"));

        let wal = JsonlFileWal::<String>::new(&path).unwrap();
        wal.append(&"before-rotate".to_string()).unwrap();

        wal.rotate().unwrap();
        // After rotation the main file should be empty.
        assert_eq!(wal.entries().unwrap().len(), 0);
        assert_eq!(wal.len(), 0);

        // A backup file should exist.
        let backups: Vec<_> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(&format!("rust_exchange_wal_rot_{unique}.bak"))
            })
            .collect();
        assert!(!backups.is_empty(), "rotation should create a backup file");

        // Clean up.
        std::fs::remove_file(&path).unwrap();
        for b in backups {
            std::fs::remove_file(b.path()).unwrap();
        }
    }

    #[test]
    fn legacy_bare_json_lines_still_readable() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("rust_exchange_wal_legacy_{unique}.jsonl"));

        // Write bare JSON (no CRC prefix) to simulate legacy WAL.
        std::fs::write(&path, "\"legacy-entry\"\n").unwrap();

        let wal = JsonlFileWal::<String>::new(&path).unwrap();
        assert_eq!(wal.entries().unwrap(), vec!["legacy-entry".to_string()]);

        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn in_memory_wal_is_empty_initially() {
        let wal = InMemoryWal::<String>::new();
        assert!(wal.is_empty());
        assert_eq!(wal.len(), 0);
        assert!(wal.entries().unwrap().is_empty());
    }

    #[test]
    fn in_memory_wal_sync_is_noop() {
        let wal = InMemoryWal::<String>::new();
        assert!(wal.sync().is_ok());
    }

    #[test]
    fn jsonl_file_wal_empty_file_returns_no_entries() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("rust_exchange_wal_empty_{unique}.jsonl"));

        let wal = JsonlFileWal::<String>::new(&path).unwrap();
        assert!(wal.entries().unwrap().is_empty());
        assert_eq!(wal.len(), 0);
        assert!(wal.is_empty());

        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn jsonl_file_wal_auto_rotation_resets_count() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("rust_exchange_wal_autorot_{unique}.jsonl"));

        // Set max_entries = 3 so rotation happens on 4th append
        let wal = JsonlFileWal::<String>::with_rotation(&path, 3).unwrap();
        wal.append(&"a".to_string()).unwrap();
        wal.append(&"b".to_string()).unwrap();
        wal.append(&"c".to_string()).unwrap();
        // 4th append should trigger auto-rotation, then append in the fresh file
        wal.append(&"d".to_string()).unwrap();

        let entries = wal.entries().unwrap();
        assert_eq!(entries, vec!["d".to_string()]);

        // Clean up
        std::fs::remove_file(&path).unwrap();
        for entry in std::fs::read_dir(path.parent().unwrap()).unwrap().flatten() {
            if entry
                .file_name()
                .to_string_lossy()
                .contains(&format!("autorot_{unique}"))
            {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }

    #[test]
    fn jsonl_file_wal_group_commit_defers_sync() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("rust_exchange_wal_gc_{unique}.jsonl"));

        let wal = JsonlFileWal::<String>::new(&path)
            .unwrap()
            .with_group_commit(3);
        wal.append(&"x".to_string()).unwrap();
        wal.append(&"y".to_string()).unwrap();
        // After 2 appends with group_commit_size=3, pending_syncs should be 2
        // (no fsync yet, but data is flushed to OS buffer)

        // 3rd append should trigger fsync (pending reaches group_commit_size)
        wal.append(&"z".to_string()).unwrap();

        let entries = wal.entries().unwrap();
        assert_eq!(
            entries,
            vec!["x".to_string(), "y".to_string(), "z".to_string()]
        );

        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn jsonl_file_wal_explicit_sync() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("rust_exchange_wal_sync_{unique}.jsonl"));

        let wal = JsonlFileWal::<String>::new(&path)
            .unwrap()
            .with_group_commit(100);
        wal.append(&"pending".to_string()).unwrap();
        // Explicit sync should flush even though group_commit_size not reached
        assert!(wal.sync().is_ok());

        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn jsonl_file_wal_creates_parent_dirs() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir()
            .join(format!("rust_exchange_nested_{unique}"))
            .join("sub")
            .join("wal.jsonl");

        let wal = JsonlFileWal::<String>::new(&path).unwrap();
        wal.append(&"nested".to_string()).unwrap();
        assert_eq!(wal.entries().unwrap(), vec!["nested".to_string()]);

        // Clean up
        let root = std::env::temp_dir().join(format!("rust_exchange_nested_{unique}"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn jsonl_file_wal_handles_blank_lines() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("rust_exchange_wal_blank_{unique}.jsonl"));

        // Write content with blank lines interspersed
        std::fs::write(&path, "\"entry1\"\n\n\"entry2\"\n  \n").unwrap();

        let wal = JsonlFileWal::<String>::new(&path).unwrap();
        let entries = wal.entries().unwrap();
        assert_eq!(entries, vec!["entry1".to_string(), "entry2".to_string()]);

        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn best_effort_recovery_skips_corrupt_entries() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("rust_exchange_wal_besteffort_{unique}.jsonl"));

        let wal = JsonlFileWal::<String>::new(&path).unwrap();
        wal.append(&"good-1".to_string()).unwrap();
        wal.append(&"good-2".to_string()).unwrap();
        wal.append(&"good-3".to_string()).unwrap();

        // Corrupt the second entry's payload while keeping CRC intact.
        let content = std::fs::read_to_string(&path).unwrap();
        let corrupted = content.replacen("good-2", "baad-2", 1);
        std::fs::write(&path, corrupted).unwrap();

        let wal2 = JsonlFileWal::<String>::new(&path).unwrap();

        // Strict mode should fail.
        assert!(wal2.entries_with_recovery(WalRecoveryMode::Strict).is_err());

        // BestEffort should skip the corrupt entry and return the other two.
        let recovered = wal2
            .entries_with_recovery(WalRecoveryMode::BestEffort)
            .unwrap();
        assert_eq!(recovered, vec!["good-1".to_string(), "good-3".to_string()]);

        std::fs::remove_file(path).unwrap();
    }
}
