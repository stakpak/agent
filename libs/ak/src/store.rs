use crate::Error;
use serde::Serialize;
use std::cmp::Ordering;
use std::fs;
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};
use walkdir::WalkDir;

pub trait StorageBackend {
    fn create(&self, path: &str, content: &[u8]) -> Result<(), Error>;
    fn overwrite(&self, path: &str, content: &[u8]) -> Result<(), Error>;
    fn read(&self, path: &str) -> Result<Vec<u8>, Error>;
    fn read_prefix(&self, path: &str, max_bytes: usize) -> Result<Vec<u8>, Error>;
    fn remove(&self, path: &str) -> Result<(), Error>;
    fn list(&self, path: &str) -> Result<Vec<Entry>, Error>;
    /// Returns a directory tree rooted at `prefix` (empty string for the store root).
    /// The root node's name is the prefix's last component, or `.` for the store root.
    /// Missing prefixes return an empty directory node.
    fn tree(&self, prefix: &str) -> Result<TreeNode, Error>;
    /// Returns sorted store-relative file paths under `prefix`, excluding dotfiles.
    /// Missing prefixes return an empty result.
    fn walk(&self, prefix: &str) -> Result<Vec<String>, Error>;
    fn exists(&self, path: &str) -> Result<bool, Error>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Entry {
    pub name: String,
    pub is_dir: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TreeNode {
    pub name: String,
    pub is_dir: bool,
    pub children: Vec<TreeNode>,
}

impl TreeNode {
    pub fn print(&self) -> String {
        let mut lines = vec![self.name.clone()];
        self.render_children("", &mut lines);
        lines.join("\n")
    }

    fn render_children(&self, prefix: &str, lines: &mut Vec<String>) {
        let last_index = self.children.len().saturating_sub(1);

        for (index, child) in self.children.iter().enumerate() {
            let connector = if index == last_index {
                "└──"
            } else {
                "├──"
            };
            lines.push(format!("{prefix}{connector} {}", child.name));

            let next_prefix = if index == last_index {
                format!("{prefix}    ")
            } else {
                format!("{prefix}│   ")
            };
            child.render_children(&next_prefix, lines);
        }
    }
}

#[derive(Debug, Clone)]
pub struct LocalFsBackend {
    root: PathBuf,
}

impl LocalFsBackend {
    /// Return the store-relative version of an absolute path for use in error messages.
    fn relative_path(&self, path: &Path) -> PathBuf {
        path.strip_prefix(&self.root)
            .map(PathBuf::from)
            .unwrap_or_else(|_| path.to_path_buf())
    }

    pub fn new() -> Result<Self, Error> {
        if let Some(root) = std::env::var_os("AK_STORE") {
            return Ok(Self {
                root: PathBuf::from(root),
            });
        }

        let home = dirs::home_dir()
            .ok_or_else(|| Error::Parse("could not determine home directory".to_string()))?;
        Ok(Self {
            root: default_store_root(&home),
        })
    }

    pub fn with_root(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn file_count(&self) -> Result<usize, Error> {
        if !self.root.exists() {
            return Ok(0);
        }

        let mut count = 0;
        for entry in WalkDir::new(&self.root)
            .into_iter()
            .filter_entry(|entry| !is_hidden_path(entry.path(), &self.root))
        {
            let entry = entry.map_err(|error| Error::Io(std::io::Error::other(error)))?;
            if entry.path() != self.root && entry.file_type().is_symlink() {
                return Err(Error::UnsafePath(self.relative_path(entry.path())));
            }
            if entry.file_type().is_file() {
                count += 1;
            }
        }

        Ok(count)
    }

    fn ensure_store(&self) -> Result<(), Error> {
        fs::create_dir_all(&self.root)?;
        Ok(())
    }

    fn resolve_path(&self, path: &str) -> Result<PathBuf, Error> {
        if path.is_empty() {
            return Ok(self.root.clone());
        }

        let mut relative = PathBuf::new();
        for component in Path::new(path).components() {
            match component {
                Component::Normal(part) => relative.push(part),
                Component::CurDir => {}
                Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                    return Err(Error::Parse(format!("invalid store path: {path}")));
                }
            }
        }

        Ok(self.root.join(relative))
    }

    fn ensure_no_symlinks_below_root(&self, path: &Path) -> Result<(), Error> {
        let relative = path.strip_prefix(&self.root).map_err(|_| {
            Error::Parse(format!(
                "path is outside the configured store root: {}",
                self.relative_path(path).display()
            ))
        })?;

        let mut current = self.root.clone();
        for component in relative.components() {
            let Component::Normal(part) = component else {
                return Err(Error::Parse(format!(
                    "invalid resolved store path: {}",
                    self.relative_path(path).display()
                )));
            };
            current.push(part);

            match fs::symlink_metadata(&current) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    return Err(Error::UnsafePath(self.relative_path(&current)));
                }
                Ok(_) => {}
                Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
                Err(error) => return Err(Error::Io(error)),
            }
        }

        Ok(())
    }

    fn metadata_if_exists(&self, path: &Path) -> Result<Option<fs::Metadata>, Error> {
        match fs::symlink_metadata(path) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    Err(Error::UnsafePath(self.relative_path(path)))
                } else {
                    Ok(Some(metadata))
                }
            }
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
            Err(error) => Err(Error::Io(error)),
        }
    }

    fn read_file_prefix(&self, path: &Path, max_bytes: usize) -> Result<Vec<u8>, Error> {
        let mut file = fs::File::open(path)?;
        let mut buffer = vec![0; max_bytes];
        let bytes_read = std::io::Read::read(&mut file, &mut buffer)?;
        buffer.truncate(bytes_read);
        Ok(buffer)
    }

    fn cleanup_empty_parents(&self, mut current: Option<&Path>) -> Result<(), Error> {
        while let Some(path) = current {
            if path == self.root {
                break;
            }
            if !path.exists() || !path.is_dir() || fs::read_dir(path)?.next().is_some() {
                break;
            }

            fs::remove_dir(path)?;
            current = path.parent();
        }

        Ok(())
    }

    fn build_tree_node(path: &Path, name: String) -> Result<TreeNode, Error> {
        if !path.exists() {
            return Ok(TreeNode {
                name,
                is_dir: true,
                children: vec![],
            });
        }

        let metadata = fs::metadata(path)?;
        if !metadata.is_dir() {
            return Ok(TreeNode {
                name,
                is_dir: false,
                children: vec![],
            });
        }

        let mut children = Vec::new();
        for child in read_sorted_children(path, None)? {
            children.push(Self::build_tree_node(&child.path, child.name)?);
        }

        Ok(TreeNode {
            name,
            is_dir: true,
            children,
        })
    }
}

impl StorageBackend for LocalFsBackend {
    fn create(&self, path: &str, content: &[u8]) -> Result<(), Error> {
        self.ensure_store()?;
        let target = self.resolve_path(path)?;
        self.ensure_no_symlinks_below_root(&target)?;
        if self.metadata_if_exists(&target)?.is_some() {
            return Err(Error::AlreadyExists(self.relative_path(&target)));
        }

        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(target, content)?;
        Ok(())
    }

    fn overwrite(&self, path: &str, content: &[u8]) -> Result<(), Error> {
        self.ensure_store()?;
        let target = self.resolve_path(path)?;
        self.ensure_no_symlinks_below_root(&target)?;
        let _ = self.metadata_if_exists(&target)?;
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(target, content)?;
        Ok(())
    }

    fn read(&self, path: &str) -> Result<Vec<u8>, Error> {
        let target = self.resolve_path(path)?;
        self.ensure_no_symlinks_below_root(&target)?;
        if self.metadata_if_exists(&target)?.is_none() {
            return Err(Error::NotFound(self.relative_path(&target)));
        }
        Ok(fs::read(target)?)
    }

    fn read_prefix(&self, path: &str, max_bytes: usize) -> Result<Vec<u8>, Error> {
        let target = self.resolve_path(path)?;
        self.ensure_no_symlinks_below_root(&target)?;
        if self.metadata_if_exists(&target)?.is_none() {
            return Err(Error::NotFound(self.relative_path(&target)));
        }
        self.read_file_prefix(&target, max_bytes)
    }

    fn remove(&self, path: &str) -> Result<(), Error> {
        let target = self.resolve_path(path)?;
        self.ensure_no_symlinks_below_root(&target)?;
        let metadata = self
            .metadata_if_exists(&target)?
            .ok_or_else(|| Error::NotFound(self.relative_path(&target)))?;

        let parent = target.parent().map(Path::to_path_buf);
        if metadata.is_dir() {
            fs::remove_dir_all(&target)?;
        } else {
            fs::remove_file(&target)?;
        }

        self.cleanup_empty_parents(parent.as_deref())
    }

    fn list(&self, path: &str) -> Result<Vec<Entry>, Error> {
        let target = self.resolve_path(path)?;
        self.ensure_no_symlinks_below_root(&target)?;

        let Some(metadata) = self.metadata_if_exists(&target)? else {
            return if path.is_empty() {
                Ok(vec![])
            } else {
                Err(Error::NotFound(self.relative_path(&target)))
            };
        };
        if !metadata.is_dir() {
            return Err(Error::NotADirectory(self.relative_path(&target)));
        }

        read_sorted_children(&target, Some(&self.root)).map(|children| {
            children
                .into_iter()
                .map(|child| Entry {
                    name: child.name,
                    is_dir: child.is_dir,
                })
                .collect()
        })
    }

    fn tree(&self, prefix: &str) -> Result<TreeNode, Error> {
        let trimmed = prefix.trim_matches('/');
        let target = self.resolve_path(trimmed)?;
        self.ensure_no_symlinks_below_root(&target)?;
        let name = Path::new(trimmed)
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| ".".to_string());
        Self::build_tree_node(&target, name)
    }

    fn walk(&self, prefix: &str) -> Result<Vec<String>, Error> {
        let target = self.resolve_path(prefix)?;
        self.ensure_no_symlinks_below_root(&target)?;

        let Some(metadata) = self.metadata_if_exists(&target)? else {
            return Ok(vec![]);
        };
        if is_hidden_path(&target, &self.root) {
            return Ok(vec![]);
        }

        let mut walked = Vec::new();
        for entry in WalkDir::new(&target)
            .into_iter()
            .filter_entry(|entry| !is_hidden_path(entry.path(), &self.root))
        {
            let entry = entry.map_err(|error| Error::Io(std::io::Error::other(error)))?;
            if entry.path() != target && entry.file_type().is_symlink() {
                return Err(Error::UnsafePath(self.relative_path(entry.path())));
            }
            if metadata.is_file() || entry.file_type().is_file() {
                walked.push(
                    entry
                        .path()
                        .strip_prefix(&self.root)
                        .map_err(|_| {
                            Error::Parse(format!(
                                "path is outside the configured store root: {}",
                                self.relative_path(entry.path()).display()
                            ))
                        })?
                        .to_string_lossy()
                        .to_string(),
                );
            }
        }

        walked.sort();
        Ok(walked)
    }

    fn exists(&self, path: &str) -> Result<bool, Error> {
        let target = self.resolve_path(path)?;
        self.ensure_no_symlinks_below_root(&target)?;
        Ok(self.metadata_if_exists(&target)?.is_some())
    }
}

fn default_store_root(home: &Path) -> PathBuf {
    home.join(".stakpak/knowledge")
}

struct ChildEntry {
    path: PathBuf,
    name: String,
    is_dir: bool,
}

fn read_sorted_children(path: &Path, root: Option<&Path>) -> Result<Vec<ChildEntry>, Error> {
    let mut children = Vec::new();
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }

        let file_type = entry.file_type()?;
        let child_path = entry.path();
        if file_type.is_symlink() {
            let display_path = root
                .and_then(|r| child_path.strip_prefix(r).ok().map(PathBuf::from))
                .unwrap_or_else(|| child_path.clone());
            return Err(Error::UnsafePath(display_path));
        }

        children.push(ChildEntry {
            path: child_path,
            name,
            is_dir: file_type.is_dir(),
        });
    }

    children.sort_by(compare_entries);
    Ok(children)
}

fn compare_entries(left: &ChildEntry, right: &ChildEntry) -> Ordering {
    match right.is_dir.cmp(&left.is_dir) {
        Ordering::Equal => left.name.cmp(&right.name),
        other => other,
    }
}

fn is_hidden_path(path: &Path, root: &Path) -> bool {
    path.strip_prefix(root)
        .map(|relative| {
            relative
                .components()
                .any(|component| matches!(component, Component::Normal(part) if part.to_string_lossy().starts_with('.')))
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::{Entry, LocalFsBackend, StorageBackend, TreeNode};

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    #[cfg(unix)]
    use std::os::unix::fs::symlink;

    fn backend() -> (tempfile::TempDir, LocalFsBackend) {
        let temp_dir = tempfile::TempDir::new().expect("temp dir");
        let backend = LocalFsBackend::with_root(temp_dir.path().join("store"));
        (temp_dir, backend)
    }

    #[test]
    fn create_writes_new_file() {
        let (_temp_dir, backend) = backend();

        backend
            .create("knowledge/rate-limits.md", b"1000/min")
            .expect("create file");

        let content = std::fs::read_to_string(backend.root().join("knowledge/rate-limits.md"))
            .expect("read file from disk");
        assert_eq!(content, "1000/min");
    }

    #[test]
    fn create_fails_when_file_already_exists() {
        let (_temp_dir, backend) = backend();
        backend
            .create("knowledge/rate-limits.md", b"first")
            .expect("create initial file");

        let error = backend
            .create("knowledge/rate-limits.md", b"second")
            .expect_err("duplicate create should fail");

        assert!(matches!(error, crate::Error::AlreadyExists(_)));
    }

    #[test]
    fn overwrite_replaces_existing_content() {
        let (_temp_dir, backend) = backend();
        backend
            .create("summaries/auth.md", b"old")
            .expect("create initial summary");

        backend
            .overwrite("summaries/auth.md", b"new")
            .expect("overwrite file");

        let content = backend
            .read("summaries/auth.md")
            .expect("read overwritten file");
        assert_eq!(content, b"new");
    }

    #[test]
    fn read_returns_not_found_for_missing_file() {
        let (_temp_dir, backend) = backend();

        let error = backend
            .read("knowledge/missing.md")
            .expect_err("missing file should fail");

        assert!(matches!(error, crate::Error::NotFound(_)));
    }

    #[test]
    fn remove_deletes_file() {
        let (_temp_dir, backend) = backend();
        backend
            .create("knowledge/old.md", b"old")
            .expect("create file");

        backend.remove("knowledge/old.md").expect("remove file");

        assert!(!backend.root().join("knowledge/old.md").exists());
    }

    #[test]
    fn remove_cleans_empty_parent_directories() {
        let (_temp_dir, backend) = backend();
        backend
            .create("deep/nested/only-file.md", b"old")
            .expect("create nested file");

        backend
            .remove("deep/nested/only-file.md")
            .expect("remove nested file");

        assert!(!backend.root().join("deep/nested").exists());
        assert!(!backend.root().join("deep").exists());
        assert!(backend.root().exists());
    }

    #[test]
    fn list_returns_sorted_entries_without_dotfiles() {
        let (_temp_dir, backend) = backend();
        std::fs::create_dir_all(backend.root().join("knowledge/subdir")).expect("create subdir");
        std::fs::write(backend.root().join("knowledge/z-last.md"), "z").expect("write z file");
        std::fs::write(backend.root().join("knowledge/a-first.md"), "a").expect("write a file");
        std::fs::write(backend.root().join("knowledge/.hidden.md"), "h")
            .expect("write hidden file");

        let entries = backend.list("knowledge").expect("list directory");

        assert_eq!(
            entries,
            vec![
                Entry {
                    name: "subdir".to_string(),
                    is_dir: true,
                },
                Entry {
                    name: "a-first.md".to_string(),
                    is_dir: false,
                },
                Entry {
                    name: "z-last.md".to_string(),
                    is_dir: false,
                },
            ]
        );
    }

    #[test]
    fn tree_builds_recursive_sorted_structure_without_dotfiles() {
        let (_temp_dir, backend) = backend();
        backend
            .create("knowledge/rate-limits.md", b"1000/min")
            .expect("create knowledge file");
        backend
            .create("entities/auth-service.md", b"OAuth")
            .expect("create entity file");
        std::fs::write(backend.root().join(".hidden.md"), "hidden").expect("write hidden file");

        let tree = backend.tree("").expect("build tree");

        assert_eq!(
            tree,
            TreeNode {
                name: ".".to_string(),
                is_dir: true,
                children: vec![
                    TreeNode {
                        name: "entities".to_string(),
                        is_dir: true,
                        children: vec![TreeNode {
                            name: "auth-service.md".to_string(),
                            is_dir: false,
                            children: vec![],
                        }],
                    },
                    TreeNode {
                        name: "knowledge".to_string(),
                        is_dir: true,
                        children: vec![TreeNode {
                            name: "rate-limits.md".to_string(),
                            is_dir: false,
                            children: vec![],
                        }],
                    },
                ],
            }
        );
    }

    #[test]
    fn tree_returns_scoped_subtree() {
        let (_temp_dir, backend) = backend();
        backend
            .create("services/auth/flows.md", b"Auth flow\n")
            .expect("create auth file");
        backend
            .create("services/rate-limits.md", b"Rate limit\n")
            .expect("create rate file");
        backend
            .create("notes/todo.md", b"Todo\n")
            .expect("create notes file");

        assert_eq!(
            backend.tree("services").expect("scoped tree"),
            TreeNode {
                name: "services".to_string(),
                is_dir: true,
                children: vec![
                    TreeNode {
                        name: "auth".to_string(),
                        is_dir: true,
                        children: vec![TreeNode {
                            name: "flows.md".to_string(),
                            is_dir: false,
                            children: vec![],
                        }],
                    },
                    TreeNode {
                        name: "rate-limits.md".to_string(),
                        is_dir: false,
                        children: vec![],
                    },
                ],
            }
        );
    }

    #[test]
    fn tree_returns_empty_directory_for_missing_prefix() {
        let (_temp_dir, backend) = backend();

        assert_eq!(
            backend.tree("missing").expect("missing tree"),
            TreeNode {
                name: "missing".to_string(),
                is_dir: true,
                children: vec![],
            }
        );
    }

    #[test]
    fn tree_node_print_renders_connectors() {
        let tree = TreeNode {
            name: ".".to_string(),
            is_dir: true,
            children: vec![
                TreeNode {
                    name: "knowledge".to_string(),
                    is_dir: true,
                    children: vec![TreeNode {
                        name: "rate-limits.md".to_string(),
                        is_dir: false,
                        children: vec![],
                    }],
                },
                TreeNode {
                    name: "notes.md".to_string(),
                    is_dir: false,
                    children: vec![],
                },
            ],
        };

        assert_eq!(
            tree.print(),
            ".\n├── knowledge\n│   └── rate-limits.md\n└── notes.md"
        );
    }

    #[test]
    fn exists_reports_whether_path_exists() {
        let (_temp_dir, backend) = backend();
        backend
            .create("knowledge/rate-limits.md", b"1000/min")
            .expect("create file");

        assert!(
            backend
                .exists("knowledge/rate-limits.md")
                .expect("existing path check")
        );
        assert!(
            !backend
                .exists("knowledge/missing.md")
                .expect("missing path check")
        );
    }

    #[test]
    fn file_count_counts_non_dotfiles_only() {
        let (_temp_dir, backend) = backend();
        backend
            .create("knowledge/rate-limits.md", b"1000/min")
            .expect("create knowledge file");
        backend
            .create("entities/auth-service.md", b"OAuth")
            .expect("create entity file");
        std::fs::write(backend.root().join(".hidden.md"), "hidden").expect("write hidden file");

        assert_eq!(backend.file_count().expect("count files"), 2);
    }

    #[test]
    fn new_defaults_to_stakpak_knowledge_store() {
        let home = std::path::Path::new("/tmp/test-home");

        assert_eq!(
            super::default_store_root(home),
            home.join(".stakpak/knowledge")
        );
    }

    #[test]
    fn list_root_returns_empty_when_store_does_not_exist() {
        let temp_dir = tempfile::TempDir::new().expect("temp dir");
        let backend = LocalFsBackend::with_root(temp_dir.path().join("missing-store"));

        let entries = backend.list("").expect("list missing root");

        assert!(entries.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn read_rejects_symlinked_file_inside_store() {
        let (_temp_dir, backend) = backend();
        let outside = tempfile::NamedTempFile::new().expect("outside temp file");
        std::fs::write(outside.path(), "secret").expect("write outside file");
        std::fs::create_dir_all(backend.root()).expect("create store root");
        symlink(outside.path(), backend.root().join("leak.md")).expect("create symlink");

        let error = backend
            .read("leak.md")
            .expect_err("symlink read should fail");

        assert!(matches!(error, crate::Error::UnsafePath(_)));
    }

    #[cfg(unix)]
    #[test]
    fn create_rejects_symlinked_parent_directory_inside_store() {
        let (_temp_dir, backend) = backend();
        let outside = tempfile::TempDir::new().expect("outside temp dir");
        std::fs::create_dir_all(backend.root()).expect("create store root");
        symlink(outside.path(), backend.root().join("knowledge")).expect("create symlink dir");

        let error = backend
            .create("knowledge/pwned.md", b"hello")
            .expect_err("symlink parent should fail");

        assert!(matches!(error, crate::Error::UnsafePath(_)));
        assert!(!outside.path().join("pwned.md").exists());
    }

    #[test]
    fn create_rejects_parent_directory_traversal() {
        let (temp_dir, backend) = backend();
        let outside = temp_dir.path().join("outside.md");

        let error = backend
            .create("../outside.md", b"pwned")
            .expect_err("parent traversal should fail");

        assert!(matches!(error, crate::Error::Parse(_)));
        assert!(!outside.exists());
    }

    #[test]
    fn read_rejects_parent_directory_traversal() {
        let (temp_dir, backend) = backend();
        let outside = temp_dir.path().join("outside.md");
        std::fs::write(&outside, "secret").expect("write outside file");

        let error = backend
            .read("../outside.md")
            .expect_err("parent traversal read should fail");

        assert!(matches!(error, crate::Error::Parse(_)));
    }

    #[test]
    fn list_rejects_absolute_path_traversal() {
        let (_temp_dir, backend) = backend();
        let absolute = backend.root().join("knowledge");
        let absolute = absolute.to_string_lossy().to_string();

        let error = backend
            .list(&absolute)
            .expect_err("absolute path traversal should fail");

        assert!(matches!(error, crate::Error::Parse(_)));
    }

    #[cfg(unix)]
    #[test]
    fn file_count_returns_error_for_unreadable_directory() {
        let (_temp_dir, backend) = backend();
        backend
            .create("knowledge/readable.md", b"ok")
            .expect("create readable file");
        std::fs::create_dir_all(backend.root().join("knowledge/private"))
            .expect("create private dir");

        let private_dir = backend.root().join("knowledge/private");
        let original_permissions = std::fs::metadata(&private_dir)
            .expect("read metadata")
            .permissions();
        std::fs::set_permissions(&private_dir, std::fs::Permissions::from_mode(0o0))
            .expect("remove permissions");

        let result = backend.file_count();

        std::fs::set_permissions(&private_dir, original_permissions).expect("restore permissions");
        assert!(
            result.is_err(),
            "expected unreadable directory to return an error"
        );
    }

    #[test]
    fn walk_returns_sorted_relative_files_from_store_root() {
        let (_temp_dir, backend) = backend();
        backend
            .create("services/auth/flows.md", b"auth")
            .expect("create nested file");
        backend
            .create("notes/todo.md", b"todo")
            .expect("create top-level file");
        std::fs::create_dir_all(backend.root().join("services/.private"))
            .expect("create hidden dir");
        std::fs::write(backend.root().join(".hidden.md"), "hidden")
            .expect("write hidden root file");
        std::fs::write(backend.root().join("services/.secret.md"), "hidden")
            .expect("write hidden nested file");
        std::fs::write(
            backend.root().join("services/.private/ignored.md"),
            "hidden",
        )
        .expect("write hidden-dir file");

        let walked = backend.walk("").expect("walk store root");

        assert_eq!(
            walked,
            vec![
                "notes/todo.md".to_string(),
                "services/auth/flows.md".to_string(),
            ]
        );
    }

    #[test]
    fn walk_scopes_to_prefix() {
        let (_temp_dir, backend) = backend();
        backend
            .create("services/auth/flows.md", b"auth")
            .expect("create auth file");
        backend
            .create("services/billing/limits.md", b"limits")
            .expect("create billing file");
        backend
            .create("notes/todo.md", b"todo")
            .expect("create notes file");

        let walked = backend.walk("services/auth").expect("walk subtree");

        assert_eq!(walked, vec!["services/auth/flows.md".to_string()]);
    }

    #[test]
    fn walk_returns_empty_for_missing_prefix() {
        let (_temp_dir, backend) = backend();
        backend
            .create("notes/todo.md", b"todo")
            .expect("create notes file");

        let walked = backend.walk("missing").expect("walk missing prefix");

        assert!(walked.is_empty());
    }
}
