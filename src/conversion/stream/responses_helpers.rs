use std::collections::HashMap;

use serde_json::Value;

use crate::conversion::response::map_responses_incomplete_reason;
use crate::conversion::stream::state::{StreamState, StreamUsage};

pub(crate) fn update_from_completed(event: &Value, state: &mut StreamState) {
    let payload = event.get("response").unwrap_or(event);
    let usage = payload.get("usage").unwrap_or(&Value::Null);
    state.usage_data = StreamUsage {
        input_tokens: usage
            .get("input_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        output_tokens: usage
            .get("output_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        cache_read_input_tokens: usage
            .get("input_tokens_details")
            .and_then(|v| v.get("cached_tokens"))
            .and_then(Value::as_u64)
            .filter(|v| *v > 0),
    };

    state.final_stop_reason = resolve_completed_stop_reason(payload).to_string();
}

fn resolve_completed_stop_reason(payload: &Value) -> &'static str {
    if output_contains_function_call(payload) {
        return crate::constants::STOP_TOOL_USE;
    }

    let status = payload.get("status").and_then(Value::as_str);
    if status == Some("incomplete") {
        let reason = payload
            .get("incomplete_details")
            .and_then(|v| v.get("reason"))
            .and_then(Value::as_str);
        return map_responses_incomplete_reason(reason);
    }

    crate::constants::STOP_END_TURN
}

fn output_contains_function_call(payload: &Value) -> bool {
    payload
        .get("output")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .any(|item| item.get("type").and_then(Value::as_str) == Some("function_call"))
        })
        .unwrap_or(false)
}

pub(crate) fn event_type(event: &Value) -> Option<&str> {
    event.get("type").and_then(Value::as_str)
}

pub(crate) fn text_delta(event: &Value) -> Option<&str> {
    event
        .get("delta")
        .and_then(Value::as_str)
        .or_else(|| event.get("text").and_then(Value::as_str))
        .or_else(|| {
            event
                .get("item")
                .and_then(|item| item.get("text"))
                .and_then(Value::as_str)
        })
}

pub(crate) fn has_tool_event(event_type: Option<&str>, event: &Value) -> bool {
    if matches!(
        event_type,
        Some("response.output_item.added")
            | Some("response.function_call_arguments.delta")
            | Some("response.function_call_arguments.done")
    ) {
        return tool_kind(event).is_some() || call_id(event).is_some();
    }
    false
}

pub(crate) fn tool_kind(event: &Value) -> Option<&str> {
    event
        .get("item")
        .and_then(|item| item.get("type"))
        .and_then(Value::as_str)
}

pub(crate) fn call_id(event: &Value) -> Option<String> {
    event
        .get("call_id")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .or_else(|| {
            event
                .get("item")
                .and_then(|item| item.get("call_id"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
}

pub(crate) fn event_error_message(event: &Value) -> String {
    event
        .get("error")
        .and_then(|value| value.get("message"))
        .and_then(Value::as_str)
        .or_else(|| event.get("message").and_then(Value::as_str))
        .unwrap_or("upstream responses stream failed")
        .to_string()
}

pub(crate) fn resolve_tool_index(event: &Value, context: &mut ResponsesStreamContext) -> usize {
    if let Some(index) = event_output_index(event) {
        context.bump_next_tool_index(index + 1);
        return index;
    }

    if let Some(call_id) = call_id(event)
        && let Some(index) = context.tool_index_by_call_id.get(&call_id).copied()
    {
        return index;
    }

    if let Some(item_id) = item_id(event)
        && let Some(index) = context.tool_index_by_item_id.get(&item_id).copied()
    {
        return index;
    }

    context.take_next_tool_index()
}

fn event_output_index(event: &Value) -> Option<usize> {
    event
        .get("output_index")
        .and_then(Value::as_u64)
        .map(|value| value as usize)
        .or_else(|| {
            event
                .get("item")
                .and_then(|item| item.get("output_index"))
                .and_then(Value::as_u64)
                .map(|value| value as usize)
        })
}

pub(crate) fn update_tool_maps(
    event: &Value,
    tool_index: usize,
    context: &mut ResponsesStreamContext,
) {
    if let Some(call_id) = call_id(event) {
        context.tool_index_by_call_id.insert(call_id, tool_index);
    }
    if let Some(item_id) = item_id(event) {
        context.tool_index_by_item_id.insert(item_id, tool_index);
    }
}

pub(crate) fn update_tool_identity(event: &Value, tool_index: usize, state: &mut StreamState) {
    let tool_state = state.tool_calls.entry(tool_index).or_default();

    if let Some(call_id) = call_id(event) {
        tool_state.id = Some(call_id);
    }
    if let Some(name) = tool_name(event) {
        tool_state.name = Some(name.to_string());
    }
}

fn tool_name(event: &Value) -> Option<&str> {
    event.get("name").and_then(Value::as_str).or_else(|| {
        event
            .get("item")
            .and_then(|item| item.get("name"))
            .and_then(Value::as_str)
    })
}

fn item_id(event: &Value) -> Option<String> {
    event
        .get("item_id")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .or_else(|| {
            event
                .get("item")
                .and_then(|item| item.get("id"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
}

pub(crate) fn arguments_from_item(event: &Value) -> Option<&str> {
    event
        .get("item")
        .and_then(|item| item.get("arguments"))
        .and_then(Value::as_str)
}

pub(crate) fn value_to_string(value: &Value) -> String {
    match value {
        Value::String(text) => text.to_string(),
        _ => value.to_string(),
    }
}

#[derive(Default)]
pub(crate) struct ResponsesStreamContext {
    next_tool_index: usize,
    tool_index_by_call_id: HashMap<String, usize>,
    tool_index_by_item_id: HashMap<String, usize>,
}

impl ResponsesStreamContext {
    fn bump_next_tool_index(&mut self, minimum_next: usize) {
        if minimum_next > self.next_tool_index {
            self.next_tool_index = minimum_next;
        }
    }

    fn take_next_tool_index(&mut self) -> usize {
        let current = self.next_tool_index;
        self.next_tool_index += 1;
        current
    }
}
