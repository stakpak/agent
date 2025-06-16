// Tool descriptions
pub const RUN_COMMAND_DESCRIPTION: &str = "A system command execution tool that allows running shell commands with full system access. 

SECRET HANDLING: 
- Output containing secrets will be redacted and shown as placeholders like [REDACTED_SECRET:rule-id:hash]
- You can use these placeholders in subsequent commands - they will be automatically restored to actual values before execution
- Example: If you see 'export API_KEY=[REDACTED_SECRET:api-key:abc123]', you can use '[REDACTED_SECRET:api-key:abc123]' in later commands

If the command's output exceeds 300 lines the result will be truncated and the full output will be saved to a file in the current directory";

pub const VIEW_DESCRIPTION: &str = "View the contents of a file or list the contents of a directory. Can read entire files or specific line ranges.

SECRET HANDLING:
- File contents containing secrets will be redacted and shown as placeholders like [REDACTED_SECRET:rule-id:hash]
- These placeholders represent actual secret values that are safely stored for later use
- You can reference these placeholders when working with the file content

A maximum of 300 lines will be shown at a time, the rest will be truncated.";

pub const STR_REPLACE_DESCRIPTION: &str = "Replace a specific string in a file with new text. The old_str must match exactly including whitespace and indentation.

SECRET HANDLING:
- You can use secret placeholders like [REDACTED_SECRET:rule-id:hash] in both old_str and new_str parameters
- These placeholders will be automatically restored to actual secret values before performing the replacement
- This allows you to safely work with secret values without exposing them

When replacing code, ensure the new text maintains proper syntax, indentation, and follows the codebase style.";

pub const CREATE_DESCRIPTION: &str = "Create a new file with the specified content. Will fail if file already exists. When creating code, ensure the new text has proper syntax, indentation, and follows the codebase style. Parent directories will be created automatically if they don't exist.";

pub const INSERT_DESCRIPTION: &str =
    "Insert text at a specific line number in a file. Line numbers are 1-indexed.";

pub const GENERATE_CODE_DESCRIPTION: &str = "Advanced Generate/Edit devops configurations and infrastructure as code with suggested file names using a given prompt. This code generation/editing only works for Terraform, Kubernetes, Dockerfile, and Github Actions. If save_files is true, the generated files will be saved to the filesystem. The printed shell output will redact any secrets, will be replaced with a placeholder [REDACTED_SECRET:rule-id:short-hash]

IMPORTANT: When breaking down large projects into multiple generation steps, always include previously generated files in the 'context' parameter to maintain coherent references and consistent structure across all generated files.";

pub const SMART_SEARCH_CODE_DESCRIPTION: &str = "Query remote configurations and infrastructure as code indexed in Stakpak using natural language. This function uses a smart retrival system to find relevant code blocks with a relevance score, not just keyword matching. This function is useful for finding code blocks that are not in your local filesystem.";

// Parameter descriptions
pub const COMMAND_PARAM_DESCRIPTION: &str = "The shell command to execute";
pub const WORK_DIR_PARAM_DESCRIPTION: &str = "Optional working directory for command execution";

pub const PATH_PARAM_DESCRIPTION: &str = "The path to the file or directory to view";
pub const VIEW_RANGE_PARAM_DESCRIPTION: &str = "Optional line range to view [start_line, end_line]. Line numbers are 1-indexed. Use -1 for end_line to read to end of file.";

pub const FILE_PATH_PARAM_DESCRIPTION: &str = "The path to the file to modify";
pub const OLD_STR_PARAM_DESCRIPTION: &str =
    "The exact text to replace (must match exactly, including whitespace and indentation)";
pub const NEW_STR_PARAM_DESCRIPTION: &str = "The new text to insert in place of the old text. When replacing code, ensure the new text maintains proper syntax, indentation, and follows the codebase style.";

pub const CREATE_PATH_PARAM_DESCRIPTION: &str = "The path where the new file should be created";
pub const FILE_TEXT_PARAM_DESCRIPTION: &str = "The content to write to the new file, when creating code, ensure the new text has proper syntax, indentation, and follows the codebase style.";

pub const INSERT_LINE_PARAM_DESCRIPTION: &str =
    "The line number where text should be inserted (1-indexed)";
pub const INSERT_TEXT_PARAM_DESCRIPTION: &str = "The text to insert";

pub const GENERATE_PROMPT_PARAM_DESCRIPTION: &str = "Prompt to use to generate code, this should be as detailed as possible. Make sure to specify the paths of the files to be created or modified if you want to save changes to the filesystem.";
pub const PROVISIONER_PARAM_DESCRIPTION: &str =
    "Type of code to generate one of Dockerfile, Kubernetes, Terraform, GithubActions";
pub const SAVE_FILES_PARAM_DESCRIPTION: &str =
    "Whether to save the generated files to the filesystem (default: false)";
pub const CONTEXT_PARAM_DESCRIPTION: &str = "Optional list of file paths to include as context for the generation. CRITICAL: When generating code in multiple steps (breaking down large projects), always include previously generated files from earlier steps to ensure consistent references, imports, and overall project coherence. Add any files you want to edit, or that you want to use as context for the generation (default: empty)";

pub const SEARCH_QUERY_PARAM_DESCRIPTION: &str = "The natural language query to find relevant code blocks, the more detailed the query the better the results will be";
pub const SEARCH_LIMIT_PARAM_DESCRIPTION: &str =
    "The maximum number of results to return (default: 10)";
