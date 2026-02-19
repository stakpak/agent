//! Unified agent context for all execution modes.
//!
//! `AgentContext` gathers and injects environment context (local system info,
//! rulebooks, AGENTS.md, APPS.md) into the first user message. It replaces the
//! per-mode `add_local_context`, `add_rulebooks`, `add_agents_md`, `add_apps_md`
//! helpers with a single `enrich_prompt()` call, ensuring consistent behavior
//! across interactive, async, watch/schedule, and channel/gateway modes.

use std::path::Path;
use std::sync::Arc;

use crate::config::AppConfig;
use crate::utils::agents_md::{AgentsMdInfo, discover_agents_md, format_agents_md_for_context};
use crate::utils::apps_md::{AppsMdInfo, discover_apps_md, format_apps_md_for_context};
use crate::utils::local_context::{LocalContext, analyze_local_context};
use stakpak_api::models::ListRuleBook;
use stakpak_api::{AgentClient, AgentClientConfig, AgentProvider};

/// Unified context for all agent execution modes.
///
/// Built via [`AgentContext::gather`] or [`AgentContext::from_parts`], then
/// injected into the first user message via [`AgentContext::enrich_prompt`].
#[derive(Debug, Clone)]
pub struct AgentContext {
    /// Pre-formatted local context display string (avoids async at injection time).
    pub local_context_formatted: Option<String>,
    pub rulebooks: Option<Vec<ListRuleBook>>,
    pub agents_md: Option<AgentsMdInfo>,
    pub apps_md: Option<AppsMdInfo>,
}

impl AgentContext {
    /// Gather all available context from the environment.
    ///
    /// Loads `AppConfig` from the given profile, discovers AGENTS.md/APPS.md
    /// from `cwd`, analyzes local system context, and fetches rulebooks via
    /// the Stakpak API (if an API key is configured).
    ///
    /// This is the primary constructor for callers that don't need custom
    /// gathering logic (e.g., scheduler, gateway, dry-run).
    pub async fn gather(profile: &str, cwd: &Path) -> Self {
        let config = AppConfig::load::<&str>(profile, None).ok();

        // Local context (machine info, cwd, git, etc.)
        let local_context = if let Some(ref cfg) = config {
            analyze_local_context(cfg).await.ok()
        } else {
            None
        };

        let local_context_formatted = if let Some(ref ctx) = local_context {
            ctx.format_display().await.ok()
        } else {
            None
        };

        // AGENTS.md and APPS.md discovery
        let agents_md = discover_agents_md(cwd);
        let apps_md = discover_apps_md(cwd);

        // Rulebooks (requires Stakpak API key)
        let rulebooks = if let Some(ref cfg) = config {
            Self::fetch_rulebooks(cfg).await
        } else {
            None
        };

        Self {
            local_context_formatted,
            rulebooks,
            agents_md,
            apps_md,
        }
    }

    /// Build an `AgentContext` from pre-gathered components.
    ///
    /// Use this when context pieces are already available (e.g., gathered
    /// separately in `main.rs` with custom logic like `--ignore-agents-md`).
    pub async fn from_parts(
        local_context: Option<LocalContext>,
        rulebooks: Option<Vec<ListRuleBook>>,
        agents_md: Option<AgentsMdInfo>,
        apps_md: Option<AppsMdInfo>,
    ) -> Self {
        let local_context_formatted = if let Some(ref ctx) = local_context {
            ctx.format_display().await.ok()
        } else {
            None
        };

        Self {
            local_context_formatted,
            rulebooks,
            agents_md,
            apps_md,
        }
    }

    /// Update the rulebooks in this context.
    ///
    /// Used by interactive mode when the user changes rulebook selection
    /// mid-session.
    pub fn update_rulebooks(&mut self, rulebooks: Option<Vec<ListRuleBook>>) {
        self.rulebooks = rulebooks;
    }

    /// Enrich a user prompt with context XML tags.
    ///
    /// Appends `<local_context>`, `<rulebooks>`, `<agents_md>`, and `<apps_md>`
    /// sections to the user input, matching the format expected by the system
    /// prompt.
    ///
    /// # Arguments
    /// * `user_input` - The raw user prompt text
    /// * `is_first_message` - Whether this is the first non-system message in the session.
    ///   Context is only injected on the first message (except when `force_context` is true).
    /// * `force_context` - Force context injection even if not the first message.
    ///   Used by interactive mode when rulebooks change mid-session.
    pub fn enrich_prompt(
        &self,
        user_input: &str,
        is_first_message: bool,
        force_context: bool,
    ) -> String {
        if !is_first_message && !force_context {
            return user_input.to_string();
        }

        let mut result = user_input.to_string();

        // Local context
        if let Some(ref formatted) = self.local_context_formatted {
            result = format!(
                "{}\n<local_context>\n{}\n</local_context>",
                result, formatted
            );
        }

        // Rulebooks
        if let Some(ref rulebooks) = self.rulebooks {
            let rulebooks_text = format_rulebooks(rulebooks);
            result = format!("{}\n<rulebooks>\n{}\n</rulebooks>", result, rulebooks_text);
        }

        // AGENTS.md (only on first message, not on force_context-only updates)
        if is_first_message {
            if let Some(ref agents_md) = self.agents_md {
                let agents_text = format_agents_md_for_context(agents_md);
                result = format!("{}\n<agents_md>\n{}\n</agents_md>", result, agents_text);
            }

            if let Some(ref apps_md) = self.apps_md {
                let apps_text = format_apps_md_for_context(apps_md);
                result = format!("{}\n<apps_md>\n{}\n</apps_md>", result, apps_text);
            }
        }

        result
    }

    /// Fetch rulebooks from the Stakpak API using the given config's credentials.
    ///
    /// Returns `None` if no API key is configured or the fetch fails.
    async fn fetch_rulebooks(config: &AppConfig) -> Option<Vec<ListRuleBook>> {
        let api_key = config.get_stakpak_api_key()?;

        let providers = config.get_llm_provider_config_async().await;
        let mut client_config = AgentClientConfig::new().with_providers(providers);
        client_config = client_config.with_stakpak(
            stakpak_api::StakpakConfig::new(api_key).with_endpoint(config.api_endpoint.clone()),
        );

        let client: Arc<dyn AgentProvider> = Arc::new(AgentClient::new(client_config).await.ok()?);

        let rulebooks = client.list_rulebooks().await.ok()?;

        // Apply rulebook filter from config if present
        Some(if let Some(rulebook_config) = &config.rulebooks {
            rulebook_config.filter_rulebooks(rulebooks)
        } else {
            rulebooks
        })
    }
}

/// Format rulebooks list for context injection.
///
/// Produces the same format as the old `add_rulebooks()` helper.
fn format_rulebooks(rulebooks: &[ListRuleBook]) -> String {
    if !rulebooks.is_empty() {
        format!(
            "\n\n# My Rule Books:\n\n{}",
            rulebooks
                .iter()
                .map(|rulebook| {
                    let text = rulebook.to_text();
                    let mut lines = text.lines();
                    let mut result = String::new();
                    if let Some(first) = lines.next() {
                        result.push_str(&format!("  - {}", first));
                        for line in lines {
                            result.push_str(&format!("\n    {}", line));
                        }
                    }
                    result
                })
                .collect::<Vec<String>>()
                .join("\n")
        )
    } else {
        "# No Rule Books Available".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn make_agents_md() -> AgentsMdInfo {
        AgentsMdInfo {
            content: "## Setup\n- Run tests".to_string(),
            path: PathBuf::from("/project/AGENTS.md"),
        }
    }

    fn make_apps_md() -> AppsMdInfo {
        AppsMdInfo {
            content: "## My App\n- Port 8080".to_string(),
            path: PathBuf::from("/project/APPS.md"),
        }
    }

    fn make_rulebooks() -> Vec<ListRuleBook> {
        vec![ListRuleBook {
            id: "rb_test_001".to_string(),
            uri: "stakpak://test/rulebook.md".to_string(),
            description: "Test rulebook".to_string(),
            visibility: stakpak_api::models::RuleBookVisibility::Public,
            tags: vec!["test".to_string()],
            created_at: None,
            updated_at: None,
        }]
    }

    fn make_context(
        local_context_formatted: Option<&str>,
        rulebooks: Option<Vec<ListRuleBook>>,
        agents_md: Option<AgentsMdInfo>,
        apps_md: Option<AppsMdInfo>,
    ) -> AgentContext {
        AgentContext {
            local_context_formatted: local_context_formatted.map(String::from),
            rulebooks,
            agents_md,
            apps_md,
        }
    }

    #[test]
    fn test_enrich_prompt_first_message_full_context() {
        let ctx = make_context(
            Some("# System Details\n\nMachine: test"),
            Some(make_rulebooks()),
            Some(make_agents_md()),
            Some(make_apps_md()),
        );

        let result = ctx.enrich_prompt("Hello agent", true, false);

        assert!(result.starts_with("Hello agent"));
        assert!(result.contains("<local_context>"));
        assert!(result.contains("Machine: test"));
        assert!(result.contains("</local_context>"));
        assert!(result.contains("<rulebooks>"));
        assert!(result.contains("Test rulebook"));
        assert!(result.contains("</rulebooks>"));
        assert!(result.contains("<agents_md>"));
        assert!(result.contains("## Setup"));
        assert!(result.contains("</agents_md>"));
        assert!(result.contains("<apps_md>"));
        assert!(result.contains("## My App"));
        assert!(result.contains("</apps_md>"));
    }

    #[test]
    fn test_enrich_prompt_not_first_message_returns_unchanged() {
        let ctx = make_context(
            Some("# System Details"),
            Some(make_rulebooks()),
            Some(make_agents_md()),
            Some(make_apps_md()),
        );

        let result = ctx.enrich_prompt("Follow-up question", false, false);

        assert_eq!(result, "Follow-up question");
    }

    #[test]
    fn test_enrich_prompt_force_context_injects_local_and_rulebooks() {
        let ctx = make_context(
            Some("# System Details\n\nMachine: test"),
            Some(make_rulebooks()),
            Some(make_agents_md()),
            Some(make_apps_md()),
        );

        let result = ctx.enrich_prompt("Updated question", false, true);

        // force_context should inject local_context and rulebooks
        assert!(result.contains("<local_context>"));
        assert!(result.contains("<rulebooks>"));
        // But NOT agents_md/apps_md (those are first-message only)
        assert!(!result.contains("<agents_md>"));
        assert!(!result.contains("<apps_md>"));
    }

    #[test]
    fn test_enrich_prompt_partial_context() {
        // Only local_context, no rulebooks/agents_md/apps_md
        let ctx = make_context(Some("# System Details"), None, None, None);

        let result = ctx.enrich_prompt("Hello", true, false);

        assert!(result.contains("<local_context>"));
        assert!(!result.contains("<rulebooks>"));
        assert!(!result.contains("<agents_md>"));
        assert!(!result.contains("<apps_md>"));
    }

    #[test]
    fn test_enrich_prompt_empty_context() {
        let ctx = make_context(None, None, None, None);

        let result = ctx.enrich_prompt("Hello", true, false);

        assert_eq!(result, "Hello");
    }

    #[test]
    fn test_enrich_prompt_no_local_context_but_has_others() {
        let ctx = make_context(None, Some(make_rulebooks()), Some(make_agents_md()), None);

        let result = ctx.enrich_prompt("Hello", true, false);

        assert!(!result.contains("<local_context>"));
        assert!(result.contains("<rulebooks>"));
        assert!(result.contains("<agents_md>"));
        assert!(!result.contains("<apps_md>"));
    }

    #[test]
    fn test_update_rulebooks() {
        let mut ctx = make_context(None, None, None, None);
        assert!(ctx.rulebooks.is_none());

        ctx.update_rulebooks(Some(make_rulebooks()));
        assert!(ctx.rulebooks.is_some());

        let result = ctx.enrich_prompt("Hello", true, false);
        assert!(result.contains("<rulebooks>"));
        assert!(result.contains("Test rulebook"));
    }

    #[test]
    fn test_format_rulebooks_empty() {
        let result = format_rulebooks(&[]);
        assert_eq!(result, "# No Rule Books Available");
    }

    #[test]
    fn test_format_rulebooks_non_empty() {
        let rulebooks = make_rulebooks();
        let result = format_rulebooks(&rulebooks);
        assert!(result.contains("# My Rule Books:"));
        assert!(result.contains("stakpak://test/rulebook.md"));
    }

    #[test]
    fn test_enrich_prompt_xml_tag_order() {
        let ctx = make_context(
            Some("system info"),
            Some(make_rulebooks()),
            Some(make_agents_md()),
            Some(make_apps_md()),
        );

        let result = ctx.enrich_prompt("prompt", true, false);

        // Verify order: local_context → rulebooks → agents_md → apps_md
        let lc_pos = result.find("<local_context>").unwrap();
        let rb_pos = result.find("<rulebooks>").unwrap();
        let am_pos = result.find("<agents_md>").unwrap();
        let ap_pos = result.find("<apps_md>").unwrap();

        assert!(
            lc_pos < rb_pos,
            "local_context should come before rulebooks"
        );
        assert!(rb_pos < am_pos, "rulebooks should come before agents_md");
        assert!(am_pos < ap_pos, "agents_md should come before apps_md");
    }
}
