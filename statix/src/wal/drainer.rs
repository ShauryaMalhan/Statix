//! WAL replay coordinator (Phase 11, P11-5).
//!
//! A background `tokio::spawn` task that drains the disk backlog back through the
//! normal in-memory retry path once the gateway recovers. Health is inferred
//! from the circuit breaker (driven by the retry worker's POST outcomes), so
//! there is no steady-state health polling. Replay is FIFO and gated on the
//! retry queue having headroom, so it never overruns the in-memory buffer.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use super::{circuit_state, try_half_open, CircuitState, Wal};

const SWEEP_INTERVAL: Duration = Duration::from_millis(500);

enum ReplayOutcome {
    Sent,
    Empty,
    NoCapacity,
}

/// Spawn the drainer. `retry_tx` is a clone of the ingest retry queue sender;
/// replayed frames re-enter the normal backoff/jitter delivery path.
pub fn spawn_drainer(
    wal: Wal,
    retry_tx: mpsc::Sender<bytes::Bytes>,
    node: String,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        if !wal.is_enabled() {
            return;
        }
        log::info!("WAL drainer started (replays disk backlog when gateway is healthy)");
        loop {
            tokio::time::sleep(SWEEP_INTERVAL).await;
            match circuit_state() {
                CircuitState::Open => {
                    // Stagger the recovery probe across the fleet (same node-hash
                    // spread the retry worker uses post-outage), then allow a trial.
                    let spread = recovery_spread_secs(&node);
                    tokio::time::sleep(Duration::from_secs_f64(spread)).await;
                    try_half_open();
                }
                CircuitState::HalfOpen => {
                    // One trial frame; the retry worker's outcome moves the circuit
                    // to Closed (success) or back to Open (failure).
                    if wal.has_backlog() {
                        let _ = replay_one(&wal, &retry_tx).await;
                    }
                }
                CircuitState::Closed => {
                    // Cheap in-memory fast-path: skip the blocking peek when idle.
                    if !wal.has_backlog() {
                        continue;
                    }
                    // Drain while the retry queue has room and the gateway stays up.
                    loop {
                        if retry_tx.capacity() == 0 || circuit_state() != CircuitState::Closed {
                            break;
                        }
                        match replay_one(&wal, &retry_tx).await {
                            ReplayOutcome::Sent => continue,
                            ReplayOutcome::Empty | ReplayOutcome::NoCapacity => break,
                        }
                    }
                }
            }
        }
    })
}

async fn replay_one(wal: &Wal, retry_tx: &mpsc::Sender<bytes::Bytes>) -> ReplayOutcome {
    let w = wal.clone();
    let frame = match tokio::task::spawn_blocking(move || w.peek_next_blocking()).await {
        Ok(Ok(Some(f))) => f,
        Ok(Ok(None)) => return ReplayOutcome::Empty,
        Ok(Err(e)) => {
            log::warn!("WAL drainer peek failed: {e}");
            return ReplayOutcome::Empty;
        }
        Err(e) => {
            log::warn!("WAL drainer peek task join error: {e}");
            return ReplayOutcome::Empty;
        }
    };

    match retry_tx.try_send(bytes::Bytes::from(frame.payload)) {
        Ok(()) => {
            // Only advance (delete from disk) once the batch is back in the retry
            // queue. If it later fails delivery, overflow re-spills it to the WAL.
            let w = wal.clone();
            let _ = tokio::task::spawn_blocking(move || w.commit_blocking()).await;
            ReplayOutcome::Sent
        }
        // Leave the frame uncommitted on disk; retry on the next sweep.
        Err(_) => ReplayOutcome::NoCapacity,
    }
}

/// Deterministic node-hash spread over 30s + 0–5s PRNG — mirrors
/// `output::recovery_spread_sleep_secs` to keep fleet-wide probes staggered.
fn recovery_spread_secs(node: &str) -> f64 {
    let mut hasher = DefaultHasher::new();
    node.hash(&mut hasher);
    let spread_secs = (hasher.finish() % 30_000) as f64 / 1000.0;
    rand::random::<f64>() * 5.0 + spread_secs
}
