use std::path::Path;

use super::AppConfig;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ResolvedProfileOverrides {
    pub model: Option<String>,
    pub auto_approve: Option<Vec<String>>,
    pub allowed_tools: Option<Vec<String>>,
    pub system_prompt: Option<String>,
    pub max_turns: Option<usize>,
}

pub(crate) fn resolve_profile_run_overrides(
    profile_name: &str,
    config_path: Option<&str>,
) -> Option<ResolvedProfileOverrides> {
    let config = AppConfig::load(profile_name, config_path.map(Path::new)).ok()?;

    let model = normalize_optional_string(config.model);
    let auto_approve = normalize_tool_list(config.auto_approve);
    let allowed_tools = normalize_tool_list(config.allowed_tools);
    let system_prompt = normalize_optional_string(config.system_prompt);
    let max_turns = config.max_turns;

    if model.is_none()
        && auto_approve.is_none()
        && allowed_tools.is_none()
        && system_prompt.is_none()
        && max_turns.is_none()
    {
        return None;
    }

    Some(ResolvedProfileOverrides {
        model,
        auto_approve,
        allowed_tools,
        system_prompt,
        max_turns,
    })
}

fn normalize_tool_list(tools: Option<Vec<String>>) -> Option<Vec<String>> {
    tools.map(|tools| {
        tools
            .into_iter()
            .filter_map(|tool| {
                let normalized = stakpak_server::strip_tool_prefix(&tool).trim().to_string();
                if normalized.is_empty() {
                    None
                } else {
                    Some(normalized)
                }
            })
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect()
    })
}

fn normalize_optional_string(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

#[cfg(test)]
mod tests {
    use super::resolve_profile_run_overrides;
    use std::path::PathBuf;

    fn temp_file_path(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|value| value.as_nanos())
            .unwrap_or(0);

        std::env::temp_dir().join(format!(
            "stakpak-{}-{}-{}.toml",
            name,
            std::process::id(),
            nanos
        ))
    }

    fn write_profile_config(path: &PathBuf, content: &str) {
        let write_result = std::fs::write(path, content);
        assert!(write_result.is_ok());
    }

    #[test]
    fn resolve_profile_trims_whitespace_values() {
        let path = temp_file_path("profile-resolver-trim");
        write_profile_config(
            &path,
            r#"
[settings]
editor = "nano"

[profiles.default]
api_key = "default-key"

[profiles.monitoring]
model = "  anthropic/claude-sonnet-4-5  "
system_prompt = "  Report only  "
"#,
        );

        let resolved =
            resolve_profile_run_overrides("monitoring", Some(path.to_string_lossy().as_ref()));
        assert!(resolved.is_some());

        if let Some(resolved) = resolved {
            assert_eq!(
                resolved.model.as_deref(),
                Some("anthropic/claude-sonnet-4-5")
            );
            assert_eq!(resolved.system_prompt.as_deref(), Some("Report only"));
        }

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn resolve_profile_extracts_system_prompt_and_max_turns() {
        let path = temp_file_path("profile-resolver-overrides");
        write_profile_config(
            &path,
            r#"
[settings]
editor = "nano"

[profiles.default]
api_key = "default-key"

[profiles.monitoring]
system_prompt = "Report only"
max_turns = 16
"#,
        );

        let resolved =
            resolve_profile_run_overrides("monitoring", Some(path.to_string_lossy().as_ref()));
        assert!(resolved.is_some());

        if let Some(resolved) = resolved {
            assert_eq!(resolved.system_prompt.as_deref(), Some("Report only"));
            assert_eq!(resolved.max_turns, Some(16));
        }

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn resolve_profile_filters_empty_system_prompt() {
        let path = temp_file_path("profile-resolver-empty-prompt");
        write_profile_config(
            &path,
            r#"
[settings]
editor = "nano"

[profiles.default]
api_key = "default-key"

[profiles.monitoring]
system_prompt = "   "
"#,
        );

        let resolved =
            resolve_profile_run_overrides("monitoring", Some(path.to_string_lossy().as_ref()));
        assert!(resolved.is_none());

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn resolve_profile_default_name_is_not_short_circuited() {
        let path = temp_file_path("profile-resolver-default");
        write_profile_config(
            &path,
            r#"
[settings]
editor = "nano"

[profiles.default]
model = "anthropic/claude-sonnet-4-5"
"#,
        );

        let resolved =
            resolve_profile_run_overrides("default", Some(path.to_string_lossy().as_ref()));
        assert!(resolved.is_some());

        let _ = std::fs::remove_file(path);
    }
}
