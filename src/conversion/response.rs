use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::warn;
use uuid::Uuid;

use crate::constants::{
    ROLE_ASSISTANT, STOP_END_TURN, STOP_MAX_TOKENS, STOP_TOOL_USE, TOOL_FUNCTION,
};
use crate::models::ClaudeMessagesRequest;

pub(crate) fn convert_openai_to_claude_response(
    openai_response: &OpenAiChatResponse,
    original_request: &ClaudeMessagesRequest,
) -> Result<ClaudeResponse, String> {
    let choice = first_choice(openai_response)?;
    let message = choice
        .message
        .as_ref()
        .ok_or_else(|| "missing message in upstream choice".to_string())?;

    let mut content_blocks = Vec::new();
    push_text_content(message.content.as_ref(), &mut content_blocks);
    push_tool_use_content(&message.tool_calls, &mut content_blocks);

    ensure_non_empty_content(&mut content_blocks);
    let stop_reason = map_finish_reason(choice.finish_reason.as_deref().unwrap_or("stop"));

    Ok(build_claude_response(
        openai_response,
        original_request,
        content_blocks,
        stop_reason,
    ))
}

fn first_choice(openai_response: &OpenAiChatResponse) -> Result<&OpenAiChoice, String> {
    openai_response
        .choices
        .first()
        .ok_or_else(|| "no first choice in upstream response".to_string())
}

fn push_text_content(
    content: Option<&OpenAiResponseContent>,
    content_blocks: &mut Vec<ClaudeContentBlock>,
) {
    let Some(content_value) = content else {
        return;
    };

    match content_value {
        OpenAiResponseContent::Text(content_text) => {
            content_blocks.push(ClaudeContentBlock::Text {
                text: content_text.to_string(),
            });
        }
        OpenAiResponseContent::Other(content_json) => {
            if !content_json.is_null() {
                content_blocks.push(ClaudeContentBlock::Text {
                    text: content_json.to_string(),
                });
            }
        }
    }
}

fn push_tool_use_content(
    tool_calls: &[OpenAiResponseToolCall],
    content_blocks: &mut Vec<ClaudeContentBlock>,
) {
    for tool_call in tool_calls {
        let Some(block) = map_tool_call(tool_call) else {
            continue;
        };
        content_blocks.push(block);
    }
}

fn map_tool_call(tool_call: &OpenAiResponseToolCall) -> Option<ClaudeContentBlock> {
    if tool_call.kind.as_deref() != Some(TOOL_FUNCTION) {
        return None;
    }

    let function_data = tool_call.function.as_ref()?;
    let arguments_raw = function_data.arguments.as_deref().unwrap_or("{}");

    let arguments_value = serde_json::from_str::<Value>(arguments_raw).unwrap_or_else(|_| {
        serde_json::Value::Object(
            [("raw_arguments".to_string(), Value::String(arguments_raw.to_string()))]
                .into_iter()
                .collect(),
        )
    });

    let Some(raw_tool_call_id) = tool_call.id.as_deref() else {
        warn!(
            phase = "drop_tool_use",
            reason = "missing_tool_call_id",
            "Dropping upstream tool_call without id"
        );
        return None;
    };

    let tool_call_id = raw_tool_call_id.trim();
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
        name: function_data.name.clone().unwrap_or_default(),
        input: arguments_value,
    })
}

fn ensure_non_empty_content(content_blocks: &mut Vec<ClaudeContentBlock>) {
    if content_blocks.is_empty() {
        content_blocks.push(ClaudeContentBlock::Text {
            text: String::new(),
        });
    }
}

fn build_claude_response(
    openai_response: &OpenAiChatResponse,
    original_request: &ClaudeMessagesRequest,
    content_blocks: Vec<ClaudeContentBlock>,
    stop_reason: &str,
) -> ClaudeResponse {
    let usage = openai_response.usage.as_ref();

    ClaudeResponse {
        id: openai_response
            .id
            .clone()
            .unwrap_or_else(|| format!("msg_{}", Uuid::new_v4())),
        response_type: "message".to_string(),
        role: ROLE_ASSISTANT.to_string(),
        model: original_request.model.clone(),
        content: content_blocks,
        stop_reason: stop_reason.to_string(),
        stop_sequence: None,
        usage: ClaudeUsage {
            input_tokens: usage.and_then(|value| value.prompt_tokens).unwrap_or(0),
            output_tokens: usage.and_then(|value| value.completion_tokens).unwrap_or(0),
        },
    }
}

pub fn map_finish_reason(finish_reason: &str) -> &str {
    match finish_reason {
        "length" => STOP_MAX_TOKENS,
        "tool_calls" | "function_call" => STOP_TOOL_USE,
        _ => STOP_END_TURN,
    }
}

#[derive(Debug, Deserialize)]
pub struct OpenAiChatResponse {
    id: Option<String>,
    #[serde(default)]
    choices: Vec<OpenAiChoice>,
    usage: Option<OpenAiUsage>,
}

impl OpenAiChatResponse {
    pub(crate) fn id(&self) -> Option<&str> {
        self.id.as_deref()
    }
}

#[derive(Debug, Deserialize)]
struct OpenAiChoice {
    finish_reason: Option<String>,
    message: Option<OpenAiResponseMessage>,
}

#[derive(Debug, Deserialize)]
struct OpenAiResponseMessage {
    content: Option<OpenAiResponseContent>,
    #[serde(default)]
    tool_calls: Vec<OpenAiResponseToolCall>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum OpenAiResponseContent {
    Text(String),
    Other(Value),
}

#[derive(Debug, Deserialize)]
struct OpenAiResponseToolCall {
    id: Option<String>,
    #[serde(rename = "type")]
    kind: Option<String>,
    function: Option<OpenAiFunctionPayload>,
}

#[derive(Debug, Deserialize)]
struct OpenAiFunctionPayload {
    name: Option<String>,
    arguments: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAiUsage {
    prompt_tokens: Option<u64>,
    completion_tokens: Option<u64>,
}

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

#[derive(Debug, Serialize)]
struct ClaudeUsage {
    input_tokens: u64,
    output_tokens: u64,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
enum ClaudeContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse { id: String, name: String, input: Value },
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn empty_request() -> ClaudeMessagesRequest {
        ClaudeMessagesRequest {
            model: "claude-3-5-sonnet-20241022".to_string(),
            max_tokens: 256,
            messages: vec![],
            system: None,
            stop_sequences: None,
            stream: Some(false),
            temperature: Some(1.0),
            top_p: None,
            tools: None,
            tool_choice: None,
        }
    }

    #[test]
    fn skips_tool_call_without_id() {
        let openai_response = json!({
            "id": "chatcmpl_test",
            "choices": [{
                "finish_reason": "tool_calls",
                "message": {
                    "content": null,
                    "tool_calls": [{
                        "type": "function",
                        "function": {
                            "name": "Bash",
                            "arguments": "{}"
                        }
                    }]
                }
            }],
            "usage": {"prompt_tokens": 1, "completion_tokens": 1}
        });

        let parsed: OpenAiChatResponse =
            serde_json::from_value(openai_response).expect("response should deserialize");
        let converted = convert_openai_to_claude_response(&parsed, &empty_request())
            .expect("conversion should succeed");

        assert_eq!(converted.content.len(), 1);
        assert!(matches!(
            &converted.content[0],
            ClaudeContentBlock::Text { text } if text.is_empty()
        ));
    }

    #[test]
    fn keeps_tool_call_with_valid_id() {
        let openai_response = json!({
            "id": "chatcmpl_test",
            "choices": [{
                "finish_reason": "tool_calls",
                "message": {
                    "content": null,
                    "tool_calls": [{
                        "id": "call_abc123",
                        "type": "function",
                        "function": {
                            "name": "Bash",
                            "arguments": "{\"command\":\"cargo fmt\"}"
                        }
                    }]
                }
            }],
            "usage": {"prompt_tokens": 1, "completion_tokens": 1}
        });

        let parsed: OpenAiChatResponse =
            serde_json::from_value(openai_response).expect("response should deserialize");
        let converted = convert_openai_to_claude_response(&parsed, &empty_request())
            .expect("conversion should succeed");

        assert_eq!(converted.content.len(), 1);
        let tool_use = match &converted.content[0] {
            ClaudeContentBlock::ToolUse { id, name, input } => (id, name, input),
            _ => panic!("expected tool_use content block"),
        };

        assert_eq!(tool_use.0, "call_abc123");
        assert_eq!(tool_use.1, "Bash");
        assert_eq!(
            tool_use.2.get("command").and_then(Value::as_str),
            Some("cargo fmt")
        );
    }
}
