use serde::Deserialize;
use serde_json::Value;
use tracing::warn;

use crate::constants::{CONTENT_TEXT, ROLE_USER};
use crate::conversion::request::models::{OpenAiMessage, OpenAiToolMessage};
use crate::models::{ClaudeContent, ClaudeContentBlock, ClaudeMessage};

pub fn convert_claude_tool_results(message: &ClaudeMessage) -> Vec<OpenAiMessage> {
    let Some(content) = &message.content else {
        return Vec::new();
    };
    let ClaudeContent::Blocks(blocks) = content else {
        return Vec::new();
    };

    blocks
        .iter()
        .filter_map(convert_tool_result_block)
        .map(OpenAiMessage::Tool)
        .collect()
}

pub fn is_tool_result_user_message(message: &ClaudeMessage) -> bool {
    if message.role != ROLE_USER {
        return false;
    }
    let Some(content) = &message.content else {
        return false;
    };
    let ClaudeContent::Blocks(blocks) = content else {
        return false;
    };

    blocks
        .iter()
        .any(|block| matches!(block, ClaudeContentBlock::ToolResult { .. }))
}

pub fn has_non_tool_result_content(message: &ClaudeMessage) -> bool {
    if message.role != ROLE_USER {
        return false;
    }

    let Some(content) = &message.content else {
        return false;
    };

    match content {
        ClaudeContent::Text(_) => true,
        ClaudeContent::Blocks(blocks) => blocks
            .iter()
            .any(|block| !matches!(block, ClaudeContentBlock::ToolResult { .. })),
        ClaudeContent::Other(_) => false,
    }
}

fn convert_tool_result_block(block: &ClaudeContentBlock) -> Option<OpenAiToolMessage> {
    let ClaudeContentBlock::ToolResult {
        tool_use_id,
        content,
        ..
    } = block
    else {
        return None;
    };

    let Some(raw_tool_use_id) = tool_use_id.as_deref() else {
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

    let normalized_content = parse_tool_result_content(content.as_ref());
    Some(OpenAiToolMessage::new(
        tool_use_id.to_string(),
        normalized_content,
    ))
}

fn parse_tool_result_content(content: Option<&Value>) -> String {
    let Some(content) = content else {
        return "No content provided".to_string();
    };

    match content {
        Value::Null => "No content provided".to_string(),
        Value::String(text) => text.to_string(),
        Value::Array(items) => normalize_array_tool_content(items),
        Value::Object(_) => normalize_object_tool_content(content),
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
    match serde_json::from_value::<LooseTextBlock>(item.clone()) {
        Ok(block) if block.block_type.as_deref() == Some(CONTENT_TEXT) => block.text_as_owned(),
        Ok(block) => block.text_as_owned(),
        Err(_) => item.as_str().map(ToOwned::to_owned),
    }
}

fn normalize_object_tool_content(content: &Value) -> String {
    let parsed = serde_json::from_value::<LooseTextBlock>(content.clone());
    if let Ok(block) = parsed {
        if block.block_type.as_deref() == Some(CONTENT_TEXT) {
            return block.text_as_owned().unwrap_or_default();
        }
    }

    content.to_string()
}

#[derive(Debug, Deserialize)]
struct LooseTextBlock {
    #[serde(rename = "type")]
    block_type: Option<String>,
    text: Option<Value>,
}

impl LooseTextBlock {
    fn text_as_owned(&self) -> Option<String> {
        self.text
            .as_ref()
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
    }
}
