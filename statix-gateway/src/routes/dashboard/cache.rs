//! Short-TTL response cache + a global request cap for the dashboard read tier.
//!
//! Statix is single-tenant: every viewer sees the same fleet, so identical query
//! parameters have identical answers. The cache is therefore keyed on the
//! *question* (normalized params), never on the caller — 20 tabs polling the
//! default view collapse into one ClickHouse round trip per TTL ([ADR 061]).
//!
//! No single-flight in v1a: the TTL alone collapses steady-state load, and the
//! stampede window is only the instant after expiry.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

struct Entry {
    stored_at: Instant,
    body: Arc<str>,
    /// The unfiltered default view — the one ~95% of traffic uses. Arbitrary
    /// `q` values must never evict it, or a junk-filter loop degrades everyone.
    pinned: bool,
}

pub struct TtlCache {
    inner: Mutex<HashMap<String, Entry>>,
    ttl: Duration,
    max_entries: usize,
    hits: AtomicU64,
    misses: AtomicU64,
}

impl TtlCache {
    pub fn new(ttl: Duration, max_entries: usize) -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
            ttl,
            max_entries: max_entries.max(4),
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
        }
    }

    pub fn get(&self, key: &str) -> Option<Arc<str>> {
        let map = self.inner.lock().ok()?;
        let entry = map.get(key)?;
        if entry.stored_at.elapsed() <= self.ttl {
            self.hits.fetch_add(1, Ordering::Relaxed);
            Some(Arc::clone(&entry.body))
        } else {
            self.misses.fetch_add(1, Ordering::Relaxed);
            None
        }
    }

    pub fn put(&self, key: String, body: Arc<str>, pinned: bool) {
        let Ok(mut map) = self.inner.lock() else { return };
        map.retain(|_, e| e.stored_at.elapsed() <= self.ttl || e.pinned);
        if map.len() >= self.max_entries && !map.contains_key(&key) {
            let victim = map
                .iter()
                .filter(|(_, e)| !e.pinned)
                .min_by_key(|(_, e)| e.stored_at)
                .map(|(k, _)| k.clone());
            if let Some(k) = victim {
                map.remove(&k);
            }
        }
        map.insert(
            key,
            Entry {
                stored_at: Instant::now(),
                body,
                pinned,
            },
        );
    }

    #[cfg(test)]
    pub fn stats(&self) -> (u64, u64) {
        (
            self.hits.load(Ordering::Relaxed),
            self.misses.load(Ordering::Relaxed),
        )
    }
}

/// Fixed-window global cap on dashboard requests.
///
/// Deliberately **global**, not per-IP: in Compose the gateway sees the Docker
/// bridge address and behind the ALB ([ADR 043]) it sees the load balancer, so a
/// `ConnectInfo` limiter would read as per-IP while silently being global.
/// Honest global cap now; real per-client limiting needs `X-Forwarded-For` plus
/// a trusted-proxy list, which is its own decision.
pub struct RateLimiter {
    inner: Mutex<(Instant, u32)>,
    max_per_window: u32,
    window: Duration,
}

impl RateLimiter {
    pub fn new(max_per_window: u32, window: Duration) -> Self {
        Self {
            inner: Mutex::new((Instant::now(), 0)),
            max_per_window: max_per_window.max(1),
            window,
        }
    }

    pub fn allow(&self) -> bool {
        let Ok(mut guard) = self.inner.lock() else {
            return true; // never fail closed on a poisoned lock
        };
        let (started, count) = &mut *guard;
        if started.elapsed() >= self.window {
            *started = Instant::now();
            *count = 0;
        }
        if *count >= self.max_per_window {
            return false;
        }
        *count += 1;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_fresh_entry_and_expires_stale_one() {
        let c = TtlCache::new(Duration::from_millis(50), 8);
        c.put("k".into(), "body".into(), false);
        assert_eq!(c.get("k").as_deref(), Some("body"));
        std::thread::sleep(Duration::from_millis(70));
        assert!(c.get("k").is_none(), "entry must expire after the TTL");
    }

    #[test]
    fn identical_params_share_one_entry() {
        let c = TtlCache::new(Duration::from_secs(5), 8);
        c.put("range=300&sort=cpu".into(), "payload".into(), true);
        // A second viewer asking the same question hits the same key.
        assert_eq!(c.get("range=300&sort=cpu").as_deref(), Some("payload"));
        assert_eq!(c.stats().0, 1);
    }

    #[test]
    fn junk_filters_never_evict_the_default_view() {
        let c = TtlCache::new(Duration::from_secs(5), 4);
        c.put("default".into(), "hot".into(), true);
        for i in 0..50 {
            c.put(format!("q=junk{i}"), "x".into(), false);
        }
        assert_eq!(
            c.get("default").as_deref(),
            Some("hot"),
            "pinned default view must survive a cache-busting filter loop"
        );
    }

    #[test]
    fn rate_limiter_caps_then_refills_next_window() {
        let rl = RateLimiter::new(3, Duration::from_millis(50));
        assert!(rl.allow() && rl.allow() && rl.allow());
        assert!(!rl.allow(), "fourth request in the window must be refused");
        std::thread::sleep(Duration::from_millis(70));
        assert!(rl.allow(), "next window must refill");
    }
}
