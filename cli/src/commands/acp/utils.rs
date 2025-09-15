use regex::Regex;

/// Convert XML tags to markdown headers using pattern matching
/// Handles the 4 specific tags: scratchpad, todo, local_context, rulebooks
pub fn convert_xml_tags_to_markdown(text: &str) -> String {
    let mut result = text.to_string();

    // Define the 4 specific tags we want to handle
    let tag_patterns = [
        ("<scratchpad>", "## **Scratchpad**\n"),
        ("<todo>", "### **Todo**\n"),
        ("<local_context>", "### **Local Context**\n"),
        ("<rulebooks>", "### **Rulebooks**\n"),
    ];

    let closing_patterns = [
        "</scratchpad>",
        "</todo>",
        "</local_context>",
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
    fn test_convert_xml_tags_to_markdown() {
        let input = "<scratchpad>\n<todo>\n- Task 1\n- Task 2\n</todo>\n</scratchpad>";
        let expected = "## **Scratchpad**\n### **Todo**\n- Task 1\n- Task 2\n";
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
        let expected = "## **Scratchpad**\n### **Todo**\n- Task\n";
        let result = process_all_xml_patterns(input);
        assert_eq!(result, expected);
    }
}
