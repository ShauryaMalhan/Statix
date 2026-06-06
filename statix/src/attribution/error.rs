//! Typed attribution errors for cgroup path and memory sampling I/O.

use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum AttributionError {
    #[error("failed to open {path}: {source}")]
    OpenFile {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("empty cgroup file: {path}")]
    EmptyCgroupFile { path: String },

    #[error("invalid utf-8 in cgroup file {path}: {source}")]
    InvalidCgroupUtf8 {
        path: String,
        #[source]
        source: std::str::Utf8Error,
    },

    #[error("no cgroup path in {path}")]
    NoCgroupPath { path: String },

    #[error("empty memory.current at {path}")]
    EmptyMemoryCurrent { path: PathBuf },

    #[error("invalid utf-8 in memory.current at {path}: {source}")]
    InvalidMemoryUtf8 {
        path: PathBuf,
        #[source]
        source: std::str::Utf8Error,
    },

    #[error("failed to parse memory bytes from {value:?} at {path}")]
    ParseMemoryBytes { path: PathBuf, value: String },

    #[error("Kubernetes API list pods failed: {0}")]
    K8sList(#[from] kube::Error),
}
