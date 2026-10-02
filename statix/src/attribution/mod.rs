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

use parking_lot::RwLock;
use rustc_hash::FxHashMap;
use statix_common::StatixEvent;
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

/// What the pod watcher knows about one pod: where it lives, and which
/// container ID belongs to which container name.
#[derive(Debug, Default, Clone)]
pub struct PodInfo {
    pub namespace: String,
    pub name: String,
    /// Container ID -> container name mapping.
    pub containers: FxHashMap<String, String>,
}

pub static DEFAULT_LABELS: LazyLock<Arc<WorkloadLabels>> =
    LazyLock::new(|| Arc::new(WorkloadLabels::default()));

#[derive(Debug, Default)]
struct CacheState {
    cgroup_paths: FxHashMap<u64, PathBuf>,
    memory_current_paths: FxHashMap<u64, Arc<PathBuf>>,
    memory_stat_paths: FxHashMap<u64, Arc<PathBuf>>,
    cpu_stat_paths: FxHashMap<u64, Arc<PathBuf>>,
    cgroup_labels: FxHashMap<u64, Arc<WorkloadLabels>>,
    pod_by_uid: FxHashMap<String, Arc<PodInfo>>,
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

    pub fn on_identity_event(&self, event: &StatixEvent) {
        {
            let state = self.state.read();
            if state.cgroup_paths.contains_key(&event.cgroup_id) {
                return;
            }
        }

        let rel_path = cgroup_path_from_pid(event.pid).ok();
        let mut state = self.state.write();
        if state.cgroup_paths.contains_key(&event.cgroup_id) {
            return;
        }
        if let Some(rel_path) = rel_path {
            let memory_current = precompute_memory_current(&self.cgroup_root, &rel_path);
            let memory_stat = precompute_memory_stat(&self.cgroup_root, &rel_path);
            let cpu_stat = precompute_cpu_stat(&self.cgroup_root, &rel_path);
            state.cgroup_paths.insert(event.cgroup_id, rel_path);
            state
                .memory_current_paths
                .insert(event.cgroup_id, Arc::new(memory_current));
            state
                .memory_stat_paths
                .insert(event.cgroup_id, Arc::new(memory_stat));
            state
                .cpu_stat_paths
                .insert(event.cgroup_id, Arc::new(cpu_stat));
        }
        let labels = Arc::new(labels_from_cgroup_path(
            state.cgroup_paths.get(&event.cgroup_id),
        ));
        state.cgroup_labels.insert(event.cgroup_id, labels);
    }

    pub fn for_each_sample_target(
        &self,
        mut f: impl FnMut(u64, Arc<PathBuf>, Arc<PathBuf>, Arc<PathBuf>),
    ) {
        let state = self.state.read();
        for (cgroup_id, mem_path) in state.memory_current_paths.iter() {
            if let (Some(cpu_path), Some(stat_path)) = (
                state.cpu_stat_paths.get(cgroup_id),
                state.memory_stat_paths.get(cgroup_id),
            ) {
                f(
                    *cgroup_id,
                    Arc::clone(mem_path),
                    Arc::clone(cpu_path),
                    Arc::clone(stat_path),
                );
            }
        }
    }

    /// Read-only label lookup — K8s merge runs in `watch_k8s_pods`, not on the hot path.
    pub fn labels_for_cgroup(&self, cgroup_id: u64) -> Arc<WorkloadLabels> {
        let state = self.state.read();
        state
            .cgroup_labels
            .get(&cgroup_id)
            .cloned()
            .unwrap_or_else(|| Arc::clone(&DEFAULT_LABELS))
    }

    pub fn upsert_pod(&self, uid: String, info: PodInfo) {
        self.state.write().pod_by_uid.insert(uid, Arc::new(info));
    }

    /// Register a cgroup directory discovered at startup (inode = `cgroup_id` in cgroup v2).
    pub fn register_cgroup_directory(&self, cgroup_id: u64, rel_path: PathBuf) {
        let mut state = self.state.write();
        let memory_current = precompute_memory_current(&self.cgroup_root, &rel_path);
        let memory_stat = precompute_memory_stat(&self.cgroup_root, &rel_path);
        let cpu_stat = precompute_cpu_stat(&self.cgroup_root, &rel_path);
        state.cgroup_paths.insert(cgroup_id, rel_path);
        state
            .memory_current_paths
            .insert(cgroup_id, Arc::new(memory_current));
        state
            .memory_stat_paths
            .insert(cgroup_id, Arc::new(memory_stat));
        state.cpu_stat_paths.insert(cgroup_id, Arc::new(cpu_stat));
        let labels = Arc::new(labels_from_cgroup_path(state.cgroup_paths.get(&cgroup_id)));
        state.cgroup_labels.insert(cgroup_id, labels);
    }

    /// Remove entries whose `memory.current` path no longer exists (terminated pods/cgroups).
    pub fn evict_stale_cgroups(&self) -> usize {
        let stale_ids: Vec<u64> = {
            let state = self.state.read();
            state
                .memory_current_paths
                .iter()
                .filter(|(_, path)| !path.exists())
                .map(|(id, _)| *id)
                .collect()
        };
        if stale_ids.is_empty() {
            return 0;
        }
        let mut state = self.state.write();
        for id in &stale_ids {
            state.cgroup_paths.remove(id);
            state.memory_current_paths.remove(id);
            state.memory_stat_paths.remove(id);
            state.cpu_stat_paths.remove(id);
            state.cgroup_labels.remove(id);
        }
        stale_ids.len()
    }

    pub fn remove_pod_by_uid(&self, uid: &str) {
        self.state.write().pod_by_uid.remove(uid);
    }
}

/// Walk the cgroup v2 hierarchy and register every existing cgroup, so the
/// sampler can read workloads that started before the agent from the first tick.
/// Registers only: it does not feed the aggregator. Leaf cgroups get their row
/// from the first sample; a fake exec event here only produced zero-value rows
/// for parents and one phantom exec per cgroup (ADR 068).
pub async fn bootstrap_existing_cgroups(cache: &AttributionCache) {
    let root = cgroup_v2_mount();
    let walk_root = root.clone();

    let discovered = tokio::task::spawn_blocking(move || {
        let mut entries = Vec::new();
        for entry in WalkDir::new(&walk_root).into_iter().filter_map(|e| e.ok()) {
            if !entry.file_type().is_dir() {
                continue;
            }
            let dir = entry.path();
            if dir == walk_root.as_path() {
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
                .strip_prefix(&walk_root)
                .ok()
                .map(|p| PathBuf::from("/").join(p));
            if let Some(rel_path) = rel_path {
                entries.push((cgroup_id, rel_path));
            }
        }
        entries
    })
    .await
    .unwrap_or_default();

    let mut bootstrapped = 0usize;
    for (cgroup_id, rel_path) in discovered {
        cache.register_cgroup_directory(cgroup_id, rel_path);
        bootstrapped += 1;
    }

    log::info!(
        "Bootstrapped {bootstrapped} existing cgroups from {}",
        root.display()
    );
}

fn cgroup_v2_mount() -> PathBuf {
    statix_infra::env::var("STATIX_CGROUP_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/sys/fs/cgroup"))
}

fn precompute_cpu_stat(cgroup_root: &Path, rel_path: &Path) -> PathBuf {
    let rel = rel_path.strip_prefix(Path::new("/")).unwrap_or(rel_path);
    cgroup_root.join(rel).join("cpu.stat")
}

fn precompute_memory_current(cgroup_root: &Path, rel_path: &Path) -> PathBuf {
    let rel = rel_path.strip_prefix(Path::new("/")).unwrap_or(rel_path);
    cgroup_root.join(rel).join("memory.current")
}

fn precompute_memory_stat(cgroup_root: &Path, rel_path: &Path) -> PathBuf {
    let rel = rel_path.strip_prefix(Path::new("/")).unwrap_or(rel_path);
    cgroup_root.join(rel).join("memory.stat")
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
    let n = file
        .read(&mut buf)
        .map_err(|source| AttributionError::OpenFile {
            path: cgroup_file.clone(),
            source,
        })?;
    if n == 0 {
        return Err(AttributionError::EmptyCgroupFile { path: cgroup_file });
    }

    let contents =
        std::str::from_utf8(&buf[..n]).map_err(|source| AttributionError::InvalidCgroupUtf8 {
            path: cgroup_file.clone(),
            source,
        })?;
    for line in contents.lines() {
        if let Some(path) = parse_cgroup_v2_path_line(line) {
            return Ok(PathBuf::from(path));
        }
    }
    Err(AttributionError::NoCgroupPath { path: cgroup_file })
}

/// Read cgroup v2 `memory.current` (used from the memory sampler blocking task).
pub fn read_memory_current_at(path: &Path) -> Result<u64, AttributionError> {
    let path_buf = path.to_path_buf();
    let mut file = File::open(path).map_err(|source| AttributionError::OpenFile {
        path: path.display().to_string(),
        source,
    })?;

    let mut buf = [0u8; 32];
    let n = file
        .read(&mut buf)
        .map_err(|source| AttributionError::OpenFile {
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
    raw_str
        .parse::<u64>()
        .map_err(|_| AttributionError::ParseMemoryBytes {
            path: path.to_path_buf(),
            value: raw_str.to_string(),
        })
}

pub fn read_cpu_usage_usec_at(path: &Path) -> Result<u64, AttributionError> {
    let path_buf = path.to_path_buf();
    let mut file = File::open(path).map_err(|source| AttributionError::OpenFile {
        path: path.display().to_string(),
        source,
    })?;

    let mut buf = [0u8; 256];
    let n = file
        .read(&mut buf)
        .map_err(|source| AttributionError::OpenFile {
            path: path.display().to_string(),
            source,
        })?;
    if n == 0 {
        return Err(AttributionError::EmptyCpuStat { path: path_buf });
    }

    let contents =
        std::str::from_utf8(&buf[..n]).map_err(|source| AttributionError::InvalidCpuUtf8 {
            path: path_buf.clone(),
            source,
        })?;
    let first_line = contents
        .lines()
        .next()
        .ok_or(AttributionError::NoCpuUsageField {
            path: path_buf.clone(),
        })?;
    let value = first_line
        .strip_prefix("usage_usec ")
        .ok_or(AttributionError::NoCpuUsageField {
            path: path_buf.clone(),
        })?
        .trim();
    value
        .parse::<u64>()
        .map_err(|_| AttributionError::ParseCpuUsage {
            path: path_buf,
            value: value.to_string(),
        })
}

/// Read `inactive_file` from cgroup v2 `memory.stat`: page cache not used
/// recently, the first thing the kernel reclaims. Working set =
/// `memory.current - inactive_file`, the number `kubectl top` shows (ADR 071).
/// `None` if the file or field is missing; the caller then falls back to
/// `memory.current` alone.
pub fn read_inactive_file_at(path: &Path) -> Option<u64> {
    let mut file = File::open(path).ok()?;
    let mut buf = [0u8; 4096];
    let n = file.read(&mut buf).ok()?;
    let contents = std::str::from_utf8(&buf[..n]).ok()?;
    contents
        .lines()
        .find_map(|line| line.strip_prefix("inactive_file "))?
        .trim()
        .parse()
        .ok()
}

fn labels_from_cgroup_path(path: Option<&PathBuf>) -> WorkloadLabels {
    let Some(path) = path else {
        return WorkloadLabels::default();
    };
    let pod_uid = pod_uid_from_path(path);
    // Outside Kubernetes (e.g. plain Docker) the short container ID is the only
    // name there is: the same 12 characters `docker ps` shows. Inside a known
    // pod, merge_cgroup_labels_from_k8s replaces it with the real name.
    let container = path
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(container_id_from_dir_name)
        .map(|id| id[..12].to_string());

    WorkloadLabels {
        namespace: None,
        pod: None,
        container,
        pod_uid: pod_uid.clone(),
        k8s_resolved: false,
    }
}

/// Pod UID from a cgroup path, for every layout: systemd driver
/// (`kubepods-burstable-pod<uid_with_underscores>.slice`, Guaranteed
/// `kubepods-pod<uid>.slice`) and cgroupfs driver (`pod<uid-with-dashes>`).
/// Finds the last "pod" in each path component and accepts it only if a real
/// UUID follows. A UUID is hex, so "pod" can never appear inside one.
fn pod_uid_from_path(path: &Path) -> Option<String> {
    path.components().find_map(|component| {
        let Component::Normal(part) = component else {
            return None;
        };
        let part = part.to_str()?;
        let start = part.rfind("pod")? + 3;
        let uid = part.get(start..start + 36)?;
        is_uuid(uid).then(|| uid.replace('_', "-"))
    })
}

/// 36 chars: hex, with `-` (or `_`, as systemd writes it) at 8, 13, 18, 23.
fn is_uuid(s: &str) -> bool {
    s.len() == 36
        && s.bytes().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => b == b'_' || b == b'-',
            _ => b.is_ascii_hexdigit(),
        })
}

/// 64-hex container ID from a container cgroup's folder name, for every
/// runtime and driver: `cri-containerd-<id>.scope`, `crio-<id>.scope`,
/// `docker-<id>.scope`, or a bare `<id>` (cgroupfs driver). Matches on the
/// ID, never on the prefix, so a new runtime's prefix doesn't break it.
pub fn container_id_from_dir_name(name: &str) -> Option<&str> {
    let name = name.strip_suffix(".scope").unwrap_or(name);
    let id = name.rsplit('-').next()?;
    (id.len() == 64 && id.bytes().all(|b| b.is_ascii_hexdigit())).then_some(id)
}

/// Build a `PodInfo` from a pod as the Kubernetes API returns it.
/// `None` if the pod has no UID. Reads all three status lists, because init
/// and ephemeral (debug) containers get cgroups too.
fn pod_info_from(pod: &k8s_openapi::api::core::v1::Pod) -> Option<(String, PodInfo)> {
    let meta = &pod.metadata;
    let uid = meta.uid.clone().filter(|uid| !uid.is_empty())?;
    let mut info = PodInfo {
        namespace: meta.namespace.clone().unwrap_or_else(|| "default".into()),
        name: meta.name.clone().unwrap_or_default(),
        containers: FxHashMap::default(),
    };
    if let Some(status) = &pod.status {
        let lists = [
            &status.container_statuses,
            &status.init_container_statuses,
            &status.ephemeral_container_statuses,
        ];
        for statuses in lists.into_iter().flatten() {
            for cs in statuses {
                if let Some((_, id)) = cs.container_id.as_deref().and_then(|s| s.split_once("://"))
                {
                    info.containers.insert(id.to_string(), cs.name.clone());
                }
            }
        }
    }
    Some((uid, info))
}

/// Container name for a container cgroup folder inside a known pod. `None` for:
/// the pause/sandbox container (its ID is in no status list); a container whose
/// status hasn't arrived yet; and CRI-O's `crio-conmon-<id>` monitor, which
/// carries the real container's ID and would otherwise be charged its requests twice.
fn container_name_in_pod(dir_name: &str, pod: &PodInfo) -> Option<String> {
    if dir_name.starts_with("crio-conmon-") {
        return None;
    }
    let id = container_id_from_dir_name(dir_name)?;
    pod.containers.get(id).cloned()
}

/// Merge pod API labels into `cgroup_labels` for every tracked cgroup (background only).
fn merge_cgroup_labels_from_k8s(cache: &AttributionCache) {
    let (cgroup_snap, pod_snap) = {
        let state = cache.state.read();
        let cgroups: Vec<(u64, PathBuf)> = state
            .cgroup_paths
            .iter()
            .map(|(k, v)| (*k, v.clone()))
            .collect();
        let pods = state.pod_by_uid.clone();
        (cgroups, pods)
    };

    let mut new_labels: Vec<(u64, Arc<WorkloadLabels>)> = Vec::with_capacity(cgroup_snap.len());
    let mut unmatched = 0u64;
    for (cgroup_id, path) in &cgroup_snap {
        let mut labels = labels_from_cgroup_path(Some(path));
        if let Some(uid) = &labels.pod_uid {
            match pod_snap.get(uid) {
                Some(pod) => {
                    labels.namespace = Some(pod.namespace.clone());
                    labels.pod = Some(pod.name.clone());
                    labels.k8s_resolved = true;
                    labels.container = path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .and_then(|name| container_name_in_pod(name, pod));
                }
                // Under a pod the watcher doesn't know: an unfamiliar layout,
                // or a pod that started a moment ago. Counted so it's visible.
                None => unmatched += 1,
            }
        }
        new_labels.push((*cgroup_id, Arc::new(labels)));
    }
    metrics::gauge!("statix.k8s.unmatched_cgroups").set(unmatched as f64);

    let mut state = cache.state.write();
    for (cgroup_id, labels) in new_labels {
        state.cgroup_labels.insert(cgroup_id, labels);
    }
}

/// Kubeconfig path for DEVELOPMENT only (e.g. the agent as a plain binary next
/// to a local k3s). Deliberately not plain `KUBECONFIG`: that is often set on
/// machines for other reasons, and the file is usually an admin key.
/// Production runs as a DaemonSet pod with its own limited ServiceAccount.
fn dev_kubeconfig() -> Option<String> {
    std::env::var("STATIX_DEV_KUBECONFIG")
        .ok()
        .filter(|path| !path.is_empty())
}

/// Is there a Kubernetes API to talk to? In a pod, Kubernetes sets
/// `KUBERNETES_SERVICE_HOST`; in development, `STATIX_DEV_KUBECONFIG`.
pub fn k8s_configured() -> bool {
    std::env::var("KUBERNETES_SERVICE_HOST").is_ok() || dev_kubeconfig().is_some()
}

/// The Kubernetes client. In a pod: the pod's own ServiceAccount (production).
/// With `STATIX_DEV_KUBECONFIG`: that file, with a loud warning.
pub async fn k8s_client() -> anyhow::Result<kube::Client> {
    let Some(path) = dev_kubeconfig() else {
        return Ok(kube::Client::try_default().await?);
    };
    log::warn!(
        "DEV MODE: Kubernetes access via STATIX_DEV_KUBECONFIG={path} — \
         usually an admin key; never use this in production"
    );
    let kubeconfig = kube::config::Kubeconfig::read_from(&path)?;
    let config = kube::Config::from_custom_kubeconfig(
        kubeconfig,
        &kube::config::KubeConfigOptions::default(),
    )
    .await?;
    Ok(kube::Client::try_from(config)?)
}

/// Stream pod label updates via the Kubernetes watch API (node-scoped field selector).
/// Reconnects on stream end; runs `refresh_k8s_pods` list fallback between retries.
pub async fn watch_k8s_pods(cache: AttributionCache, client: kube::Client) {
    use futures::TryStreamExt;
    use kube::runtime::watcher;
    use kube::runtime::watcher::Event;
    use kube::runtime::WatchStreamExt;
    use std::pin::pin;
    use std::time::Duration;

    let mut reconnect_backoff = Duration::from_secs(5);
    const MAX_RECONNECT_BACKOFF: Duration = Duration::from_secs(300);

    loop {
        let node_name = statix_infra::env::var("STATIX_NODE_NAME")
            .or_else(|| std::env::var("NODE_NAME").ok())
            .unwrap_or_else(|| hostname());

        let pods: kube::Api<k8s_openapi::api::core::v1::Pod> = kube::Api::all(client.clone());
        let wc = watcher::Config::default().fields(&format!("spec.nodeName={node_name}"));

        let mut stream = pin!(watcher(pods, wc).default_backoff());

        while let Ok(Some(event)) = stream.try_next().await {
            reconnect_backoff = Duration::from_secs(5);

            match event {
                Event::Apply(pod) => {
                    if let Some((uid, info)) = pod_info_from(&pod) {
                        cache.upsert_pod(uid, info);
                        merge_cgroup_labels_from_k8s(&cache);
                    }
                }

                Event::Delete(pod) => {
                    if let Some(uid) = pod.metadata.uid.as_ref() {
                        cache.remove_pod_by_uid(uid);
                    }
                }

                // Initial list: store every pod now, merge once at InitDone.
                Event::InitApply(pod) => {
                    if let Some((uid, info)) = pod_info_from(&pod) {
                        cache.upsert_pod(uid, info);
                    }
                }

                Event::Init => {}
                Event::InitDone => {
                    merge_cgroup_labels_from_k8s(&cache);
                    log::info!("K8s pod watcher initial list applied for node {node_name}");
                }
            }
        }

        log::warn!("K8s pod watcher stream ended; reconnecting in {reconnect_backoff:?}");
        if let Err(e) = refresh_k8s_pods(&cache, &client).await {
            log::warn!("K8s list fallback failed: {e}");
        }
        let jitter = rand::random::<f64>() * reconnect_backoff.as_secs_f64() * 0.3;
        tokio::time::sleep(reconnect_backoff + Duration::from_secs_f64(jitter)).await;
        reconnect_backoff = (reconnect_backoff * 2).min(MAX_RECONNECT_BACKOFF);
    }
}

/// One-shot list refresh (watcher reconnect fallback).
pub async fn refresh_k8s_pods(
    cache: &AttributionCache,
    client: &kube::Client,
) -> Result<(), AttributionError> {
    if !k8s_configured() {
        return Ok(());
    }

    let node_name = statix_infra::env::var("STATIX_NODE_NAME")
        .or_else(|| std::env::var("NODE_NAME").ok())
        .unwrap_or_else(|| hostname());

    let pods: kube::Api<k8s_openapi::api::core::v1::Pod> = kube::Api::all(client.clone());

    let list = pods
        .list(&kube::api::ListParams::default().fields(&format!("spec.nodeName={node_name}")))
        .await?;

    for pod in &list.items {
        if let Some((uid, info)) = pod_info_from(pod) {
            cache.upsert_pod(uid, info);
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_test_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("{prefix}-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    const ID_B: &str = "b01aa86080a7764f1c493ede232c77d5abea6632756dc61ae95abd7fbd0698f6";

    /// One `containerStatuses` entry as the Kubernetes API sends it.
    fn status(name: &str, id: &str) -> serde_json::Value {
        serde_json::json!({
            "name": name,
            "containerID": format!("containerd://{id}"),
            "image": "", "imageID": "", "ready": true, "restartCount": 0
        })
    }

    #[test]
    fn pod_info_maps_container_ids_to_names() {
        let pod: k8s_openapi::api::core::v1::Pod = serde_json::from_value(serde_json::json!({
            "metadata": {
                "uid": "2b7669e8-6828-4310-9a1f-0eaad6933466",
                "name": "reqtest-7694f84d54-84klg",
                "namespace": "default"
            },
            "status": {
                "containerStatuses": [ status("web", ID_A) ],
                "initContainerStatuses": [ status("init-db", ID_B) ]
            }
        }))
        .unwrap();

        let (uid, info) = pod_info_from(&pod).unwrap();
        assert_eq!(uid, "2b7669e8-6828-4310-9a1f-0eaad6933466");
        assert_eq!(info.name, "reqtest-7694f84d54-84klg");
        assert_eq!(info.containers.get(ID_A).map(String::as_str), Some("web"));
        assert_eq!(
            info.containers.get(ID_B).map(String::as_str),
            Some("init-db")
        );
    }

    #[test]
    fn container_name_in_pod_cases() {
        let mut pod = PodInfo::default();
        pod.containers.insert(ID_A.to_string(), "web".to_string());

        let web = format!("cri-containerd-{ID_A}.scope");
        let pause = format!("cri-containerd-{ID_B}.scope");
        let conmon = format!("crio-conmon-{ID_A}.scope");

        assert_eq!(container_name_in_pod(&web, &pod), Some("web".to_string()));
        assert_eq!(
            container_name_in_pod(&pause, &pod),
            None,
            "pause: ID in no status list"
        );
        assert_eq!(
            container_name_in_pod(&conmon, &pod),
            None,
            "CRI-O monitor must not be the container"
        );
    }

    #[test]
    fn phase14_read_cpu_usage_usec_at_parses() {
        let dir = temp_test_dir("statix-cpu-stat");
        let path = dir.join("cpu.stat");
        fs::write(&path, b"usage_usec 123456\nnr_periods 10\n").unwrap();
        assert_eq!(read_cpu_usage_usec_at(&path).unwrap(), 123456);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn phase14_soft_miss_memory_still_works() {
        let dir = temp_test_dir("statix-soft-miss");
        let mem_path = dir.join("memory.current");
        let cpu_path = dir.join("cpu.stat");
        fs::write(&mem_path, b"4096").unwrap();
        assert!(read_cpu_usage_usec_at(&cpu_path).is_err());
        assert_eq!(read_memory_current_at(&mem_path).unwrap(), 4096);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn working_set_reads_inactive_file() {
        let dir = temp_test_dir("statix-memory-stat");
        let path = dir.join("memory.stat");
        fs::write(
            &path,
            b"anon 100\nfile 900\ninactive_anon 0\nactive_anon 100\ninactive_file 700\nactive_file 200\n",
        )
        .unwrap();
        assert_eq!(read_inactive_file_at(&path), Some(700));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn working_set_missing_memory_stat_is_none() {
        let dir = temp_test_dir("statix-memory-stat-missing");
        assert_eq!(read_inactive_file_at(&dir.join("memory.stat")), None);
        let _ = fs::remove_dir_all(&dir);
    }

    const ID_A: &str = "af223617009e7b29a5359185bbe2a9b79b296314cc8e4eca93ecbd19d7c0daae";

    #[test]
    fn pod_uid_every_layout() {
        let want = Some("2b7669e8-6828-4310-9a1f-0eaad6933466".to_string());
        // Real k3s paths (containerd + systemd driver), 2026-09-27.
        let burstable = "/kubepods.slice/kubepods-burstable.slice/kubepods-burstable-pod2b7669e8_6828_4310_9a1f_0eaad6933466.slice/cri-containerd-af22.scope";
        let guaranteed = "/kubepods.slice/kubepods-pod2b7669e8_6828_4310_9a1f_0eaad6933466.slice/cri-containerd-af22.scope";
        // cgroupfs driver: dashes kept, no .slice.
        let cgroupfs = "/kubepods/besteffort/pod2b7669e8-6828-4310-9a1f-0eaad6933466/af22";

        assert_eq!(pod_uid_from_path(Path::new(burstable)), want);
        assert_eq!(pod_uid_from_path(Path::new(guaranteed)), want);
        assert_eq!(pod_uid_from_path(Path::new(cgroupfs)), want);
    }

    #[test]
    fn pod_uid_none_outside_kubernetes() {
        assert_eq!(
            pod_uid_from_path(Path::new("/system.slice/ssh.service")),
            None
        );
        assert_eq!(
            pod_uid_from_path(Path::new("/kubepods.slice/kubepods-burstable.slice")),
            None
        );
    }

    #[test]
    fn container_id_every_runtime() {
        for name in [
            format!("cri-containerd-{ID_A}.scope"), // containerd, systemd driver
            format!("crio-{ID_A}.scope"),           // CRI-O
            format!("docker-{ID_A}.scope"),         // Docker / cri-dockerd
            ID_A.to_string(),                       // cgroupfs driver: bare ID
        ] {
            assert_eq!(container_id_from_dir_name(&name), Some(ID_A), "{name}");
        }
    }

    #[test]
    fn container_id_none_for_non_containers() {
        for name in [
            "kubepods-burstable-pod2b7669e8_6828_4310_9a1f_0eaad6933466.slice",
            "ssh.service",
            "session-4.scope",
            "cri-containerd-tooshort.scope",
        ] {
            assert_eq!(container_id_from_dir_name(name), None, "{name}");
        }
    }
}
