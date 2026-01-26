//! Basic tool calling example
//!
//! This example demonstrates:
//! - Defining a tool with JSON schema
//! - Making a request with tools
//! - Handling tool calls from the model
//! - Executing tools and sending results back
//! - Getting the final response

use serde_json::json;
use stakai::{ContentPart, GenerateRequest, Inference, Message, Model, Role, Tool};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize client
    let client = Inference::new();

    // 1. Define a weather tool
    let weather_tool = Tool::function("get_weather", "Get the current weather for a location")
        .parameters(json!({
            "type": "object",
            "properties": {
                "city": {
                    "type": "string",
                    "description": "The city name"
                },
                "country": {
                    "type": "string",
                    "description": "The country of the city"
                },
                "unit": {
                    "type": "string",
                    "enum": ["C", "F"],
                    "description": "Temperature unit (C for Celsius, F for Fahrenheit)"
                }
            },
            "required": ["city", "country", "unit"]
        }));

    // 2. Create initial request with the user query and tool
    let mut request = GenerateRequest::new(
        Model::custom("gpt-4o-mini", "openai"),
        vec![Message::new(
            Role::User,
            "What's the weather like in Tokyo, Japan?",
        )],
    );
    request.options = request.options.add_tool(weather_tool.clone());

    // 3. Make the initial call to get the tool call
    println!("--- Getting tool call from model");
    let response = client.generate(&request).await?;

    // 4. Check if we got tool calls
    let tool_calls = response.tool_calls();

    if tool_calls.is_empty() {
        println!("No tool calls received. Response: {}", response.text());
        return Ok(());
    }

    println!("\n--- Tool calls received:");
    for tool_call in &tool_calls {
        println!("  ID: {}", tool_call.id);
        println!("  Function: {}", tool_call.name);
        println!("  Arguments: {}", tool_call.arguments);
    }

    // 5. Simulate executing the function
    // In a real app, you would call your actual API or service here
    let first_tool_call = tool_calls[0];
    let tool_result = json!({
        "temperature": 22.5,
        "condition": "Sunny",
        "humidity": 65
    });

    println!("\n--- Executing tool: {}", first_tool_call.name);
    println!("Result: {}", tool_result);

    // 6. Create a new request with the tool result
    // Build new request with all messages including tool call and result
    let mut messages = request.messages.clone();

    // Add assistant message with tool call
    messages.push(Message::new(
        Role::Assistant,
        vec![ContentPart::tool_call(
            first_tool_call.id.clone(),
            first_tool_call.name.clone(),
            first_tool_call.arguments.clone(),
        )],
    ));

    // Add tool result message
    messages.push(Message::new(
        Role::Tool,
        vec![ContentPart::tool_result(
            first_tool_call.id.clone(),
            tool_result,
        )],
    ));

    let mut request_with_result =
        GenerateRequest::new(Model::custom("gpt-4o-mini", "openai"), messages);
    request_with_result.options = request_with_result.options.add_tool(
        Tool::function("get_weather", "Get the current weather for a location").parameters(json!({
            "type": "object",
            "properties": {
                "city": {"type": "string", "description": "The city name"},
                "country": {"type": "string", "description": "The country of the city"},
                "unit": {"type": "string", "enum": ["C", "F"], "description": "Temperature unit"}
            },
            "required": ["city", "country", "unit"]
        })),
    );

    // 7. Get the final response from the model with the function results
    println!("\n--- Getting final response with tool results");
    let final_response = client.generate(&request_with_result).await?;

    println!("\n--- Final response:");
    println!("{}", final_response.text());

    Ok(())
}
