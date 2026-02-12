use serde_json::Value;
use tracing::warn;

use crate::conversion::request::models::{
    OpenAiAssistantMessage, OpenAiMessage, OpenAiToolCall,
};
use crate::models::{ClaudeContent, ClaudeContentBlock, ClaudeMessage};

pub fn convert_claude_assistant_message(message: &ClaudeMessage) -> OpenAiMessage {
    let Some(content) = &message.content else {
        return OpenAiMessage::Assistant(OpenAiAssistantMessage::from_text_and_tools(None, vec![]));
    };

    match content {
        ClaudeContent::Text(text_content) => OpenAiMessage::Assistant(
            OpenAiAssistantMessage::from_text_and_tools(Some(text_content.to_string()), vec![]),
        ),
        ClaudeContent::Blocks(blocks) => {
            let (text_parts, tool_calls) = extract_assistant_parts(blocks);
            let content_text = if text_parts.is_empty() {
                None
            } else {
                Some(text_parts.join(""))
            };

            OpenAiMessage::Assistant(OpenAiAssistantMessage::from_text_and_tools(
                content_text,
                tool_calls,
            ))
        }
        ClaudeContent::Other(_) => {
            OpenAiMessage::Assistant(OpenAiAssistantMessage::from_text_and_tools(None, vec![]))
        }
    }
}

fn extract_assistant_parts(blocks: &[ClaudeContentBlock]) -> (Vec<String>, Vec<OpenAiToolCall>) {
    let mut text_parts = Vec::new();
    let mut tool_calls = Vec::new();

    for block in blocks {
        match block {
            ClaudeContentBlock::Text { text, .. } => text_parts.push(text.clone()),
            ClaudeContentBlock::ToolUse {
                id, name, input, ..
            } => {
                if let Some(tool_call) = build_tool_call(id.clone(), name.clone(), input.clone()) {
                    tool_calls.push(tool_call);
                }
            }
            _ => {}
        }
    }

    (text_parts, tool_calls)
}

fn build_tool_call(
    id: Option<String>,
    name: Option<String>,
    input: Option<Value>,
) -> Option<OpenAiToolCall> {
    let Some(raw_tool_id) = id.as_deref() else {
        warn!(
            phase = "drop_tool_use",
            reason = "missing_id",
            "Dropping assistant tool_use block"
        );
        return None;
    };
    let Some(raw_tool_name) = name.as_deref() else {
        warn!(
            phase = "drop_tool_use",
            reason = "missing_name",
            tool_id = raw_tool_id,
            "Dropping assistant tool_use block"
        );
        return None;
    };

    let tool_id = raw_tool_id.trim();
    if tool_id.is_empty() {
        warn!(
            phase = "drop_tool_use",
            reason = "empty_id",
            "Dropping assistant tool_use block"
        );
        return None;
    }

    let tool_name = raw_tool_name.trim();
    if tool_name.is_empty() {
        warn!(
            phase = "drop_tool_use",
            reason = "empty_name",
            tool_id,
            "Dropping assistant tool_use block"
        );
        return None;
    }

    let tool_input = input.unwrap_or_else(|| Value::Object(Default::default()));
    let arguments = serde_json::to_string(&tool_input).unwrap_or_else(|_| "{}".to_string());

    Some(OpenAiToolCall::function(
        tool_id.to_string(),
        tool_name.to_string(),
        arguments,
    ))
}
