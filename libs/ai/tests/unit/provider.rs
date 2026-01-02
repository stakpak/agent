//! Unit tests for provider system

use stakai::provider::ProviderKind;

#[test]
fn test_provider_kind_from_str() {
    assert_eq!(
        "openai".parse::<ProviderKind>().unwrap(),
        ProviderKind::OpenAI
    );
    assert_eq!(
        "OpenAI".parse::<ProviderKind>().unwrap(),
        ProviderKind::OpenAI
    );
    assert_eq!(
        "anthropic".parse::<ProviderKind>().unwrap(),
        ProviderKind::Anthropic
    );
    assert_eq!(
        "google".parse::<ProviderKind>().unwrap(),
        ProviderKind::Google
    );
    assert_eq!(
        "gemini".parse::<ProviderKind>().unwrap(),
        ProviderKind::Google
    );
}

#[test]
fn test_provider_kind_from_str_invalid() {
    assert!("invalid".parse::<ProviderKind>().is_err());
}

#[test]
fn test_provider_kind_as_str() {
    assert_eq!(ProviderKind::OpenAI.as_str(), "openai");
    assert_eq!(ProviderKind::Anthropic.as_str(), "anthropic");
    assert_eq!(ProviderKind::Google.as_str(), "google");
}
