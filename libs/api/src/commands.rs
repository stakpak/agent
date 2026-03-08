//! Predefined Slash Commands
//!
//! Loads Stakpak-shipped slash commands from `.md` files embedded at compile time.
//! Any `.md` file placed in `libs/api/src/commands/` automatically becomes a
//! predefined slash command that appears in the TUI dropdown.
//!
//! Files are named `<command-name>.v<version>.md`. The command name is derived
//! by stripping the `.v<N>` version suffix (e.g., `review.v1.md` → `review`).
//! If multiple versions of the same command exist, the highest version wins.
//!
//! Optional YAML front matter provides a description:
//! ```markdown
//! ---
//! description: Review code changes
//! ---
//!
//! You are a code reviewer...
//! ```
//!
//! If no front matter is present, the first non-empty line (truncated to 60 chars)
//! is used as the description.

use include_dir::{Dir, include_dir};
use std::collections::HashMap;

/// The embedded commands directory, baked into the binary at compile time.
static COMMANDS_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/src/commands");

/// Maximum description length when derived from the first line of content.
const MAX_DESCRIPTION_LEN: usize = 60;

/// Load all predefined commands from the embedded `commands/` directory.
///
/// Returns a vec of `(name, description, prompt_content)` tuples.
/// - `name`: the slash command name without `/` prefix (e.g., `"review"`)
/// - `description`: human-readable description for the dropdown
/// - `prompt_content`: the full prompt body sent as a user message
///
/// When multiple versions of a command exist (e.g., `review.v1.md` and
/// `review.v2.md`), only the highest version is returned.
pub fn load_predefined_commands() -> Vec<(String, String, String)> {
    // Collect all versioned entries, keeping only the highest version per name
    let mut best: HashMap<String, (u32, String, String)> = HashMap::new();

    for file in COMMANDS_DIR.files() {
        let path = file.path();

        // Only process .md files
        let ext = path.extension().and_then(|e| e.to_str());
        if ext != Some("md") {
            continue;
        }

        let stem = match path.file_stem().and_then(|s| s.to_str()) {
            Some(s) => s,
            None => continue,
        };

        // Parse "name.vN" → (name, version)
        let (name, version) = parse_versioned_name(stem);

        // Validate command name
        if !is_valid_command_name(&name) {
            continue;
        }

        let content = match file.contents_utf8() {
            Some(c) => c.trim(),
            None => continue,
        };

        if content.is_empty() {
            continue;
        }

        // Only keep the highest version of each command
        if let Some(existing) = best.get(&name)
            && existing.0 >= version
        {
            continue;
        }

        let (description, prompt_body) = extract_front_matter(content);

        let description = description.unwrap_or_else(|| {
            let first_line = prompt_body
                .lines()
                .find(|l| !l.trim().is_empty())
                .unwrap_or("Predefined command");
            let first_line = first_line.trim();
            if first_line.len() > MAX_DESCRIPTION_LEN {
                let truncated: String = first_line.chars().take(MAX_DESCRIPTION_LEN).collect();
                format!("{truncated}...")
            } else {
                first_line.to_string()
            }
        });

        best.insert(name, (version, description, prompt_body.to_string()));
    }

    let mut commands: Vec<(String, String, String)> = best
        .into_iter()
        .map(|(name, (_, desc, content))| (name, desc, content))
        .collect();

    // Sort by name for stable ordering in the dropdown
    commands.sort_by(|a, b| a.0.cmp(&b.0));

    commands
}

/// Parse a versioned filename stem like `"review.v1"` into `("review", 1)`.
///
/// If no version suffix is found, returns version `0`.
///
/// Filenames are ASCII-only (validated by `is_valid_command_name` + `.vN`),
/// so `rfind('.')` returns a safe char boundary.
#[allow(clippy::string_slice)] // dot_pos from rfind('.') on same ASCII string
fn parse_versioned_name(stem: &str) -> (String, u32) {
    if let Some(dot_pos) = stem.rfind('.')
        && let Some(suffix) = stem.get(dot_pos + 1..)
        && let Some(num_str) = suffix.strip_prefix('v')
        && let Ok(version) = num_str.parse::<u32>()
    {
        // dot_pos is from rfind on the same string, always a valid boundary
        return (stem[..dot_pos].to_string(), version);
    }
    (stem.to_string(), 0)
}

/// Validate that a command name contains only allowed characters.
///
/// Allowed: `a-z`, `A-Z`, `0-9`, `-`, `_`
fn is_valid_command_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Extract YAML front matter from markdown content.
///
/// Front matter is delimited by `---` at the start:
/// ```text
/// ---
/// description: Some description
/// ---
/// Body content here
/// ```
///
/// Returns `(Option<description>, body_content)`.
#[allow(clippy::string_slice)] // indices from starts_with/match_indices on "---" (ASCII) are safe
fn extract_front_matter(content: &str) -> (Option<String>, &str) {
    // Must start with "---"
    if !content.starts_with("---") {
        return (None, content);
    }

    // Find the closing "---" that appears at the start of a line.
    let after_first = &content[3..];
    let closing = after_first
        .match_indices("---")
        .find(|(pos, _)| {
            *pos == 0 || after_first.as_bytes().get(pos.wrapping_sub(1)) == Some(&b'\n')
        })
        .map(|(pos, _)| pos);

    match closing {
        Some(pos) => {
            let front_matter = after_first[..pos].trim();
            let body = after_first[pos + 3..].trim();

            // Parse description from front matter
            let description = front_matter.lines().find_map(|line| {
                let line = line.trim();
                if let Some(value) = line.strip_prefix("description:") {
                    let value = value.trim();
                    // Remove surrounding quotes if present
                    let value = value
                        .strip_prefix('"')
                        .and_then(|v| v.strip_suffix('"'))
                        .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
                        .unwrap_or(value);
                    if !value.is_empty() {
                        Some(value.to_string())
                    } else {
                        None
                    }
                } else {
                    None
                }
            });

            if body.is_empty() {
                (description, content)
            } else {
                (description, body)
            }
        }
        None => (None, content),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_versioned_name() {
        assert_eq!(parse_versioned_name("review.v1"), ("review".into(), 1));
        assert_eq!(parse_versioned_name("claw.v2"), ("claw".into(), 2));
        assert_eq!(
            parse_versioned_name("my-command.v10"),
            ("my-command".into(), 10)
        );
        assert_eq!(parse_versioned_name("plain"), ("plain".into(), 0));
        assert_eq!(
            parse_versioned_name("dotted.name.v3"),
            ("dotted.name".into(), 3)
        );
    }

    #[test]
    fn test_is_valid_command_name() {
        assert!(is_valid_command_name("hello"));
        assert!(is_valid_command_name("hello-world"));
        assert!(is_valid_command_name("hello_world"));
        assert!(is_valid_command_name("Hello123"));
        assert!(!is_valid_command_name(""));
        assert!(!is_valid_command_name("hello world"));
        assert!(!is_valid_command_name("hello.world"));
        assert!(!is_valid_command_name("hello/world"));
    }

    #[test]
    fn test_extract_front_matter_with_description() {
        let content = "---\ndescription: Run a security audit\n---\n\nPerform a comprehensive security audit.";
        let (desc, body) = extract_front_matter(content);
        assert_eq!(desc, Some("Run a security audit".into()));
        assert_eq!(body, "Perform a comprehensive security audit.");
    }

    #[test]
    fn test_extract_front_matter_with_quoted_description() {
        let content = "---\ndescription: \"Check health status\"\n---\n\nCheck the health.";
        let (desc, body) = extract_front_matter(content);
        assert_eq!(desc, Some("Check health status".into()));
        assert_eq!(body, "Check the health.");
    }

    #[test]
    fn test_extract_front_matter_no_front_matter() {
        let content = "Just a plain prompt.";
        let (desc, body) = extract_front_matter(content);
        assert_eq!(desc, None);
        assert_eq!(body, content);
    }

    #[test]
    fn test_load_predefined_commands_returns_known_commands() {
        let commands = load_predefined_commands();
        let names: Vec<&str> = commands.iter().map(|(n, _, _)| n.as_str()).collect();
        assert!(names.contains(&"claw"), "Expected 'claw' in {names:?}");
        assert!(names.contains(&"review"), "Expected 'review' in {names:?}");
    }

    #[test]
    fn test_predefined_commands_have_descriptions() {
        let commands = load_predefined_commands();
        for (name, desc, _) in &commands {
            assert!(
                !desc.is_empty(),
                "Command '{name}' has an empty description"
            );
        }
    }

    #[test]
    fn test_predefined_commands_have_content() {
        let commands = load_predefined_commands();
        for (name, _, content) in &commands {
            assert!(
                !content.is_empty(),
                "Command '{name}' has empty prompt content"
            );
        }
    }
}
