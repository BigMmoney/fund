// Step 3B scaffold: no producers wired yet (consumer task lands in 3C).
// Until then this module is reachable only from its own unit tests.
#![allow(dead_code)]

//! Order Flow Monitor — JSONL trail writer.
//!
//! Step 3B of `docs/MONITOR_DESIGN.md` §7. Append-only, single-writer JSONL
//! file at `<dir>/order_trace.jsonl` rotated to dated archives when it
//! exceeds `cap_bytes`. Per design §3.6 the writer **never** records
//! per-record recovery events (`recovery_replayed`,
//! `recovery_skipped_terminal`) — those are debug-only and live on the
//! broadcast channel only.
//!
//! Scope of this commit:
//! - `JsonlWriter` with `open`, `write_event`, `flush`, internal rotation.
//! - Stage filter for recovery events.
//!
//! Out of scope (later steps):
//! - Spawning the writer task and bridging from `Event::OrderTrace` to it.
//! - Retention of old archives (the writer creates archives; cleanup is a
//!   future commit). Design says 14 archives — not enforced here yet.
//! - `/metrics` counters for writes / drops / rotations.

use chrono::Utc;
use std::io;
use std::path::{Path, PathBuf};
use tokio::fs::{File, OpenOptions};
use tokio::io::AsyncWriteExt;

use types::{OrderTraceEvent, OrderTraceStage};

const DEFAULT_CAP_BYTES: u64 = 100 * 1024 * 1024; // 100 MB
const DEFAULT_FSYNC_EVERY: u32 = 64;
const ACTIVE_FILE_NAME: &str = "order_trace.jsonl";

#[derive(Debug, Clone, Copy)]
pub(crate) struct JsonlWriterConfig {
    pub(crate) cap_bytes: u64,
    pub(crate) fsync_every: u32,
}

impl Default for JsonlWriterConfig {
    fn default() -> Self {
        Self {
            cap_bytes: DEFAULT_CAP_BYTES,
            fsync_every: DEFAULT_FSYNC_EVERY,
        }
    }
}

pub(crate) struct JsonlWriter {
    dir: PathBuf,
    config: JsonlWriterConfig,
    file: Option<File>,
    current_size: u64,
    write_counter: u32,
}

impl JsonlWriter {
    /// Open or create `<dir>/order_trace.jsonl` for appending. Creates `dir`
    /// if it does not already exist.
    pub(crate) async fn open(dir: PathBuf, config: JsonlWriterConfig) -> io::Result<Self> {
        tokio::fs::create_dir_all(&dir).await?;
        let active = dir.join(ACTIVE_FILE_NAME);
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&active)
            .await?;
        let current_size = file.metadata().await?.len();
        Ok(Self {
            dir,
            config,
            file: Some(file),
            current_size,
            write_counter: 0,
        })
    }

    /// Append one event to the JSONL trail. Returns `Ok(true)` if a line
    /// was written, `Ok(false)` if the event was filtered (recovery
    /// per-record events per design §3.6).
    pub(crate) async fn write_event(&mut self, ev: &OrderTraceEvent) -> io::Result<bool> {
        if !is_writable_to_jsonl(ev) {
            return Ok(false);
        }
        let mut bytes =
            serde_json::to_vec(ev).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        bytes.push(b'\n');

        if self.current_size + bytes.len() as u64 > self.config.cap_bytes {
            self.rotate().await?;
        }

        let f = self
            .file
            .as_mut()
            .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "writer closed"))?;
        f.write_all(&bytes).await?;
        self.current_size += bytes.len() as u64;
        self.write_counter = self.write_counter.saturating_add(1);

        if self.write_counter >= self.config.fsync_every {
            f.flush().await?;
            self.write_counter = 0;
        }
        Ok(true)
    }

    /// Force-flush pending writes to the OS buffer.
    pub(crate) async fn flush(&mut self) -> io::Result<()> {
        if let Some(f) = self.file.as_mut() {
            f.flush().await?;
            self.write_counter = 0;
        }
        Ok(())
    }

    /// Currently active file path (for tests / introspection).
    pub(crate) fn active_path(&self) -> PathBuf {
        self.dir.join(ACTIVE_FILE_NAME)
    }

    pub(crate) fn current_size(&self) -> u64 {
        self.current_size
    }

    async fn rotate(&mut self) -> io::Result<()> {
        if let Some(mut f) = self.file.take() {
            f.flush().await?;
            // dropping `f` here closes the OS handle; required on Windows
            // before renaming the active file.
            drop(f);
        }
        let active = self.dir.join(ACTIVE_FILE_NAME);
        let stamp = Utc::now().format("%Y%m%dT%H%M%S%3f").to_string();
        let archive = self.dir.join(format!("order_trace.{}Z.jsonl", stamp));
        tokio::fs::rename(&active, &archive).await?;
        let new_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&active)
            .await?;
        self.file = Some(new_file);
        self.current_size = 0;
        self.write_counter = 0;
        Ok(())
    }
}

/// Per design §3.6: the JSONL trail never records per-record recovery
/// events regardless of the `MONITOR_TRACE_RECOVERY_DETAIL` flag. Those
/// flow on the broadcast channel only. The aggregate `recovery_completed`
/// stage is always written.
fn is_writable_to_jsonl(ev: &OrderTraceEvent) -> bool {
    !matches!(
        ev.stage,
        OrderTraceStage::RecoveryReplayed | OrderTraceStage::RecoverySkippedTerminal
    )
}

/// List `order_trace.<stamp>.jsonl` archives in `dir`, oldest first by
/// filename (timestamps embed in the name, so lexicographic == chronological).
/// Helper for future retention work; not used in the current commit.
pub(crate) async fn list_archives(dir: &Path) -> io::Result<Vec<PathBuf>> {
    let mut entries = match tokio::fs::read_dir(dir).await {
        Ok(e) => e,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut out = Vec::new();
    while let Some(entry) = entries.next_entry().await? {
        let name = entry.file_name();
        let name_s = name.to_string_lossy().to_string();
        if name_s.starts_with("order_trace.")
            && name_s.ends_with(".jsonl")
            && name_s != ACTIVE_FILE_NAME
        {
            out.push(entry.path());
        }
    }
    out.sort();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use tokio::io::AsyncReadExt;
    use types::{OrderTraceEvent, OrderTraceStage};

    async fn read_lines(path: &Path) -> Vec<String> {
        let mut f = match tokio::fs::File::open(path).await {
            Ok(f) => f,
            Err(_) => return Vec::new(),
        };
        let mut s = String::new();
        f.read_to_string(&mut s).await.unwrap();
        s.lines()
            .filter(|l| !l.is_empty())
            .map(|l| l.to_string())
            .collect()
    }

    #[tokio::test]
    async fn writes_one_line_per_event() {
        let dir = tempdir().unwrap();
        let mut w = JsonlWriter::open(dir.path().to_path_buf(), JsonlWriterConfig::default())
            .await
            .unwrap();

        let e1 = OrderTraceEvent::new(OrderTraceStage::SequencerAccepted, "ord-1");
        let e2 = OrderTraceEvent::new(OrderTraceStage::MatchingResting, "ord-1");
        let e3 = OrderTraceEvent::new(OrderTraceStage::MatchingFilled, "ord-1");
        assert!(w.write_event(&e1).await.unwrap());
        assert!(w.write_event(&e2).await.unwrap());
        assert!(w.write_event(&e3).await.unwrap());
        w.flush().await.unwrap();

        let lines = read_lines(&w.active_path()).await;
        assert_eq!(lines.len(), 3);
        // Every line is valid JSON for an OrderTraceEvent.
        for line in &lines {
            let _back: OrderTraceEvent = serde_json::from_str(line).expect("valid jsonl line");
        }
    }

    #[tokio::test]
    async fn recovery_per_record_events_are_filtered() {
        let dir = tempdir().unwrap();
        let mut w = JsonlWriter::open(dir.path().to_path_buf(), JsonlWriterConfig::default())
            .await
            .unwrap();

        let replayed = OrderTraceEvent::new(OrderTraceStage::RecoveryReplayed, "ord-r");
        let skipped = OrderTraceEvent::new(OrderTraceStage::RecoverySkippedTerminal, "ord-s");
        assert_eq!(w.write_event(&replayed).await.unwrap(), false);
        assert_eq!(w.write_event(&skipped).await.unwrap(), false);
        w.flush().await.unwrap();

        let lines = read_lines(&w.active_path()).await;
        assert!(
            lines.is_empty(),
            "per-record recovery events must be filtered (got {} lines)",
            lines.len()
        );
    }

    #[tokio::test]
    async fn recovery_completed_is_written_through() {
        let dir = tempdir().unwrap();
        let mut w = JsonlWriter::open(dir.path().to_path_buf(), JsonlWriterConfig::default())
            .await
            .unwrap();

        let completed = OrderTraceEvent::new(OrderTraceStage::RecoveryCompleted, "boot");
        assert_eq!(w.write_event(&completed).await.unwrap(), true);
        w.flush().await.unwrap();

        let lines = read_lines(&w.active_path()).await;
        assert_eq!(lines.len(), 1);
        let back: OrderTraceEvent = serde_json::from_str(&lines[0]).unwrap();
        assert_eq!(back.stage, OrderTraceStage::RecoveryCompleted);
    }

    #[tokio::test]
    async fn rotation_creates_archive_and_resets_active() {
        let dir = tempdir().unwrap();
        // Tiny cap so a couple of events trigger rotation.
        let cfg = JsonlWriterConfig {
            cap_bytes: 256,
            fsync_every: 64,
        };
        let mut w = JsonlWriter::open(dir.path().to_path_buf(), cfg)
            .await
            .unwrap();

        // Two events whose payload exceeds 256 bytes each event must rotate.
        for i in 0..6 {
            let mut e = OrderTraceEvent::new(OrderTraceStage::MatchingPartiallyFilled, "ord-x");
            e.client_order_id = Some(format!("cli-{:04}", i));
            e.user_id = Some("alice".into());
            e.market_id = Some("btc-usdt".into());
            w.write_event(&e).await.unwrap();
        }
        w.flush().await.unwrap();

        // Active file exists and is below cap.
        let active = w.active_path();
        assert!(active.exists());
        let active_size = tokio::fs::metadata(&active).await.unwrap().len();
        assert!(
            active_size <= 256,
            "post-rotation active size should be <= cap (got {})",
            active_size
        );

        // At least one archive exists.
        let archives = list_archives(dir.path()).await.unwrap();
        assert!(
            !archives.is_empty(),
            "expected at least one archive after rotation"
        );
    }

    #[tokio::test]
    async fn flush_makes_writes_visible() {
        let dir = tempdir().unwrap();
        let mut w = JsonlWriter::open(dir.path().to_path_buf(), JsonlWriterConfig::default())
            .await
            .unwrap();
        let e = OrderTraceEvent::new(OrderTraceStage::SequencerAccepted, "ord-f");
        w.write_event(&e).await.unwrap();
        w.flush().await.unwrap();
        let lines = read_lines(&w.active_path()).await;
        assert_eq!(lines.len(), 1);
    }

    #[tokio::test]
    async fn reopening_appends_rather_than_truncates() {
        let dir = tempdir().unwrap();
        {
            let mut w =
                JsonlWriter::open(dir.path().to_path_buf(), JsonlWriterConfig::default())
                    .await
                    .unwrap();
            let e = OrderTraceEvent::new(OrderTraceStage::SequencerAccepted, "ord-1");
            w.write_event(&e).await.unwrap();
            w.flush().await.unwrap();
        }
        // Re-open, write another event.
        let mut w = JsonlWriter::open(dir.path().to_path_buf(), JsonlWriterConfig::default())
            .await
            .unwrap();
        let e = OrderTraceEvent::new(OrderTraceStage::MatchingFilled, "ord-1");
        w.write_event(&e).await.unwrap();
        w.flush().await.unwrap();
        let lines = read_lines(&w.active_path()).await;
        assert_eq!(lines.len(), 2, "second open must append, not truncate");
    }

    #[tokio::test]
    async fn list_archives_returns_empty_for_missing_dir() {
        let dir = tempdir().unwrap();
        let archives = list_archives(&dir.path().join("does-not-exist"))
            .await
            .unwrap();
        assert!(archives.is_empty());
    }
}
