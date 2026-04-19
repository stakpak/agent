use crate::Error;
use crate::format::{extract_description, extract_peek};
use crate::store::StorageBackend;
use serde::Serialize;

const DESCRIPTION_READ_LIMIT_BYTES: usize = 16 * 1024;

pub trait SearchEngine {
    fn list_with_descriptions(&self, path: &str) -> Result<Vec<ListEntry>, Error>;
    fn peek(&self, path: &str) -> Result<String, Error>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ListEntry {
    pub name: String,
    pub is_dir: bool,
    pub description: Option<String>,
}

pub struct TreeNavEngine<T> {
    store: T,
}

impl<T> TreeNavEngine<T>
where
    T: StorageBackend,
{
    pub fn new(store: T) -> Self {
        Self { store }
    }
}

impl<T> SearchEngine for TreeNavEngine<T>
where
    T: StorageBackend,
{
    fn list_with_descriptions(&self, path: &str) -> Result<Vec<ListEntry>, Error> {
        self.store
            .list(path)?
            .into_iter()
            .map(|entry| {
                let description = if entry.is_dir {
                    None
                } else {
                    let content = self
                        .store
                        .read_prefix(&join_path(path, &entry.name), DESCRIPTION_READ_LIMIT_BYTES)?;
                    extract_description(&String::from_utf8_lossy(&content))
                };

                Ok(ListEntry {
                    name: entry.name,
                    is_dir: entry.is_dir,
                    description,
                })
            })
            .collect()
    }

    fn peek(&self, path: &str) -> Result<String, Error> {
        let content = self.store.read(path)?;
        Ok(extract_peek(&String::from_utf8_lossy(&content)))
    }
}

fn join_path(parent: &str, child: &str) -> String {
    if parent.is_empty() {
        child.to_string()
    } else {
        format!("{}/{}", parent.trim_end_matches('/'), child)
    }
}

#[cfg(test)]
mod tests {
    use super::{ListEntry, SearchEngine, TreeNavEngine};
    use crate::Error;
    use crate::store::{Entry, LocalFsBackend, StorageBackend, TreeNode};
    use std::cell::Cell;

    #[test]
    fn list_with_descriptions_reads_file_descriptions() {
        let root = tempfile::TempDir::new().expect("temp dir");
        let backend = LocalFsBackend::with_root(root.path().join("store"));
        backend
            .create(
                "knowledge/rate-limits.md",
                b"---\ndescription: API rate limits\n---\nBody\n",
            )
            .expect("create described file");
        backend
            .create("knowledge/auth-flow.md", b"OAuth2 PKCE flow\n\nMore\n")
            .expect("create plain file");
        std::fs::create_dir_all(backend.root().join("knowledge/subdir")).expect("create subdir");
        let engine = TreeNavEngine::new(backend);

        assert_eq!(
            engine
                .list_with_descriptions("knowledge")
                .expect("list with descriptions"),
            vec![
                ListEntry {
                    name: "subdir".to_string(),
                    is_dir: true,
                    description: None,
                },
                ListEntry {
                    name: "auth-flow.md".to_string(),
                    is_dir: false,
                    description: Some("OAuth2 PKCE flow".to_string()),
                },
                ListEntry {
                    name: "rate-limits.md".to_string(),
                    is_dir: false,
                    description: Some("API rate limits".to_string()),
                },
            ]
        );
    }

    #[test]
    fn peek_returns_frontmatter_and_first_paragraph() {
        let root = tempfile::TempDir::new().expect("temp dir");
        let backend = LocalFsBackend::with_root(root.path().join("store"));
        backend
            .create(
                "knowledge/rate-limits.md",
                b"---\ndescription: API rate limits\n---\nThe auth service rate limits at 1000 req/min.\nAfter hitting the limit, responses return 429.\n\nSecond paragraph.\n",
            )
            .expect("create file");
        let engine = TreeNavEngine::new(backend);

        assert_eq!(
            engine.peek("knowledge/rate-limits.md").expect("peek file"),
            "---\ndescription: API rate limits\n---\nThe auth service rate limits at 1000 req/min.\nAfter hitting the limit, responses return 429."
        );
    }

    #[derive(Default)]
    struct PrefixOnlyBackend {
        read_called: Cell<bool>,
        read_prefix_called: Cell<bool>,
    }

    impl StorageBackend for PrefixOnlyBackend {
        fn create(&self, _path: &str, _content: &[u8]) -> Result<(), Error> {
            unimplemented!("not needed for this test")
        }

        fn overwrite(&self, _path: &str, _content: &[u8]) -> Result<(), Error> {
            unimplemented!("not needed for this test")
        }

        fn read(&self, _path: &str) -> Result<Vec<u8>, Error> {
            self.read_called.set(true);
            Err(Error::Parse(
                "full read should not be used for ls descriptions".to_string(),
            ))
        }

        fn read_prefix(&self, _path: &str, _max_bytes: usize) -> Result<Vec<u8>, Error> {
            self.read_prefix_called.set(true);
            Ok(b"---\ndescription: Prefix description\n---\nBody\n".to_vec())
        }

        fn remove(&self, _path: &str) -> Result<(), Error> {
            unimplemented!("not needed for this test")
        }

        fn list(&self, _path: &str) -> Result<Vec<Entry>, Error> {
            Ok(vec![Entry {
                name: "note.md".to_string(),
                is_dir: false,
            }])
        }

        fn tree(&self) -> Result<TreeNode, Error> {
            unimplemented!("not needed for this test")
        }

        fn exists(&self, _path: &str) -> Result<bool, Error> {
            unimplemented!("not needed for this test")
        }
    }

    #[test]
    fn list_with_descriptions_reads_only_prefixes() {
        let backend = PrefixOnlyBackend::default();
        let engine = TreeNavEngine::new(backend);

        let entries = engine
            .list_with_descriptions("")
            .expect("list with prefix reads");

        assert_eq!(
            entries,
            vec![ListEntry {
                name: "note.md".to_string(),
                is_dir: false,
                description: Some("Prefix description".to_string()),
            }]
        );
        assert!(!engine.store.read_called.get());
        assert!(engine.store.read_prefix_called.get());
    }
}
