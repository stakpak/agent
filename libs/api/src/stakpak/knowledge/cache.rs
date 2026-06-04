//! Cache for the knowledge store.
//!
//! Mirrors the server's S3 layout under `~/.stakpak/remote-knowledge/<account>/<path>`.
//! The local file IS the cached body; the SHA-256 of its contents IS the ETag we
//! send back to the server in `If-None-Match`. No sidecar files, no metadata.
//!
//! All operations are best-effort.

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tracing::{debug, warn};

/// Returns `~/.stakpak/remote-knowledge/<account>`
fn knowledge_cache_root(account: &str) -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    Some(
        home.join(".stakpak")
            .join("remote-knowledge")
            .join(urlencoding::encode(account).as_ref()),
    )
}

/// Compute the absolute on-disk path for a cached knowledge file.
///
/// Refuses to resolve paths that contain [`..`, absolute paths, Windows-style backslashe] and returns `None`.
pub fn cached_path(account: &str, rel_path: &str) -> Option<PathBuf> {
    if rel_path.is_empty() || rel_path.contains("..") || rel_path.contains('\\') {
        return None;
    }
    let trimmed = rel_path.trim_start_matches('/');
    if trimmed.is_empty() {
        return None;
    }
    let root = knowledge_cache_root(account)?;
    let mut full = root.clone();
    for segment in trimmed.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." {
            return None;
        }
        full.push(segment);
    }
    if !full.starts_with(&root) {
        return None;
    }
    Some(full)
}

/// Read a cached file and return its bytes plus the hex SHA-256 ETag.
/// Returns `None` if the file does not exist or could not be read.
pub async fn read_cached(path: &Path) -> Option<(Vec<u8>, String)> {
    match fs::read(path).await {
        Ok(bytes) => {
            let etag = sha256_hex(&bytes);
            Some((bytes, etag))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            debug!("cache read failed for {}: {}", path.display(), e);
            None
        }
    }
}

/// Atomically write `content` to `path`. Creates parent directories as needed.
/// Best-effort: logs and swallows errors so cache failures don't fail requests.
pub async fn write_cached_atomic(path: &Path, content: &[u8]) {
    let Some(parent) = path.parent() else {
        return;
    };
    if let Err(e) = fs::create_dir_all(parent).await {
        warn!(
            "failed to create cache dir {}: {} (cache write skipped)",
            parent.display(),
            e
        );
        return;
    }

    // Write to a unique temp file in the same directory so `rename` is atomic
    // Include pid + nanoseconds to avoid races between concurrent writers.
    let suffix = format!(
        "tmp.{}.{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    );
    let tmp = path.with_extension(suffix);

    let write_result = async {
        let mut file = fs::File::create(&tmp).await?;
        file.write_all(content).await?;
        file.sync_all().await?;
        drop(file);
        fs::rename(&tmp, path).await
    }
    .await;

    if let Err(e) = write_result {
        warn!(
            "failed to write cache file {}: {} (best-effort, ignoring)",
            path.display(),
            e
        );
        let _ = fs::remove_file(&tmp).await;
    }
}

/// Remove a cached entry. If the path resolves to a directory, removes it
/// recursively (used after directory deletes on the server). Best-effort.
pub async fn evict_cached(path: &Path) {
    match fs::metadata(path).await {
        Ok(meta) if meta.is_dir() => {
            if let Err(e) = fs::remove_dir_all(path).await {
                debug!("cache evict (dir) failed for {}: {}", path.display(), e);
            }
        }
        Ok(_) => {
            if let Err(e) = fs::remove_file(path).await {
                debug!("cache evict (file) failed for {}: {}", path.display(), e);
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // Already gone, nothing to do.
        }
        Err(e) => {
            debug!("cache stat failed for {}: {}", path.display(), e);
        }
    }
}

/// Hex-encoded SHA-256 of `bytes`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut out = String::with_capacity(digest.len() * 2);
    for b in digest {
        use std::fmt::Write;
        let _ = write!(&mut out, "{:02x}", b);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_matches_known_vector() {
        // SHA-256 of empty string
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        // SHA-256 of "abc"
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn cached_path_rejects_traversal() {
        assert!(cached_path("acct", "../etc/passwd").is_none());
        assert!(cached_path("acct", "foo/../../bar").is_none());
        assert!(cached_path("acct", "").is_none());
        assert!(cached_path("acct", "/").is_none());
        assert!(cached_path("acct", "foo\\bar").is_none());
    }

    #[test]
    fn cached_path_normal_paths_resolve() {
        let p = cached_path("acct", "docs/runbooks/db.md").unwrap();
        let s = p.to_string_lossy();
        assert!(
            s.ends_with("acct/docs/runbooks/db.md") || s.ends_with("acct\\docs\\runbooks\\db.md")
        );
    }

    #[tokio::test]
    async fn write_then_read_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("nested/dir/file.bin");
        let content = b"hello world";
        write_cached_atomic(&target, content).await;
        let (bytes, etag) = read_cached(&target).await.unwrap();
        assert_eq!(bytes, content);
        assert_eq!(etag, sha256_hex(content));
    }

    #[tokio::test]
    async fn evict_removes_file_and_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("a/b/file");
        write_cached_atomic(&f, b"x").await;
        assert!(f.exists());
        evict_cached(&f).await;
        assert!(!f.exists());

        // Directory eviction
        let d = tmp.path().join("dir");
        write_cached_atomic(&d.join("inner.txt"), b"y").await;
        assert!(d.exists());
        evict_cached(&d).await;
        assert!(!d.exists());
    }

    #[tokio::test]
    async fn read_missing_returns_none() {
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("does-not-exist");
        assert!(read_cached(&f).await.is_none());
    }
}
