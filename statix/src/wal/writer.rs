//! Dedicated OS-thread WAL writer with group-commit (Phase 11, P11-2).
//!
//! ALL disk I/O for appends happens here, off the ring-buffer hot path. The hot
//! path only `try_send`s a `bytes::Bytes` to `append_rx`; this thread drains it,
//! appends frames under the store lock, and `fdatasync`s in groups. Using a
//! dedicated `std::thread` (not the shared `spawn_blocking` pool) avoids pool
//! starvation and keeps the active-segment fd owned by a single thread.

use std::sync::Arc;
use std::time::Instant;

use parking_lot::Mutex;
use tokio::sync::mpsc;

use super::{WalInner, M_WRITE_ERRORS};

/// Spawn the writer thread. It exits when `append_rx` is closed (all `Wal`
/// handles dropped), issuing a final `fdatasync` so an orderly stop loses nothing.
pub fn spawn_writer_thread(
    inner: Arc<Mutex<WalInner>>,
    mut append_rx: mpsc::Receiver<bytes::Bytes>,
) {
    let builder = std::thread::Builder::new().name("statix-wal-writer".into());
    let spawn_result = builder.spawn(move || {
        log::info!("WAL writer thread started");
        loop {
            // Block until at least one payload is queued.
            let first = match append_rx.blocking_recv() {
                Some(b) => b,
                None => break,
            };

            let mut guard = inner.lock();
            let fsync_frames = guard.cfg.fsync_frames;
            let fsync_interval = guard.cfg.fsync_interval;
            let mut since_sync: u64 = 0;
            let mut last_sync = Instant::now();

            append_one(&mut guard, &first);
            since_sync += 1;

            // Drain everything already queued into this group commit.
            loop {
                match append_rx.try_recv() {
                    Ok(b) => {
                        append_one(&mut guard, &b);
                        since_sync += 1;
                        // Bound a very large burst: sync mid-drain on frame/interval cap.
                        if since_sync >= fsync_frames || last_sync.elapsed() >= fsync_interval {
                            sync(&guard);
                            since_sync = 0;
                            last_sync = Instant::now();
                        }
                    }
                    Err(mpsc::error::TryRecvError::Empty) => break,
                    Err(mpsc::error::TryRecvError::Disconnected) => break,
                }
            }

            // Group-commit the tail of the batch so every received frame is durable.
            if since_sync > 0 {
                sync(&guard);
            }
        }
        log::info!("WAL writer thread stopped (channel closed)");
    });

    if let Err(e) = spawn_result {
        log::error!("failed to spawn WAL writer thread: {e}; disk spill disabled");
    }
}

fn append_one(guard: &mut WalInner, payload: &[u8]) {
    if let Err(e) = guard.append(payload) {
        metrics::counter!(M_WRITE_ERRORS).increment(1);
        log::error!("WAL append failed ({e}); truncating to last good frame and dropping batch");
        guard.recover_active_after_error();
    }
}

fn sync(guard: &WalInner) {
    if let Err(e) = guard.sync_active() {
        metrics::counter!(M_WRITE_ERRORS).increment(1);
        log::error!("WAL fdatasync failed: {e}");
    }
}
