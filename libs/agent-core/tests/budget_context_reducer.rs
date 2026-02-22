use serde_json::json;
use stakai::{ContentPart, Message, MessageContent, Model, ModelLimit, Role, Tool};
use stakpak_agent_core::{BudgetAwareContextReducer, ContextReducer};

fn test_model(context_window: u64) -> Model {
    Model::new(
        "claude-sonnet-test",
        "Claude Sonnet Test",
        "anthropic",
        false,
        None,
        ModelLimit::new(context_window, 8192),
    )
}

fn user_message(text: &str) -> Message {
    Message::new(Role::User, text)
}

fn assistant_message(text: &str) -> Message {
    Message::new(Role::Assistant, text)
}

fn system_message(text: &str) -> Message {
    Message::new(Role::System, text)
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

fn text_content(msg: &Message) -> Option<String> {
    match &msg.content {
        MessageContent::Text(text) => Some(text.clone()),
        MessageContent::Parts(parts) => parts.iter().find_map(|part| {
            if let ContentPart::Text { text, .. } = part {
                Some(text.clone())
            } else {
                None
            }
        }),
    }
}

#[test]
fn under_threshold_keeps_messages_unmodified_and_metadata_unchanged() {
    let reducer = BudgetAwareContextReducer::new(2, 0.8);
    let model = test_model(200_000);

    let messages = vec![
        user_message("Hello"),
        assistant_message("Hi there"),
        user_message("How are you?"),
    ];

    let mut metadata = json!({});
    let reduced = reducer.reduce(messages.clone(), &model, 4096, &[], &mut metadata);

    assert_eq!(reduced.len(), messages.len());
    assert_eq!(reduced[0].text(), messages[0].text());
    assert_eq!(reduced[1].text(), messages[1].text());
    assert_eq!(reduced[2].text(), messages[2].text());
    assert!(
        metadata.get("trimmed_up_to_message_index").is_none(),
        "metadata should not include trim index when no trimming happened"
    );
}

#[test]
fn over_threshold_trims_assistant_and_tool_but_never_user_or_system() {
    let reducer = BudgetAwareContextReducer::new(2, 0.8);
    let model = test_model(128);

    let long_a = "A".repeat(500);
    let long_b = "B".repeat(500);

    let messages = vec![
        system_message("base system"),
        user_message("user-1"),
        assistant_message(&long_a),
        tool_call_message("tc_1"),
        tool_result_message("tc_1", &long_b),
        user_message("user-2"),
        assistant_message("recent assistant"),
    ];

    let mut metadata = json!({});
    let reduced = reducer.reduce(messages, &model, 32, &[], &mut metadata);

    assert!(
        metadata
            .get("trimmed_up_to_message_index")
            .and_then(serde_json::Value::as_u64)
            .is_some(),
        "trimming metadata should be written"
    );

    for msg in &reduced {
        match msg.role {
            Role::User | Role::System => {
                let content = text_content(msg).unwrap_or_default();
                assert_ne!(content, "[trimmed]", "user/system must never be trimmed");
            }
            Role::Assistant | Role::Tool => {}
        }
    }

    let has_trimmed = reduced
        .iter()
        .any(|msg| text_content(msg).as_deref() == Some("[trimmed]"));
    assert!(
        has_trimmed,
        "expected at least one assistant/tool message to be trimmed"
    );
}

#[test]
fn keeps_last_n_assistant_messages_untrimmed() {
    let reducer = BudgetAwareContextReducer::new(2, 0.8);
    let model = test_model(700);

    let mut messages = Vec::new();
    for i in 0..8 {
        messages.push(user_message(&format!("user-{i}")));
        messages.push(assistant_message(&format!(
            "assistant-{i}-{}",
            "X".repeat(200)
        )));
    }

    let mut metadata = json!({});
    let reduced = reducer.reduce(messages, &model, 32, &[], &mut metadata);

    let assistant_texts: Vec<String> = reduced
        .iter()
        .filter(|msg| msg.role == Role::Assistant)
        .filter_map(text_content)
        .collect();

    assert!(
        assistant_texts.len() >= 2,
        "expected at least 2 assistant messages"
    );

    let tail = &assistant_texts[assistant_texts.len() - 2..];
    assert!(
        tail.iter().all(|text| text != "[trimmed]"),
        "last 2 assistant messages should remain untrimmed"
    );
}

#[test]
fn previous_trimmed_prefix_is_reapplied_stably() {
    let reducer = BudgetAwareContextReducer::new(2, 0.8);
    let model = test_model(180);

    let mut messages = Vec::new();
    for i in 0..6 {
        messages.push(user_message(&format!("u{i}")));
        messages.push(assistant_message(&format!("a{i}-{}", "Y".repeat(220))));
    }

    let mut metadata = json!({});
    let reduced_1 = reducer.reduce(messages.clone(), &model, 32, &[], &mut metadata);

    let first_trimmed = metadata
        .get("trimmed_up_to_message_index")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0) as usize;

    assert!(first_trimmed > 0, "first reduce should trim some prefix");

    let reduced_2 = reducer.reduce(messages, &model, 32, &[], &mut metadata);
    let second_trimmed = metadata
        .get("trimmed_up_to_message_index")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0) as usize;

    assert!(
        second_trimmed >= first_trimmed,
        "trim boundary must not regress: {second_trimmed} < {first_trimmed}"
    );

    for msg in reduced_2.iter().take(first_trimmed.min(reduced_2.len())) {
        if msg.role == Role::Assistant || msg.role == Role::Tool {
            assert_eq!(
                text_content(msg).unwrap_or_default(),
                "[trimmed]",
                "prefix assistant/tool messages should remain trimmed for cache stability"
            );
        }
    }

    assert_eq!(
        reduced_1.len(),
        reduced_2.len(),
        "message structure should be stable"
    );
}

#[test]
fn tool_overhead_can_trigger_trimming_even_if_messages_are_small() {
    let reducer = BudgetAwareContextReducer::new(1, 0.8);
    let model = test_model(220);

    let messages = vec![
        user_message("small user message"),
        assistant_message("small assistant message"),
    ];

    let big_tool = Tool::function("huge_tool", "tool with massive schema").parameters(json!({
        "type":"object",
        "properties": {
            "payload": {
                "type":"string",
                "description": "Z".repeat(4000)
            }
        },
        "required":["payload"]
    }));

    let mut metadata = json!({});
    let reduced = reducer.reduce(messages, &model, 32, &[big_tool], &mut metadata);

    let maybe_trimmed = reduced
        .iter()
        .filter(|msg| msg.role == Role::Assistant || msg.role == Role::Tool)
        .filter_map(text_content)
        .any(|text| text == "[trimmed]");

    assert!(
        maybe_trimmed || metadata.get("trimmed_up_to_message_index").is_some(),
        "tool schema overhead should participate in budget decisions"
    );
}

#[test]
fn merges_consecutive_tool_messages_into_single_tool_turn() {
    let reducer = BudgetAwareContextReducer::new(5, 0.8);
    let model = test_model(200_000);

    let messages = vec![
        Message {
            role: Role::Assistant,
            content: MessageContent::Parts(vec![
                ContentPart::tool_call("tc_1", "stakpak__view", json!({"path":"README.md"})),
                ContentPart::tool_call("tc_2", "stakpak__view", json!({"path":"Cargo.toml"})),
            ]),
            name: None,
            provider_options: None,
        },
        tool_result_message("tc_1", "result-1"),
        tool_result_message("tc_2", "result-2"),
    ];

    let mut metadata = json!({});
    let reduced = reducer.reduce(messages, &model, 256, &[], &mut metadata);

    assert_eq!(
        reduced.len(),
        2,
        "tool results should be merged into one tool message"
    );
    assert_eq!(reduced[1].role, Role::Tool);

    if let MessageContent::Parts(parts) = &reduced[1].content {
        let tool_results = parts
            .iter()
            .filter(|part| matches!(part, ContentPart::ToolResult { .. }))
            .count();
        assert_eq!(
            tool_results, 2,
            "both tool results should remain in merged message"
        );
    } else {
        panic!("expected tool message with parts content");
    }
}

#[test]
fn deduplicates_tool_results_keeping_latest() {
    let reducer = BudgetAwareContextReducer::new(5, 0.8);
    let model = test_model(200_000);

    let messages = vec![
        tool_call_message("tc_1"),
        tool_result_message("tc_1", "old-value"),
        tool_result_message("tc_1", "new-value"),
    ];

    let mut metadata = json!({});
    let reduced = reducer.reduce(messages, &model, 256, &[], &mut metadata);

    assert_eq!(reduced.len(), 2);
    if let MessageContent::Parts(parts) = &reduced[1].content {
        let mut seen_new = false;
        for part in parts {
            if let ContentPart::ToolResult {
                tool_call_id,
                content,
                ..
            } = part
                && tool_call_id == "tc_1"
            {
                assert_eq!(content, &json!("new-value"));
                seen_new = true;
            }
        }
        assert!(seen_new, "latest tool result should be retained");
    } else {
        panic!("expected tool message parts");
    }
}

#[test]
fn removes_dangling_tool_calls_and_orphaned_results() {
    let reducer = BudgetAwareContextReducer::new(5, 0.8);
    let model = test_model(200_000);

    let assistant_with_tool_call = Message {
        role: Role::Assistant,
        content: MessageContent::Parts(vec![
            ContentPart::text("let me check"),
            ContentPart::tool_call("tc_1", "stakpak__view", json!({"path":"README.md"})),
        ]),
        name: None,
        provider_options: None,
    };

    let messages = vec![
        assistant_with_tool_call,
        user_message("new user input"),
        tool_result_message("tc_1", "late result"),
    ];

    let mut metadata = json!({});
    let reduced = reducer.reduce(messages, &model, 4096, &[], &mut metadata);

    assert_eq!(reduced.len(), 2, "dangling tool flow should be repaired");
    assert_eq!(reduced[0].role, Role::Assistant);
    assert_eq!(reduced[1].role, Role::User);

    if let MessageContent::Parts(parts) = &reduced[0].content {
        assert!(
            parts
                .iter()
                .all(|part| !matches!(part, ContentPart::ToolCall { .. })),
            "dangling tool_call should be stripped"
        );
    } else {
        panic!("expected assistant parts content");
    }
}

#[test]
fn trimming_preserves_assistant_tool_call_parts() {
    let reducer = BudgetAwareContextReducer::new(1, 0.8);
    let model = test_model(120);

    let messages = vec![
        Message {
            role: Role::Assistant,
            content: MessageContent::Parts(vec![
                ContentPart::text("thinking..."),
                ContentPart::tool_call("tc_1", "stakpak__view", json!({"path":"README.md"})),
            ]),
            name: None,
            provider_options: None,
        },
        tool_result_message("tc_1", &"X".repeat(350)),
        user_message("follow up"),
        assistant_message("recent"),
    ];

    let mut metadata = json!({});
    let reduced = reducer.reduce(messages, &model, 64, &[], &mut metadata);

    let first_assistant = reduced
        .iter()
        .find(|msg| msg.role == Role::Assistant)
        .expect("expected assistant message");

    if let MessageContent::Parts(parts) = &first_assistant.content {
        let has_tool_call = parts
            .iter()
            .any(|part| matches!(part, ContentPart::ToolCall { .. }));
        assert!(
            has_tool_call,
            "assistant tool_call part must be preserved when text is trimmed"
        );
    }
}

#[test]
fn estimate_tokens_handles_images_and_tool_parts_conservatively() {
    let messages = vec![Message {
        role: Role::User,
        content: MessageContent::Parts(vec![
            ContentPart::text("hello"),
            ContentPart::image("https://example.com/test.png"),
            ContentPart::tool_result("tc_1", json!("done")),
        ]),
        name: None,
        provider_options: None,
    }];

    let estimate = BudgetAwareContextReducer::estimate_tokens(&messages);

    assert!(
        estimate >= 2000,
        "image tokens should be conservatively high, got {estimate}"
    );
}

#[test]
fn unicode_content_does_not_panic_during_trim_operations() {
    let reducer = BudgetAwareContextReducer::new(1, 0.8);
    let model = test_model(120);

    let messages = vec![
        user_message("مرحبا 🌍 こんにちは Привет"),
        assistant_message(&"🚀✨漢字🙂".repeat(200)),
        user_message("tail"),
        assistant_message("recent"),
    ];

    let mut metadata = json!({});
    let reduced = reducer.reduce(messages, &model, 64, &[], &mut metadata);

    assert!(!reduced.is_empty(), "reducer should return valid messages");
}

#[test]
fn keep_last_n_zero_trims_all_assistant_messages() {
    let reducer = BudgetAwareContextReducer::new(0, 0.8);
    let model = test_model(200);

    let mut messages = Vec::new();
    for i in 0..4 {
        messages.push(user_message(&format!("u{i}")));
        messages.push(assistant_message(&format!("a{i}-{}", "W".repeat(200))));
    }

    let mut metadata = json!({});
    let reduced = reducer.reduce(messages, &model, 32, &[], &mut metadata);

    // With keep_last_n=0, all assistants are candidates for trimming
    let trimmed_assistants = reduced
        .iter()
        .filter(|msg| msg.role == Role::Assistant)
        .filter_map(text_content)
        .filter(|text| text == "[trimmed]")
        .count();

    assert!(
        trimmed_assistants > 0,
        "with keep_last_n=0, at least some assistants should be trimmed"
    );
}

#[test]
fn headroom_keeps_trim_boundary_stable_across_turns() {
    let reducer = BudgetAwareContextReducer::new(2, 0.8);
    let model = test_model(600);

    // Build a conversation that's over threshold
    let mut messages = Vec::new();
    for i in 0..10 {
        messages.push(user_message(&format!("user-{i}")));
        messages.push(assistant_message(&format!(
            "assistant-{i}-{}",
            "Z".repeat(200)
        )));
    }

    let mut metadata = json!({});

    // First reduce — establishes trim boundary
    let _ = reducer.reduce(messages.clone(), &model, 32, &[], &mut metadata);
    let first_trimmed = metadata
        .get("trimmed_up_to_message_index")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);

    assert!(first_trimmed > 0, "should trigger trimming");

    // Simulate adding one new turn (small addition)
    messages.push(user_message("new question"));
    messages.push(assistant_message("short answer"));

    let _ = reducer.reduce(messages.clone(), &model, 32, &[], &mut metadata);
    let second_trimmed = metadata
        .get("trimmed_up_to_message_index")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);

    // Simulate adding another small turn
    messages.push(user_message("another question"));
    messages.push(assistant_message("another short answer"));

    let _ = reducer.reduce(messages, &model, 32, &[], &mut metadata);
    let third_trimmed = metadata
        .get("trimmed_up_to_message_index")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);

    // With headroom, boundary should be stable for at least one of these turns
    let stable_count = [
        first_trimmed == second_trimmed,
        second_trimmed == third_trimmed,
    ]
    .iter()
    .filter(|x| **x)
    .count();

    assert!(
        stable_count >= 1,
        "headroom should keep trim boundary stable for at least 1 consecutive turn, \
         got boundaries: [{first_trimmed}, {second_trimmed}, {third_trimmed}]"
    );
}

#[test]
fn trim_boundary_never_regresses() {
    let reducer = BudgetAwareContextReducer::new(2, 0.8);
    let model = test_model(300);

    let mut messages = Vec::new();
    for i in 0..6 {
        messages.push(user_message(&format!("u{i}")));
        messages.push(assistant_message(&format!("a{i}-{}", "Q".repeat(200))));
    }

    let mut metadata = json!({});
    let mut prev_boundary = 0u64;

    for turn in 0..5 {
        messages.push(user_message(&format!("extra-{turn}")));
        messages.push(assistant_message(&format!("reply-{turn}")));

        let _ = reducer.reduce(messages.clone(), &model, 32, &[], &mut metadata);
        let current_boundary = metadata
            .get("trimmed_up_to_message_index")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);

        assert!(
            current_boundary >= prev_boundary,
            "trim boundary must not regress: turn {turn}, {current_boundary} < {prev_boundary}"
        );
        prev_boundary = current_boundary;
    }
}

#[test]
fn empty_messages_returns_empty() {
    let reducer = BudgetAwareContextReducer::new(2, 0.8);
    let model = test_model(200_000);

    let mut metadata = json!({});
    let reduced = reducer.reduce(vec![], &model, 4096, &[], &mut metadata);

    assert!(reduced.is_empty());
}

#[test]
fn system_messages_are_never_trimmed() {
    let reducer = BudgetAwareContextReducer::new(1, 0.8);
    let model = test_model(120);

    let messages = vec![
        system_message(&"S".repeat(200)),
        user_message("u1"),
        assistant_message(&"A".repeat(500)),
        user_message("u2"),
        assistant_message("recent"),
    ];

    let mut metadata = json!({});
    let reduced = reducer.reduce(messages, &model, 32, &[], &mut metadata);

    for msg in &reduced {
        if msg.role == Role::System {
            let content = text_content(msg).unwrap_or_default();
            assert_ne!(
                content, "[trimmed]",
                "system messages must never be trimmed"
            );
        }
    }
}

#[test]
fn user_messages_are_never_trimmed() {
    let reducer = BudgetAwareContextReducer::new(1, 0.8);
    let model = test_model(120);

    let messages = vec![
        user_message(&"U".repeat(200)),
        assistant_message(&"A".repeat(500)),
        user_message(&"V".repeat(200)),
        assistant_message("recent"),
    ];

    let mut metadata = json!({});
    let reduced = reducer.reduce(messages, &model, 32, &[], &mut metadata);

    for msg in &reduced {
        if msg.role == Role::User {
            let content = text_content(msg).unwrap_or_default();
            assert_ne!(content, "[trimmed]", "user messages must never be trimmed");
        }
    }
}
