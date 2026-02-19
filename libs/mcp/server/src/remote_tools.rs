use crate::tool_container::ToolContainer;
use chrono::{DateTime, Utc};
use rmcp::{
    ErrorData as McpError, handler::server::wrapper::Parameters, model::*, schemars, tool,
    tool_router,
};
use serde::Deserialize;
use stakpak_api::models::{
    SearchDocsRequest as ApiSearchDocsRequest, SearchMemoryRequest as ApiSearchMemoryRequest,
};
use stakpak_shared::utils::{handle_large_output, sanitize_text_output};
// use stakpak_api::models::CodeIndex;
// use stakpak_shared::local_store::LocalStore;
// use stakpak_shared::models::indexing::IndexingStatus;
// use tracing::error;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GenerateCodeRequest {
    #[schemars(
        description = "Prompt to use to generate code, this should be as detailed as possible. Make sure to specify the paths of the files to be created or modified if you want to save changes to the filesystem."
    )]
    pub prompt: String,
    #[schemars(
        description = "Whether to save the generated files to the filesystem (default: false)"
    )]
    pub save_files: Option<bool>,
    #[schemars(
        description = "Optional list of file paths to include as context for the generation. CRITICAL: When generating code in multiple steps (breaking down large projects), always include previously generated files from earlier steps to ensure consistent references, imports, and overall project coherence. Add any files you want to edit, or that you want to use as context for the generation (default: empty)"
    )]
    pub context: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum KeywordInput {
    String(String),
    List(Vec<String>),
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SearchDocsRequest {
    #[schemars(
        description = "Search keywords. Preferred format: array of strings (e.g., [\"kubernetes\", \"ingress\", \"nginx\", \"latest\"]). Backward compatible legacy format: a space-separated string (e.g., \"kubernetes ingress nginx latest\")."
    )]
    pub keywords: KeywordInput,
    #[schemars(
        description = "Optional keywords to exclude. Preferred format: array of strings (e.g., [\"deprecated\", \"legacy\"]). Backward compatible legacy format: a space-separated string."
    )]
    pub exclude_keywords: Option<KeywordInput>,
    #[schemars(description = "The maximum number of results to return (default: 5, max: 5)")]
    pub limit: Option<u32>,
}

fn normalize_keyword_input(input: KeywordInput) -> Vec<String> {
    match input {
        KeywordInput::String(value) => value
            .split_whitespace()
            .map(std::string::ToString::to_string)
            .collect(),
        KeywordInput::List(values) => values,
    }
    .into_iter()
    .map(|keyword| keyword.trim().to_string())
    .filter(|keyword| !keyword.is_empty())
    .collect()
}

fn normalize_optional_keyword_input(input: Option<KeywordInput>) -> Option<Vec<String>> {
    input.and_then(|value| {
        let normalized = normalize_keyword_input(value);
        if normalized.is_empty() {
            None
        } else {
            Some(normalized)
        }
    })
}

const MAX_SEARCH_DOCS_KEYWORDS: usize = 32;
const MAX_SEARCH_DOCS_KEYWORD_LEN: usize = 128;
const MAX_SEARCH_DOCS_QUERY_LEN: usize = 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
enum SearchDocsValidationError {
    EmptyKeywords,
    TooManyKeywords {
        field: &'static str,
        actual: usize,
        max: usize,
    },
    KeywordTooLong {
        field: &'static str,
        actual: usize,
        max: usize,
    },
    QueryTooLong {
        field: &'static str,
        actual: usize,
        max: usize,
    },
}

fn validate_search_docs_keywords(
    field: &'static str,
    keywords: &[String],
) -> Result<(), SearchDocsValidationError> {
    if keywords.len() > MAX_SEARCH_DOCS_KEYWORDS {
        return Err(SearchDocsValidationError::TooManyKeywords {
            field,
            actual: keywords.len(),
            max: MAX_SEARCH_DOCS_KEYWORDS,
        });
    }

    if let Some(actual) = keywords
        .iter()
        .map(|keyword| keyword.chars().count())
        .max()
        .filter(|actual| *actual > MAX_SEARCH_DOCS_KEYWORD_LEN)
    {
        return Err(SearchDocsValidationError::KeywordTooLong {
            field,
            actual,
            max: MAX_SEARCH_DOCS_KEYWORD_LEN,
        });
    }

    let actual = keywords
        .iter()
        .map(|keyword| keyword.chars().count())
        .sum::<usize>()
        + keywords.len().saturating_sub(1);
    if actual > MAX_SEARCH_DOCS_QUERY_LEN {
        return Err(SearchDocsValidationError::QueryTooLong {
            field,
            actual,
            max: MAX_SEARCH_DOCS_QUERY_LEN,
        });
    }

    Ok(())
}

fn build_search_docs_api_request(
    request: SearchDocsRequest,
) -> Result<ApiSearchDocsRequest, SearchDocsValidationError> {
    let keywords = normalize_keyword_input(request.keywords);
    if keywords.is_empty() {
        return Err(SearchDocsValidationError::EmptyKeywords);
    }

    validate_search_docs_keywords("keywords", &keywords)?;

    let exclude_keywords = normalize_optional_keyword_input(request.exclude_keywords);
    if let Some(exclude_keywords_ref) = exclude_keywords.as_ref() {
        validate_search_docs_keywords("exclude_keywords", exclude_keywords_ref)?;
    }

    Ok(ApiSearchDocsRequest {
        keywords: keywords.join(" "),
        exclude_keywords: exclude_keywords.map(|items| items.join(" ")),
        limit: request.limit,
    })
}

fn search_docs_validation_error_payload(
    error: SearchDocsValidationError,
) -> (&'static str, String) {
    match error {
        SearchDocsValidationError::EmptyKeywords => (
            "INVALID_SEARCH_DOCS_KEYWORDS",
            "keywords must contain at least one non-empty term (array format preferred)."
                .to_string(),
        ),
        SearchDocsValidationError::TooManyKeywords { field, actual, max } => (
            "INVALID_SEARCH_DOCS_KEYWORD_COUNT",
            format!(
                "{} contains {} keywords, but the maximum is {}.",
                field, actual, max
            ),
        ),
        SearchDocsValidationError::KeywordTooLong { field, actual, max } => (
            "INVALID_SEARCH_DOCS_KEYWORD_LENGTH",
            format!(
                "{} contains a keyword with length {}, but the maximum is {} characters.",
                field, actual, max
            ),
        ),
        SearchDocsValidationError::QueryTooLong { field, actual, max } => (
            "INVALID_SEARCH_DOCS_QUERY_LENGTH",
            format!(
                "{} joined query length is {}, but the maximum is {} characters.",
                field, actual, max
            ),
        ),
    }
}

fn search_docs_validation_error_result(error: SearchDocsValidationError) -> CallToolResult {
    let (code, message) = search_docs_validation_error_payload(error);
    CallToolResult::error(vec![Content::text(code), Content::text(message)])
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SearchMemoryRequest {
    #[schemars(
        description = "Space-separated keywords to search for in your memory (e.g., 'kubernetes deployment config'). Searches against the title, tags, and content of your memory."
    )]
    pub keywords: String,
    #[schemars(
        description = "Start time for filtering memories by creation time (inclusive range, ISO 8601 format)"
    )]
    pub start_time: Option<DateTime<Utc>>,
    #[schemars(
        description = "End time for filtering memories by creation time (inclusive range, ISO 8601 format)"
    )]
    pub end_time: Option<DateTime<Utc>>,
}

impl From<SearchMemoryRequest> for ApiSearchMemoryRequest {
    fn from(req: SearchMemoryRequest) -> Self {
        Self {
            keywords: req
                .keywords
                .split_whitespace()
                .map(|s| s.to_string())
                .collect(),
            start_time: req.start_time,
            end_time: req.end_time,
        }
    }
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct LoadSkillRequest {
    #[schemars(
        description = "The URI of the skill to load. For local skills this is the file path; for remote skills (rulebooks) this is the rulebook URI."
    )]
    pub uri: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct LocalCodeSearchRequest {
    #[schemars(
        description = "Space-separated keywords to search for in code blocks (e.g., 'kubernetes service deployment'). Searches against block names, types, content, and file paths. Blocks matching multiple keywords will be ranked higher than those matching only one keyword."
    )]
    pub keywords: String,
    #[schemars(description = "Maximum number of results to return (default: 10)")]
    pub limit: Option<u32>,
    #[schemars(
        description = "Whether to show dependencies and dependents for each matching block (default: false)"
    )]
    pub show_dependencies: Option<bool>,
}

#[tool_router(router = tool_router_remote, vis = "pub")]
impl ToolContainer {
    #[tool(
        description = "Web search for technical documentation. This includes documentation for tools, cloud providers, development frameworks, release notes, and other technical resources. searches against the url, title, description, and content of documentation chunks.
KEYWORD FORMAT REQUIREMENTS:
- Preferred format: JSON array of strings
- Backward compatibility: legacy space-separated keyword strings are still accepted
- Use hyphens for compound terms (e.g., 'cloud-native', 'service-mesh')
- Include explicit version terms when the user specifies one; otherwise add 'latest'

CORRECT EXAMPLES:
✅ keywords: [\"stakpak\", \"cli\", \"latest\"]
✅ keywords: [\"kubernetes\", \"ingress\", \"nginx\", \"ssl\"]
✅ keywords: [\"docker\", \"multi-stage\", \"build\"]
✅ legacy keywords: \"kubernetes ingress nginx ssl\"

QUERY STRATEGY GUIDANCE:
- For more fine-grained queries: Use many keywords in a single call to get highly targeted results (e.g., [\"kubernetes\", \"ingress\", \"nginx\", \"ssl\", \"tls\"] for a specific SSL setup question)
- For broader knowledge gathering: Break down your query into multiple parallel calls with fewer keywords each to cover more ground (e.g., separate calls for [\"kubernetes\", \"networking\"], [\"kubernetes\", \"storage\"], [\"kubernetes\", \"security\"] instead of cramming all topics into one call)

If your goal requires understanding multiple distinct topics or technologies, make separate search calls rather than combining all keywords into one overly-specific search that may miss relevant documentation."
    )]
    pub async fn search_docs(
        &self,
        Parameters(mut request): Parameters<SearchDocsRequest>,
    ) -> Result<CallToolResult, McpError> {
        request.limit = request.limit.map(|l| l.min(5)).or(Some(5));

        let api_request = match build_search_docs_api_request(request) {
            Ok(req) => req,
            Err(error) => return Ok(search_docs_validation_error_result(error)),
        };

        let client = match self.get_client() {
            Some(client) => client,
            None => {
                return Ok(CallToolResult::error(vec![
                    Content::text("CLIENT_NOT_FOUND"),
                    Content::text("Client not found"),
                ]));
            }
        };

        let response = match client.search_docs(&api_request).await {
            Ok(response) => response,
            Err(e) => {
                return Ok(CallToolResult::error(vec![
                    Content::text("SEARCH_DOCS_ERROR"),
                    Content::text(format!("Failed to search for docs: {}", e)),
                ]));
            }
        };

        const MAX_LINES: usize = 600;

        let mut remaining_lines = MAX_LINES;
        let mut remaining_items = response.len();

        let processed: Vec<Content> = response
            .into_iter()
            .map(|c| {
                // Compute this element's allowance at the last possible moment
                let allowance = if remaining_items > 0 {
                    (remaining_lines / remaining_items).max(1)
                } else {
                    1
                };

                remaining_items = remaining_items.saturating_sub(1);

                if let Some(RawTextContent { text, meta: None }) = c.as_text() {
                    let sanitized = sanitize_text_output(text);
                    match handle_large_output(&sanitized, "search", allowance, true) {
                        Ok(final_text) => {
                            // Estimate consumption (best-effort)
                            let used = final_text.lines().count().min(remaining_lines);
                            remaining_lines = remaining_lines.saturating_sub(used);
                            Content::text(final_text)
                        }
                        Err(e) => Content::text(format!("FAILED_TO_HANDLE_LARGE_OUTPUT: {}", e)),
                    }
                } else {
                    c
                }
            })
            .collect();

        Ok(CallToolResult::success(processed))
    }

    #[tool(
        description = "Search your memory for relevant information from previous conversations and code generation steps to accelerate request fulfillment."
    )]
    pub async fn search_memory(
        &self,
        Parameters(request): Parameters<SearchMemoryRequest>,
    ) -> Result<CallToolResult, McpError> {
        let client = match self.get_client() {
            Some(client) => client,
            None => {
                return Ok(CallToolResult::error(vec![
                    Content::text("CLIENT_NOT_FOUND"),
                    Content::text("Client not found"),
                ]));
            }
        };

        let response = match client.search_memory(&request.into()).await {
            Ok(response) => response,
            Err(e) => {
                return Ok(CallToolResult::error(vec![
                    Content::text("SEARCH_MEMORY_ERROR"),
                    Content::text(format!("Failed to search for memory: {}", e)),
                ]));
            }
        };

        Ok(CallToolResult::success(response))
    }

    #[tool(
        description = "Load a skill's full instructions by its URI. Use this to retrieve the complete content of any skill listed in the <available_skills> block. For local skills the URI is a file path; for remote skills the URI is the rulebook URI. This tool is auto-approved and does not require user confirmation."
    )]
    pub async fn load_skill(
        &self,
        Parameters(LoadSkillRequest { uri }): Parameters<LoadSkillRequest>,
    ) -> Result<CallToolResult, McpError> {
        // Try loading as a local skill first (URI is a file path to SKILL.md)
        let path = std::path::Path::new(&uri);
        if path.exists() && path.is_file() {
            // Restrict to configured skill directories
            let is_allowed = self.skill_directories.iter().any(|dir| {
                if let (Ok(abs_path), Ok(abs_dir)) =
                    (std::fs::canonicalize(path), std::fs::canonicalize(dir))
                {
                    abs_path.starts_with(abs_dir)
                } else {
                    false
                }
            });

            if is_allowed {
                match stakpak_api::local::skills::load_skill_from_path(path) {
                    Ok((skill_dir, body)) => {
                        let response =
                            format!("Skill directory: {}\n\n{}", skill_dir.display(), body);
                        return Ok(CallToolResult::success(vec![Content::text(response)]));
                    }
                    Err(e) => {
                        return Ok(CallToolResult::error(vec![
                            Content::text("LOAD_SKILL_ERROR"),
                            Content::text(format!("Failed to load local skill: {}", e)),
                        ]));
                    }
                }
            }
        }

        // Fall back to remote rulebook fetch
        let client = match self.get_client() {
            Some(client) => client,
            None => {
                return Ok(CallToolResult::error(vec![
                    Content::text("LOAD_SKILL_ERROR"),
                    Content::text("Skill not found locally and no API client available"),
                ]));
            }
        };

        match client.get_rulebook_by_uri(&uri).await {
            Ok(rulebook) => Ok(CallToolResult::success(vec![Content::text(
                rulebook.content,
            )])),
            Err(e) => Ok(CallToolResult::error(vec![
                Content::text("LOAD_SKILL_ERROR"),
                Content::text(format!("Failed to load skill: {}", e)),
            ])),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_SEARCH_DOCS_KEYWORD_LEN, MAX_SEARCH_DOCS_KEYWORDS, SearchDocsRequest,
        SearchDocsValidationError, build_search_docs_api_request, normalize_keyword_input,
        normalize_optional_keyword_input, search_docs_validation_error_payload,
    };

    #[test]
    fn search_docs_accepts_legacy_string_keywords() {
        let request: SearchDocsRequest = serde_json::from_value(serde_json::json!({
            "keywords": "stakpak cli latest",
            "exclude_keywords": "deprecated legacy"
        }))
        .expect("legacy string keyword format should deserialize");

        assert_eq!(
            normalize_keyword_input(request.keywords),
            vec!["stakpak", "cli", "latest"]
        );
        assert_eq!(
            normalize_optional_keyword_input(request.exclude_keywords),
            Some(vec!["deprecated".to_string(), "legacy".to_string()])
        );
    }

    #[test]
    fn search_docs_accepts_array_keywords_and_normalizes_blanks() {
        let request: SearchDocsRequest = serde_json::from_value(serde_json::json!({
            "keywords": ["  stakpak  ", "", "cli", "   ", "latest"],
            "exclude_keywords": ["", " legacy "]
        }))
        .expect("array keyword format should deserialize");

        assert_eq!(
            normalize_keyword_input(request.keywords),
            vec!["stakpak", "cli", "latest"]
        );
        assert_eq!(
            normalize_optional_keyword_input(request.exclude_keywords),
            Some(vec!["legacy".to_string()])
        );
    }

    #[test]
    fn search_docs_preserves_empty_keywords_for_runtime_validation() {
        let request: SearchDocsRequest =
            serde_json::from_value(serde_json::json!({ "keywords": ["", "   "] }))
                .expect("empty keyword arrays should deserialize for explicit validation");

        assert!(normalize_keyword_input(request.keywords).is_empty());
    }

    #[test]
    fn search_docs_runtime_validation_rejects_empty_keywords() {
        let request: SearchDocsRequest =
            serde_json::from_value(serde_json::json!({ "keywords": ["", "   "] }))
                .expect("request should deserialize");

        let result = build_search_docs_api_request(request);
        assert!(matches!(
            result,
            Err(SearchDocsValidationError::EmptyKeywords)
        ));
    }

    #[test]
    fn search_docs_runtime_builds_normalized_api_payload() {
        let request: SearchDocsRequest = serde_json::from_value(serde_json::json!({
            "keywords": ["  stakpak  ", "cli", "", "latest"],
            "exclude_keywords": ["", " deprecated ", "  "]
        }))
        .expect("request should deserialize");

        let payload = build_search_docs_api_request(request).expect("payload should build");
        assert_eq!(payload.keywords, "stakpak cli latest");
        assert_eq!(payload.exclude_keywords, Some("deprecated".to_string()));
    }

    #[test]
    fn search_docs_runtime_drops_empty_exclude_keywords() {
        let request: SearchDocsRequest = serde_json::from_value(serde_json::json!({
            "keywords": "stakpak cli latest",
            "exclude_keywords": ["", "   "]
        }))
        .expect("request should deserialize");

        let payload = build_search_docs_api_request(request).expect("payload should build");
        assert_eq!(payload.keywords, "stakpak cli latest");
        assert_eq!(payload.exclude_keywords, None);
    }

    #[test]
    fn search_docs_runtime_rejects_too_many_keywords() {
        let keywords: Vec<String> = (0..=MAX_SEARCH_DOCS_KEYWORDS)
            .map(|idx| format!("k{}", idx))
            .collect();

        let request: SearchDocsRequest = serde_json::from_value(serde_json::json!({
            "keywords": keywords,
        }))
        .expect("request should deserialize");

        let result = build_search_docs_api_request(request);
        assert!(matches!(
            result,
            Err(SearchDocsValidationError::TooManyKeywords {
                field: "keywords",
                ..
            })
        ));
    }

    #[test]
    fn search_docs_runtime_rejects_overlong_keyword() {
        let overlong_keyword = "x".repeat(MAX_SEARCH_DOCS_KEYWORD_LEN + 1);
        let request: SearchDocsRequest = serde_json::from_value(serde_json::json!({
            "keywords": [overlong_keyword],
        }))
        .expect("request should deserialize");

        let result = build_search_docs_api_request(request);
        assert!(matches!(
            result,
            Err(SearchDocsValidationError::KeywordTooLong {
                field: "keywords",
                ..
            })
        ));
    }

    #[test]
    fn search_docs_runtime_rejects_overlong_joined_query() {
        let token = "x".repeat(MAX_SEARCH_DOCS_KEYWORD_LEN.saturating_sub(8));
        let keywords: Vec<String> = (0..10).map(|_| token.clone()).collect();

        let request: SearchDocsRequest = serde_json::from_value(serde_json::json!({
            "keywords": keywords,
        }))
        .expect("request should deserialize");

        let result = build_search_docs_api_request(request);
        assert!(matches!(
            result,
            Err(SearchDocsValidationError::QueryTooLong {
                field: "keywords",
                ..
            })
        ));
    }

    #[test]
    fn search_docs_validation_payload_empty_keywords_mapping() {
        let (code, message) =
            search_docs_validation_error_payload(SearchDocsValidationError::EmptyKeywords);
        assert_eq!(code, "INVALID_SEARCH_DOCS_KEYWORDS");
        assert!(message.contains("at least one non-empty term"));
    }

    #[test]
    fn search_docs_validation_payload_count_mapping() {
        let (code, message) =
            search_docs_validation_error_payload(SearchDocsValidationError::TooManyKeywords {
                field: "keywords",
                actual: 99,
                max: 16,
            });
        assert_eq!(code, "INVALID_SEARCH_DOCS_KEYWORD_COUNT");
        assert!(message.contains("keywords contains 99 keywords"));
        assert!(message.contains("maximum is 16"));
    }

    #[test]
    fn search_docs_validation_payload_keyword_len_mapping() {
        let (code, message) =
            search_docs_validation_error_payload(SearchDocsValidationError::KeywordTooLong {
                field: "exclude_keywords",
                actual: 140,
                max: 128,
            });
        assert_eq!(code, "INVALID_SEARCH_DOCS_KEYWORD_LENGTH");
        assert!(message.contains("exclude_keywords"));
        assert!(message.contains("length 140"));
    }

    #[test]
    fn search_docs_validation_payload_query_len_mapping() {
        let (code, message) =
            search_docs_validation_error_payload(SearchDocsValidationError::QueryTooLong {
                field: "keywords",
                actual: 2048,
                max: 1024,
            });
        assert_eq!(code, "INVALID_SEARCH_DOCS_QUERY_LENGTH");
        assert!(message.contains("joined query length is 2048"));
        assert!(message.contains("maximum is 1024"));
    }
}
