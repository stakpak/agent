use clap::Subcommand;
use stakpak_ak::search::SearchEngine;
use stakpak_ak::skills::{SKILL_MAINTAIN, SKILL_USAGE};
use stakpak_ak::{LocalFsBackend, StorageBackend, TreeNavEngine};
use std::io::Read;
use std::path::PathBuf;

pub const AK_LONG_ABOUT: &str =
    "LLM-oriented commands for reading and writing persistent knowledge.

default store root: ~/.stakpak/knowledge
override: AK_STORE
paths are relative to the store root

Recommended read flow:
- tree: discover structure
- ls [path]: inspect one directory
- peek <path>: preview frontmatter + first paragraph
- cat <path>...: read full content
- use `peek` before `cat` to save tokens
- prefer `ls --json` for programmatic use

Write behavior:
- `write` creates a new file
- `write` fails if the path already exists
- use `--force` only when you want to overwrite intentionally";

pub const AK_AFTER_HELP: &str = "Examples:
  stakpak ak tree
  stakpak ak ls services/
  stakpak ak ls --json services/
  stakpak ak peek services/rate-limits.md
  stakpak ak cat services/rate-limits.md services/auth-flow.md
  echo 'Rate limit is 1000/min' | stakpak ak write services/rate-limits.md
  stakpak ak write notes.md --file /tmp/notes.md
  stakpak ak write --force summaries/auth-overview.md";

#[derive(Subcommand, PartialEq, Debug)]
#[command(
    about = "Persistent knowledge store operations",
    long_about = AK_LONG_ABOUT,
    after_help = AK_AFTER_HELP
)]
pub enum AkCommands {
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

If you remove the last file in a directory, empty parent directories are cleaned up automatically until the store root."
    )]
    Rm {
        /// Relative path inside the knowledge store to remove
        path: String,
    },

    #[command(
        about = "Print the full directory tree of the knowledge store",
        long_about = "Print the full directory tree of the ak store.

This is the best starting point when you want to understand the overall structure before drilling into a specific directory or file."
    )]
    Tree,

    #[command(
        about = "List one directory with one-line file descriptions",
        long_about = "List one directory at a time.

Directories are shown first. File descriptions are extracted from frontmatter or the first non-empty body line.

prefer `--json` when another tool or agent will parse the output"
    )]
    Ls {
        /// Relative directory path inside the knowledge store. Omit to list the store root
        path: Option<String>,

        /// Output the listing as structured JSON
        #[arg(
            long,
            default_value_t = false,
            help = "Print a JSON array with name, is_dir, and description fields. Prefer this when another tool or agent will parse the output"
        )]
        json: bool,
    },

    #[command(
        about = "Show frontmatter and the first paragraph of a file",
        long_about = "Show a lightweight preview of a file.

`peek` is useful when you want enough context to decide whether you should read the whole file with `cat`."
    )]
    Peek {
        /// Relative path of the file to summarize
        path: String,
    },

    #[command(
        about = "Print the full contents of one or more files",
        long_about = "Print the full contents of one or more files from the ak store.

When multiple paths are provided, each file is separated with a `---` delimiter so the output is still easy to parse or read."
    )]
    Cat {
        /// One or more relative file paths to print in full
        #[arg(required = true, num_args = 1..)]
        paths: Vec<String>,
    },

    #[command(
        about = "Show the store location and total file count",
        long_about = "Show where the ak store currently lives and how many non-dotfiles it contains.

This reflects the default root (~/.stakpak/knowledge) unless AK_STORE is set."
    )]
    Status,

    #[command(
        about = "Print one of the built-in ak skill prompts",
        long_about = "Print one of the built-in behavior prompts for `ak`.

Use `usage` to teach an agent how to navigate and write to the store. Use `maintain` to teach an agent how to audit, deduplicate, and clean up stored knowledge."
    )]
    Skill {
        /// Built-in skill name: usage or maintain
        name: String,
    },
}

impl AkCommands {
    pub fn run(self) -> Result<(), String> {
        let backend = LocalFsBackend::new().map_err(|error| error.to_string())?;
        let search = TreeNavEngine::new(backend.clone());

        match self {
            Self::Write { path, file, force } => {
                let content = read_input(file)?;
                if force {
                    backend
                        .overwrite(&path, &content)
                        .map_err(|error| error.to_string())?;
                } else {
                    backend.create(&path, &content).map_err(|error| match error {
                        stakpak_ak::Error::AlreadyExists(existing) => {
                            format!(
                                "destination already exists: {}. next action: choose a new path or rerun with `stakpak ak write --force {path}` if overwrite is intentional",
                                existing.display()
                            )
                        }
                        other => other.to_string(),
                    })?;
                }
            }
            Self::Rm { path } => {
                backend.remove(&path).map_err(|error| error.to_string())?;
            }
            Self::Tree => {
                println!(
                    "{}",
                    backend.tree().map_err(|error| error.to_string())?.print()
                );
            }
            Self::Ls { path, json } => {
                let path = path.unwrap_or_default();
                let entries = search
                    .list_with_descriptions(&path)
                    .map_err(|error| error.to_string())?;
                if json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&entries).map_err(|error| error.to_string())?
                    );
                } else {
                    print_entries(&entries);
                }
            }
            Self::Peek { path } => {
                println!("{}", search.peek(&path).map_err(|error| error.to_string())?);
            }
            Self::Cat { paths } => {
                for (index, path) in paths.iter().enumerate() {
                    if index > 0 {
                        println!("---");
                    }

                    let content = backend.read(path).map_err(|error| error.to_string())?;
                    let text = String::from_utf8_lossy(&content);
                    print!("{text}");
                    if index + 1 < paths.len() && !text.ends_with('\n') {
                        println!();
                    }
                }
            }
            Self::Status => {
                println!("Store: {}", backend.root().display());
                println!(
                    "Files: {}",
                    backend.file_count().map_err(|error| error.to_string())?
                );
            }
            Self::Skill { name } => match name.as_str() {
                "usage" => println!("{SKILL_USAGE}"),
                "maintain" => println!("{SKILL_MAINTAIN}"),
                other => {
                    return Err(format!(
                        "invalid skill: {other}. valid values: usage, maintain"
                    ));
                }
            },
        }

        Ok(())
    }
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

fn print_entries(entries: &[stakpak_ak::ListEntry]) {
    let width = entries
        .iter()
        .map(display_name)
        .map(|name| name.chars().count())
        .max()
        .unwrap_or(0);

    for entry in entries {
        let name = display_name(entry);
        match &entry.description {
            Some(description) => println!("{name:<width$}  — {description}"),
            None => println!("{name}"),
        }
    }
}

fn display_name(entry: &stakpak_ak::ListEntry) -> String {
    if entry.is_dir {
        format!("{}/", entry.name)
    } else {
        entry.name.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::AkCommands;

    #[test]
    fn unknown_skill_error_lists_valid_values() {
        let error = AkCommands::Skill {
            name: "unknown".to_string(),
        }
        .run()
        .expect_err("unknown skill should fail");

        assert!(error.contains("invalid skill"));
        assert!(error.contains("valid values: usage, maintain"));
    }
}
