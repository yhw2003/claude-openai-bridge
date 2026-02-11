use serde_json::Value;

use crate::constants::TOOL_FUNCTION;
use crate::conversion::response::map_finish_reason;
use crate::conversion::stream::state::StreamState;

pub fn first_choice(parsed_chunk: &Value) -> Option<&Value> {
    parsed_chunk
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
}

pub fn update_usage(parsed_chunk: &Value, state: &mut StreamState) {
    let Some(usage) = parsed_chunk.get("usage").and_then(Value::as_object) else {
        return;
    };

    let input_tokens = usage
        .get("prompt_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let output_tokens = usage
        .get("completion_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let cached_tokens = usage
        .get("prompt_tokens_details")
        .and_then(Value::as_object)
        .and_then(|details| details.get("cached_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);

    state.usage_data = if cached_tokens > 0 {
        serde_json::json!({
            "input_tokens": input_tokens,
            "output_tokens": output_tokens,
            "cache_read_input_tokens": cached_tokens,
        })
    } else {
        serde_json::json!({"input_tokens": input_tokens, "output_tokens": output_tokens})
    };
}

pub fn update_finish_reason(choice: &Value, state: &mut StreamState) {
    let Some(finish_reason) = choice.get("finish_reason").and_then(Value::as_str) else {
        return;
    };
    state.final_stop_reason = map_finish_reason(finish_reason).to_string();
}

pub fn tool_call_index(tool_call_delta: &Value) -> usize {
    tool_call_delta
        .get("index")
        .and_then(Value::as_u64)
        .unwrap_or(0) as usize
}

pub fn update_tool_identity(
    tool_call_delta: &Value,
    state: &mut StreamState,
    tool_call_index: usize,
) {
    let tool_call_state = state.tool_calls.entry(tool_call_index).or_default();

    if let Some(id) = tool_call_delta.get("id").and_then(Value::as_str) {
        tool_call_state.id = Some(id.to_string());
    }
    if let Some(name) = tool_call_delta
        .get(TOOL_FUNCTION)
        .and_then(|function| function.get("name"))
        .and_then(Value::as_str)
    {
        tool_call_state.name = Some(name.to_string());
    }
}

pub fn tool_arguments_delta(tool_call_delta: &Value) -> Option<&str> {
    tool_call_delta
        .get(TOOL_FUNCTION)
        .and_then(|function| function.get("arguments"))
        .and_then(Value::as_str)
}

pub fn content_delta(choice: &Value) -> Option<&str> {
    choice
        .get("delta")
        .and_then(|delta| delta.get("content"))
        .and_then(Value::as_str)
}

pub fn tool_call_deltas(choice: &Value) -> Option<&Vec<Value>> {
    choice
        .get("delta")
        .and_then(|delta| delta.get("tool_calls"))
        .and_then(Value::as_array)
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
    let has_complete_json = serde_json::from_str::<Value>(&tool_call_state.args_buffer).is_ok();

    (
        tool_call_state.json_sent,
        has_complete_json,
        tool_call_state.claude_index,
        tool_call_state.args_buffer.clone(),
    )
}
