//! Command scanning from predefined, personal, project, and config sources.

use stakpak_shared::file_scanner;
use stakpak_shared::markdown;
use stakpak_shared::models::commands::{CommandSource, CommandsConfig, CustomCommand};
use stakpak_shared::path_utils;
use std::collections::HashMap;

const MAX_CUSTOM_COMMAND_BYTES: u64 = 64 * 1024;
pub const CMD_PREFIX: &str = "/cmd:";

/// Scan commands from predefined → personal → project → definitions (config highest).
pub fn scan_commands(commands_config: Option<&CommandsConfig>) -> Vec<CustomCommand> {
    let mut by_id: HashMap<String, CustomCommand> = HashMap::new();

    let (file_prefix, id_prefix) = commands_config
        .map(|c| (c.file_prefix(), c.id_prefix()))
        .unwrap_or(("cmd_", CMD_PREFIX));

    for (name, content) in super::PREDEFINED_COMMANDS.iter() {
        if let Some(config) = commands_config
            && !config.should_load(name)
        {
            continue;
        }
        add_command(
            &mut by_id,
            format!("/{name}"),
            name,
            content,
            CommandSource::Predefined,
        );
    }

    let personal = dirs::home_dir().map(|h| h.join(".stakpak").join("commands"));
    let project = std::env::current_dir()
        .ok()
        .map(|cwd| cwd.join(".stakpak").join("commands"));

    let name_prefix = if file_prefix.is_empty() {
        None
    } else {
        Some(file_prefix)
    };

    for (dir, source) in [
        (personal.as_deref(), CommandSource::PersonalFile),
        (project.as_deref(), CommandSource::ProjectFile),
    ] {
        if let Some(d) = dir {
            for (name, content) in
                file_scanner::scan_flat_markdown_dir(d, name_prefix, MAX_CUSTOM_COMMAND_BYTES)
            {
                if let Some(config) = commands_config
                    && !config.should_load(&name)
                {
                    continue;
                }
                add_command(
                    &mut by_id,
                    format!("{id_prefix}{name}"),
                    &name,
                    &content,
                    source,
                );
            }
        }
    }

    if let Some(config) = commands_config {
        for (name, file_path) in &config.definitions {
            if !config.should_load(name) {
                continue;
            }
            let content = match std::fs::read_to_string(path_utils::expand_path(file_path)) {
                Ok(c) => c.trim().to_string(),
                Err(_) => continue,
            };
            if content.is_empty() {
                continue;
            }
            add_command(
                &mut by_id,
                format!("{id_prefix}{name}"),
                name,
                &content,
                CommandSource::ConfigDefinition,
            );
        }
    }

    let mut commands: Vec<_> = by_id.into_values().collect();
    commands.sort_by(|a, b| a.id.cmp(&b.id));
    commands
}

fn add_command(
    by_id: &mut HashMap<String, CustomCommand>,
    id: String,
    name: &str,
    content: &str,
    source: CommandSource,
) {
    let description = markdown::extract_markdown_title(content).unwrap_or_else(|| name.to_string());
    by_id.insert(
        id.clone(),
        CustomCommand {
            id,
            description,
            content: content.to_string(),
            source,
        },
    );
}
