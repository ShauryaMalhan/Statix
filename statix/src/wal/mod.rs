//! Phase 11 — bounded local-disk Write-Ahead Log spillway.
//!
//! When the in-memory ingest retry queue saturates (gateway outage / network
//! partition), batches spill to a segmented append-only log on disk instead of
//! being dropped. A background drainer replays them once the gateway recovers.
//! Loss only occurs at the disk hard cap, is FIFO-ordered, and is metered.
//!
//! Design (see `docs/adr/phase11/054-phase11-wal-spillway.md`):
//! - Hot path never touches disk: `try_append` is a non-blocking `try_send` to a
//!   dedicated OS writer thread (`writer.rs`).
//! - The writer thread owns segment files and group-commits with `fdatasync`.
//! - Recovery (`recovery.rs`) self-heals torn tails / corrupt segments at boot.
//! - The drainer (`drainer.rs`) replays oldest-first through the normal retry
//!   path, gated by a circuit breaker driven by gateway POST outcomes.

pub mod drainer;
pub mod recovery;
pub mod segment;
pub mod writer;

use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::io::{self, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use tokio::sync::mpsc;

use segment::{
    advise_dontneed, encode_frame_header, encode_segment_header, fdatasync, frame_len_on_disk,
    read_frame_at, segment_path, DecodedFrame, FrameRead, SEGMENT_HEADER_LEN,
};

// ---- metric names (statix_* prefix, :9091) -------------------------------

pub const M_BYTES_CURRENT: &str = "statix_wal_bytes_current";
pub const M_SEGMENTS_CURRENT: &str = "statix_wal_segments_current";
pub const M_FRAMES_WRITTEN: &str = "statix_wal_frames_written_total";
pub const M_FRAMES_REPLAYED: &str = "statix_wal_frames_replayed_total";
pub const M_DROPPED_BATCHES: &str = "statix_wal_dropped_batches_total";
pub const M_DROPPED_BYTES: &str = "statix_wal_dropped_bytes_total";
pub const M_CORRUPT_FRAMES: &str = "statix_wal_corrupt_frames_total";
pub const M_WRITE_ERRORS: &str = "statix_wal_write_errors_total";
pub const M_FSYNC_SECONDS: &str = "statix_wal_fsync_seconds";
pub const M_CIRCUIT_STATE: &str = "statix_wal_circuit_state";

// ---- circuit breaker -----------------------------------------------------

/// Gateway-health circuit, driven by the retry worker's POST outcomes. No
/// steady-state polling: transitions piggyback on real ingest traffic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CircuitState {
    /// Gateway healthy: drainer replays WAL → retry queue.
    Closed = 0,
    /// Trial in progress after an outage.
    HalfOpen = 1,
    /// Gateway down: overflow routes straight to WAL, drainer paused.
    Open = 2,
}

static CIRCUIT: AtomicU8 = AtomicU8::new(CircuitState::Closed as u8);

/// Consecutive retryable failures before the circuit trips Open.
const OPEN_THRESHOLD: u32 = 3;
static CONSECUTIVE_FAILURES: AtomicU8 = AtomicU8::new(0);

pub fn circuit_state() -> CircuitState {
    match CIRCUIT.load(Ordering::Relaxed) {
        0 => CircuitState::Closed,
        1 => CircuitState::HalfOpen,
        _ => CircuitState::Open,
    }
}

fn set_circuit(state: CircuitState) {
    CIRCUIT.store(state as u8, Ordering::Relaxed);
    metrics::gauge!(M_CIRCUIT_STATE).set(state as u8 as f64);
}

/// Record a successful POST: close the circuit and reset the failure run.
pub fn record_post_success() {
    CONSECUTIVE_FAILURES.store(0, Ordering::Relaxed);
    if circuit_state() != CircuitState::Closed {
        log::info!("WAL circuit → Closed (gateway healthy)");
        set_circuit(CircuitState::Closed);
    }
}

/// Record a retryable POST failure: trip Open once the threshold is reached.
pub fn record_post_failure() {
    let prev = CONSECUTIVE_FAILURES.fetch_add(1, Ordering::Relaxed);
    if prev.saturating_add(1) as u32 >= OPEN_THRESHOLD && circuit_state() != CircuitState::Open {
        log::warn!(
            "WAL circuit → Open after {OPEN_THRESHOLD} consecutive failures; overflow spills to disk"
        );
        set_circuit(CircuitState::Open);
    }
}

/// Move Open → HalfOpen so the drainer issues a single trial replay.
pub fn try_half_open() {
    if circuit_state() == CircuitState::Open {
        set_circuit(CircuitState::HalfOpen);
    }
}

// ---- config --------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct WalConfig {
    pub enabled: bool,
    pub dir: PathBuf,
    pub max_bytes: u64,
    pub segment_bytes: u64,
    pub fsync_frames: u64,
    pub fsync_interval: Duration,
}

impl WalConfig {
    pub fn from_env() -> Self {
        use statix_infra::env::{read_env_u64, var};
        let enabled = var("STATIX_WAL_ENABLED")
            .map(|v| {
                let v = v.trim().to_ascii_lowercase();
                !(v == "0" || v == "false" || v == "no" || v == "off")
            })
            .unwrap_or(true);
        let dir = var("STATIX_WAL_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/var/lib/statix/wal"));
        let max_bytes = read_env_u64("STATIX_WAL_MAX_BYTES", 512 * 1024 * 1024);
        let segment_bytes = read_env_u64("STATIX_WAL_SEGMENT_BYTES", 8 * 1024 * 1024);
        let fsync_frames = read_env_u64("STATIX_WAL_FSYNC_FRAMES", 64);
        let fsync_interval =
            Duration::from_millis(read_env_u64("STATIX_WAL_FSYNC_INTERVAL_MS", 200));
        Self {
            enabled,
            dir,
            // Cap must hold at least one full segment so a single segment is never
            // larger than the whole WAL (which would deadlock the drop-oldest loop).
            max_bytes: max_bytes.max(segment_bytes),
            segment_bytes,
            fsync_frames: fsync_frames.max(1),
            fsync_interval,
        }
    }
}

// ---- on-disk store -------------------------------------------------------

pub(crate) struct SegmentMeta {
    pub(crate) seq: u64,
    pub(crate) path: PathBuf,
    pub(crate) bytes: u64,
    pub(crate) frames: u64,
}

pub(crate) struct ActiveSegment {
    pub(crate) seq: u64,
    pub(crate) file: File,
    pub(crate) offset: u64,
}

/// Single-owner store state. Guarded by a `parking_lot::Mutex`; the writer
/// thread appends, the drainer task reads/commits. Locks are held briefly.
pub struct WalInner {
    cfg: WalConfig,
    segments: VecDeque<SegmentMeta>,
    active: Option<ActiveSegment>,
    total_bytes: u64,
    next_seq: u64,
    next_batch_seq: u64,
    // read cursor (drainer side)
    read_seq: u64,
    read_offset: u64,
    pending_advance: Option<u64>,
}

impl WalInner {
    fn refresh_gauges(&self) {
        metrics::gauge!(M_BYTES_CURRENT).set(self.total_bytes as f64);
        metrics::gauge!(M_SEGMENTS_CURRENT).set(self.segments.len() as f64);
    }

    /// Append one pre-serialized batch payload. Rotates segments and enforces
    /// the hard cap (drop-oldest); the caller (writer thread) group-commits with
    /// `sync_active`. Errors are reported via metrics by the writer (never panic).
    fn append(&mut self, payload: &[u8]) -> io::Result<()> {
        let frame_size = frame_len_on_disk(payload.len());
        self.enforce_cap(frame_size);
        self.ensure_active_with_room(frame_size)?;

        let batch_seq = self.next_batch_seq;
        self.next_batch_seq = self.next_batch_seq.wrapping_add(1);
        let header = encode_frame_header(payload, batch_seq);

        let active = self
            .active
            .as_mut()
            .expect("ensure_active_with_room guarantees an active segment");
        active.file.write_all(&header)?;
        active.file.write_all(payload)?;
        active.offset += frame_size;

        if let Some(meta) = self.segments.back_mut() {
            meta.bytes += frame_size;
            meta.frames += 1;
        }
        self.total_bytes += frame_size;

        metrics::counter!(M_FRAMES_WRITTEN).increment(1);
        self.refresh_gauges();
        Ok(())
    }

    /// After a failed `append` (e.g. ENOSPC mid-write), truncate the active
    /// segment back to the last known-good frame boundary so the next append
    /// does not build on a torn partial frame.
    fn recover_active_after_error(&mut self) {
        if let Some(active) = self.active.as_mut() {
            let _ = active.file.set_len(active.offset);
            let _ = active.file.seek(SeekFrom::Start(active.offset));
        }
    }

    fn sync_active(&self) -> io::Result<()> {
        if let Some(active) = self.active.as_ref() {
            let start = std::time::Instant::now();
            fdatasync(&active.file)?;
            metrics::histogram!(M_FSYNC_SECONDS).record(start.elapsed().as_secs_f64());
        }
        Ok(())
    }

    fn ensure_active_with_room(&mut self, frame_size: u64) -> io::Result<()> {
        let needs_rotate = match self.active.as_ref() {
            None => true,
            // Rotate only if the segment already holds data; a fresh segment must
            // accept even an oversized frame to guarantee forward progress.
            Some(a) => {
                a.offset > SEGMENT_HEADER_LEN && a.offset + frame_size > self.cfg.segment_bytes
            }
        };
        if needs_rotate {
            self.rotate()?;
        }
        Ok(())
    }

    fn rotate(&mut self) -> io::Result<()> {
        if let Some(old) = self.active.take() {
            let _ = fdatasync(&old.file);
            advise_dontneed(&old.file, old.offset);
        }
        std::fs::create_dir_all(&self.cfg.dir)?;
        let seq = self.next_seq;
        self.next_seq += 1;
        let path = segment_path(&self.cfg.dir, seq);
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .read(true)
            .write(true)
            .open(&path)?;
        file.write_all(&encode_segment_header())?;
        self.segments.push_back(SegmentMeta {
            seq,
            path: path.clone(),
            bytes: SEGMENT_HEADER_LEN,
            frames: 0,
        });
        self.total_bytes += SEGMENT_HEADER_LEN;
        self.active = Some(ActiveSegment {
            seq,
            file,
            offset: SEGMENT_HEADER_LEN,
        });
        Ok(())
    }

    /// Drop oldest segments until there is room for `incoming`. Never drops the
    /// active segment (always keeps at least one writable segment).
    fn enforce_cap(&mut self, incoming: u64) {
        while self.total_bytes + incoming > self.cfg.max_bytes && self.segments.len() > 1 {
            let Some(victim) = self.segments.pop_front() else {
                break;
            };
            // The victim cannot be the active segment because len() > 1 and active
            // is always the back element.
            self.total_bytes = self.total_bytes.saturating_sub(victim.bytes);
            metrics::counter!(M_DROPPED_BATCHES).increment(victim.frames);
            metrics::counter!(M_DROPPED_BYTES).increment(victim.bytes);
            log::warn!(
                "SEVERE: WAL hard cap ({} bytes) reached; dropped oldest segment seq={} ({} frames, {} bytes)",
                self.cfg.max_bytes,
                victim.seq,
                victim.frames,
                victim.bytes
            );
            let _ = std::fs::remove_file(&victim.path);
            // If the drainer was reading the dropped segment, advance its cursor.
            if self.read_seq == victim.seq {
                if let Some(front) = self.segments.front() {
                    self.read_seq = front.seq;
                    self.read_offset = SEGMENT_HEADER_LEN;
                    self.pending_advance = None;
                }
            }
        }
        self.refresh_gauges();
    }

    /// Peek the next undelivered frame without advancing the committed cursor.
    /// Lazily GCs fully-consumed non-active segments.
    fn peek_next(&mut self) -> io::Result<Option<DecodedFrame>> {
        loop {
            if self.segments.is_empty() {
                return Ok(None);
            }
            // Resync the read cursor if its segment was GC'd / dropped.
            if !self.segments.iter().any(|s| s.seq == self.read_seq) {
                let front = self.segments.front().expect("non-empty checked above");
                self.read_seq = front.seq;
                self.read_offset = SEGMENT_HEADER_LEN;
            }
            let is_active = self
                .active
                .as_ref()
                .map(|a| a.seq == self.read_seq)
                .unwrap_or(false);
            let path = self
                .segments
                .iter()
                .find(|s| s.seq == self.read_seq)
                .map(|s| s.path.clone())
                .expect("read_seq resynced to an existing segment");

            let mut file = File::open(&path)?;
            match read_frame_at(&mut file, self.read_offset)? {
                FrameRead::Frame(frame, next) => {
                    self.pending_advance = Some(next);
                    return Ok(Some(frame));
                }
                FrameRead::End | FrameRead::Corrupt => {
                    if is_active {
                        // Caught up to the live write tail; nothing to replay now.
                        return Ok(None);
                    }
                    // Fully consumed (or corrupt remainder of a) non-active segment:
                    // GC it and move on to the next.
                    self.gc_front_consumed();
                }
            }
        }
    }

    /// Commit the last peeked frame: advance the cursor past it.
    fn commit_pending(&mut self) {
        if let Some(next) = self.pending_advance.take() {
            self.read_offset = next;
            metrics::counter!(M_FRAMES_REPLAYED).increment(1);
        }
    }

    /// Delete the segment the read cursor currently sits on (already drained) and
    /// advance to the next one.
    fn gc_front_consumed(&mut self) {
        let cur = self.read_seq;
        if let Some(pos) = self.segments.iter().position(|s| s.seq == cur) {
            let meta = self.segments.remove(pos).expect("position just found");
            self.total_bytes = self.total_bytes.saturating_sub(meta.bytes);
            if let Ok(f) = File::open(&meta.path) {
                advise_dontneed(&f, meta.bytes);
            }
            let _ = std::fs::remove_file(&meta.path);
        }
        if let Some(front) = self.segments.front() {
            self.read_seq = front.seq;
            self.read_offset = SEGMENT_HEADER_LEN;
        }
        self.pending_advance = None;
        self.refresh_gauges();
    }

    /// True when the read cursor has caught up to the live write tail and there
    /// is no backlog in older segments. Pure in-memory check (no file I/O).
    fn has_backlog(&self) -> bool {
        let Some(active) = self.active.as_ref() else {
            return false;
        };
        self.read_seq != active.seq || self.read_offset < active.offset
    }
}

// ---- public handle -------------------------------------------------------

/// Cloneable handle to the WAL. Holds the hot-path append channel and a shared
/// reference to the store for the drainer.
#[derive(Clone)]
pub struct Wal {
    inner: Arc<Mutex<WalInner>>,
    append_tx: mpsc::Sender<bytes::Bytes>,
    enabled: bool,
}

/// Hot-path append channel depth (number of in-flight spill payloads queued for
/// the writer thread). Bounded so a stuck disk cannot grow memory unbounded.
const APPEND_CHANNEL_CAPACITY: usize = 256;

impl Wal {
    /// Recover any existing WAL, then spawn the dedicated writer thread.
    pub fn open(cfg: WalConfig) -> io::Result<Self> {
        let enabled = cfg.enabled;
        let inner = recovery::recover(cfg)?;
        let inner = Arc::new(Mutex::new(inner));
        let (append_tx, append_rx) = mpsc::channel(APPEND_CHANNEL_CAPACITY);
        if enabled {
            writer::spawn_writer_thread(Arc::clone(&inner), append_rx);
        }
        Ok(Self {
            inner,
            append_tx,
            enabled,
        })
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Non-blocking hot-path spill. Returns `Err` (payload returned) if the
    /// writer channel is full (caller falls back to drop-oldest) or WAL disabled.
    pub fn try_append(&self, payload: bytes::Bytes) -> Result<(), bytes::Bytes> {
        if !self.enabled {
            return Err(payload);
        }
        self.append_tx.try_send(payload).map_err(|e| match e {
            mpsc::error::TrySendError::Full(p) => p,
            mpsc::error::TrySendError::Closed(p) => p,
        })
    }

    /// Drainer-side: peek the next undelivered frame (blocking file I/O — call
    /// from `spawn_blocking`).
    pub fn peek_next_blocking(&self) -> io::Result<Option<DecodedFrame>> {
        self.inner.lock().peek_next()
    }

    /// Drainer-side: commit the last peeked frame after successful delivery.
    pub fn commit_blocking(&self) {
        self.inner.lock().commit_pending();
    }

    /// True when there is unreplayed backlog on disk.
    pub fn has_backlog(&self) -> bool {
        self.inner.lock().has_backlog()
    }

    #[cfg(test)]
    pub(crate) fn append_for_test(&self, payload: &[u8]) -> io::Result<()> {
        let mut g = self.inner.lock();
        g.append(payload)?;
        g.sync_active()
    }

    #[cfg(test)]
    pub(crate) fn total_bytes_for_test(&self) -> u64 {
        self.inner.lock().total_bytes
    }

    #[cfg(test)]
    pub(crate) fn segment_count_for_test(&self) -> usize {
        self.inner.lock().segments.len()
    }
}

/// Build a fresh, empty `WalInner` for `cfg` (used by recovery when no segments
/// exist on disk).
pub(crate) fn empty_inner(cfg: WalConfig) -> WalInner {
    WalInner {
        cfg,
        segments: VecDeque::new(),
        active: None,
        total_bytes: 0,
        next_seq: 0,
        next_batch_seq: 0,
        read_seq: 0,
        read_offset: SEGMENT_HEADER_LEN,
        pending_advance: None,
    }
}

/// Internal constructor used by recovery to install a rebuilt segment set.
pub(crate) fn inner_from_recovered(
    cfg: WalConfig,
    segments: VecDeque<SegmentMeta>,
    active: Option<ActiveSegment>,
    total_bytes: u64,
    next_seq: u64,
    next_batch_seq: u64,
) -> WalInner {
    let (read_seq, read_offset) = segments
        .front()
        .map(|s| (s.seq, SEGMENT_HEADER_LEN))
        .unwrap_or((next_seq, SEGMENT_HEADER_LEN));
    let inner = WalInner {
        cfg,
        segments,
        active,
        total_bytes,
        next_seq,
        next_batch_seq,
        read_seq,
        read_offset,
        pending_advance: None,
    };
    inner.refresh_gauges();
    inner
}

/// Open a segment file for continued appends positioned at `offset`.
pub(crate) fn open_active_for_append(path: &Path, offset: u64) -> io::Result<File> {
    use std::io::{Seek, SeekFrom};
    let mut file = OpenOptions::new().read(true).write(true).open(path)?;
    file.seek(SeekFrom::Start(offset))?;
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(dir: PathBuf, seg: u64, max: u64) -> WalConfig {
        WalConfig {
            enabled: true,
            dir,
            max_bytes: max,
            segment_bytes: seg,
            fsync_frames: 1,
            fsync_interval: Duration::from_millis(200),
        }
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "statix-wal-mod-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn append_then_drain_round_trips_all_frames() {
        let dir = temp_dir("roundtrip");
        let wal = Wal::open(cfg(dir, 4096, 1 << 20)).unwrap();
        let n = 50usize;
        for i in 0..n {
            let payload = format!(r#"{{"i":{i}}}"#);
            wal.append_for_test(payload.as_bytes()).unwrap();
        }
        let mut out = Vec::new();
        while let Some(f) = wal.peek_next_blocking().unwrap() {
            out.push(String::from_utf8(f.payload).unwrap());
            wal.commit_blocking();
        }
        assert_eq!(out.len(), n);
        for (i, s) in out.iter().enumerate() {
            assert_eq!(s, &format!(r#"{{"i":{i}}}"#));
        }
        assert!(!wal.has_backlog());
    }

    #[test]
    fn rotation_produces_multiple_segments() {
        let dir = temp_dir("rotate");
        // Tiny segments force rotation after a couple of frames.
        let wal = Wal::open(cfg(dir, 128, 1 << 20)).unwrap();
        for i in 0..40 {
            wal.append_for_test(format!(r#"{{"payload":"xxxxxxxxxxxx-{i}"}}"#).as_bytes())
                .unwrap();
        }
        assert!(
            wal.segment_count_for_test() >= 2,
            "expected rotation into multiple segments, got {}",
            wal.segment_count_for_test()
        );
    }

    #[test]
    fn circuit_trips_open_after_threshold_and_recovers() {
        // Reset shared circuit state (only this test touches it).
        CONSECUTIVE_FAILURES.store(0, Ordering::Relaxed);
        set_circuit(CircuitState::Closed);

        assert_eq!(circuit_state(), CircuitState::Closed);
        for _ in 0..(OPEN_THRESHOLD - 1) {
            record_post_failure();
            assert_eq!(circuit_state(), CircuitState::Closed);
        }
        record_post_failure();
        assert_eq!(
            circuit_state(),
            CircuitState::Open,
            "trips Open at threshold"
        );

        try_half_open();
        assert_eq!(circuit_state(), CircuitState::HalfOpen);

        record_post_success();
        assert_eq!(
            circuit_state(),
            CircuitState::Closed,
            "success closes circuit"
        );

        // A single later failure must not immediately re-open (run was reset).
        record_post_failure();
        assert_eq!(circuit_state(), CircuitState::Closed);
    }

    /// Disk-degradation assertion. Ignored by default; run via
    /// `scripts/wal-faultfs.sh` which mounts a tiny tmpfs and sets
    /// `STATIX_WAL_TEST_DIR`. Asserts the WAL surfaces ENOSPC as a recoverable
    /// `Err` (never a panic) and remains usable.
    #[test]
    #[ignore = "requires a size-limited tmpfs via scripts/wal-faultfs.sh"]
    fn enospc_is_handled_without_panic() {
        let Some(dir) = std::env::var_os("STATIX_WAL_TEST_DIR") else {
            eprintln!("STATIX_WAL_TEST_DIR unset; skipping (run via scripts/wal-faultfs.sh)");
            return;
        };
        let dir = PathBuf::from(dir);
        // max_bytes far above the tmpfs size so our own cap does NOT free space —
        // forcing a real filesystem ENOSPC.
        let wal = Wal::open(cfg(dir, 64 * 1024, 1 << 40)).unwrap();
        let big = vec![b'x'; 32 * 1024];
        let mut saw_error = false;
        for _ in 0..100_000 {
            if wal.append_for_test(&big).is_err() {
                saw_error = true;
                break;
            }
        }
        assert!(saw_error, "expected the tmpfs to fill and surface ENOSPC");
        // The WAL stays usable for reads after a write error (no panic).
        let _ = wal.peek_next_blocking();
    }

    #[test]
    fn hard_cap_drops_oldest_segment() {
        let dir = temp_dir("hardcap");
        // max_bytes only holds ~2 small segments → 3rd forces an oldest drop.
        let wal = Wal::open(cfg(dir, 128, 300)).unwrap();
        for i in 0..200 {
            wal.append_for_test(format!(r#"{{"d":"yyyyyyyyyyyyyyyy-{i}"}}"#).as_bytes())
                .unwrap();
        }
        assert!(
            wal.total_bytes_for_test() <= 300 + 128,
            "WAL exceeded hard cap: {} bytes",
            wal.total_bytes_for_test()
        );
        // Some frames must have been dropped (bounded loss), and the WAL stays usable.
        assert!(wal.peek_next_blocking().unwrap().is_some());
    }
}
