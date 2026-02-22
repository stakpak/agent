use crate::utils::agents_md::{AgentsMdInfo, format_agents_md_for_context};
use crate::utils::apps_md::{AppsMdInfo, format_apps_md_for_context};
use crate::utils::local_context::LocalContext;
use stakpak_api::models::Skill;

#[derive(Debug, Clone)]
pub struct AgentContext {
    /// Pre-formatted local context string. Snapshotted once at construction;
    /// does not refresh on subsequent injections (by design — avoids blocking
    /// filesystem walks on every message).
    pub local_context_formatted: Option<String>,
    pub skills: Option<Vec<Skill>>,
    pub agents_md: Option<AgentsMdInfo>,
    pub apps_md: Option<AppsMdInfo>,
}

impl AgentContext {
    pub async fn from_parts(
        local_context: Option<LocalContext>,
        skills: Option<Vec<Skill>>,
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
            skills,
            agents_md,
            apps_md,
        }
    }

    pub fn update_skills(&mut self, skills: Option<Vec<Skill>>) {
        self.skills = skills;
    }

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

        if let Some(ref formatted) = self.local_context_formatted {
            result = format!(
                "{}\n<local_context>\n{}\n</local_context>",
                result, formatted
            );
        }

        if let Some(ref skills) = self.skills
            && !skills.is_empty()
        {
            let skills_text = format_skills(skills);
            result = format!(
                "{}\n<available_skills>\n{}\n</available_skills>",
                result, skills_text
            );
        }

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
}

fn format_skills(skills: &[Skill]) -> String {
    format!(
        "# Available Skills:\n\n{}",
        skills
            .iter()
            .map(|skill| format!("  - {}", skill.to_metadata_text()))
            .collect::<Vec<String>>()
            .join("\n")
    )
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

    fn make_skills() -> Vec<Skill> {
        vec![Skill {
            name: "skill_test_001".to_string(),
            uri: "stakpak://test/skill.md".to_string(),
            description: "Test skill".to_string(),
            source: stakpak_api::models::SkillSource::Remote {
                provider: stakpak_api::models::RemoteProvider::Rulebook {
                    visibility: stakpak_api::models::RuleBookVisibility::Public,
                },
            },
            content: None,
            tags: vec!["test".to_string()],
            license: None,
            compatibility: None,
            metadata: None,
            allowed_tools: None,
        }]
    }

    fn make_context(
        local_context_formatted: Option<&str>,
        skills: Option<Vec<Skill>>,
        agents_md: Option<AgentsMdInfo>,
        apps_md: Option<AppsMdInfo>,
    ) -> AgentContext {
        AgentContext {
            local_context_formatted: local_context_formatted.map(String::from),
            skills,
            agents_md,
            apps_md,
        }
    }

    #[test]
    fn enrich_prompt_first_message_full_context() {
        let ctx = make_context(
            Some("# System Details\n\nMachine: test"),
            Some(make_skills()),
            Some(make_agents_md()),
            Some(make_apps_md()),
        );

        let result = ctx.enrich_prompt("Hello agent", true, false);

        assert!(result.starts_with("Hello agent"));
        assert!(result.contains("<local_context>"));
        assert!(result.contains("Machine: test"));
        assert!(result.contains("<available_skills>"));
        assert!(result.contains("Test skill"));
        assert!(result.contains("<agents_md>"));
        assert!(result.contains("<apps_md>"));
    }

    #[test]
    fn enrich_prompt_not_first_message_returns_unchanged() {
        let ctx = make_context(
            Some("# System Details"),
            Some(make_skills()),
            Some(make_agents_md()),
            Some(make_apps_md()),
        );

        let result = ctx.enrich_prompt("Follow-up question", false, false);
        assert_eq!(result, "Follow-up question");
    }

    #[test]
    fn enrich_prompt_force_context_injects_local_and_skills_only() {
        let ctx = make_context(
            Some("# System Details\n\nMachine: test"),
            Some(make_skills()),
            Some(make_agents_md()),
            Some(make_apps_md()),
        );

        let result = ctx.enrich_prompt("Updated question", false, true);

        assert!(result.contains("<local_context>"));
        assert!(result.contains("<available_skills>"));
        assert!(!result.contains("<agents_md>"));
        assert!(!result.contains("<apps_md>"));
    }

    #[test]
    fn enrich_prompt_empty_context_returns_input_unchanged() {
        let ctx = make_context(None, None, None, None);
        let result = ctx.enrich_prompt("Hello", true, false);
        assert_eq!(result, "Hello");
    }

    #[test]
    fn enrich_prompt_skips_empty_skills_block() {
        let ctx = make_context(None, Some(vec![]), None, None);
        let result = ctx.enrich_prompt("Hello", true, false);
        assert_eq!(result, "Hello");
    }

    #[test]
    fn update_skills_replaces_skills() {
        let mut ctx = make_context(None, None, None, None);
        assert!(ctx.skills.is_none());

        ctx.update_skills(Some(make_skills()));
        assert!(ctx.skills.is_some());
    }
}
