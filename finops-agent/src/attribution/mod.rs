//! cgroup_id and cgroup path resolution; optional Kubernetes pod/namespace labels

mod error;

pub use error::AttributionError;

use std::{
    fs::{self, File},
    io::Read,
    os::unix::fs::MetadataExt,
    path::{Component, Path, PathBuf},
    sync::{Arc, LazyLock},
};

use finops_common::{FinopsEvent, EVENT_KIND_WORKLOAD_IDENTITY};
use parking_lot::RwLock;
use rustc_hash::FxHashMap;
use walkdir::WalkDir;

/// Resolved workload metadata for aggregation and JSON output.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct WorkloadLabels {
    pub namespace: Option<String>,
    pub pod: Option<String>,
    pub container: Option<String>,
    pub pod_uid: Option<String>,
    pub k8s_resolved: bool,
}

pub static DEFAULT_LABELS: LazyLock<Arc<WorkloadLabels>> =
    LazyLock::new(|| Arc::new(WorkloadLabels::default()));

#[derive(Debug, Default)]
struct CacheState {
    cgroup_paths: FxHashMap<u64, PathBuf>,
    memory_current_paths: FxHashMap<u64, Arc<PathBuf>>,
    cgroup_labels: FxHashMap<u64, Arc<WorkloadLabels>>,
    pod_by_uid: FxHashMap<String, Arc<WorkloadLabels>>,
}

#[derive(Clone, Debug)]
pub struct AttributionCache {
    cgroup_root: PathBuf,
    state: Arc<RwLock<CacheState>>,
}

impl AttributionCache {
    pub fn new() -> Self {
        Self {
            cgroup_root: cgroup_v2_mount(),
            state: Arc::new(RwLock::new(CacheState::default())),
        }
    }

    pub fn on_identity_event(&self, event: &FinopsEvent) {
        let rel_path = cgroup_path_from_pid(event.pid).ok();
        let mut state = self.state.write();
        if let Some(rel_path) = rel_path {
            let memory_current = precompute_memory_current(&self.cgroup_root, &rel_path);
            state.cgroup_paths.insert(event.cgroup_id, rel_path);
            state
                .memory_current_paths
                .insert(event.cgroup_id, Arc::new(memory_current));
        }
        let labels = Arc::new(labels_from_cgroup_path(state.cgroup_paths.get(&event.cgroup_id)));
        state.cgroup_labels.insert(event.cgroup_id, labels);
    }

    /// Yields `Arc<PathBuf>` — refcount clone only (no per-tick path string alloc).
    pub fn for_each_memory_current_path(&self, mut f: impl FnMut(u64, Arc<PathBuf>)) {
        let state = self.state.read();
        for (cgroup_id, path) in state.memory_current_paths.iter() {
            f(*cgroup_id, Arc::clone(path));
        }
    }

    /// Read-only label lookup — K8s merge runs in `refresh_k8s_pods`, not on the hot path.
    pub fn labels_for_cgroup(&self, cgroup_id: u64) -> Arc<WorkloadLabels> {
        let state = self.state.read();
        state
            .cgroup_labels
            .get(&cgroup_id)
            .cloned()
            .unwrap_or_else(|| Arc::clone(&DEFAULT_LABELS))
    }

    pub fn upsert_pod_labels(&self, uid: String, labels: WorkloadLabels) {
        self.state.write().pod_by_uid.insert(uid, Arc::new(labels));
    }

    /// Register a cgroup directory discovered at startup (inode = `cgroup_id` in cgroup v2).
    pub fn register_cgroup_directory(&self, cgroup_id: u64, rel_path: PathBuf) {
        let mut state = self.state.write();
        let memory_current = precompute_memory_current(&self.cgroup_root, &rel_path);
        state.cgroup_paths.insert(cgroup_id, rel_path);
        state
            .memory_current_paths
            .insert(cgroup_id, Arc::new(memory_current));
        let labels = Arc::new(labels_from_cgroup_path(state.cgroup_paths.get(&cgroup_id)));
        state.cgroup_labels.insert(cgroup_id, labels);
    }
}

/// Walk cgroup v2 hierarchy and seed the aggregator for workloads that started before the agent.
/// Returns any early-flush batches triggered by `max_keys` during bootstrap (caller should `emit_batch`).
pub async fn bootstrap_existing_cgroups(
    cache: &AttributionCache,
    agg: &mut crate::aggregator::Aggregator,
    node: &str,
) -> Vec<crate::aggregator::BatchPayload> {
    let root = cgroup_v2_mount();
    let mut bootstrapped = 0usize;
    let mut early_flushes = Vec::new();

    for entry in WalkDir::new(&root).into_iter().filter_map(|e| e.ok()) {
        if !entry.file_type().is_dir() {
            continue;
        }
        let dir = entry.path();
        if dir == root.as_path() {
            continue;
        }

        let Ok(meta) = fs::metadata(dir) else {
            continue;
        };
        let cgroup_id = meta.ino();
        if cgroup_id == 0 {
            continue;
        }

        let rel_path = dir
            .strip_prefix(&root)
            .ok()
            .map(|p| PathBuf::from("/").join(p));
        if let Some(rel_path) = rel_path {
            cache.register_cgroup_directory(cgroup_id, rel_path);
        }

        let event = FinopsEvent {
            kind: EVENT_KIND_WORKLOAD_IDENTITY,
            _pad: [0u8; 7],
            pid: 0,
            tgid: 0,
            cpu_id: 0,
            _pad2: 0,
            cgroup_id,
            timestamp: 0,
            memory_bytes: 0,
            comm: [0u8; 16],
        };

        if let Some(batch) = agg.on_finops_event(&event, cache, node) {
            early_flushes.push(batch);
        }
        bootstrapped += 1;
    }

    log::info!("Bootstrapped {bootstrapped} existing cgroups from {}", root.display());
    early_flushes
}

fn cgroup_v2_mount() -> PathBuf {
    std::env::var("FINOPS_CGROUP_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/sys/fs/cgroup"))
}

fn precompute_memory_current(cgroup_root: &Path, rel_path: &Path) -> PathBuf {
    let rel = rel_path
        .strip_prefix(Path::new("/"))
        .unwrap_or(rel_path);
    cgroup_root.join(rel).join("memory.current")
}

/// Parse one line from `/proc/{pid}/cgroup`.
///
/// cgroup v2 unified hierarchy: `0::/kubepods.slice/...` (two colons before path).
/// Using `split_once(':')` is wrong — it yields `:/kubepods...` instead of `/kubepods...`.
fn parse_cgroup_v2_path_line(line: &str) -> Option<&str> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    if let Some((_, path)) = line.split_once("::") {
        if path.starts_with('/') {
            return Some(path);
        }
    }
    // cgroup v1 fallback: path is the segment after the last colon
    let path = line.rsplit(':').next()?;
    if path.starts_with('/') {
        Some(path)
    } else {
        None
    }
}

fn cgroup_path_from_pid(pid: u32) -> Result<PathBuf, AttributionError> {
    let cgroup_file = format!("/proc/{pid}/cgroup");
    let mut file = File::open(&cgroup_file).map_err(|source| AttributionError::OpenFile {
        path: cgroup_file.clone(),
        source,
    })?;

    let mut buf = [0u8; 1024];
    let n = file.read(&mut buf).map_err(|source| AttributionError::OpenFile {
        path: cgroup_file.clone(),
        source,
    })?;
    if n == 0 {
        return Err(AttributionError::EmptyCgroupFile {
            path: cgroup_file,
        });
    }

    let contents = std::str::from_utf8(&buf[..n]).map_err(|source| {
        AttributionError::InvalidCgroupUtf8 {
            path: cgroup_file.clone(),
            source,
        }
    })?;
    for line in contents.lines() {
        if let Some(path) = parse_cgroup_v2_path_line(line) {
            return Ok(PathBuf::from(path));
        }
    }
    Err(AttributionError::NoCgroupPath {
        path: cgroup_file,
    })
}

/// Read cgroup v2 `memory.current` (used from the memory sampler blocking task).
pub fn read_memory_current_at(path: &Path) -> Result<u64, AttributionError> {
    let path_buf = path.to_path_buf();
    let mut file = File::open(path).map_err(|source| AttributionError::OpenFile {
        path: path.display().to_string(),
        source,
    })?;

    let mut buf = [0u8; 32];
    let n = file.read(&mut buf).map_err(|source| AttributionError::OpenFile {
        path: path.display().to_string(),
        source,
    })?;
    if n == 0 {
        return Err(AttributionError::EmptyMemoryCurrent { path: path_buf });
    }

    let raw_str = std::str::from_utf8(&buf[..n])
        .map_err(|source| AttributionError::InvalidMemoryUtf8 {
            path: path.to_path_buf(),
            source,
        })?
        .trim();
    raw_str.parse::<u64>().map_err(|_| AttributionError::ParseMemoryBytes {
        path: path.to_path_buf(),
        value: raw_str.to_string(),
    })
}

fn labels_from_cgroup_path(path: Option<&PathBuf>) -> WorkloadLabels {
    let Some(path) = path else {
        return WorkloadLabels::default();
    };
    let pod_uid = extract_pod_uid_from_path(path);
    let container = extract_container_from_path(path);

    WorkloadLabels {
        namespace: None,
        pod: None,
        container,
        pod_uid: pod_uid.clone(),
        k8s_resolved: false,
    }
}

/// Walk `Path::components` — no `to_string_lossy()` heap allocation for the full path.
fn extract_pod_uid_from_path(path: &Path) -> Option<String> {
    for component in path.components() {
        let Component::Normal(part) = component else {
            continue;
        };
        let part = part.to_str()?;
        if let Some(rest) = part.strip_prefix("kubepods-") {
            if let Some(uid_part) = rest.split("-pod").nth(1) {
                let uid = uid_part.trim_end_matches(".slice");
                return Some(uid.replace('_', "-"));
            }
        }
    }
    None
}

fn extract_container_from_path(path: &Path) -> Option<String> {
    for component in path.components() {
        let Component::Normal(part) = component else {
            continue;
        };
        let part = part.to_str()?;
        if let Some(id) = part.strip_prefix("cri-container-") {
            let id = id.trim_end_matches(".scope");
            return Some(id.to_string());
        }
        if let Some(name) = part.strip_prefix("docker-") {
            let name = name.trim_end_matches(".scope");
            return Some(name.to_string());
        }
    }
    None
}

/// Merge pod API labels into `cgroup_labels` for every tracked cgroup (write lock — background only).
fn merge_cgroup_labels_from_k8s(cache: &AttributionCache) {
    let mut state = cache.state.write();
    let cgroup_ids: Vec<u64> = state.cgroup_paths.keys().copied().collect();
    let pod_by_uid = state.pod_by_uid.clone();
    for cgroup_id in cgroup_ids {
        let Some(path) = state.cgroup_paths.get(&cgroup_id) else {
            continue;
        };
        let mut labels = labels_from_cgroup_path(Some(path));
        if let Some(uid) = extract_pod_uid_from_path(path) {
            if let Some(pod_labels) = pod_by_uid.get(&uid) {
                labels.namespace = pod_labels.namespace.clone();
                labels.pod = pod_labels.pod.clone();
                labels.k8s_resolved = true;
                if labels.container.is_none() {
                    labels.container = pod_labels.container.clone();
                }
            }
        }
        state.cgroup_labels.insert(cgroup_id, Arc::new(labels));
    }
}

/// In-cluster Kubernetes API refresh (best-effort).
pub async fn refresh_k8s_pods(
    cache: &AttributionCache,
    client: &kube::Client,
) -> Result<(), AttributionError> {
    if std::env::var("KUBERNETES_SERVICE_HOST").is_err() {
        return Ok(());
    }

    let node_name = std::env::var("FINOPS_NODE_NAME")
        .or_else(|_| std::env::var("NODE_NAME"))
        .unwrap_or_else(|_| hostname());

    let pods: kube::Api<k8s_openapi::api::core::v1::Pod> = kube::Api::all(client.clone());

    let list = pods
        .list(&kube::api::ListParams::default().fields(&format!(
            "spec.nodeName={node_name}"
        )))
        .await?;

    for pod in list.items {
        let meta = &pod.metadata;
        let uid = meta.uid.clone().unwrap_or_default();
        if uid.is_empty() {
            continue;
        }
        let namespace = meta
            .namespace
            .clone()
            .unwrap_or_else(|| "default".into());
        let pod_name = meta.name.clone().unwrap_or_default();
        let mut container = None;
        if let Some(spec) = &pod.spec {
            if let Some(first) = spec.containers.first() {
                container = Some(first.name.clone());
            }
        }
        cache.upsert_pod_labels(
            uid,
            WorkloadLabels {
                namespace: Some(namespace),
                pod: Some(pod_name),
                container,
                pod_uid: None,
                k8s_resolved: true,
            },
        );
    }

    merge_cgroup_labels_from_k8s(cache);

    log::debug!("K8s pod cache refreshed for node {node_name}");
    Ok(())
}

fn hostname() -> String {
    fs::read_to_string("/etc/hostname")
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "localhost".into())
}
