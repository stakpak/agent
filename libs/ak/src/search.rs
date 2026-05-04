use crate::Error;
use crate::format::extract_peek;
use crate::store::StorageBackend;
use globset::{GlobBuilder, GlobMatcher};
use grep_matcher::Matcher;
use grep_regex::{RegexMatcher, RegexMatcherBuilder};
use serde::Serialize;
use std::path::Path;

const BINARY_DETECTION_BYTES: usize = 8 * 1024;

pub trait SearchEngine {
    fn search_default(&self, path: &str) -> Result<Vec<PeekResult>, Error>;
    fn search_glob(&self, path: &str, glob: &str) -> Result<Vec<PeekResult>, Error>;
    fn search_grep(
        &self,
        path: &str,
        regex: &str,
        case_insensitive: bool,
    ) -> Result<Vec<GrepResult>, Error>;
    fn search_grep_glob(
        &self,
        path: &str,
        regex: &str,
        glob: &str,
        case_insensitive: bool,
    ) -> Result<Vec<GrepResult>, Error>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PeekResult {
    pub path: String,
    pub peek: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GrepResult {
    pub path: String,
    pub matches: Vec<(usize, String)>,
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

    fn search_peeks(
        &self,
        path: &str,
        glob_matcher: Option<&GlobMatcher>,
    ) -> Result<Vec<PeekResult>, Error> {
        let mut results = Vec::new();

        for relative_path in self.store.walk(path)? {
            if !matches_glob(glob_matcher, &relative_path) {
                continue;
            }

            let content = self.store.read(&relative_path)?;
            results.push(PeekResult {
                path: relative_path,
                peek: extract_peek(&String::from_utf8_lossy(&content)),
            });
        }

        Ok(results)
    }

    fn search_matches(
        &self,
        path: &str,
        matcher: &RegexMatcher,
        glob_matcher: Option<&GlobMatcher>,
    ) -> Result<Vec<GrepResult>, Error> {
        let mut results = Vec::new();

        for relative_path in self.store.walk(path)? {
            if !matches_glob(glob_matcher, &relative_path) {
                continue;
            }

            let content = self.store.read(&relative_path)?;
            if contains_nul_byte(&content[..content.len().min(BINARY_DETECTION_BYTES)]) {
                continue;
            }

            let matches = grep_lines(matcher, &content)?;
            if matches.is_empty() {
                continue;
            }

            results.push(GrepResult {
                path: relative_path,
                matches,
            });
        }

        Ok(results)
    }
}

impl<T> SearchEngine for TreeNavEngine<T>
where
    T: StorageBackend,
{
    fn search_default(&self, path: &str) -> Result<Vec<PeekResult>, Error> {
        self.search_peeks(path, None)
    }

    fn search_glob(&self, path: &str, glob: &str) -> Result<Vec<PeekResult>, Error> {
        let matcher = compile_glob(glob)?;
        self.search_peeks(path, Some(&matcher))
    }

    fn search_grep(
        &self,
        path: &str,
        regex: &str,
        case_insensitive: bool,
    ) -> Result<Vec<GrepResult>, Error> {
        let matcher = compile_regex(regex, case_insensitive)?;
        self.search_matches(path, &matcher, None)
    }

    fn search_grep_glob(
        &self,
        path: &str,
        regex: &str,
        glob: &str,
        case_insensitive: bool,
    ) -> Result<Vec<GrepResult>, Error> {
        let regex_matcher = compile_regex(regex, case_insensitive)?;
        let glob_matcher = compile_glob(glob)?;
        self.search_matches(path, &regex_matcher, Some(&glob_matcher))
    }
}

fn compile_glob(glob: &str) -> Result<GlobMatcher, Error> {
    GlobBuilder::new(glob)
        .literal_separator(true)
        .build()
        .map(|compiled| compiled.compile_matcher())
        .map_err(|error| Error::Parse(format!("invalid glob pattern: {error}")))
}

fn compile_regex(pattern: &str, case_insensitive: bool) -> Result<RegexMatcher, Error> {
    let mut builder = RegexMatcherBuilder::new();
    builder.case_insensitive(case_insensitive);
    builder.line_terminator(Some(b'\n'));
    builder
        .build(pattern)
        .map_err(|error| Error::Parse(format!("invalid regex pattern: {error}")))
}

fn matches_glob(glob_matcher: Option<&GlobMatcher>, path: &str) -> bool {
    glob_matcher.is_none_or(|matcher| matcher.is_match(Path::new(path)))
}

fn grep_lines(matcher: &RegexMatcher, content: &[u8]) -> Result<Vec<(usize, String)>, Error> {
    let text = String::from_utf8_lossy(content);
    let mut matches = Vec::new();

    for (index, line) in text.lines().enumerate() {
        if matcher
            .find(line.as_bytes())
            .map_err(|error| Error::Parse(format!("failed to run regex search: {error}")))?
            .is_some()
        {
            matches.push((index + 1, line.to_string()));
        }
    }

    Ok(matches)
}

fn contains_nul_byte(content: &[u8]) -> bool {
    content.contains(&0)
}

#[cfg(test)]
mod tests {
    use super::{GrepResult, PeekResult, SearchEngine, TreeNavEngine};
    use crate::store::{LocalFsBackend, StorageBackend};

    fn engine() -> (
        tempfile::TempDir,
        LocalFsBackend,
        TreeNavEngine<LocalFsBackend>,
    ) {
        let root = tempfile::TempDir::new().expect("temp dir");
        let backend = LocalFsBackend::with_root(root.path().join("store"));
        let engine = TreeNavEngine::new(backend.clone());
        (root, backend, engine)
    }

    #[test]
    fn search_default_returns_peeks_sorted_by_full_path() {
        let (_root, backend, engine) = engine();
        backend
            .create(
                "services/rate-limits.md",
                b"---\ndescription: API rate limits\n---\nBody\n",
            )
            .expect("create rate limits file");
        backend
            .create("notes/todo.md", b"First paragraph\n\nSecond paragraph\n")
            .expect("create todo file");

        assert_eq!(
            engine.search_default("").expect("default search"),
            vec![
                PeekResult {
                    path: "notes/todo.md".to_string(),
                    peek: "First paragraph".to_string(),
                },
                PeekResult {
                    path: "services/rate-limits.md".to_string(),
                    peek: "---\ndescription: API rate limits\n---\nBody".to_string(),
                },
            ]
        );
    }

    #[test]
    fn search_glob_filters_by_pattern() {
        let (_root, backend, engine) = engine();
        backend
            .create("services/rate-limits.md", b"Body\n")
            .expect("create service file");
        backend
            .create("notes/todo.md", b"Body\n")
            .expect("create notes file");

        assert_eq!(
            engine
                .search_glob("", "services/**/*.md")
                .expect("glob search"),
            vec![PeekResult {
                path: "services/rate-limits.md".to_string(),
                peek: "Body".to_string(),
            }]
        );
    }

    #[test]
    fn search_grep_returns_matching_lines_with_line_numbers() {
        let (_root, backend, engine) = engine();
        backend
            .create(
                "services/rate-limits.md",
                b"first\nRate limit is 1000/min\nthird\n",
            )
            .expect("create service file");

        assert_eq!(
            engine
                .search_grep("", "Rate limit", false)
                .expect("grep search"),
            vec![GrepResult {
                path: "services/rate-limits.md".to_string(),
                matches: vec![(2, "Rate limit is 1000/min".to_string())],
            }]
        );
    }

    #[test]
    fn search_grep_honors_case_insensitive_flag() {
        let (_root, backend, engine) = engine();
        backend
            .create("services/rate-limits.md", b"rate limit is 1000/min\n")
            .expect("create service file");

        assert_eq!(
            engine
                .search_grep("", "RATE LIMIT", true)
                .expect("case insensitive grep"),
            vec![GrepResult {
                path: "services/rate-limits.md".to_string(),
                matches: vec![(1, "rate limit is 1000/min".to_string())],
            }]
        );
    }

    #[test]
    fn search_grep_matches_frontmatter_lines() {
        let (_root, backend, engine) = engine();
        backend
            .create(
                "services/rate-limits.md",
                b"---\ndescription: API rate limits\n---\nBody\n",
            )
            .expect("create file");

        assert_eq!(
            engine
                .search_grep("", "API rate", false)
                .expect("frontmatter grep"),
            vec![GrepResult {
                path: "services/rate-limits.md".to_string(),
                matches: vec![(2, "description: API rate limits".to_string())],
            }]
        );
    }

    #[test]
    fn search_grep_skips_binary_files() {
        let (_root, backend, engine) = engine();
        backend
            .create("services/binary.bin", b"text\0hidden\nRate limit\n")
            .expect("create binary file");
        backend
            .create("services/rate-limits.md", b"Rate limit is 1000/min\n")
            .expect("create text file");

        assert_eq!(
            engine
                .search_grep("", "Rate limit", false)
                .expect("grep search"),
            vec![GrepResult {
                path: "services/rate-limits.md".to_string(),
                matches: vec![(1, "Rate limit is 1000/min".to_string())],
            }]
        );
    }

    #[test]
    fn search_grep_glob_composes_filters() {
        let (_root, backend, engine) = engine();
        backend
            .create("services/rate-limits.md", b"Rate limit\n")
            .expect("create markdown file");
        backend
            .create("services/rate-limits.txt", b"Rate limit\n")
            .expect("create text file");

        assert_eq!(
            engine
                .search_grep_glob("", "Rate limit", "**/*.md", false)
                .expect("grep glob search"),
            vec![GrepResult {
                path: "services/rate-limits.md".to_string(),
                matches: vec![(1, "Rate limit".to_string())],
            }]
        );
    }
}
