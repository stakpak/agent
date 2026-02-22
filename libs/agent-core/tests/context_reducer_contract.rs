use serde_json::json;
use stakai::{ContentPart, Message, MessageContent, Model, ModelLimit, Role};
use stakpak_agent_core::{
    AgentLoopResult, ContextConfig, ContextReducer, DefaultContextReducer, StopReason,
    reduce_context,
};
use uuid::Uuid;

fn test_model() -> Model {
    Model::new(
        "claude-sonnet-test",
        "Claude Sonnet Test",
        "anthropic",
        false,
        None,
        ModelLimit::new(200_000, 8192),
    )
}

fn tool_call_message(id: &str) -> Message {
    Message {
        role: Role::Assistant,
        content: MessageContent::Parts(vec![ContentPart::tool_call(
            id.to_string(),
            "stakpak__view",
            json!({"path":"README.md"}),
        )]),
        name: None,
        provider_options: None,
    }
}

fn tool_result_message(id: &str, value: &str) -> Message {
    Message {
        role: Role::Tool,
        content: MessageContent::Parts(vec![ContentPart::tool_result(
            id.to_string(),
            json!(value),
        )]),
        name: None,
        provider_options: None,
    }
}

#[test]
fn default_reducer_matches_legacy_reduce_context_behavior() {
    let config = ContextConfig {
        keep_last_messages: 2,
    };
    let reducer = DefaultContextReducer::new(config.clone());

    let messages = vec![
        tool_call_message("tc_1"),
        tool_result_message("tc_1", "old"),
        tool_result_message("tc_1", "new"),
        Message::new(Role::Assistant, "analysis"),
    ];

    let expected = reduce_context(messages.clone(), &config);

    let mut metadata = json!({});
    let reduced = reducer.reduce(messages, &test_model(), 4096, &[], &mut metadata);

    assert_eq!(
        reduced.len(),
        expected.len(),
        "default reducer should preserve existing reduce_context semantics"
    );

    for (lhs, rhs) in reduced.iter().zip(expected.iter()) {
        assert_eq!(lhs.role, rhs.role);
        assert_eq!(lhs.text(), rhs.text());
    }
}

#[test]
fn default_reducer_does_not_mutate_metadata() {
    let reducer = DefaultContextReducer::default();
    let mut metadata = json!({"trimmed_up_to_message_index": 7});

    let _ = reducer.reduce(
        vec![Message::new(Role::User, "hello")],
        &test_model(),
        1024,
        &[],
        &mut metadata,
    );

    assert_eq!(
        metadata
            .get("trimmed_up_to_message_index")
            .and_then(serde_json::Value::as_u64),
        Some(7)
    );
}

#[test]
fn agent_loop_result_carries_metadata_for_checkpoint_persistence() {
    let run_id = Uuid::new_v4();

    let result = AgentLoopResult {
        run_id,
        total_turns: 3,
        total_usage: stakai::Usage::default(),
        stop_reason: StopReason::Completed,
        messages: vec![Message::new(Role::User, "hello")],
        metadata: json!({"trimmed_up_to_message_index": 12}),
    };

    assert_eq!(
        result
            .metadata
            .get("trimmed_up_to_message_index")
            .and_then(serde_json::Value::as_u64),
        Some(12)
    );
}
