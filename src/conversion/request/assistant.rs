use serde_json::{Value, json};
use tracing::warn;

use crate::constants::{CONTENT_TEXT, CONTENT_TOOL_USE, ROLE_ASSISTANT, TOOL_FUNCTION};
use crate::models::ClaudeMessage;

pub fn convert_claude_assistant_message(message: &ClaudeMessage) -> Value {
    let Some(content) = &message.content else {
        return json!({ "role": ROLE_ASSISTANT, "content": Value::Null });
    };
    if let Some(text_content) = content.as_str() {
        return json!({ "role": ROLE_ASSISTANT, "content": text_content });
    }

    let Some(blocks) = content.as_array() else {
        return json!({ "role": ROLE_ASSISTANT, "content": Value::Null });
    };

    let (text_parts, tool_calls) = extract_assistant_parts(blocks);
    let content_value = if text_parts.is_empty() {
        Value::Null
    } else {
        Value::String(text_parts.join(""))
    };

    if tool_calls.is_empty() {
        json!({"role": ROLE_ASSISTANT, "content": content_value})
    } else {
        json!({"role": ROLE_ASSISTANT, "content": content_value, "tool_calls": tool_calls})
    }
}

fn extract_assistant_parts(blocks: &[Value]) -> (Vec<String>, Vec<Value>) {
    let mut text_parts = Vec::new();
    let mut tool_calls = Vec::new();

    for block in blocks {
        let block_type = block
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if block_type == CONTENT_TEXT {
            push_assistant_text(block, &mut text_parts);
            continue;
        }
        if block_type == CONTENT_TOOL_USE {
            push_assistant_tool_call(block, &mut tool_calls);
        }
    }

    (text_parts, tool_calls)
}

fn push_assistant_text(block: &Value, text_parts: &mut Vec<String>) {
    let Some(text) = block.get("text").and_then(Value::as_str) else {
        return;
    };
    text_parts.push(text.to_string());
}

fn push_assistant_tool_call(block: &Value, tool_calls: &mut Vec<Value>) {
    let Some(raw_tool_id) = block.get("id").and_then(Value::as_str) else {
        warn!(
            phase = "drop_tool_use",
            reason = "missing_id",
            "Dropping assistant tool_use block"
        );
        return;
    };
    let Some(raw_tool_name) = block.get("name").and_then(Value::as_str) else {
        warn!(
            phase = "drop_tool_use",
            reason = "missing_name",
            tool_id = raw_tool_id,
            "Dropping assistant tool_use block"
        );
        return;
    };

    let tool_id = raw_tool_id.trim();
    if tool_id.is_empty() {
        warn!(
            phase = "drop_tool_use",
            reason = "empty_id",
            "Dropping assistant tool_use block"
        );
        return;
    }

    let tool_name = raw_tool_name.trim();
    if tool_name.is_empty() {
        warn!(
            phase = "drop_tool_use",
            reason = "empty_name",
            tool_id,
            "Dropping assistant tool_use block"
        );
        return;
    }

    let tool_input = block.get("input").cloned().unwrap_or_else(|| json!({}));
    let arguments = serde_json::to_string(&tool_input).unwrap_or_else(|_| "{}".to_string());

    tool_calls.push(json!({
        "id": tool_id,
        "type": TOOL_FUNCTION,
        TOOL_FUNCTION: {
            "name": tool_name,
            "arguments": arguments,
        }
    }));
}
