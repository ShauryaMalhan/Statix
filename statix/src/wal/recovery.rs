//! Bootstrap recovery / self-heal for the disk WAL (Phase 11, P11-4).
//!
//! Runs once at agent startup, before the writer thread / drainer start. It:
//! - enumerates `seg-*.wal` ordered by seq,
//! - validates each segment header (drops segments with a corrupt header),
//! - walks frames validating length + CRC, truncating a torn tail (the expected
//!   post-SIGKILL / power-loss state) or a corrupt remainder via `set_len`,
//! - rebuilds head/tail cursors purely from the surviving segment set (the
//!   advisory superblock is intentionally not trusted).
//!
//! Delivery is at-least-once: any frame durable on disk but already delivered
//! pre-crash is replayed and de-duplicated downstream by ClickHouse
//! (`ReplacingMergeTree` on `batch_id` / window key).

use std::collections::VecDeque;
use std::fs::File;
use std::io::{self, Read};

use super::segment::{
    parse_segment_seq, read_frame_at, segment_path, validate_segment_header, FrameRead,
    SEGMENT_HEADER_LEN,
};
use super::{
    empty_inner, inner_from_recovered, open_active_for_append, ActiveSegment, SegmentMeta,
    WalConfig, WalInner, M_CORRUPT_FRAMES,
};

pub fn recover(cfg: WalConfig) -> io::Result<WalInner> {
    if !cfg.dir.exists() {
        return Ok(empty_inner(cfg));
    }

    // Collect segment files ordered by seq.
    let mut found: Vec<u64> = Vec::new();
    for entry in std::fs::read_dir(&cfg.dir)? {
        let entry = entry?;
        if let Some(name) = entry.file_name().to_str() {
            if let Some(seq) = parse_segment_seq(name) {
                found.push(seq);
            }
        }
    }
    if found.is_empty() {
        return Ok(empty_inner(cfg));
    }
    found.sort_unstable();

    let mut segments: VecDeque<SegmentMeta> = VecDeque::new();
    let mut total_bytes: u64 = 0;
    let mut next_batch_seq: u64 = 0;
    let mut max_seq: u64 = 0;

    for seq in found {
        max_seq = max_seq.max(seq);
        let path = segment_path(&cfg.dir, seq);
        let mut file = match File::open(&path) {
            Ok(f) => f,
            Err(e) => {
                log::warn!("WAL recovery: cannot open segment {path:?} ({e}); skipping");
                continue;
            }
        };

        // Validate the segment header; a bad header means the whole file is
        // unusable — drop it (bounded loss beats a crash loop).
        let mut header = [0u8; SEGMENT_HEADER_LEN as usize];
        match file
            .read_exact(&mut header)
            .and_then(|_| validate_segment_header(&header))
        {
            Ok(()) => {}
            Err(e) => {
                log::warn!("WAL recovery: corrupt header in {path:?} ({e}); dropping segment");
                metrics::counter!(M_CORRUPT_FRAMES).increment(1);
                let _ = std::fs::remove_file(&path);
                continue;
            }
        }

        // Walk frames to the first invalid boundary (torn tail / corruption).
        let mut offset = SEGMENT_HEADER_LEN;
        let mut frames: u64 = 0;
        loop {
            match read_frame_at(&mut file, offset)? {
                FrameRead::Frame(frame, next) => {
                    offset = next;
                    frames += 1;
                    next_batch_seq = next_batch_seq.max(frame.batch_seq.wrapping_add(1));
                }
                FrameRead::End => break,
                FrameRead::Corrupt => {
                    log::warn!(
                        "WAL recovery: corrupt frame in {path:?} at offset {offset}; truncating tail"
                    );
                    metrics::counter!(M_CORRUPT_FRAMES).increment(1);
                    break;
                }
            }
        }

        // Truncate any torn / corrupt tail so future appends start clean.
        let actual_len = file.metadata().map(|m| m.len()).unwrap_or(offset);
        if actual_len > offset {
            log::info!(
                "WAL recovery: truncating {path:?} from {actual_len} to {offset} bytes ({frames} valid frames)"
            );
            if let Ok(w) = std::fs::OpenOptions::new().write(true).open(&path) {
                let _ = w.set_len(offset);
            }
        }

        segments.push_back(SegmentMeta {
            seq,
            path,
            bytes: offset,
            frames,
        });
        total_bytes += offset;
    }

    let next_seq = max_seq.wrapping_add(1);

    // The newest surviving segment becomes the active write target, positioned
    // at its recovered (post-truncation) extent.
    let active: Option<ActiveSegment> = match segments.back() {
        Some(last) => {
            let file = open_active_for_append(&last.path, last.bytes)?;
            Some(ActiveSegment {
                seq: last.seq,
                file,
                offset: last.bytes,
            })
        }
        None => None,
    };

    let recovered_frames: u64 = segments.iter().map(|s| s.frames).sum();
    if recovered_frames > 0 {
        log::info!(
            "WAL recovery: {} segment(s), {recovered_frames} frame(s), {total_bytes} bytes ready for replay",
            segments.len()
        );
    }

    Ok(inner_from_recovered(
        cfg,
        segments,
        active,
        total_bytes,
        next_seq,
        next_batch_seq,
    ))
}

#[cfg(test)]
mod tests {
    use super::super::segment::{encode_frame_header, encode_segment_header};
    use super::super::{Wal, WalConfig};
    use std::io::Write;
    use std::path::PathBuf;
    use std::time::Duration;

    fn temp_dir(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "statix-wal-recovery-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn cfg(dir: PathBuf) -> WalConfig {
        WalConfig {
            enabled: true,
            dir,
            max_bytes: 1 << 20,
            segment_bytes: 4096,
            fsync_frames: 1,
            fsync_interval: Duration::from_millis(200),
        }
    }

    fn drain_all(wal: &Wal) -> Vec<String> {
        let mut out = Vec::new();
        while let Some(f) = wal.peek_next_blocking().unwrap() {
            out.push(String::from_utf8(f.payload).unwrap());
            wal.commit_blocking();
        }
        out
    }

    #[test]
    fn torn_tail_is_truncated_and_valid_frames_survive() {
        let dir = temp_dir("torn");
        let path = super::segment_path(&dir, 0);
        {
            let mut f = std::fs::File::create(&path).unwrap();
            f.write_all(&encode_segment_header()).unwrap();
            for i in 0..5u64 {
                let payload = format!(r#"{{"i":{i}}}"#).into_bytes();
                f.write_all(&encode_frame_header(&payload, i)).unwrap();
                f.write_all(&payload).unwrap();
            }
            // Torn write: a partial frame header (power-loss mid-append).
            f.write_all(&[0x01, 0x02, 0x03, 0x04, 0x05]).unwrap();
            f.flush().unwrap();
        }
        let len_before = std::fs::metadata(&path).unwrap().len();
        let wal = Wal::open(cfg(dir)).unwrap();
        let out = drain_all(&wal);
        assert_eq!(out.len(), 5, "all fully-written frames must survive");
        let len_after = std::fs::metadata(&path).unwrap().len();
        assert!(len_after < len_before, "torn tail must be truncated");
    }

    #[test]
    fn corrupt_segment_header_is_dropped() {
        let dir = temp_dir("badhdr");
        let path = super::segment_path(&dir, 0);
        std::fs::write(&path, b"NOT-A-WAL-HEADER-junk-bytes-here").unwrap();
        let wal = Wal::open(cfg(dir.clone())).unwrap();
        assert!(drain_all(&wal).is_empty());
        assert!(!path.exists(), "corrupt-header segment must be removed");
    }

    #[test]
    fn crash_then_recover_loses_nothing_synced() {
        let dir = temp_dir("crash");
        let n = 120usize;
        {
            // Simulate a run that writes+syncs, then is SIGKILLed (handle dropped,
            // no graceful close).
            let wal = Wal::open(cfg(dir.clone())).unwrap();
            for i in 0..n {
                wal.append_for_test(format!(r#"{{"seq":{i}}}"#).as_bytes())
                    .unwrap();
            }
            drop(wal);
        }
        // Fresh process: recover from disk and replay everything.
        let wal = Wal::open(cfg(dir)).unwrap();
        let out = drain_all(&wal);
        assert_eq!(out.len(), n, "count_in must equal count_out after crash");
        for (i, s) in out.iter().enumerate() {
            assert_eq!(s, &format!(r#"{{"seq":{i}}}"#));
        }
    }
}
