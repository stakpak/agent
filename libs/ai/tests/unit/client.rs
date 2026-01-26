//! Unit tests for client

use stakai::providers::openai::{OpenAIConfig, OpenAIProvider};
use stakai::registry::ProviderRegistry;
use stakai::{GenerateRequest, Inference, Message, Model, Role};

#[test]
fn test_client_creation() {
    let client = Inference::new();
    // Inference should be created successfully even without providers
    let _ = client.registry().list_providers().len();
}

#[test]
fn test_client_builder() {
    let client = Inference::builder()
        .with_temperature(0.7)
        .with_max_tokens(100)
        .build()
        .unwrap();

    // Inference should be created successfully
    let _ = client.registry().list_providers().len();
}

#[test]
fn test_client_default() {
    let client = Inference::default();
    // Inference should be created successfully
    let _ = client.registry().list_providers().len();
}

#[test]
fn test_client_with_custom_provider() {
    // Create a client with a manually registered provider
    let provider = OpenAIProvider::new(OpenAIConfig::new("test-key")).unwrap();
    let client = Inference::builder()
        .register_provider("openai", provider)
        .build()
        .unwrap();

    assert!(client.registry().has_provider("openai"));
    assert_eq!(client.registry().list_providers().len(), 1);
}

#[test]
fn test_registry_list_providers() {
    let provider = OpenAIProvider::new(OpenAIConfig::new("test-key")).unwrap();
    let registry = ProviderRegistry::new().register("openai", provider);

    let providers = registry.list_providers();
    assert!(providers.contains(&"openai".to_string()));
}

#[test]
fn test_request_creation() {
    let mut request = GenerateRequest::new(
        Model::custom("gpt-4", "openai"),
        vec![
            Message {
                role: Role::System,
                content: "You are helpful".into(),
                name: None,
                provider_options: None,
            },
            Message {
                role: Role::User,
                content: "Hello".into(),
                name: None,
                provider_options: None,
            },
        ],
    );
    request.options.temperature = Some(0.7);
    request.options.max_tokens = Some(100);

    assert_eq!(request.messages.len(), 2);
    assert_eq!(request.options.temperature, Some(0.7));
    assert_eq!(request.options.max_tokens, Some(100));
}

#[test]
fn test_request_with_model() {
    let request = GenerateRequest::new(
        Model::custom("gpt-4", "openai"),
        vec![Message {
            role: Role::User,
            content: "Hello".into(),
            name: None,
            provider_options: None,
        }],
    );

    assert_eq!(request.model.id, "gpt-4");
    assert_eq!(request.messages.len(), 1);
}

#[test]
fn test_request_multiple_messages() {
    let messages = vec![
        Message {
            role: Role::System,
            content: "You are helpful".into(),
            name: None,
            provider_options: None,
        },
        Message {
            role: Role::User,
            content: "Hello".into(),
            name: None,
            provider_options: None,
        },
    ];

    let request = GenerateRequest::new(Model::custom("gpt-4", "openai"), messages);

    assert_eq!(request.messages.len(), 2);
}

#[test]
fn test_provider_registry_creation() {
    let registry = ProviderRegistry::new();
    assert_eq!(registry.list_providers().len(), 0);
}

#[test]
fn test_provider_registry_default() {
    let registry = ProviderRegistry::default();
    // Default registry may or may not have providers depending on environment
    let _ = registry.list_providers().len();
}

#[test]
fn test_provider_registry_manual_registration() {
    let provider = OpenAIProvider::new(OpenAIConfig::new("test-key")).unwrap();
    let registry = ProviderRegistry::new().register("openai", provider);

    assert_eq!(registry.list_providers().len(), 1);
    assert!(registry.has_provider("openai"));
}
