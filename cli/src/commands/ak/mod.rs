use clap::Subcommand;
use stakpak_ak::search::SearchEngine;
use stakpak_ak::skills::{SKILL_MAINTAIN, SKILL_RETROSPECT, SKILL_USAGE};
use stakpak_ak::{
    GrepResult, LocalFsBackend, PeekResult, RemoteBackend, StorageBackend, TreeNavEngine,
};
use stakpak_api::stakpak::StakpakApiConfig;
use std::io::Read;
use std::path::PathBuf;
use std::rc::Rc;

use crate::config::AppConfig;

pub const AK_LONG_ABOUT: &str =
    "LLM-oriented commands for reading and writing persistent knowledge.

default store root: ~/.stakpak/knowledge
override: AK_STORE
paths are relative to the store root

Key commands:
- ak search [path]: recursive preview by default; add --tree, --glob, --grep, or -i
- ak read <path>...: read one or more files in full
- ak write <path>: create a new file; use --force to overwrite intentionally
- ak remove <path>: remove a file or directory recursively
- ak skill <name>: print a built-in ak skill prompt

Recommended discovery flow:
- start with `ak search [path]`
- use `ak search --tree` for structure-only discovery
- use `ak search --glob` or `ak search --grep` to narrow results
- use `ak read` only after search tells you which files matter";

pub const AK_AFTER_HELP: &str = "Examples:
  stakpak ak search
  stakpak ak search services --tree
  stakpak ak search --glob 'services/**/*.md'
  stakpak ak search --grep 'rate.limit'
  stakpak ak search --grep 'rate.limit' --glob '**/*.md'
  stakpak ak read services/rate-limits.md
  stakpak ak read services/rate-limits.md services/auth-flow.md
  echo 'Rate limit is 1000/min' | stakpak ak write services/rate-limits.md
  stakpak ak write notes.md --file /tmp/notes.md
  stakpak ak remove services/rate-limits.md";

#[derive(Subcommand, PartialEq, Debug)]
#[command(
    about = "Persistent knowledge store operations",
    long_about = AK_LONG_ABOUT,
    after_help = AK_AFTER_HELP
)]
pub enum AkCommands {
    #[command(
        about = "Search the knowledge store",
        long_about = "Search the ak store recursively.

Default output is peek body per file (frontmatter, if present, plus the first paragraph). Use `--tree` for structure-only output, `--glob` to filter by path pattern, and `--grep` to filter by content regex. `--grep` matches frontmatter too. `-i` makes `--grep` case-insensitive.",
        after_help = "Examples:
  stakpak ak search
  stakpak ak search services
  stakpak ak search services --tree
  stakpak ak search --glob 'services/**/*.md'
  stakpak ak search --grep 'rate.limit'
  stakpak ak search --grep 'rate.limit' -i
  stakpak ak search --grep 'rate.limit' --glob '**/*.md'"
    )]
    Search {
        /// Optional relative path to scope the search to a subtree or single file
        path: Option<String>,

        /// Regex to match against file content (including frontmatter)
        #[arg(long)]
        grep: Option<String>,

        /// Glob to match against relative file paths
        #[arg(long)]
        glob: Option<String>,

        /// Render a directory tree instead of file previews
        #[arg(long, conflicts_with = "grep", conflicts_with = "glob")]
        tree: bool,

        /// Make `--grep` matching case-insensitive
        #[arg(short = 'i')]
        case_insensitive: bool,
    },

    #[command(
        about = "Read one or more files in full",
        long_about = "Print the full contents of one or more files from the ak store.

When multiple paths are provided, each file is separated with a `---` delimiter. Reading a directory is not supported; use `ak search <path>` to preview content first.",
        after_help = "Examples:
  stakpak ak read services/rate-limits.md
  stakpak ak read services/rate-limits.md services/auth-flow.md"
    )]
    Read {
        /// One or more relative file paths to print in full
        #[arg(required = true, num_args = 1..)]
        paths: Vec<String>,
    },

    #[command(
        about = "Create a new knowledge file",
        long_about = "Create only.

This command reads content from stdin by default. Use `--file` to read from a local file instead.

Behavior:
- fails if the destination already exists
- use `--force` to overwrite intentionally
- paths are relative to the store root",
        after_help = "Examples:
  echo 'Rate limit is 1000/min' | stakpak ak write services/rate-limits.md
  stakpak ak write notes.md --file /tmp/notes.md
  stakpak ak write --force summaries/auth-overview.md"
    )]
    Write {
        /// Relative path inside the knowledge store where the new file should be created
        path: String,

        /// Read content from a local file instead of stdin
        #[arg(
            short = 'f',
            long = "file",
            help = "Path to a local file to read and store instead of reading from stdin"
        )]
        file: Option<PathBuf>,

        /// Overwrite the destination if it already exists
        #[arg(
            long,
            default_value_t = false,
            help = "Replace an existing file at the destination path. Without this flag, write fails if the path already exists"
        )]
        force: bool,
    },

    #[command(
        about = "Remove a file or directory from the knowledge store",
        long_about = "Remove a file or an entire directory tree from the ak store.

Removal is recursive for directories. Missing paths fail fast. If you remove the last file in a directory, empty parent directories are cleaned up automatically until the store root.",
        after_help = "Examples:
  stakpak ak remove services/rate-limits.md
  stakpak ak remove services/old/"
    )]
    Remove {
        /// Relative path inside the knowledge store to remove
        path: String,
    },

    #[command(
        about = "Print one of the built-in ak skill prompts",
        long_about = "Print one of the built-in behavior prompts for `ak`.

Use `usage` to teach an agent how to navigate and write to the store. Use `maintain` to teach an agent how to audit, deduplicate, and clean up stored knowledge. Use `retrospect` to teach an agent how to turn past sessions into durable entries in the store (pipe its output into `stakpak autopilot schedule add --prompt ...` to run it on cron)."
    )]
    Skill {
        /// Built-in skill name: usage, maintain, or retrospect
        name: String,
    },
}

impl AkCommands {
    pub fn run(self, config: AppConfig) -> Result<(), String> {
        let backend = create_backend(&config)?;
        match self {
            Self::Search {
                path,
                grep,
                glob,
                tree,
                case_insensitive,
            } => run_search(backend.clone(), path, grep, glob, tree, case_insensitive)?,
            Self::Read { paths } => run_read(backend.clone(), &paths)?,
            Self::Write { path, file, force } => run_write(backend.clone(), path, file, force)?,
            Self::Remove { path } => run_remove(backend.clone(), &path)?,
            Self::Skill { name } => run_skill(&name)?,
        }

        Ok(())
    }
}

fn run_search(
    backend: Rc<dyn StorageBackend>,
    path: Option<String>,
    grep: Option<String>,
    glob: Option<String>,
    tree: bool,
    case_insensitive: bool,
) -> Result<(), String> {
    let path = path.unwrap_or_default();
    if tree {
        let rendered = backend
            .as_ref()
            .tree(&path)
            .map_err(|error| error.to_string())?;
        println!("{}", rendered.print());
        return Ok(());
    }

    let search = TreeNavEngine::new(backend.clone());

    if let (Some(regex), Some(glob)) = (grep.as_deref(), glob.as_deref()) {
        let results = search
            .search_grep_glob(&path, regex, glob, case_insensitive)
            .map_err(|error| error.to_string())?;
        print_rendered(&render_grep_results(&results));
    } else if let Some(regex) = grep.as_deref() {
        let results = search
            .search_grep(&path, regex, case_insensitive)
            .map_err(|error| error.to_string())?;
        print_rendered(&render_grep_results(&results));
    } else if let Some(glob) = glob.as_deref() {
        let results = search
            .search_glob(&path, glob)
            .map_err(|error| error.to_string())?;
        print_rendered(&render_peek_results(&results));
    } else {
        let results = search
            .search_default(&path)
            .map_err(|error| error.to_string())?;
        print_rendered(&render_peek_results(&results));
    }

    Ok(())
}

fn run_read(backend: Rc<dyn StorageBackend>, paths: &[String]) -> Result<(), String> {
    for path in paths.iter() {
        ensure_path_is_not_directory(&backend, path)?;
    }

    for (index, path) in paths.iter().enumerate() {
        if index > 0 {
            println!("---");
        }

        let content = backend
            .as_ref()
            .read(path)
            .map_err(|error| error.to_string())?;
        let text = String::from_utf8_lossy(&content);
        print!("{text}");
        if index + 1 < paths.len() && !text.ends_with('\n') {
            println!();
        }
    }

    Ok(())
}

fn run_write(
    backend: Rc<dyn StorageBackend>,
    path: String,
    file: Option<PathBuf>,
    force: bool,
) -> Result<(), String> {
    let content = read_input(file)?;
    if force {
        backend
            .as_ref()
            .overwrite(&path, &content)
            .map_err(|error| error.to_string())?;
    } else {
        backend.as_ref().create(&path, &content).map_err(|error| match error {
            stakpak_ak::Error::AlreadyExists(existing) => {
                format!(
                    "destination already exists: {}. next action: choose a new path or rerun with `stakpak ak write --force {path}` if overwrite is intentional",
                    existing.display()
                )
            }
            other => other.to_string(),
        })?;
    }

    Ok(())
}

fn run_remove(backend: Rc<dyn StorageBackend>, path: &str) -> Result<(), String> {
    backend.as_ref().remove(path).map_err(|error| match error {
        stakpak_ak::Error::NotFound(missing) => format!("path not found: {}", missing.display()),
        other => other.to_string(),
    })?;

    Ok(())
}

fn run_skill(name: &str) -> Result<(), String> {
    match name {
        "usage" => println!("{SKILL_USAGE}"),
        "maintain" => println!("{SKILL_MAINTAIN}"),
        "retrospect" => println!("{SKILL_RETROSPECT}"),
        other => {
            return Err(format!(
                "invalid skill: {other}. valid values: usage, maintain, retrospect"
            ));
        }
    }

    Ok(())
}

fn read_input(file: Option<PathBuf>) -> Result<Vec<u8>, String> {
    if let Some(path) = file {
        std::fs::read(&path).map_err(|error| {
            format!(
                "failed to read input file: {}. source error: {error}",
                path.display()
            )
        })
    } else {
        let mut buffer = Vec::new();
        std::io::stdin()
            .read_to_end(&mut buffer)
            .map_err(|error| error.to_string())?;
        Ok(buffer)
    }
}

fn ensure_path_is_not_directory(
    backend: &Rc<dyn StorageBackend>,
    path: &str,
) -> Result<(), String> {
    match backend.as_ref().list(path) {
        Ok(_) => Err(format!(
            "{path} is a directory; use 'ak search {path}' to preview content"
        )),
        Err(stakpak_ak::Error::NotADirectory(_) | stakpak_ak::Error::NotFound(_)) => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

fn render_peek_results(results: &[PeekResult]) -> String {
    results
        .iter()
        .map(|result| format!("# {}\n{}", result.path, result.peek))
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn render_grep_results(results: &[GrepResult]) -> String {
    results
        .iter()
        .map(render_grep_result)
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn render_grep_result(result: &GrepResult) -> String {
    let lines = result
        .matches
        .iter()
        .map(|(line_number, line)| format!("{line_number}: {line}"))
        .collect::<Vec<_>>()
        .join("\n");

    format!("# {}\n{lines}", result.path)
}

fn print_rendered(rendered: &str) {
    if !rendered.is_empty() {
        println!("{rendered}");
    }
}

/// Create the appropriate backend based on configuration
/// - If explicit AK_STORE, use LocalFsBackend
/// - If Stakpak API key is available, use RemoteBackend
/// - Otherwise, fall back to LocalFsBackend
fn create_backend(config: &AppConfig) -> Result<Rc<dyn StorageBackend>, String> {
    // Check for explicit AK_STORE override first - always use local if set
    if std::env::var("AK_STORE").is_ok() {
        let local =
            LocalFsBackend::new().map_err(|e| format!("Failed to create local backend: {e}"))?;
        let backend: Rc<dyn StorageBackend> = Rc::new(local);
        return Ok(backend);
    }

    // Otherwise, auto-detect based on API key presence
    if let Some(api_key) = config.get_stakpak_api_key()
        && !api_key.is_empty()
    {
        let api_config = StakpakApiConfig::new(api_key).with_endpoint(config.api_endpoint.clone());
        let remote = RemoteBackend::new(&api_config)
            .map_err(|e| format!("Failed to create remote backend: {e}"))?;
        let backend: Rc<dyn StorageBackend> = Rc::new(remote);
        return Ok(backend);
    }

    // Default to local backend
    let local =
        LocalFsBackend::new().map_err(|e| format!("Failed to create local backend: {e}"))?;
    let backend: Rc<dyn StorageBackend> = Rc::new(local);
    Ok(backend)
}

#[cfg(test)]
mod tests {
    use super::AkCommands;
    use crate::config::AppConfig;

    #[test]
    fn unknown_skill_error_lists_valid_values() {
        let config = AppConfig::load("default", None::<&str>).expect("load config");
        let error = AkCommands::Skill {
            name: "unknown".to_string(),
        }
        .run(config)
        .expect_err("unknown skill should fail");

        assert!(error.contains("invalid skill"));
        assert!(error.contains("usage"));
        assert!(error.contains("maintain"));
        assert!(error.contains("retrospect"));
    }
}
