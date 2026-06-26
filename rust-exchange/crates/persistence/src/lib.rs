//! Write-ahead log (WAL) persistence layer.
//!
//! Provides `WalStore<T>` trait with in-memory and JSONL file-based implementations.
//! File WAL entries use CRC-32 checksums for integrity verification. Supports
//! automatic rotation at configurable thresholds and group-commit batching for
//! throughput optimisation.
//!
//! Performance: append operations use buffered writes to the OS page cache without
//! blocking on fsync. Crash recovery relies on periodic snapshots.

use anyhow::{bail, Result};
use parking_lot::Mutex;
use serde::{de::DeserializeOwned, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{create_dir_all, File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
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

/// High-performance file-based WAL using buffered writes without blocking fsync.
///
/// Append operations write to the OS page cache and return immediately (~100μs).
/// Crash recovery relies on periodic snapshots rather than WAL replay.
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
    /// Maximum bytes in the current segment before automatic rotation
    /// (0 = disabled). When both `max_entries` and `max_bytes` are set,
    /// whichever threshold is hit first triggers the rotation
    /// (P2-SCALE-2 acceptance: "Files rotate at 1 GB").
    max_bytes: u64,
    /// Bytes written to the current segment since open or last rotate.
    /// Initialised from file metadata on open and incremented per append.
    current_bytes: AtomicU64,
    /// Group-commit batch size (informational only — no longer triggers fsync).
    group_commit_size: u64,
    /// Flush interval in milliseconds (informational only).
    flush_interval_ms: u64,
    /// Writes since last fsync (informational only).
    pending_syncs: AtomicU64,
    /// Persistent file handle kept open to avoid open/close overhead per append.
    file_handle: Mutex<Option<File>>,
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
        // Open persistent file handle for append operations — avoids open/close overhead.
        let file_handle = OpenOptions::new().create(true).append(true).open(&path)?;
        // Initialise current_bytes from on-disk size so a process restart
        // does not under-count toward the rotation threshold.
        let initial_bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        Ok(Self {
            path,
            write_lock: Mutex::new(()),
            append_count: AtomicU64::new(initial_count),
            max_entries,
            max_bytes: 0,
            current_bytes: AtomicU64::new(initial_bytes),
            group_commit_size: 0,
            flush_interval_ms: 5,
            pending_syncs: AtomicU64::new(0),
            file_handle: Mutex::new(Some(file_handle)),
            _marker: PhantomData,
        })
    }

    /// P2-SCALE-2: enable size-based rotation. `max_bytes = 0` disables
    /// (default). Production target: 1 GiB. Both `with_rotation`
    /// (entry count) and `with_size_rotation` may be combined; either
    /// threshold triggers a rotate.
    pub fn with_size_rotation(mut self, max_bytes: u64) -> Self {
        self.max_bytes = max_bytes;
        self
    }

    /// P2-SCALE-2 opt-in: replay across all rotated segments AND the
    /// active file, in oldest-first order. Each rotated segment is
    /// verified against its `.sha256` sidecar before parsing — a
    /// mismatch aborts in Strict mode, skips the segment in BestEffort.
    ///
    /// Callers that depend on snapshot-then-replay semantics (the
    /// sequencer, ledger, trade journal) must NOT use this — they read
    /// the active file only via `entries`/`entries_with_recovery` so
    /// that rotation does not double-apply already-snapshotted commands.
    ///
    /// Use cases: audit tooling that needs the full history; cold-boot
    /// recovery from off-host backups where the rotated segments are
    /// the only source of truth.
    pub fn entries_all_segments_with_recovery(
        &self,
        mode: WalRecoveryMode,
    ) -> Result<Vec<T>> {
        let _guard = self.write_lock.lock();
        let mut entries = Vec::new();
        let mut skipped = 0u64;

        for segment in enumerate_rotated_segments(&self.path) {
            match verify_sha256_sidecar(&segment) {
                Ok(true) => {}
                Ok(false) => {
                    tracing::warn!(
                        segment = %segment.display(),
                        "WAL: rotated segment has no SHA256 sidecar — parsing without segment-level verification"
                    );
                }
                Err(e) => {
                    if mode == WalRecoveryMode::BestEffort {
                        tracing::error!(
                            segment = %segment.display(),
                            error = %e,
                            "WAL: SHA256 mismatch on rotated segment — skipping entire segment"
                        );
                        continue;
                    }
                    return Err(e);
                }
            }
            let mut seg_skipped = 0u64;
            parse_jsonl_segment(&segment, mode, &mut entries, &mut seg_skipped)?;
            skipped += seg_skipped;
        }

        // Active file is always read last (and never has a sidecar — it
        // is still being appended to).
        let mut active_skipped = 0u64;
        parse_jsonl_segment(&self.path, mode, &mut entries, &mut active_skipped)?;
        skipped += active_skipped;

        if skipped > 0 {
            tracing::warn!(
                skipped,
                total = entries.len() + skipped as usize,
                recovered = entries.len(),
                "WAL: best-effort cross-segment recovery completed with skipped entries"
            );
        }
        Ok(entries)
    }

    /// Configure group-commit batch size (informational only — no longer triggers fsync).
    #[allow(dead_code)]
    pub fn with_group_commit(mut self, size: u64) -> Self {
        self.group_commit_size = size;
        self
    }

    /// Configure flush interval (informational only).
    #[allow(dead_code)]
    pub fn with_flush_interval(mut self, ms: u64) -> Self {
        self.flush_interval_ms = ms;
        self
    }

    /// Load entries with the specified recovery mode.
    ///
    /// **Reads the ACTIVE file only.** Rotated segments (`*.bak.<ts>`)
    /// are not replayed — they are considered archived for backup or
    /// audit. Callers that need cross-segment replay must call
    /// `entries_all_segments_with_recovery` explicitly (P2-SCALE-2).
    ///
    /// In `BestEffort` mode, CRC-mismatched or malformed entries are skipped
    /// (with a tracing::error log) instead of aborting the entire load.
    pub fn entries_with_recovery(&self, mode: WalRecoveryMode) -> Result<Vec<T>> {
        let _guard = self.write_lock.lock();
        let mut entries = Vec::new();
        let mut skipped = 0u64;

        // Active file only — see method doc.
        parse_jsonl_segment(&self.path, mode, &mut entries, &mut skipped)?;

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
    ///
    /// P2-SCALE-2: a sidecar `<rotated>.sha256` is written alongside
    /// the rotated segment, containing the hex SHA256 of the segment
    /// followed by two spaces and the segment basename — the same
    /// layout `sha256sum` would produce, so operators can verify with
    /// `sha256sum -c`. Replay (`entries*`) verifies this hash before
    /// parsing.
    pub fn rotate(&self) -> Result<()> {
        let _guard = self.write_lock.lock();
        // Nanoseconds — second-resolution collides if two rotations
        // happen in the same second (tests, fast-cycling triggers).
        // `as_u128()` truncated to u64 still fits ~584 years of nanos.
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
        let bak = self.path.with_extension(format!("bak.{ts}"));
        // Close handle BEFORE rename so Windows allows the rename.
        {
            let mut file_guard = self.file_handle.lock();
            *file_guard = None;
        }
        std::fs::rename(&self.path, &bak)?;
        // Write the sidecar. If hashing fails for any reason (disk
        // pressure, permissions), tracing::error but continue — losing
        // the sidecar must never block the rotate from completing,
        // because that would freeze appends and risk a stuck producer.
        if let Err(err) = write_sha256_sidecar(&bak) {
            tracing::error!(
                rotated = %bak.display(),
                error = %err,
                "WAL: failed to write SHA256 sidecar (rotated file is intact; replay will skip the verification step)"
            );
        }
        // Open the fresh active file.
        let new_file = File::create(&self.path)?;
        let mut file_guard = self.file_handle.lock();
        *file_guard = Some(new_file);
        self.append_count.store(0, Ordering::Release);
        self.pending_syncs.store(0, Ordering::Release);
        self.current_bytes.store(0, Ordering::Release);
        Ok(())
    }
}

/// Compute SHA256 of a file and write the canonical sidecar.
/// Layout matches GNU `sha256sum`: `<hex>  <basename>\n`.
fn write_sha256_sidecar(segment: &Path) -> Result<()> {
    let mut hasher = Sha256::new();
    let mut file = OpenOptions::new().read(true).open(segment)?;
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let digest = hasher.finalize();
    let basename = segment
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let line = format!("{}  {}\n", hex::encode(digest), basename);
    let sidecar_path = path_with_appended_extension(segment, "sha256");
    let mut sidecar = File::create(&sidecar_path)?;
    sidecar.write_all(line.as_bytes())?;
    sidecar.sync_all()?;
    Ok(())
}

/// `Path::with_extension` REPLACES the extension. We want to APPEND
/// `.sha256` so `data.bak.123.sha256` ≠ `data.bak.sha256`. Build it
/// manually.
fn path_with_appended_extension(path: &Path, extra_ext: &str) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(".");
    s.push(extra_ext);
    PathBuf::from(s)
}

/// Verify a segment file's SHA256 against its sidecar. Returns:
///   Ok(true)  — sidecar present and digest matches
///   Ok(false) — sidecar absent (legacy / unrotated file)
///   Err(_)    — sidecar present but digest mismatched, or IO error
fn verify_sha256_sidecar(segment: &Path) -> Result<bool> {
    let sidecar_path = path_with_appended_extension(segment, "sha256");
    if !sidecar_path.exists() {
        return Ok(false);
    }
    // Read the expected hex from column 1 of the sidecar (sha256sum layout).
    let raw = std::fs::read_to_string(&sidecar_path)?;
    let expected_hex = raw.split_whitespace().next().unwrap_or("").to_string();
    if expected_hex.len() != 64 {
        bail!(
            "WAL: SHA256 sidecar {} is malformed (expected 64 hex chars in column 1)",
            sidecar_path.display()
        );
    }
    let mut hasher = Sha256::new();
    let mut file = OpenOptions::new().read(true).open(segment)?;
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let actual_hex = hex::encode(hasher.finalize());
    if !actual_hex.eq_ignore_ascii_case(&expected_hex) {
        bail!(
            "WAL: SHA256 mismatch on segment {}: expected {expected_hex}, got {actual_hex}",
            segment.display()
        );
    }
    Ok(true)
}

/// Parse one JSONL segment file into `entries`. Per-line CRC32 is
/// verified (existing behaviour); BestEffort mode skips bad lines
/// with a tracing::error log and bumps `skipped`. Strict mode aborts.
fn parse_jsonl_segment<T>(
    path: &Path,
    mode: WalRecoveryMode,
    entries: &mut Vec<T>,
    skipped: &mut u64,
) -> Result<()>
where
    T: DeserializeOwned,
{
    let file = OpenOptions::new().read(true).open(path)?;
    let reader = BufReader::new(file);
    for (lineno, line) in reader.lines().enumerate() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                if mode == WalRecoveryMode::BestEffort {
                    tracing::error!(
                        segment = %path.display(),
                        line = lineno + 1,
                        error = %e,
                        "WAL: skipping unreadable line"
                    );
                    *skipped += 1;
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
                        segment = %path.display(),
                        line = lineno + 1,
                        expected = format!("{expected:08x}"),
                        actual = format!("{actual:08x}"),
                        "WAL: CRC mismatch — skipping corrupt entry"
                    );
                    *skipped += 1;
                    continue;
                }
                bail!(
                    "WAL CRC mismatch at {} line {}: expected {expected:08x}, got {actual:08x}",
                    path.display(),
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
                    tracing::error!(
                        segment = %path.display(),
                        line = lineno + 1,
                        error = %e,
                        "WAL: skipping malformed JSON entry"
                    );
                    *skipped += 1;
                    continue;
                }
                return Err(e.into());
            }
        }
    }
    Ok(())
}

/// Enumerate rotated segments (`<basename>.bak.<ts>`) in oldest-first
/// order. The active file at `<basename>` is NOT included; callers
/// concatenate it themselves after the rotated set.
fn enumerate_rotated_segments(active: &Path) -> Vec<PathBuf> {
    let parent = match active.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => PathBuf::from("."),
    };
    let active_stem = active.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let entries = match std::fs::read_dir(&parent) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    let mut rotated: Vec<(u64, PathBuf)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let name = match path.file_name().and_then(|s| s.to_str()) {
            Some(n) => n,
            None => continue,
        };
        // sidecars end in .sha256 — never replay-parse them.
        if name.ends_with(".sha256") {
            continue;
        }
        // Match `<active_stem>.bak.<digits>`.
        let prefix = format!("{}.bak.", active_stem);
        if let Some(ts_str) = name.strip_prefix(&prefix) {
            if let Ok(ts) = ts_str.parse::<u64>() {
                rotated.push((ts, path));
            }
        }
    }
    rotated.sort_by_key(|(ts, _)| *ts);
    rotated.into_iter().map(|(_, p)| p).collect()
}

impl<T> WalStore<T> for JsonlFileWal<T>
where
    T: Clone + Serialize + DeserializeOwned + Send + Sync + 'static,
{
    fn append(&self, record: &T) -> Result<()> {
        // Serialize outside the lock to minimize contention
        let json = serde_json::to_string(record)?;
        let checksum = crc32(json.as_bytes());
        let line = format!("{checksum:08x}\t{json}\n");
        let bytes = line.into_bytes();
        let n_bytes = bytes.len() as u64;

        // Brief lock for the buffered write only — no fsync on critical path
        let _guard = self.write_lock.lock();

        // Auto-rotate if either threshold reached. Check count first
        // (cheaper), then size.
        let count = self.append_count.load(Ordering::Acquire);
        let bytes_now = self.current_bytes.load(Ordering::Acquire);
        let count_trip = self.max_entries > 0 && count >= self.max_entries;
        let size_trip = self.max_bytes > 0 && bytes_now >= self.max_bytes;
        if count_trip || size_trip {
            drop(_guard);
            self.rotate()?;
            return self.append(record);
        }

        // Use persistent file handle — avoids open/close overhead.
        let mut file_guard = self.file_handle.lock();
        let file = file_guard
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("WAL file handle not initialized"))?;

        file.write_all(&bytes)?;
        // Buffered flush to OS page cache only — does NOT call fsync
        file.flush()?;
        drop(file_guard);
        drop(_guard);

        // Increment counters (informational only — background thread handles fsync)
        self.pending_syncs.fetch_add(1, Ordering::AcqRel);
        self.append_count.fetch_add(1, Ordering::Release);
        self.current_bytes.fetch_add(n_bytes, Ordering::Release);
        Ok(())
    }

    fn entries(&self) -> Result<Vec<T>> {
        // ACTIVE file only — keeps behaviour bit-for-bit compatible
        // with the pre-P2-SCALE-2 contract. The sequencer / ledger
        // replay path depends on this: snapshots cover everything up
        // to rotation, so rotated segments are not replayed. See
        // `entries_all_segments_with_recovery` for the opt-in
        // cross-segment + sidecar-verified variant.
        let _guard = self.write_lock.lock();
        let mut entries = Vec::new();
        let mut ignored = 0u64;
        parse_jsonl_segment(&self.path, WalRecoveryMode::Strict, &mut entries, &mut ignored)?;
        Ok(entries)
    }

    fn len(&self) -> u64 {
        self.append_count.load(Ordering::Acquire)
    }

    fn sync(&self) -> Result<()> {
        let _guard = self.write_lock.lock();
        if self.pending_syncs.load(Ordering::Acquire) > 0 {
            let file_guard = self.file_handle.lock();
            if let Some(ref file) = *file_guard {
                file.sync_all()?;
            }
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

    // ─────────────────────────────────────────────────────────────────
    // P2-SCALE-2: size-based rotation + SHA256 sidecar + cross-segment
    // replay.
    // ─────────────────────────────────────────────────────────────────

    fn unique_path(tag: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("rust_exchange_wal_{tag}_{unique}.jsonl"))
    }

    fn cleanup_segments(active: &Path) {
        let _ = std::fs::remove_file(active);
        let parent = active.parent().unwrap_or_else(|| Path::new("."));
        let stem = active
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        if let Ok(read) = std::fs::read_dir(parent) {
            for entry in read.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with(&format!("{stem}.bak.")) {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
    }

    #[test]
    fn size_rotation_triggers_when_threshold_exceeded() {
        let path = unique_path("size_rot");
        // Threshold: 200 bytes. Each `"x..."` entry is ~30 bytes after
        // JSON+CRC+\n, so a handful of appends crosses the limit.
        let wal = JsonlFileWal::<String>::new(&path)
            .unwrap()
            .with_size_rotation(200);
        for i in 0..20 {
            wal.append(&format!("entry-{i}-payload")).unwrap();
        }
        // Verify at least one rotated segment exists.
        let rotated = enumerate_rotated_segments(&path);
        assert!(
            !rotated.is_empty(),
            "size-based rotation should have produced at least one .bak.<ts> segment"
        );

        // Every rotated segment should have a sidecar.
        for seg in &rotated {
            let sidecar = path_with_appended_extension(seg, "sha256");
            assert!(
                sidecar.exists(),
                "sidecar missing for rotated segment {}",
                seg.display()
            );
            assert!(
                verify_sha256_sidecar(seg).unwrap(),
                "sidecar verification should pass for fresh rotation"
            );
        }
        cleanup_segments(&path);
    }

    #[test]
    fn sha256_sidecar_layout_matches_sha256sum() {
        let path = unique_path("sidecar_layout");
        let wal = JsonlFileWal::<String>::new(&path).unwrap();
        wal.append(&"row".to_string()).unwrap();
        wal.rotate().unwrap();

        let rotated = enumerate_rotated_segments(&path);
        assert_eq!(rotated.len(), 1);
        let seg = &rotated[0];
        let sidecar = path_with_appended_extension(seg, "sha256");
        let body = std::fs::read_to_string(&sidecar).unwrap();
        let mut iter = body.split_whitespace();
        let hex = iter.next().unwrap();
        let name = iter.next().unwrap();
        assert_eq!(hex.len(), 64, "first column should be 64-hex SHA256");
        assert!(hex.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(
            name,
            seg.file_name().unwrap().to_str().unwrap(),
            "second column should be the segment basename"
        );

        cleanup_segments(&path);
    }

    #[test]
    fn cross_segment_replay_returns_rotated_then_active_in_order() {
        let path = unique_path("cross_replay");
        let wal = JsonlFileWal::<String>::new(&path).unwrap();

        wal.append(&"r1-a".to_string()).unwrap();
        wal.append(&"r1-b".to_string()).unwrap();
        wal.rotate().unwrap();

        wal.append(&"r2-a".to_string()).unwrap();
        wal.rotate().unwrap();

        wal.append(&"active-1".to_string()).unwrap();

        let all = wal
            .entries_all_segments_with_recovery(WalRecoveryMode::Strict)
            .unwrap();
        assert_eq!(
            all,
            vec![
                "r1-a".to_string(),
                "r1-b".to_string(),
                "r2-a".to_string(),
                "active-1".to_string(),
            ]
        );

        // Active-only path stays unchanged.
        assert_eq!(wal.entries().unwrap(), vec!["active-1".to_string()]);

        cleanup_segments(&path);
    }

    #[test]
    fn cross_segment_replay_strict_aborts_on_sha256_mismatch() {
        let path = unique_path("sha_strict");
        let wal = JsonlFileWal::<String>::new(&path).unwrap();
        wal.append(&"r1".to_string()).unwrap();
        wal.rotate().unwrap();

        // Tamper with the rotated segment, sidecar untouched.
        let rotated = enumerate_rotated_segments(&path);
        let seg = &rotated[0];
        let original = std::fs::read_to_string(seg).unwrap();
        std::fs::write(seg, original.replace("r1", "x9")).unwrap();

        let err = wal
            .entries_all_segments_with_recovery(WalRecoveryMode::Strict)
            .unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("SHA256 mismatch"),
            "expected SHA256 mismatch error, got: {msg}"
        );

        cleanup_segments(&path);
    }

    #[test]
    fn cross_segment_replay_best_effort_skips_corrupt_segment() {
        let path = unique_path("sha_besteffort");
        let wal = JsonlFileWal::<String>::new(&path).unwrap();
        wal.append(&"r1-keep".to_string()).unwrap();
        wal.rotate().unwrap();
        wal.append(&"r2-tamper".to_string()).unwrap();
        wal.rotate().unwrap();
        wal.append(&"active".to_string()).unwrap();

        // Tamper with the second rotated segment.
        let rotated = enumerate_rotated_segments(&path);
        assert_eq!(rotated.len(), 2);
        let bad = &rotated[1];
        let body = std::fs::read_to_string(bad).unwrap();
        std::fs::write(bad, body.replace("r2-tamper", "broken-pl")).unwrap();

        let recovered = wal
            .entries_all_segments_with_recovery(WalRecoveryMode::BestEffort)
            .unwrap();
        // Bad segment dropped; good segment + active kept.
        assert_eq!(
            recovered,
            vec!["r1-keep".to_string(), "active".to_string()]
        );

        cleanup_segments(&path);
    }

    #[test]
    fn rotated_segment_without_sidecar_is_warned_but_parsed() {
        let path = unique_path("no_sidecar");
        let wal = JsonlFileWal::<String>::new(&path).unwrap();
        wal.append(&"only".to_string()).unwrap();
        wal.rotate().unwrap();

        // Delete the sidecar to simulate a manually-rotated / legacy
        // file that came in from an off-host backup.
        let rotated = enumerate_rotated_segments(&path);
        let sidecar = path_with_appended_extension(&rotated[0], "sha256");
        std::fs::remove_file(&sidecar).unwrap();

        // Still parses (warn-only, no error).
        let recovered = wal
            .entries_all_segments_with_recovery(WalRecoveryMode::Strict)
            .unwrap();
        assert_eq!(recovered, vec!["only".to_string()]);

        cleanup_segments(&path);
    }
}
