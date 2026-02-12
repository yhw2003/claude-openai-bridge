use serde_json::{Value, json};
use tracing::warn;

use crate::constants::{CONTENT_TEXT, CONTENT_TOOL_RESULT, ROLE_TOOL, ROLE_USER};
use crate::models::ClaudeMessage;

pub fn convert_claude_tool_results(message: &ClaudeMessage) -> Vec<Value> {
    let Some(content) = &message.content else {
        return Vec::new();
    };
    let Some(blocks) = content.as_array() else {
        return Vec::new();
    };

    blocks
        .iter()
        .filter(|block| block.get("type").and_then(Value::as_str) == Some(CONTENT_TOOL_RESULT))
        .filter_map(convert_tool_result_block)
        .collect()
}

pub fn is_tool_result_user_message(message: &ClaudeMessage) -> bool {
    if message.role != ROLE_USER {
        return false;
    }
    let Some(content) = &message.content else {
        return false;
    };
    let Some(blocks) = content.as_array() else {
        return false;
    };

    blocks
        .iter()
        .any(|block| block.get("type").and_then(Value::as_str) == Some(CONTENT_TOOL_RESULT))
}

pub fn has_non_tool_result_content(message: &ClaudeMessage) -> bool {
    if message.role != ROLE_USER {
        return false;
    }

    let Some(content) = &message.content else {
        return false;
    };

    if content.is_string() {
        return true;
    }

    let Some(blocks) = content.as_array() else {
        return false;
    };

    blocks
        .iter()
        .any(|block| block.get("type").and_then(Value::as_str) != Some(CONTENT_TOOL_RESULT))
}

fn convert_tool_result_block(block: &Value) -> Option<Value> {
    let Some(raw_tool_use_id) = block.get("tool_use_id").and_then(Value::as_str) else {
        warn!(
            phase = "drop_tool_result",
            reason = "missing_tool_use_id",
            "Dropping tool_result block"
        );
        return None;
    };

    let tool_use_id = raw_tool_use_id.trim();
    if tool_use_id.is_empty() {
        warn!(
            phase = "drop_tool_result",
            reason = "empty_tool_use_id",
            "Dropping tool_result block"
        );
        return None;
    }

    let normalized_content = parse_tool_result_content(block.get("content"));
    Some(json!({
        "role": ROLE_TOOL,
        "tool_call_id": tool_use_id,
        "content": normalized_content,
    }))
}

fn parse_tool_result_content(content: Option<&Value>) -> String {
    let Some(content) = content else {
        return "No content provided".to_string();
    };

    match content {
        Value::Null => "No content provided".to_string(),
        Value::String(text) => text.to_string(),
        Value::Array(items) => normalize_array_tool_content(items),
        Value::Object(object) => normalize_object_tool_content(content, object),
        other => other.to_string(),
    }
}

fn normalize_array_tool_content(items: &[Value]) -> String {
    let mut parts = Vec::new();
    for item in items {
        if let Some(text) = extract_item_text(item) {
            parts.push(text);
        } else {
            parts.push(item.to_string());
        }
    }
    parts.join("\n").trim().to_string()
}

fn extract_item_text(item: &Value) -> Option<String> {
    if let Some(text) = item.as_str() {
        return Some(text.to_string());
    }
    if item.get("type").and_then(Value::as_str) == Some(CONTENT_TEXT) {
        return item
            .get("text")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
    }
    item.get("text")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn normalize_object_tool_content(
    content: &Value,
    object: &serde_json::Map<String, Value>,
) -> String {
    if object.get("type").and_then(Value::as_str) == Some(CONTENT_TEXT) {
        return object
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
    }
    content.to_string()
}
