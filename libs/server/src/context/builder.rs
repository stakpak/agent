use crate::context::{
    ContextBudget,
    budget::{apply_budget, truncate_with_marker},
    environment::EnvironmentContext,
    project::{ContextFile, ProjectContext},
};

#[derive(Debug, Clone, Default)]
pub struct SessionContext {
    pub system_prompt: String,
    pub user_context_block: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct SessionContextBuilder {
    environment: Option<EnvironmentContext>,
    project: Option<ProjectContext>,
    base_system_prompt: Option<String>,
    tool_summaries: Vec<String>,
    budget: ContextBudget,
}

impl SessionContextBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn environment(mut self, environment: EnvironmentContext) -> Self {
        self.environment = Some(environment);
        self
    }

    pub fn project(mut self, project: ProjectContext) -> Self {
        self.project = Some(project);
        self
    }

    pub fn base_system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.base_system_prompt = Some(prompt.into());
        self
    }

    pub fn tools(mut self, tools: &[stakai::Tool]) -> Self {
        self.tool_summaries = tools
            .iter()
            .map(|tool| {
                let description = tool.function.description.trim();
                if description.is_empty() {
                    format!("- {}", tool.function.name)
                } else {
                    format!("- {}: {}", tool.function.name, description)
                }
            })
            .collect();
        self
    }

    pub fn budget(mut self, budget: ContextBudget) -> Self {
        self.budget = budget;
        self
    }

    pub fn build(self) -> SessionContext {
        let system_prompt = self.build_system_prompt();
        let user_context_block = self.build_user_context_block();

        SessionContext {
            system_prompt,
            user_context_block,
        }
    }

    fn build_system_prompt(&self) -> String {
        let mut sections = Vec::new();

        if let Some(base) = self.base_system_prompt.as_ref().map(|prompt| prompt.trim())
            && !base.is_empty()
        {
            sections.push(base.to_string());
        }

        if !self.tool_summaries.is_empty() {
            sections.push(format!(
                "## Available Tools\n{}",
                self.tool_summaries.join("\n")
            ));
        }

        let combined = sections.join("\n\n");
        let (truncated, _) = truncate_with_marker(
            &combined,
            self.budget.system_prompt_max_chars,
            "system prompt",
        );
        truncated
    }

    fn build_user_context_block(&self) -> Option<String> {
        let mut sections = Vec::new();

        if let Some(environment) = &self.environment {
            sections.push(format!(
                "<local_context>\n{}\n</local_context>",
                environment.to_local_context_block()
            ));
        }

        let mut files = self
            .project
            .as_ref()
            .map(|project| project.files.clone())
            .unwrap_or_default();

        if !files.is_empty() {
            apply_budget(&mut files, &self.budget);
            for file in files {
                sections.push(format_context_file(&file));
            }
        }

        if sections.is_empty() {
            return None;
        }

        Some(sections.join("\n\n"))
    }
}

fn format_context_file(file: &ContextFile) -> String {
    if file.name.eq_ignore_ascii_case("AGENTS.md") {
        return format!(
            "<agents_md>\n# AGENTS.md (from {})\n\n{}\n</agents_md>",
            file.path, file.content
        );
    }

    if file.name.eq_ignore_ascii_case("APPS.md") {
        return format!(
            "<apps_md>\n# APPS.md (from {})\n\n{}\n</apps_md>",
            file.path, file.content
        );
    }

    format!(
        "<context_file name=\"{}\" path=\"{}\">\n{}\n</context_file>",
        escape_xml_attribute(&file.name),
        escape_xml_attribute(&file.path),
        file.content
    )
}

fn escape_xml_attribute(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::{ContextPriority, project::ContextFile};

    fn test_environment() -> EnvironmentContext {
        EnvironmentContext {
            machine_name: "test-machine".to_string(),
            operating_system: "Linux".to_string(),
            shell_type: "bash".to_string(),
            is_container: false,
            working_directory: "/tmp".to_string(),
            current_datetime_utc: chrono::Utc::now(),
            directory_tree: "├── src".to_string(),
            git: None,
        }
    }

    #[test]
    fn builds_context_with_local_context_and_agents_file() {
        let project = ProjectContext {
            files: vec![ContextFile::new(
                "AGENTS.md",
                "/tmp/AGENTS.md",
                "Follow project conventions",
                ContextPriority::Critical,
            )],
        };

        let context = SessionContextBuilder::new()
            .environment(test_environment())
            .project(project)
            .build();

        assert!(context.user_context_block.is_some());
        if let Some(block) = context.user_context_block {
            assert!(block.contains("<local_context>"));
            assert!(block.contains("<agents_md>"));
        }
    }

    #[test]
    fn system_prompt_includes_base_prompt() {
        let context = SessionContextBuilder::new()
            .base_system_prompt("You are an expert DevOps agent.")
            .build();

        assert!(context.system_prompt.contains("expert DevOps agent"));
    }

    #[test]
    fn system_prompt_includes_tool_summaries() {
        let tools = vec![stakai::Tool::function(
            "run_command",
            "Execute a shell command",
        )];

        let context = SessionContextBuilder::new().tools(&tools).build();

        assert!(context.system_prompt.contains("run_command"));
        assert!(context.system_prompt.contains("Execute a shell command"));
    }

    #[test]
    fn empty_builder_produces_empty_system_prompt_and_no_user_context() {
        let context = SessionContextBuilder::new().build();

        assert!(context.system_prompt.is_empty());
        assert!(context.user_context_block.is_none());
    }

    #[test]
    fn apps_md_formatted_with_apps_md_tag() {
        let project = ProjectContext {
            files: vec![ContextFile::new(
                "APPS.md",
                "/workspace/APPS.md",
                "App configuration guide",
                ContextPriority::High,
            )],
        };

        let context = SessionContextBuilder::new().project(project).build();

        if let Some(block) = context.user_context_block {
            assert!(block.contains("<apps_md>"));
            assert!(block.contains("App configuration guide"));
        } else {
            panic!("expected user context block with APPS.md");
        }
    }

    #[test]
    fn generic_context_file_uses_context_file_tag() {
        let project = ProjectContext {
            files: vec![ContextFile::new(
                "notes.txt",
                "caller://notes.txt",
                "custom caller notes",
                ContextPriority::CallerSupplied,
            )],
        };

        let context = SessionContextBuilder::new().project(project).build();

        if let Some(block) = context.user_context_block {
            assert!(block.contains("<context_file"));
            assert!(block.contains("custom caller notes"));
        } else {
            panic!("expected user context block with context file");
        }
    }

    #[test]
    fn generic_context_file_escapes_xml_attributes() {
        let project = ProjectContext {
            files: vec![ContextFile::new(
                "bad\"name<>",
                "caller://path?x=1&y='2'",
                "content",
                ContextPriority::CallerSupplied,
            )],
        };

        let context = SessionContextBuilder::new().project(project).build();

        if let Some(block) = context.user_context_block {
            assert!(block.contains("name=\"bad&quot;name&lt;&gt;\""));
            assert!(block.contains("path=\"caller://path?x=1&amp;y=&apos;2&apos;\""));
        } else {
            panic!("expected user context block with escaped attributes");
        }
    }

    #[test]
    fn system_prompt_truncated_by_budget() {
        let budget = ContextBudget {
            system_prompt_max_chars: 50,
            ..Default::default()
        };

        let context = SessionContextBuilder::new()
            .base_system_prompt("A".repeat(1_000))
            .budget(budget)
            .build();

        assert!(
            context.system_prompt.chars().count() <= 50,
            "system prompt should be truncated to budget"
        );
    }
}
