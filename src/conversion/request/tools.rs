use serde_json::{Value, json};

use crate::constants::TOOL_FUNCTION;
use crate::models::ClaudeMessagesRequest;

pub fn add_optional_request_fields(request: &ClaudeMessagesRequest, openai_request: &mut Value) {
    if let Some(stop_sequences) = &request.stop_sequences {
        openai_request["stop"] = json!(stop_sequences);
    }
    if let Some(top_p) = request.top_p {
        openai_request["top_p"] = json!(top_p);
    }
}

pub fn add_tools(request: &ClaudeMessagesRequest, openai_request: &mut Value) {
    let Some(tools) = &request.tools else {
        return;
    };

    let converted_tools: Vec<Value> = tools.iter().filter_map(convert_single_tool).collect();
    if converted_tools.is_empty() {
        return;
    }
    openai_request["tools"] = Value::Array(converted_tools);
}

fn convert_single_tool(tool: &Value) -> Option<Value> {
    let name = tool
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    if name.is_empty() {
        return None;
    }

    let description = tool
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let parameters = tool
        .get("input_schema")
        .cloned()
        .unwrap_or_else(|| json!({ "type": "object", "properties": {} }));

    Some(json!({
        "type": TOOL_FUNCTION,
        TOOL_FUNCTION: {
            "name": name,
            "description": description,
            "parameters": parameters,
        }
    }))
}

pub fn add_tool_choice(request: &ClaudeMessagesRequest, openai_request: &mut Value) {
    let Some(tool_choice) = &request.tool_choice else {
        return;
    };
    let Some(choice_type) = tool_choice.get("type").and_then(Value::as_str) else {
        return;
    };

    openai_request["tool_choice"] = match choice_type {
        "auto" | "any" => json!("auto"),
        "tool" => create_tool_choice_payload(tool_choice.get("name").and_then(Value::as_str)),
        _ => json!("auto"),
    };
}

fn create_tool_choice_payload(selected_name: Option<&str>) -> Value {
    match selected_name {
        Some(name) => json!({
            "type": TOOL_FUNCTION,
            TOOL_FUNCTION: {"name": name}
        }),
        None => json!("auto"),
    }
}
