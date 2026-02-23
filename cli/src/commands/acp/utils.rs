use regex::Regex;

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

/// Convert XML tags to markdown headers using pattern matching.
/// Handles core context tags plus both legacy and current skill sections.
pub fn convert_xml_tags_to_markdown(text: &str) -> String {
    let mut result = text.to_string();

    let tag_patterns = [
        ("<scratchpad>", "## **Scratchpad**\n"),
        ("<todo>", "### **Todo**\n"),
        ("<local_context>", "### **Local Context**\n"),
        ("<available_skills>", "### **Skills**\n"),
        // Legacy tag kept for backward compatibility with older checkpoints.
        ("<rulebooks>", "### **Skills**\n"),
    ];

    let closing_patterns = [
        "</scratchpad>",
        "</todo>",
        "</local_context>",
        "</available_skills>",
        "</rulebooks>",
    ];

    // Convert opening tags
    for (opening_tag, markdown_header) in tag_patterns.iter() {
        result = result.replace(opening_tag, markdown_header);
    }

    // Remove closing tags
    for closing_tag in closing_patterns.iter() {
        result = result.replace(closing_tag, "");
    }

    result
}

/// Process checkpoint patterns - remove checkpoint IDs completely
pub fn remove_checkpoint_patterns(text: &str) -> String {
    let pattern = r"<checkpoint_id>([^<]*)</checkpoint_id>";
    let regex = match Regex::new(pattern) {
        Ok(r) => r,
        Err(_) => return text.to_string(),
    };

    regex.replace_all(text, "").to_string()
}

/// Process all XML patterns in sequence
pub fn process_all_xml_patterns(text: &str) -> String {
    let mut result = text.to_string();

    // First remove checkpoint patterns
    result = remove_checkpoint_patterns(&result);

    // Then convert XML tags to markdown
    result = convert_xml_tags_to_markdown(&result);

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_tool_name() {
        assert_eq!(strip_tool_name("stakpak__run_command"), "run_command");
        assert_eq!(strip_tool_name("run_command"), "run_command");
        assert_eq!(strip_tool_name("other__server__tool"), "server__tool");
        assert_eq!(strip_tool_name("prefix__"), "prefix__");
        assert_eq!(strip_tool_name("__tool"), "tool");
        assert_eq!(strip_tool_name("str_replace()"), "str_replace");
        assert_eq!(strip_tool_name("create()"), "create");
        assert_eq!(strip_tool_name("stakpak__str_replace()"), "str_replace");
    }

    #[test]
    fn test_convert_xml_tags_to_markdown() {
        let input = "<scratchpad>\n<todo>\n- Task 1\n- Task 2\n</todo>\n</scratchpad>";
        let expected = "## **Scratchpad**\n\n### **Todo**\n\n- Task 1\n- Task 2\n\n";
        let result = convert_xml_tags_to_markdown(input);
        assert_eq!(result, expected);
    }

    #[test]
    fn test_convert_available_skills_tag_to_markdown() {
        let input = "<available_skills>\n- skill one\n</available_skills>";
        let expected = "### **Skills**\n\n- skill one\n";
        let result = convert_xml_tags_to_markdown(input);
        assert_eq!(result, expected);
    }

    #[test]
    fn test_convert_legacy_rulebooks_tag_to_skills_markdown() {
        let input = "<rulebooks>\n- skill one\n</rulebooks>";
        let expected = "### **Skills**\n\n- skill one\n";
        let result = convert_xml_tags_to_markdown(input);
        assert_eq!(result, expected);
    }

    #[test]
    fn test_remove_checkpoint_patterns() {
        let input = "Hello <checkpoint_id>123</checkpoint_id> world";
        let expected = "Hello  world";
        let result = remove_checkpoint_patterns(input);
        assert_eq!(result, expected);
    }

    #[test]
    fn test_process_all_xml_patterns() {
        let input = "<checkpoint_id>abc</checkpoint_id><scratchpad>\n<todo>\n- Task\n</todo>\n</scratchpad>";
        let expected = "## **Scratchpad**\n\n### **Todo**\n\n- Task\n\n";
        let result = process_all_xml_patterns(input);
        assert_eq!(result, expected);
    }
}
