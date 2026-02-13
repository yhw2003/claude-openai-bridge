use serde::Deserialize;
use serde::de::IgnoredAny;

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
    choice.delta.as_ref().and_then(|delta| {
        delta
            .reasoning_content
            .as_deref()
            .or(delta.reasoning.as_deref())
    })
}

pub fn thinking_signature_delta(choice: &StreamChoice) -> Option<&str> {
    choice
        .delta
        .as_ref()
        .and_then(|delta| delta.signature.as_deref())
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
}

#[derive(Debug, Deserialize)]
pub struct StreamDelta {
    pub content: Option<String>,
    pub reasoning_content: Option<String>,
    pub reasoning: Option<String>,
    pub signature: Option<String>,
    pub tool_calls: Option<Vec<ToolCallDelta>>,
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
