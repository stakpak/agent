/// Strip the MCP server prefix and any trailing "()" from a tool name.
/// Example: "stakpak__run_command" -> "run_command"
/// Example: "run_command" -> "run_command"
/// Example: "str_replace()" -> "str_replace"
pub fn strip_tool_name(name: &str) -> &str {
    let mut result = name;

    // Strip the MCP server prefix (e.g., "stakpak__")
    if let Some(pos) = result.find("__")
        && pos + 2 < result.len()
    {
        result = &result[pos + 2..];
    }

    // Strip trailing "()" if present
    if result.ends_with("()") {
        result = &result[..result.len() - 2];
    }

    result
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

        assert_eq!(strip_tool_name("prefix__"), "prefix__"); // Edge case: nothing after __, return original
        assert_eq!(strip_tool_name("__tool"), "tool");

        // Test stripping "()" from tool names
        assert_eq!(strip_tool_name("str_replace()"), "str_replace");
        assert_eq!(strip_tool_name("create()"), "create");
        assert_eq!(strip_tool_name("stakpak__str_replace()"), "str_replace");
    }
}
