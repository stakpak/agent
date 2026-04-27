pub const SKILL_USAGE: &str = include_str!("skills/usage.v1.md");

pub const SKILL_RETROSPECT: &str = include_str!("skills/retrospect.v1.md");

pub const SKILL_MAINTAIN: &str = include_str!("skills/maintain.v1.md");

#[cfg(test)]
mod tests {
    use super::{SKILL_RETROSPECT, SKILL_USAGE};

    const AUTOPILOT_ONE_LINER: &str = r#"stakpak autopilot schedule add --name retrospect --cron "0 3 * * *" --prompt "$(stakpak ak skill retrospect)""#;

    #[test]
    fn retrospect_skill_matches_bundled_markdown() {
        // Golden-file: the constant must be exactly the bundled file byte-for-byte.
        // If someone wraps `include_str!` with `.trim()` / `.replace(...)` / etc.,
        // this test will fail.
        let bundled = include_str!("skills/retrospect.v1.md");
        assert_eq!(
            SKILL_RETROSPECT, bundled,
            "SKILL_RETROSPECT must match bundled retrospect.v1.md byte-for-byte"
        );
        assert!(
            SKILL_RETROSPECT.starts_with("You are running retrospection:"),
            "SKILL_RETROSPECT appears to have been trimmed or mutated at its head"
        );
        assert!(
            SKILL_RETROSPECT.trim_end().ends_with(
                r#"stakpak autopilot schedule add --name retrospect --cron "0 3 * * *" --prompt "$(stakpak ak skill retrospect)""#
            ),
            "SKILL_RETROSPECT appears to have been trimmed or mutated at its tail"
        );
    }

    #[test]
    fn retrospect_skill_contains_required_substrings() {
        for needle in [
            "stakpak ak skill usage",
            "stakpak ak skill maintain",
            "stakpak sessions list --json",
            "stakpak sessions show",
            "sources:",
            "session:",
            "checkpoint:",
            "captured_at:",
            "write --force",
            "newest-first",
            AUTOPILOT_ONE_LINER,
        ] {
            assert!(
                SKILL_RETROSPECT.contains(needle),
                "SKILL_RETROSPECT is missing required substring: {needle:?}"
            );
        }
    }

    #[test]
    fn retrospect_skill_has_no_forbidden_substrings() {
        // Hard-forbidden markers of a pinned retrospect-owned layout.
        for needle in ["retrospect/<", "patterns/<", "_index.md"] {
            assert!(
                !SKILL_RETROSPECT.contains(needle),
                "SKILL_RETROSPECT contains forbidden substring: {needle:?}"
            );
        }
        // Antipattern check: an enumerated numbered list of "things worth
        // writing" (decisions / failure modes / verified facts / ...) would
        // pre-enumerate content categories instead of deferring to ak. The
        // prompt must explicitly tell the agent NOT to follow a fixed
        // taxonomy and must point the agent at existing ak entries as the
        // live reference for what qualifies.
        assert!(
            SKILL_RETROSPECT.contains("fixed taxonomy"),
            "SKILL_RETROSPECT must reject a fixed taxonomy for 'worth extracting'"
        );
        assert!(
            SKILL_RETROSPECT.contains("existing `ak` entries"),
            "SKILL_RETROSPECT must point the agent at existing ak entries as its reference"
        );
    }

    #[test]
    fn usage_skill_contains_new_command_surface() {
        for needle in [
            "ak search [path]",
            "ak search [path] --tree",
            "ak search [path] --grep",
            "ak search [path] --glob",
            "ak read <path>...",
            "ak write <path>",
            "ak remove <path>",
        ] {
            assert!(
                SKILL_USAGE.contains(needle),
                "SKILL_USAGE is missing required command substring: {needle:?}"
            );
        }
    }

    #[test]
    fn usage_skill_contains_citation_convention() {
        for needle in [
            "sources:",
            "session",
            "checkpoint",
            "captured_at",
            "message_range",
            "audit trail",
            "idempotency",
        ] {
            assert!(
                SKILL_USAGE.contains(needle),
                "SKILL_USAGE is missing required citation-convention substring: {needle:?}"
            );
        }
    }
}
