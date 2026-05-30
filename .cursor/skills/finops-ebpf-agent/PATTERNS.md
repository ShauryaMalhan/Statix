# FinOps eBPF Agent — Patterns

Phase 2 templates. Rules: [SKILL.md](SKILL.md). Architecture: [REFERENCE.md](REFERENCE.md).

---

## Pattern 1 — `FinopsEvent` in finops-common

```rust
pub const EVENT_KIND_WORKLOAD_IDENTITY: u8 = 1;
pub const EVENT_KIND_MEMORY_SAMPLE: u8 = 2;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct FinopsEvent {
    pub kind: u8,
    pub _pad: [u8; 7],
    pub pid: u32,
    pub tgid: u32,
    pub cpu_id: u32,
    pub _pad2: u32,
    pub cgroup_id: u64,
    pub timestamp: u64,
    pub memory_bytes: u64,
    pub comm: [u8; 16],
}

#[cfg(feature = "user")]
unsafe impl aya::Pod for FinopsEvent {}
```

---

## Pattern 2 — Ring buffer map (finops-ebpf)

```rust
#[map]
static EVENTS: RingBuf = RingBuf::with_byte_size(512 * 1024, 0);
```

Loader: `bpf.map_mut("EVENTS")` — name must match exactly.

---

## Pattern 3 — Tracepoint identity capture (kernel)

```rust
#[tracepoint(name = "sched_process_exec", category = "sched")]
pub fn finops_sched_process_exec(ctx: TracePointContext) -> u32 {
    let mut entry = match EVENTS.reserve::<FinopsEvent>(0) {
        Some(e) => e,
        None => return 0,
    };
    let ptr: *mut FinopsEvent = entry.as_mut_ptr();
    // SAFETY: Exclusive slot until submit().
    unsafe {
        (*ptr).kind = EVENT_KIND_WORKLOAD_IDENTITY;
        (*ptr).cgroup_id = bpf_get_current_cgroup_id();
        (*ptr).timestamp = bpf_ktime_get_ns();
        // pid, tgid, comm, _pad fields = 0 as needed
    }
    entry.submit(0);
    0
}
```

Never use `?` after `reserve`.

---

## Pattern 4 — Attach tracepoint (loader.rs)

```rust
use aya::programs::TracePoint;

let program: &mut TracePoint = bpf
    .program_mut("finops_sched_process_exec")?
    .try_into()?;
program.load()?;
program.attach("sched", "sched_process_exec")?;
```

---

## Pattern 5 — User event loop + batch flush (main.rs)

```rust
while let Some(item) = rb.next() {
    if item.len() < size_of::<FinopsEvent>() { continue; }
    let event: &FinopsEvent =
        unsafe { &*(item.as_ptr() as *const FinopsEvent) };
    agg.on_finops_event(event, &cache);
}
// Parallel: flush_interval.tick() → agg.flush(&node, &cache) → output::emit_batch
```

---

## Pattern 6 — Memory sampling (userspace, elite hot path)

**Cold path** (`on_identity_event`): read `/proc/{pid}/cgroup`, then once:

```rust
fn precompute_memory_current(cgroup_root: &Path, rel_path: &Path) -> PathBuf {
    let rel = rel_path.strip_prefix(Path::new("/")).unwrap_or(rel_path.as_path());
    cgroup_root.join(rel).join("memory.current") // alloc once per new cgroup
}
```

Store in `memory_current_paths: HashMap<u64, PathBuf>`.

**Hot path** (`sample_tracked_cgroups`):

```rust
let sample_tick_ns = now_ns(); // one timestamp for the whole tick — intentional for TSDB batching
cache.for_each_memory_current_path(|cgroup_id, path| {
    let mut file = File::open(path)?; // path is precomputed — no join()
    let mut buf = [0u8; 32];
    let n = file.read(&mut buf)?;
    // parse u64, aggregator.ingest_memory_sample(..., sample_tick_ns, ...)
});
```

**Why:** `read_to_string` and `join()` per cgroup per tick allocate on the heap. Precompute paths on exec; use a stack buffer on read; share one timestamp per sample interval.

---

## Pattern 6b — Attribution cache

```rust
use parking_lot::RwLock;

// /proc/{pid}/cgroup line: "0::/kubepods.slice/..."
fn parse_cgroup_v2_path_line(line: &str) -> Option<&str> {
    let line = line.trim();
    if let Some((_, path)) = line.split_once("::") {
        if path.starts_with('/') { return Some(path); }
    }
    None
}

// No to_string_lossy() on the full path
for component in path.components() {
    let Component::Normal(part) = component else { continue };
    let part = part.to_str()?;
    // match kubepods-*, cri-container-*, etc.
}
```

---

## Pattern 6c — Aggregator (FxHashMap + double buffer + early flush)

```rust
use rustc_hash::FxHashMap;

buffers: [
    FxHashMap::with_capacity_and_hasher(MAX, Default::default()),
    FxHashMap::with_capacity_and_hasher(MAX, Default::default()),
],
active: 0,

// On flush: flip active first, then drain buffers[old_active], clear()
// On len >= max_keys: try_early_flush() → emit batch (no dropped cgroup rows)
```

---

## Pattern 7 — Batched JSON (schema v2)

```json
{
  "schema_version": 2,
  "window_start_ns": 0,
  "window_end_ns": 0,
  "node": "host",
  "workloads": [{
    "cgroup_id": 1,
    "k8s_resolved": false,
    "memory_bytes_max": 0,
    "memory_bytes_last": 0,
    "exec_count": 1,
    "sample_count": 0
  }]
}
```

---

## Pattern 8 — OOM-safe bounds

```rust
pub fn safe_bounds(p99_bytes: u64, current_requests_bytes: u64) -> (u64, u64) {
    const CUSHION: f64 = 1.20;
    const BURST: f64 = 1.25;
    const MIN_REQUESTS: u64 = 128 * 1024 * 1024;
    const MAX_STEP_DOWN: f64 = 0.50;
    let proposed = ((p99_bytes as f64 * CUSHION) as u64).max(MIN_REQUESTS);
    let min_allowed = (current_requests_bytes as f64 * MAX_STEP_DOWN) as u64;
    let requests = proposed.max(min_allowed);
    let limits = (requests as f64 * BURST) as u64;
    (requests, limits)
}
```

---

## Pattern 9 — GitOps PR body

Use for agent / finops-core changes (copy into `gh pr create --body`):

```markdown
## Summary
- <what changed and why — e.g. aggregator early flush, attribution path parse fix>

## Test plan
- [ ] `make build` / `make check`
- [ ] `make verify-btf` on target kernel (BTF + optional object `.BTF`)
- [ ] `make run` smoke: `schema_version: 2` batches on stdout
- [ ] kind/minikube: `k8s_resolved:true` for a known pod (if K8s touched)
- [ ] Overhead: agent on vs off CPU (if hot path changed)

## Notes
- Phase 2 ingest fields: [docs/phase3-ingest-interface.md](../../../docs/phase3-ingest-interface.md)
- Validation checklist: [docs/phase2-validation.md](../../../docs/phase2-validation.md)
```
