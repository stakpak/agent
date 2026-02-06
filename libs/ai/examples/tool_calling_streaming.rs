//! Streaming tool calling example
//!
//! This example demonstrates:
//! - Defining tools with JSON schema
//! - Making a streaming request with tools
//! - Handling tool call events in the stream
//! - Accumulating tool call arguments from deltas
//! - Executing tools and continuing the conversation

use futures::StreamExt;
use serde_json::json;
use stakai::{ContentPart, GenerateRequest, Inference, Message, Model, StreamEvent, Tool};
use std::collections::HashMap;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize client
    let client = Inference::new();

    // 1. Define tools
    let weather_tool = Tool::function("get_weather", "Get the current weather for a location")
        .parameters(json!({
            "type": "object",
            "properties": {
                "city": {
                    "type": "string",
                    "description": "The city name"
                },
                "unit": {
                    "type": "string",
                    "enum": ["celsius", "fahrenheit"],
                    "description": "Temperature unit"
                }
            },
            "required": ["city"]
        }));

    let time_tool =
        Tool::function("get_time", "Get the current time for a location").parameters(json!({
            "type": "object",
            "properties": {
                "city": {
                    "type": "string",
                    "description": "The city name"
                }
            },
            "required": ["city"]
        }));

    // 2. Create initial request
    let mut request = GenerateRequest::new(
        Model::custom("gpt-4o-mini", "openai"),
        vec![Message::new(
            stakai::Role::User,
            "What's the weather and time in Paris?",
        )],
    );
    request.options = request
        .options
        .add_tool(weather_tool.clone())
        .add_tool(time_tool.clone());

    // 3. Make streaming call
    println!("--- Streaming tool calls from model\n");
    let mut stream = client.stream(&request).await?;

    // Track tool calls being built
    let mut tool_calls: HashMap<String, ToolCallBuilder> = HashMap::new();
    let mut text_content = String::new();

    // 4. Process stream events
    while let Some(event) = stream.next().await {
        match event? {
            StreamEvent::Start { id } => {
                println!("Stream started: {}", id);
            }
            StreamEvent::TextDelta { delta, .. } => {
                print!("{}", delta);
                text_content.push_str(&delta);
            }
            StreamEvent::ReasoningDelta { delta, .. } => {
                // Display reasoning in a distinct way
                print!("[Reasoning: {}]", delta);
            }
            StreamEvent::ToolCallStart { id, name } => {
                println!("\n\n🔧 Tool call started:");
                println!("  ID: {}", id);
                println!("  Function: {}", name);
                tool_calls.insert(
                    id.clone(),
                    ToolCallBuilder {
                        id: id.clone(),
                        name: name.clone(),
                        arguments: String::new(),
                    },
                );
            }
            StreamEvent::ToolCallDelta { id, delta } => {
                if let Some(builder) = tool_calls.get_mut(&id) {
                    builder.arguments.push_str(&delta);
                }
            }
            StreamEvent::ToolCallEnd {
                id,
                name,
                arguments,
                ..
            } => {
                println!("\n✅ Tool call completed:");
                println!("  ID: {}", id);
                println!("  Function: {}", name);
                println!("  Arguments: {}", arguments);

                // Store complete tool call
                tool_calls.insert(
                    id.clone(),
                    ToolCallBuilder {
                        id: id.clone(),
                        name: name.clone(),
                        arguments: arguments.to_string(),
                    },
                );
            }
            StreamEvent::Finish { usage, reason } => {
                println!("\n\n--- Stream finished");
                println!("Reason: {:?}", reason);
                println!(
                    "Usage: {} prompt + {} completion = {} total tokens",
                    usage.prompt_tokens, usage.completion_tokens, usage.total_tokens
                );
            }
            StreamEvent::Error { message } => {
                eprintln!("Error: {}", message);
            }
        }
    }

    // 5. Execute tools if we got any
    if !tool_calls.is_empty() {
        println!("\n\n--- Executing tools");

        let mut tool_results = Vec::new();

        for (_, builder) in tool_calls.iter() {
            let result = execute_tool(&builder.name, &builder.arguments)?;
            println!("\n🔨 Executed: {}", builder.name);
            println!("   Result: {}", result);

            tool_results.push((builder.id.clone(), result));
        }

        // 6. Create follow-up request with tool results
        let mut messages = request.messages.clone();

        // Add assistant message with tool calls
        for (_, builder) in tool_calls.iter() {
            messages.push(Message::new(
                stakai::Role::Assistant,
                vec![ContentPart::tool_call(
                    builder.id.clone(),
                    builder.name.clone(),
                    serde_json::from_str(&builder.arguments).unwrap_or(serde_json::json!({})),
                )],
            ));
        }

        // Add tool results
        for (call_id, result) in tool_results {
            messages.push(Message::new(
                stakai::Role::Tool,
                vec![ContentPart::tool_result(call_id, result)],
            ));
        }

        let mut follow_up = GenerateRequest::new(Model::custom("gpt-4o-mini", "openai"), messages);
        follow_up.options = follow_up.options.add_tool(weather_tool).add_tool(time_tool);

        // 7. Get final response
        println!("\n\n--- Getting final response with tool results\n");
        let mut final_stream = client.stream(&follow_up).await?;

        while let Some(event) = final_stream.next().await {
            if let StreamEvent::TextDelta { delta, .. } = event? {
                print!("{}", delta);
            }
        }
        println!("\n");
    }

    Ok(())
}

// Helper struct to build tool calls from streaming deltas
struct ToolCallBuilder {
    id: String,
    name: String,
    arguments: String,
}

// Simulate tool execution
fn execute_tool(
    name: &str,
    arguments: &str,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let args: serde_json::Value = serde_json::from_str(arguments)?;

    match name {
        "get_weather" => {
            let city = args["city"].as_str().unwrap_or("Unknown");
            Ok(json!({
                "city": city,
                "temperature": 22.5,
                "condition": "Sunny",
                "humidity": 65
            }))
        }
        "get_time" => {
            let city = args["city"].as_str().unwrap_or("Unknown");
            Ok(json!({
                "city": city,
                "time": "14:30",
                "timezone": "CET"
            }))
        }
        _ => Ok(json!({
            "error": format!("Unknown tool: {}", name)
        })),
    }
}
