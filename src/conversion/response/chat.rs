use serde::Deserialize;
use serde_json::Value;

use crate::models::ClaudeMessagesRequest;

use super::map_finish_reason;
use super::types::{
    ClaudeContentBlock, ClaudeResponse, ClaudeUsage, build_claude_response, map_tool_use_block,
    maybe_push_text, maybe_push_thinking,
};

pub(crate) fn convert_openai_to_claude_response(
    openai_response: &OpenAiChatResponse,
    original_request: &ClaudeMessagesRequest,
) -> Result<ClaudeResponse, String> {
    let choice = openai_response
        .choices
        .first()
        .ok_or_else(|| "no first choice in upstream response".to_string())?;
    let message = choice
        .message
        .as_ref()
        .ok_or_else(|| "missing message in upstream choice".to_string())?;

    let mut content_blocks = Vec::new();
    push_message_content(message, &mut content_blocks);
    push_tool_use_content(&message.tool_calls, &mut content_blocks);

    let stop_reason = map_finish_reason(choice.finish_reason.as_deref().unwrap_or("stop"));
    Ok(build_claude_response(
        openai_response.id.clone(),
        original_request.model.clone(),
        content_blocks,
        stop_reason,
        usage_from_chat(openai_response.usage.as_ref()),
    ))
}

fn push_message_content(
    message: &OpenAiResponseMessage,
    content_blocks: &mut Vec<ClaudeContentBlock>,
) {
    match message.content.as_ref() {
        Some(OpenAiResponseContent::Text(text)) => maybe_push_text(content_blocks, Some(text)),
        Some(OpenAiResponseContent::Other(content_json)) => {
            if !content_json.is_null() {
                content_blocks.push(ClaudeContentBlock::Text {
                    text: content_json.to_string(),
                });
            }
        }
        None => {}
    }
    maybe_push_thinking(
        content_blocks,
        message
            .reasoning_content
            .as_deref()
            .or(message.reasoning.as_deref()),
        message.signature.as_deref(),
    );
}

fn push_tool_use_content(
    tool_calls: &[OpenAiResponseToolCall],
    content_blocks: &mut Vec<ClaudeContentBlock>,
) {
    for tool_call in tool_calls {
        let block = map_tool_use_block(
            tool_call.id.as_deref(),
            tool_call.kind.as_deref(),
            tool_call.function.as_ref().and_then(|f| f.name.as_deref()),
            tool_call
                .function
                .as_ref()
                .and_then(|f| f.arguments.as_deref()),
        );
        if let Some(block) = block {
            content_blocks.push(block);
        }
    }
}

fn usage_from_chat(usage: Option<&OpenAiUsage>) -> ClaudeUsage {
    ClaudeUsage {
        input_tokens: usage.and_then(|value| value.prompt_tokens).unwrap_or(0),
        output_tokens: usage.and_then(|value| value.completion_tokens).unwrap_or(0),
    }
}

#[derive(Debug, Deserialize)]
pub struct OpenAiChatResponse {
    pub id: Option<String>,
    #[serde(default)]
    choices: Vec<OpenAiChoice>,
    usage: Option<OpenAiUsage>,
}

impl OpenAiChatResponse {
    pub(crate) fn id(&self) -> Option<&str> {
        self.id.as_deref()
    }

    pub(crate) fn total_tokens(&self) -> u64 {
        self.usage
            .as_ref()
            .map(OpenAiUsage::total_tokens)
            .unwrap_or(0)
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
    reasoning_content: Option<String>,
    reasoning: Option<String>,
    signature: Option<String>,
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

impl OpenAiUsage {
    fn total_tokens(&self) -> u64 {
        self.prompt_tokens
            .unwrap_or(0)
            .saturating_add(self.completion_tokens.unwrap_or(0))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::{OpenAiChatResponse, convert_openai_to_claude_response};
    use crate::models::ClaudeMessagesRequest;

    fn empty_request() -> ClaudeMessagesRequest {
        ClaudeMessagesRequest {
            model: "claude-3-5-sonnet-20241022".to_string(),
            max_tokens: 256,
            messages: vec![],
            thinking: None,
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

        let payload = serde_json::to_value(converted).expect("serialize");
        assert_eq!(
            payload
                .get("content")
                .and_then(Value::as_array)
                .map(|value| value.len()),
            Some(1)
        );
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

        let payload = serde_json::to_value(converted).expect("serialize");
        let content = payload
            .get("content")
            .and_then(Value::as_array)
            .expect("content array");
        assert_eq!(content.len(), 1);
        assert_eq!(
            content[0].get("type").and_then(Value::as_str),
            Some("tool_use")
        );
    }

    #[test]
    fn maps_reasoning_content_to_thinking_block() {
        let openai_response = json!({
            "id": "chatcmpl_test",
            "choices": [{
                "finish_reason": "stop",
                "message": {
                    "content": "done",
                    "reasoning_content": "step by step",
                    "signature": "sig_123",
                    "tool_calls": []
                }
            }],
            "usage": {"prompt_tokens": 1, "completion_tokens": 1}
        });

        let parsed: OpenAiChatResponse =
            serde_json::from_value(openai_response).expect("response should deserialize");
        let converted = convert_openai_to_claude_response(&parsed, &empty_request())
            .expect("conversion should succeed");

        let payload = serde_json::to_value(converted).expect("serialize");
        let content = payload
            .get("content")
            .and_then(Value::as_array)
            .expect("content array");
        assert_eq!(content.len(), 2);
        assert_eq!(
            content[1].get("type").and_then(Value::as_str),
            Some("thinking")
        );
    }
}
