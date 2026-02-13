use salvo::http::body::BodySender;
use serde_json::Value;

use crate::conversion::stream::helpers::snapshot_json_state;
use crate::conversion::stream::responses_helpers::{
    ResponsesStreamContext, arguments_from_item, resolve_tool_index, update_tool_identity,
    update_tool_maps, value_to_string,
};
use crate::conversion::stream::sse::{send_tool_block_start, send_tool_json_delta};
use crate::conversion::stream::state::StreamState;

pub(crate) async fn handle_output_item_added(
    event: &Value,
    sender: &mut BodySender,
    state: &mut StreamState,
    context: &mut ResponsesStreamContext,
) -> std::io::Result<()> {
    let tool_index = resolve_tool_index(event, context);
    update_tool_maps(event, tool_index, context);
    update_tool_identity(event, tool_index, state);
    maybe_start_tool_block(tool_index, sender, state).await?;

    if let Some(arguments) = arguments_from_item(event) {
        send_tool_json_if_complete(tool_index, arguments, sender, state).await?;
    }
    Ok(())
}

pub(crate) async fn handle_function_arguments_delta(
    event: &Value,
    sender: &mut BodySender,
    state: &mut StreamState,
    context: &mut ResponsesStreamContext,
) -> std::io::Result<()> {
    let tool_index = resolve_tool_index(event, context);
    update_tool_maps(event, tool_index, context);
    update_tool_identity(event, tool_index, state);
    maybe_start_tool_block(tool_index, sender, state).await?;

    let Some(delta) = event.get("delta").and_then(Value::as_str) else {
        return Ok(());
    };
    send_tool_json_if_complete(tool_index, delta, sender, state).await
}

pub(crate) async fn handle_function_arguments_done(
    event: &Value,
    sender: &mut BodySender,
    state: &mut StreamState,
    context: &mut ResponsesStreamContext,
) -> std::io::Result<()> {
    let tool_index = resolve_tool_index(event, context);
    update_tool_maps(event, tool_index, context);
    update_tool_identity(event, tool_index, state);
    maybe_start_tool_block(tool_index, sender, state).await?;

    let Some(arguments) = event.get("arguments").map(value_to_string) else {
        return Ok(());
    };
    send_tool_json_on_done(tool_index, &arguments, sender, state).await
}

async fn maybe_start_tool_block(
    tool_index: usize,
    sender: &mut BodySender,
    state: &mut StreamState,
) -> std::io::Result<()> {
    let can_start = state
        .tool_calls
        .get(&tool_index)
        .map(|tool| tool.id.is_some() && tool.name.is_some() && !tool.started)
        .unwrap_or(false);
    if !can_start {
        return Ok(());
    }

    state.tool_block_counter += 1;
    let claude_index = state.text_block_index + state.tool_block_counter;
    let tool_call_state = state
        .tool_calls
        .get_mut(&tool_index)
        .expect("tool call state should exist");
    tool_call_state.claude_index = Some(claude_index);
    tool_call_state.started = true;

    send_tool_block_start(
        sender,
        claude_index,
        &tool_call_state.id,
        &tool_call_state.name,
    )
    .await
}

async fn send_tool_json_if_complete(
    tool_index: usize,
    delta: &str,
    sender: &mut BodySender,
    state: &mut StreamState,
) -> std::io::Result<()> {
    let snapshot = snapshot_json_state(state, tool_index, delta);
    let (json_sent, has_complete_json, claude_index, payload_json) = snapshot;
    if json_sent || !has_complete_json {
        return Ok(());
    }

    if let Some(claude_index) = claude_index {
        send_tool_json_delta(sender, claude_index, &payload_json).await?;
        if let Some(tool_state) = state.tool_calls.get_mut(&tool_index) {
            tool_state.json_sent = true;
        }
    }
    Ok(())
}

async fn send_tool_json_on_done(
    tool_index: usize,
    arguments: &str,
    sender: &mut BodySender,
    state: &mut StreamState,
) -> std::io::Result<()> {
    let Some(tool_state) = state.tool_calls.get_mut(&tool_index) else {
        return Ok(());
    };
    if tool_state.json_sent {
        return Ok(());
    }

    tool_state.args_buffer = arguments.to_string();
    let Some(claude_index) = tool_state.claude_index else {
        return Ok(());
    };
    send_tool_json_delta(sender, claude_index, &tool_state.args_buffer).await?;
    tool_state.json_sent = true;
    Ok(())
}
