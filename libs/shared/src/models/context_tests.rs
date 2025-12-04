#[cfg(test)]
mod tests {
    use crate::models::context::ContextAware;
    use crate::models::integrations::anthropic::AnthropicModel;
    use crate::models::integrations::gemini::GeminiModel;
    use crate::models::integrations::openai::OpenAIModel;

    #[test]
    fn test_anthropic_context_info() {
        let sonnet = AnthropicModel::Claude45Sonnet;
        let info = sonnet.context_info();
        assert_eq!(info.max_tokens, 200_000);
        assert_eq!(info.pricing_tiers.len(), 2);
        assert_eq!(info.pricing_tiers[0].input_cost_per_million, 3.0);
        assert_eq!(info.pricing_tiers[0].output_cost_per_million, 15.0);
        assert_eq!(info.pricing_tiers[0].upper_bound, Some(200_000));
        assert_eq!(info.pricing_tiers[1].input_cost_per_million, 6.0);
        assert_eq!(info.pricing_tiers[1].output_cost_per_million, 22.5);

        let haiku = AnthropicModel::Claude45Haiku;
        let info = haiku.context_info();
        assert_eq!(info.max_tokens, 200_000);
        assert_eq!(info.pricing_tiers.len(), 1);
        assert_eq!(info.pricing_tiers[0].input_cost_per_million, 1.0);
        assert_eq!(info.pricing_tiers[0].output_cost_per_million, 5.0);
    }

    #[test]
    fn test_openai_context_info() {
        let gpt5 = OpenAIModel::GPT5;
        let info = gpt5.context_info();
        assert_eq!(info.max_tokens, 400_000);
        assert_eq!(info.pricing_tiers.len(), 1);
        assert_eq!(info.pricing_tiers[0].input_cost_per_million, 1.25);
        assert_eq!(info.pricing_tiers[0].output_cost_per_million, 10.0);
    }

    #[test]
    fn test_gemini_context_info() {
        let gemini3 = GeminiModel::Gemini3Pro;
        let info = gemini3.context_info();
        assert_eq!(info.max_tokens, 2_000_000);
        assert_eq!(info.pricing_tiers.len(), 2);

        // Tier 1: <128k
        assert_eq!(info.pricing_tiers[0].upper_bound, Some(128_000));
        assert_eq!(info.pricing_tiers[0].input_cost_per_million, 0.0);

        // Tier 2: >128k
        assert_eq!(info.pricing_tiers[1].upper_bound, None);
        assert_eq!(info.pricing_tiers[1].input_cost_per_million, 2.5);
    }

    #[test]
    fn test_model_names() {
        // Anthropic
        assert_eq!(
            AnthropicModel::Claude45Sonnet.model_name(),
            "Claude 4.5 Sonnet"
        );
        assert_eq!(
            AnthropicModel::Claude45Haiku.model_name(),
            "Claude 4.5 Haiku"
        );
        assert_eq!(AnthropicModel::Claude45Opus.model_name(), "Claude 4.5 Opus");

        // OpenAI
        assert_eq!(OpenAIModel::GPT5.model_name(), "GPT-5");
        assert_eq!(OpenAIModel::GPT5Mini.model_name(), "GPT-5 Mini");
        assert_eq!(OpenAIModel::O3.model_name(), "o3");
        assert_eq!(
            OpenAIModel::Custom("my-model".to_string()).model_name(),
            "Custom (my-model)"
        );

        // Gemini
        assert_eq!(GeminiModel::Gemini3Pro.model_name(), "Gemini 3 Pro");
        assert_eq!(GeminiModel::Gemini25Flash.model_name(), "Gemini 2.5 Flash");
    }
}
