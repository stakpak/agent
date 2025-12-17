/// Strip the MCP server prefix from a tool name.
/// Example: "stakpak__run_command" -> "run_command"
/// Example: "run_command" -> "run_command"
pub fn strip_tool_name(name: &str) -> &str {
    if let Some(pos) = name.find("__")
        && pos + 2 < name.len()
    {
        return &name[pos + 2..];
    }
    name
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_tool_name() {
        assert_eq!(strip_tool_name("stakpak__run_command"), "run_command");
        assert_eq!(strip_tool_name("run_command"), "run_command");
        assert_eq!(strip_tool_name("other__server__tool"), "server__tool"); // Only strips first prefix if multiple? Or last?
        // The implementation finds the *first* "__".
        // "other__server__tool" -> "server__tool". This seems correct assuming hierarchical naming isn't deeper or we only care about top level.
        // Actually, if it's `server_name__tool_name`, the first match is correct.

        assert_eq!(strip_tool_name("prefix__"), ""); // Edge case
        assert_eq!(strip_tool_name("__tool"), "tool");
    }
}
