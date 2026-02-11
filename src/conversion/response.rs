use serde_json::{Value, json};
use uuid::Uuid;

use crate::constants::{
    CONTENT_TEXT, CONTENT_TOOL_USE, ROLE_ASSISTANT, STOP_END_TURN, STOP_MAX_TOKENS, STOP_TOOL_USE,
    TOOL_FUNCTION,
};
use crate::models::ClaudeMessagesRequest;

pub fn convert_openai_to_claude_response(
    openai_response: &Value,
    original_request: &ClaudeMessagesRequest,
) -> Result<Value, String> {
    let choice = first_choice(openai_response)?;
    let message = choice
        .get("message")
        .and_then(Value::as_object)
        .ok_or_else(|| "missing message in upstream choice".to_string())?;

    let mut content_blocks = Vec::new();
    push_text_content(message.get("content"), &mut content_blocks);
    push_tool_use_content(message.get("tool_calls"), &mut content_blocks);

    ensure_non_empty_content(&mut content_blocks);
    let stop_reason = map_finish_reason(
        choice
            .get("finish_reason")
            .and_then(Value::as_str)
            .unwrap_or("stop"),
    );

    Ok(build_claude_response(
        openai_response,
        original_request,
        content_blocks,
        stop_reason,
    ))
}

fn first_choice(openai_response: &Value) -> Result<&Value, String> {
    let choices = openai_response
        .get("choices")
        .and_then(Value::as_array)
        .ok_or_else(|| "no choices in upstream response".to_string())?;

    choices
        .first()
        .ok_or_else(|| "no first choice in upstream response".to_string())
}

fn push_text_content(content: Option<&Value>, content_blocks: &mut Vec<Value>) {
    let Some(content_value) = content else {
        return;
    };
    if let Some(content_text) = content_value.as_str() {
        content_blocks.push(json!({"type": CONTENT_TEXT, "text": content_text}));
        return;
    }
    if !content_value.is_null() {
        content_blocks.push(json!({"type": CONTENT_TEXT, "text": content_value.to_string()}));
    }
}

fn push_tool_use_content(tool_calls: Option<&Value>, content_blocks: &mut Vec<Value>) {
    let Some(tool_calls) = tool_calls.and_then(Value::as_array) else {
        return;
    };

    for tool_call in tool_calls {
        let Some(block) = map_tool_call(tool_call) else {
            continue;
        };
        content_blocks.push(block);
    }
}

fn map_tool_call(tool_call: &Value) -> Option<Value> {
    if tool_call.get("type").and_then(Value::as_str) != Some(TOOL_FUNCTION) {
        return None;
    }

    let function_data = tool_call.get(TOOL_FUNCTION)?.as_object()?;
    let arguments_raw = function_data
        .get("arguments")
        .and_then(Value::as_str)
        .unwrap_or("{}");

    let arguments_value = serde_json::from_str::<Value>(arguments_raw)
        .unwrap_or_else(|_| json!({ "raw_arguments": arguments_raw }));

    Some(json!({
        "type": CONTENT_TOOL_USE,
        "id": tool_call
            .get("id")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| format!("tool_{}", Uuid::new_v4())),
        "name": function_data
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        "input": arguments_value,
    }))
}

fn ensure_non_empty_content(content_blocks: &mut Vec<Value>) {
    if content_blocks.is_empty() {
        content_blocks.push(json!({"type": CONTENT_TEXT, "text": ""}));
    }
}

fn build_claude_response(
    openai_response: &Value,
    original_request: &ClaudeMessagesRequest,
    content_blocks: Vec<Value>,
    stop_reason: &str,
) -> Value {
    let usage = openai_response
        .get("usage")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();

    json!({
        "id": openai_response
            .get("id")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| format!("msg_{}", Uuid::new_v4())),
        "type": "message",
        "role": ROLE_ASSISTANT,
        "model": original_request.model,
        "content": content_blocks,
        "stop_reason": stop_reason,
        "stop_sequence": Value::Null,
        "usage": {
            "input_tokens": usage.get("prompt_tokens").and_then(Value::as_u64).unwrap_or(0),
            "output_tokens": usage.get("completion_tokens").and_then(Value::as_u64).unwrap_or(0),
        }
    })
}

pub fn map_finish_reason(finish_reason: &str) -> &str {
    match finish_reason {
        "length" => STOP_MAX_TOKENS,
        "tool_calls" | "function_call" => STOP_TOOL_USE,
        _ => STOP_END_TURN,
    }
}
