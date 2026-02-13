use serde::Deserialize;
use serde::de::IgnoredAny;
use serde_json::Value;

use crate::conversion::response::map_finish_reason;
use crate::conversion::stream::state::{StreamState, StreamUsage};

pub fn first_choice(parsed_chunk: &OpenAiStreamChunk) -> Option<&StreamChoice> {
    parsed_chunk.choices.first()
}

pub fn parse_stream_chunk(data_line: &str) -> Result<OpenAiStreamChunk, serde_json::Error> {
    serde_json::from_str(data_line)
}

pub fn update_usage(parsed_chunk: &OpenAiStreamChunk, state: &mut StreamState) {
    let Some(usage) = parsed_chunk.usage.as_ref() else {
        return;
    };

    let input_tokens = usage.prompt_tokens.unwrap_or(0);
    let output_tokens = usage.completion_tokens.unwrap_or(0);
    let cached_tokens = usage
        .prompt_tokens_details
        .as_ref()
        .and_then(|details| details.cached_tokens)
        .unwrap_or(0);

    state.usage_data = StreamUsage {
        input_tokens,
        output_tokens,
        cache_read_input_tokens: (cached_tokens > 0).then_some(cached_tokens),
    };
}

pub fn update_finish_reason(choice: &StreamChoice, state: &mut StreamState) {
    let Some(finish_reason) = choice.finish_reason.as_deref() else {
        return;
    };
    state.final_stop_reason = map_finish_reason(finish_reason).to_string();
}

pub fn tool_call_index(tool_call_delta: &ToolCallDelta) -> usize {
    tool_call_delta.index.unwrap_or(0) as usize
}

pub fn update_tool_identity(
    tool_call_delta: &ToolCallDelta,
    state: &mut StreamState,
    tool_call_index: usize,
) {
    let tool_call_state = state.tool_calls.entry(tool_call_index).or_default();

    if let Some(id) = tool_call_delta.id.as_deref() {
        tool_call_state.id = Some(id.to_string());
    }
    if let Some(name) = tool_call_delta
        .function
        .as_ref()
        .and_then(|function| function.name.as_deref())
    {
        tool_call_state.name = Some(name.to_string());
    }
}

pub fn tool_arguments_delta(tool_call_delta: &ToolCallDelta) -> Option<&str> {
    tool_call_delta
        .function
        .as_ref()
        .and_then(|function| function.arguments.as_deref())
}

pub fn content_delta(choice: &StreamChoice) -> Option<&str> {
    choice
        .delta
        .as_ref()
        .and_then(|delta| delta.content.as_deref())
}

pub fn thinking_delta(choice: &StreamChoice) -> Option<&str> {
    if let Some(delta) = choice.delta.as_ref() {
        if let Some(thinking) = extract_text(&delta.reasoning_content) {
            return Some(thinking);
        }
        if let Some(thinking) = extract_text(&delta.reasoning) {
            return Some(thinking);
        }
    }

    extract_text(&choice.reasoning_content).or_else(|| extract_text(&choice.reasoning))
}

pub fn thinking_signature_delta(choice: &StreamChoice) -> Option<&str> {
    if let Some(delta) = choice.delta.as_ref()
        && let Some(signature) = extract_signature(&delta.signature)
    {
        return Some(signature);
    }

    extract_signature(&choice.signature)
}

fn extract_text(value: &Option<Value>) -> Option<&str> {
    let value = value.as_ref()?;
    extract_text_value(value)
}

fn extract_text_value(value: &Value) -> Option<&str> {
    match value {
        Value::String(value) if !value.is_empty() => Some(value.as_str()),
        Value::Object(map) => {
            for key in [
                "text",
                "content",
                "thinking",
                "reasoning",
                "reasoning_content",
            ] {
                if let Some(candidate) = map.get(key).and_then(extract_text_value) {
                    return Some(candidate);
                }
            }
            None
        }
        Value::Array(items) => {
            for item in items {
                if let Some(candidate) = extract_text_value(item) {
                    return Some(candidate);
                }
            }
            None
        }
        _ => None,
    }
}

fn extract_signature(value: &Option<Value>) -> Option<&str> {
    let value = value.as_ref()?;
    extract_signature_value(value)
}

fn extract_signature_value(value: &Value) -> Option<&str> {
    match value {
        Value::String(value) if !value.is_empty() => Some(value.as_str()),
        Value::Object(map) => map
            .get("signature")
            .and_then(extract_signature_value)
            .or_else(|| map.get("value").and_then(extract_signature_value)),
        Value::Array(items) => {
            for item in items {
                if let Some(signature) = extract_signature_value(item) {
                    return Some(signature);
                }
            }
            None
        }
        _ => None,
    }
}

pub fn tool_call_deltas(choice: &StreamChoice) -> Option<&Vec<ToolCallDelta>> {
    choice
        .delta
        .as_ref()
        .and_then(|delta| delta.tool_calls.as_ref())
}

pub fn tool_started(state: &StreamState, tool_call_index: usize) -> bool {
    state
        .tool_calls
        .get(&tool_call_index)
        .map(|tool| tool.started)
        .unwrap_or(false)
}

pub fn snapshot_json_state(
    state: &mut StreamState,
    tool_call_index: usize,
    arguments_delta: &str,
) -> (bool, bool, Option<usize>, String) {
    let tool_call_state = state
        .tool_calls
        .get_mut(&tool_call_index)
        .expect("tool call state should exist");

    tool_call_state.args_buffer.push_str(arguments_delta);
    let has_complete_json =
        serde_json::from_str::<IgnoredAny>(&tool_call_state.args_buffer).is_ok();

    (
        tool_call_state.json_sent,
        has_complete_json,
        tool_call_state.claude_index,
        tool_call_state.args_buffer.clone(),
    )
}

#[derive(Debug, Deserialize)]
pub struct OpenAiStreamChunk {
    #[serde(default)]
    pub choices: Vec<StreamChoice>,
    pub usage: Option<OpenAiUsage>,
}

#[derive(Debug, Deserialize)]
pub struct StreamChoice {
    pub finish_reason: Option<String>,
    pub delta: Option<StreamDelta>,
    pub reasoning_content: Option<Value>,
    pub reasoning: Option<Value>,
    pub signature: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub struct StreamDelta {
    pub content: Option<String>,
    pub reasoning_content: Option<Value>,
    pub reasoning: Option<Value>,
    pub signature: Option<Value>,
    pub tool_calls: Option<Vec<ToolCallDelta>>,
}

#[cfg(test)]
mod tests {
    use super::{StreamChoice, StreamDelta, thinking_delta, thinking_signature_delta};
    use serde_json::json;

    #[test]
    fn reads_reasoning_content_string_delta() {
        let choice = StreamChoice {
            finish_reason: None,
            delta: Some(StreamDelta {
                content: None,
                reasoning_content: Some(json!("step one")),
                reasoning: None,
                signature: None,
                tool_calls: None,
            }),
            reasoning_content: None,
            reasoning: None,
            signature: None,
        };

        assert_eq!(thinking_delta(&choice), Some("step one"));
    }

    #[test]
    fn reads_reasoning_text_from_object_delta() {
        let choice = StreamChoice {
            finish_reason: None,
            delta: Some(StreamDelta {
                content: None,
                reasoning_content: Some(json!({"text":"hidden thought"})),
                reasoning: None,
                signature: None,
                tool_calls: None,
            }),
            reasoning_content: None,
            reasoning: None,
            signature: None,
        };

        assert_eq!(thinking_delta(&choice), Some("hidden thought"));
    }

    #[test]
    fn reads_reasoning_text_from_array_delta() {
        let choice = StreamChoice {
            finish_reason: None,
            delta: Some(StreamDelta {
                content: None,
                reasoning_content: None,
                reasoning: Some(json!([
                    {"type":"note"},
                    {"content":"array thought"}
                ])),
                signature: None,
                tool_calls: None,
            }),
            reasoning_content: None,
            reasoning: None,
            signature: None,
        };

        assert_eq!(thinking_delta(&choice), Some("array thought"));
    }

    #[test]
    fn reads_choice_level_reasoning_when_delta_missing() {
        let choice = StreamChoice {
            finish_reason: None,
            delta: Some(StreamDelta {
                content: Some("answer".to_string()),
                reasoning_content: None,
                reasoning: None,
                signature: None,
                tool_calls: None,
            }),
            reasoning_content: Some(json!("choice-level thought")),
            reasoning: None,
            signature: None,
        };

        assert_eq!(thinking_delta(&choice), Some("choice-level thought"));
    }

    #[test]
    fn reads_signature_from_object_delta() {
        let choice = StreamChoice {
            finish_reason: None,
            delta: Some(StreamDelta {
                content: None,
                reasoning_content: None,
                reasoning: None,
                signature: Some(json!({"signature":"sig_abc"})),
                tool_calls: None,
            }),
            reasoning_content: None,
            reasoning: None,
            signature: None,
        };

        assert_eq!(thinking_signature_delta(&choice), Some("sig_abc"));
    }
}

#[derive(Debug, Deserialize)]
pub struct ToolCallDelta {
    pub index: Option<u64>,
    pub id: Option<String>,
    #[serde(rename = "function")]
    pub function: Option<ToolFunctionDelta>,
}

#[derive(Debug, Deserialize)]
pub struct ToolFunctionDelta {
    pub name: Option<String>,
    pub arguments: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct OpenAiUsage {
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
    pub prompt_tokens_details: Option<PromptTokensDetails>,
}

#[derive(Debug, Deserialize)]
pub struct PromptTokensDetails {
    pub cached_tokens: Option<u64>,
}
