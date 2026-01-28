//! Unit tests for TLS client integration in providers
//!
//! Verifies that all providers construct successfully using the platform-verified
//! TLS client, and that the resulting providers can be registered and used.

use stakai::providers::anthropic::{AnthropicConfig, AnthropicProvider};
use stakai::providers::gemini::{GeminiConfig, GeminiProvider};
use stakai::providers::openai::{OpenAIConfig, OpenAIProvider};
use stakai::providers::stakpak::{StakpakProvider, StakpakProviderConfig};
use stakai::registry::ProviderRegistry;
use stakai::{Inference, InferenceConfig};

// --- Provider construction with TLS ---

#[test]
fn test_openai_provider_creates_with_tls_client() {
    let provider = OpenAIProvider::new(OpenAIConfig::new("test-key"));
    assert!(
        provider.is_ok(),
        "OpenAI provider should create successfully with TLS client"
    );
}

#[test]
fn test_openai_provider_custom_base_url_creates_with_tls_client() {
    let config = OpenAIConfig::new("test-key").with_base_url("https://custom.openai.example.com");
    let provider = OpenAIProvider::new(config);
    assert!(
        provider.is_ok(),
        "OpenAI provider with custom base URL should create successfully with TLS client"
    );
}

#[test]
fn test_openai_provider_empty_key_custom_url_creates_with_tls_client() {
    // OpenAI allows empty API key when using custom base URL (e.g., Ollama)
    let config = OpenAIConfig::new("").with_base_url("http://localhost:11434/v1");
    let provider = OpenAIProvider::new(config);
    assert!(
        provider.is_ok(),
        "OpenAI provider with empty key and custom URL should create successfully"
    );
}

#[test]
fn test_anthropic_provider_creates_with_tls_client() {
    let provider = AnthropicProvider::new(AnthropicConfig::new("test-key"));
    assert!(
        provider.is_ok(),
        "Anthropic provider should create successfully with TLS client"
    );
}

#[test]
fn test_anthropic_provider_custom_base_url_creates_with_tls_client() {
    let config =
        AnthropicConfig::new("test-key").with_base_url("https://custom.anthropic.example.com/v1/");
    let provider = AnthropicProvider::new(config);
    assert!(
        provider.is_ok(),
        "Anthropic provider with custom base URL should create successfully with TLS client"
    );
}

#[test]
fn test_gemini_provider_creates_with_tls_client() {
    let provider = GeminiProvider::new(GeminiConfig::new("test-key"));
    assert!(
        provider.is_ok(),
        "Gemini provider should create successfully with TLS client"
    );
}

#[test]
fn test_gemini_provider_custom_base_url_creates_with_tls_client() {
    let config =
        GeminiConfig::new("test-key").with_base_url("https://custom.gemini.example.com/v1beta/");
    let provider = GeminiProvider::new(config);
    assert!(
        provider.is_ok(),
        "Gemini provider with custom base URL should create successfully with TLS client"
    );
}

#[test]
fn test_stakpak_provider_creates_with_tls_client() {
    let provider = StakpakProvider::new(StakpakProviderConfig::new("test-key"));
    assert!(
        provider.is_ok(),
        "Stakpak provider should create successfully with TLS client"
    );
}

#[test]
fn test_stakpak_provider_custom_base_url_creates_with_tls_client() {
    let config =
        StakpakProviderConfig::new("test-key").with_base_url("https://custom.stakpak.example.com");
    let provider = StakpakProvider::new(config);
    assert!(
        provider.is_ok(),
        "Stakpak provider with custom base URL should create successfully with TLS client"
    );
}

// --- Missing API key validation still works with TLS ---

#[test]
fn test_openai_provider_rejects_empty_key_default_url() {
    let provider = OpenAIProvider::new(OpenAIConfig::new(""));
    assert!(
        provider.is_err(),
        "OpenAI provider should reject empty key with default URL"
    );
}

#[test]
fn test_anthropic_provider_rejects_empty_key() {
    let provider = AnthropicProvider::new(AnthropicConfig::new(""));
    assert!(
        provider.is_err(),
        "Anthropic provider should reject empty API key"
    );
}

#[test]
fn test_gemini_provider_rejects_empty_key() {
    let provider = GeminiProvider::new(GeminiConfig::new(""));
    assert!(
        provider.is_err(),
        "Gemini provider should reject empty API key"
    );
}

#[test]
fn test_stakpak_provider_rejects_empty_key() {
    let provider = StakpakProvider::new(StakpakProviderConfig::new(""));
    assert!(
        provider.is_err(),
        "Stakpak provider should reject empty API key"
    );
}

// --- Multiple providers can be created concurrently (TLS client reuse) ---

#[test]
fn test_all_providers_create_concurrently() {
    let openai = OpenAIProvider::new(OpenAIConfig::new("test-key"));
    let anthropic = AnthropicProvider::new(AnthropicConfig::new("test-key"));
    let gemini = GeminiProvider::new(GeminiConfig::new("test-key"));
    let stakpak = StakpakProvider::new(StakpakProviderConfig::new("test-key"));

    assert!(openai.is_ok(), "OpenAI provider creation failed");
    assert!(anthropic.is_ok(), "Anthropic provider creation failed");
    assert!(gemini.is_ok(), "Gemini provider creation failed");
    assert!(stakpak.is_ok(), "Stakpak provider creation failed");
}

#[test]
fn test_multiple_instances_of_same_provider() {
    // Creating multiple instances should not conflict on TLS/crypto provider init
    let providers: Vec<_> = (0..5)
        .map(|i| OpenAIProvider::new(OpenAIConfig::new(format!("test-key-{}", i))))
        .collect();

    for (i, provider) in providers.iter().enumerate() {
        assert!(
            provider.is_ok(),
            "OpenAI provider instance {} should create successfully",
            i
        );
    }
}

// --- Registry integration with TLS providers ---

#[test]
fn test_register_all_tls_providers_in_registry() {
    let openai = OpenAIProvider::new(OpenAIConfig::new("test-key")).unwrap();
    let anthropic = AnthropicProvider::new(AnthropicConfig::new("test-key")).unwrap();
    let gemini = GeminiProvider::new(GeminiConfig::new("test-key")).unwrap();
    let stakpak = StakpakProvider::new(StakpakProviderConfig::new("test-key")).unwrap();

    let registry = ProviderRegistry::new()
        .register("openai", openai)
        .register("anthropic", anthropic)
        .register("google", gemini)
        .register("stakpak", stakpak);

    assert_eq!(registry.list_providers().len(), 4);
    assert!(registry.has_provider("openai"));
    assert!(registry.has_provider("anthropic"));
    assert!(registry.has_provider("google"));
    assert!(registry.has_provider("stakpak"));
}

#[test]
fn test_inference_config_creates_all_providers_with_tls() {
    let client = Inference::with_config(
        InferenceConfig::new()
            .openai("test-key", None)
            .anthropic("test-key", None)
            .gemini("test-key", None)
            .stakpak("test-key", None),
    )
    .expect("Inference client should be created with all TLS providers");

    let providers = client.registry().list_providers();
    assert_eq!(providers.len(), 4, "All 4 providers should be registered");
    assert!(providers.contains(&"openai".to_string()));
    assert!(providers.contains(&"anthropic".to_string()));
    assert!(providers.contains(&"google".to_string()));
    assert!(providers.contains(&"stakpak".to_string()));
}

#[test]
fn test_inference_builder_creates_providers_with_tls() {
    let client = Inference::builder()
        .with_inference_config(
            InferenceConfig::new()
                .openai("test-key", None)
                .anthropic("test-key", None)
                .gemini("test-key", None)
                .stakpak("test-key", None),
        )
        .build()
        .unwrap();

    assert_eq!(client.registry().list_providers().len(), 4);
}
