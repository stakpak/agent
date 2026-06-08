use std::collections::HashMap;

use stakpak_ak::{FileMeta, StorageBackend};

use super::SyncDirection;

/// A single conflict: same path on both sides, different content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    pub path: String,
    pub local_hash: String,
    pub remote_hash: String,
    pub local_size: u64,
    pub remote_size: u64,
}

/// The full reconciliation plan produced by [`plan`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncPlan {
    pub direction: SyncDirection,
    pub uploads: Vec<FileMeta>,
    pub downloads: Vec<FileMeta>,
    pub skipped: Vec<String>,
    pub conflicts: Vec<Conflict>,
}

impl SyncPlan {
    pub fn is_empty(&self) -> bool {
        self.uploads.is_empty() && self.downloads.is_empty() && self.conflicts.is_empty()
    }
}

/// Build a [`SyncPlan`] by enumerating both sides and classifying each path.
///
/// Both backends are walked from the store root (`""`) and indexed by
/// path. The classification rules follow these rules
///
/// | Local | Remote | Hashes | Push          | Pull           |
/// |-------|--------|--------|---------------|----------------|
/// | yes   | no     | —      | upload        | (silently skip)|
/// | no    | yes    | —      | (silently skip)| download      |
/// | yes   | yes    | match  | skip          | skip           |
/// | yes   | yes    | differ | conflict      | conflict       |
///
/// Output vectors are sorted by path for deterministic, diff-friendly output.
pub fn plan(
    local: &dyn StorageBackend,
    remote: &dyn StorageBackend,
    direction: SyncDirection,
) -> Result<SyncPlan, String> {
    let local_metas = local
        .list_with_meta("")
        .map_err(|e| format!("failed to enumerate local store: {e}"))?;
    let remote_metas = remote
        .list_with_meta("")
        .map_err(|e| format!("failed to enumerate remote store: {e}"))?;

    let mut local_index: HashMap<String, FileMeta> = local_metas
        .into_iter()
        .map(|meta| (meta.path.clone(), meta))
        .collect();
    let remote_index: HashMap<String, FileMeta> = remote_metas
        .into_iter()
        .map(|meta| (meta.path.clone(), meta))
        .collect();

    let mut uploads: Vec<FileMeta> = Vec::new();
    let mut downloads: Vec<FileMeta> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    let mut conflicts: Vec<Conflict> = Vec::new();

    for (path, remote_meta) in &remote_index {
        match local_index.remove(path) {
            Some(local_meta) => {
                if local_meta.content_hash == remote_meta.content_hash {
                    skipped.push(path.clone());
                } else {
                    conflicts.push(Conflict {
                        path: path.clone(),
                        local_hash: local_meta.content_hash,
                        remote_hash: remote_meta.content_hash.clone(),
                        local_size: local_meta.size_bytes,
                        remote_size: remote_meta.size_bytes,
                    });
                }
            }
            None => {
                if matches!(direction, SyncDirection::Pull) {
                    downloads.push(remote_meta.clone());
                }
            }
        }
    }

    for (_path, local_meta) in local_index.drain() {
        if matches!(direction, SyncDirection::Push) {
            uploads.push(local_meta);
        }
    }

    uploads.sort_by(|a, b| a.path.cmp(&b.path));
    downloads.sort_by(|a, b| a.path.cmp(&b.path));
    skipped.sort();
    conflicts.sort_by(|a, b| a.path.cmp(&b.path));

    Ok(SyncPlan {
        direction,
        uploads,
        downloads,
        skipped,
        conflicts,
    })
}
