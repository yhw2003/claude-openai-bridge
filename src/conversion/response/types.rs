use serde::Serialize;
use serde_json::Value;
use tracing::warn;
use uuid::Uuid;

use crate::constants::{ROLE_ASSISTANT, TOOL_FUNCTION};

#[derive(Debug, Serialize)]
pub(crate) struct ClaudeResponse {
    id: String,
    #[serde(rename = "type")]
    response_type: String,
    role: String,
    model: String,
    content: Vec<ClaudeContentBlock>,
    stop_reason: String,
    stop_sequence: Option<String>,
    usage: ClaudeUsage,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ClaudeUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
pub(crate) enum ClaudeContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "thinking")]
    Thinking { thinking: String, signature: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
}

pub(crate) fn build_claude_response(
    id: Option<String>,
    model: String,
    mut content: Vec<ClaudeContentBlock>,
    stop_reason: &str,
    usage: ClaudeUsage,
) -> ClaudeResponse {
    ensure_non_empty_content(&mut content);
    ClaudeResponse {
        id: id.unwrap_or_else(|| format!("msg_{}", Uuid::new_v4())),
        response_type: "message".to_string(),
        role: ROLE_ASSISTANT.to_string(),
        model,
        content,
        stop_reason: stop_reason.to_string(),
        stop_sequence: None,
        usage,
    }
}

fn ensure_non_empty_content(content_blocks: &mut Vec<ClaudeContentBlock>) {
    if content_blocks.is_empty() {
        content_blocks.push(ClaudeContentBlock::Text {
            text: String::new(),
        });
    }
}

pub(crate) fn maybe_push_text(content_blocks: &mut Vec<ClaudeContentBlock>, text: Option<&str>) {
    let Some(text) = text else {
        return;
    };
    if text.is_empty() {
        return;
    }
    content_blocks.push(ClaudeContentBlock::Text {
        text: text.to_string(),
    });
}

pub(crate) fn maybe_push_thinking(
    content_blocks: &mut Vec<ClaudeContentBlock>,
    thinking: Option<&str>,
    signature: Option<&str>,
) {
    let Some(thinking) = thinking else {
        return;
    };
    if thinking.is_empty() {
        return;
    }

    content_blocks.push(ClaudeContentBlock::Thinking {
        thinking: thinking.to_string(),
        signature: signature.unwrap_or_default().to_string(),
    });
}

pub(crate) fn map_tool_use_block(
    id: Option<&str>,
    kind: Option<&str>,
    name: Option<&str>,
    arguments: Option<&str>,
) -> Option<ClaudeContentBlock> {
    if kind != Some(TOOL_FUNCTION) {
        warn!(
            phase = "drop_tool_use",
            reason = "unsupported_tool_call_type",
            tool_call_type = kind.unwrap_or("<missing>"),
            tool_call_id = id.unwrap_or("<missing>"),
            tool_name = name.unwrap_or("<missing>"),
            "Dropping upstream tool_call with unsupported type"
        );
        return None;
    }

    let Some(raw_id) = id else {
        warn!(
            phase = "drop_tool_use",
            reason = "missing_tool_call_id",
            "Dropping upstream tool_call without id"
        );
        return None;
    };

    let tool_call_id = raw_id.trim();
    if tool_call_id.is_empty() {
        warn!(
            phase = "drop_tool_use",
            reason = "empty_tool_call_id",
            "Dropping upstream tool_call with empty id"
        );
        return None;
    }

    Some(ClaudeContentBlock::ToolUse {
        id: tool_call_id.to_string(),
        name: name.unwrap_or_default().to_string(),
        input: parse_tool_arguments(arguments.unwrap_or("{}")),
    })
}

fn parse_tool_arguments(arguments_raw: &str) -> Value {
    serde_json::from_str::<Value>(arguments_raw).unwrap_or_else(|_| {
        serde_json::Value::Object(
            [(
                "raw_arguments".to_string(),
                Value::String(arguments_raw.to_string()),
            )]
            .into_iter()
            .collect(),
        )
    })
}
