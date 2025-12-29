//! Gemini-specific types

use serde::{Deserialize, Serialize};

/// Configuration for Gemini provider
#[derive(Debug, Clone)]
pub struct GeminiConfig {
    /// API key
    pub api_key: String,
    /// Base URL (default: https://generativelanguage.googleapis.com/v1beta)
    pub base_url: String,
}

impl GeminiConfig {
    /// Create new config with API key
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            base_url: "https://generativelanguage.googleapis.com/v1beta".to_string(),
        }
    }

    /// Set base URL
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }
}

impl Default for GeminiConfig {
    fn default() -> Self {
        Self::new(std::env::var("GEMINI_API_KEY").unwrap_or_else(|_| String::new()))
    }
}

/// Gemini generate content request
#[derive(Debug, Serialize)]
pub struct GeminiRequest {
    pub contents: Vec<GeminiContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generation_config: Option<GeminiGenerationConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub safety_settings: Option<Vec<GeminiSafetySetting>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_config: Option<serde_json::Value>,
}

/// Gemini content
#[derive(Debug, Serialize, Deserialize)]
pub struct GeminiContent {
    pub role: String,
    pub parts: Vec<GeminiPart>,
}

/// Gemini content part
#[derive(Debug, Serialize, Deserialize)]
pub struct GeminiPart {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub inline_data: Option<GeminiInlineData>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub function_call: Option<GeminiFunctionCall>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub function_response: Option<GeminiFunctionResponse>,
}

/// Gemini function call
#[derive(Debug, Serialize, Deserialize)]
pub struct GeminiFunctionCall {
    pub name: String,
    pub args: serde_json::Value,
}

/// Gemini function response
#[derive(Debug, Serialize, Deserialize)]
pub struct GeminiFunctionResponse {
    pub name: String,
    pub response: serde_json::Value,
}

/// Gemini inline data (for images)
#[derive(Debug, Serialize, Deserialize)]
pub struct GeminiInlineData {
    pub mime_type: String,
    pub data: String, // base64 encoded
}

/// Gemini generation configuration
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GeminiGenerationConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_sequences: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_mime_type: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidate_count: Option<u32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<i64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub presence_penalty: Option<f32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub frequency_penalty: Option<f32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_logprobs: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub logprobs: Option<u32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable_enhanced_civic_answers: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_config: Option<GeminiThinkingConfig>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub speech_config: Option<GeminiSpeechConfig>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_resolution: Option<GeminiMediaResolution>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_modalities: Option<Vec<GeminiModality>>,
}

/// Gemini thinking configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiThinkingConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_thoughts: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_budget: Option<u32>,
}

/// Gemini speech configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiSpeechConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub voice_config: Option<GeminiVoiceConfig>,
}

/// Gemini voice configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiVoiceConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prebuilt_voice_config: Option<GeminiPrebuiltVoiceConfig>,
}

/// Gemini prebuilt voice configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiPrebuiltVoiceConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub voice_name: Option<String>,
}

/// Gemini media resolution
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum GeminiMediaResolution {
    MediaResolutionUnspecified,
    MediaResolutionLow,
    MediaResolutionMedium,
    MediaResolutionHigh,
}

/// Gemini modality
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum GeminiModality {
    ModalityUnspecified,
    Text,
    Image,
    Audio,
}

/// Gemini safety setting
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiSafetySetting {
    pub category: GeminiHarmCategory,
    pub threshold: GeminiHarmBlockThreshold,
}

/// Gemini harm category
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum GeminiHarmCategory {
    HarmCategoryUnspecified,
    HarmCategoryHarassment,
    HarmCategoryHateSpeech,
    HarmCategorySexuallyExplicit,
    HarmCategoryDangerousContent,
    HarmCategoryCivicIntegrity,
}

/// Gemini harm block threshold
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum GeminiHarmBlockThreshold {
    HarmBlockThresholdUnspecified,
    BlockLowAndAbove,
    BlockMediumAndAbove,
    BlockOnlyHigh,
    BlockNone,
    Off,
}

/// Gemini response
#[derive(Debug, Deserialize)]
pub struct GeminiResponse {
    pub candidates: Vec<GeminiCandidate>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage_metadata: Option<GeminiUsageMetadata>,
}

/// Gemini candidate
#[derive(Debug, Deserialize)]
pub struct GeminiCandidate {
    pub content: GeminiContent,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub safety_ratings: Option<Vec<GeminiSafetyRating>>,
}

/// Gemini safety rating
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiSafetyRating {
    pub category: GeminiHarmCategory,
    pub probability: GeminiHarmProbability,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocked: Option<bool>,
}

/// Gemini harm probability
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum GeminiHarmProbability {
    HarmProbabilityUnspecified,
    Negligible,
    Low,
    Medium,
    High,
}

/// Gemini usage metadata
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiUsageMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_token_count: Option<u32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub cached_content_token_count: Option<u32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidates_token_count: Option<u32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_token_count: Option<u32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_tokens_details: Option<Vec<GeminiTokenDetail>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidates_tokens_details: Option<Vec<GeminiTokenDetail>>,
}

/// Gemini token detail
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiTokenDetail {
    pub modality: GeminiModality,
    pub token_count: u32,
}
