use futures_util::StreamExt;
use salvo::http::body::BodySender;
use serde_json::Value;
use tracing::{error, warn};
use uuid::Uuid;

use crate::conversion::stream::helpers::{
    content_delta, first_choice, snapshot_json_state, tool_arguments_delta, tool_call_deltas,
    tool_call_index, tool_started, update_finish_reason, update_tool_identity, update_usage,
};
use crate::conversion::stream::sse::{
    send_error_sse, send_start_sequence, send_stop_sequence, send_text_delta, send_tool_block_start,
    send_tool_json_delta,
};
use crate::conversion::stream::state::StreamState;

pub async fn stream_openai_to_claude_sse(
    upstream_response: reqwest::Response,
    mut sender: BodySender,
    original_model: String,
) {
    let message_id = message_id();
    if send_start_sequence(&mut sender, &original_model, &message_id)
        .await
        .is_err()
    {
        return;
    }

    let mut state = StreamState::new();
    let mut line_buffer = String::new();
    let mut upstream_stream = upstream_response.bytes_stream();

    while let Some(chunk_result) = upstream_stream.next().await {
        let Ok(chunk) = chunk_result else {
            if let Some(error) = chunk_result.err() {
                log_stream_read_error(&error);
                let _ = send_error_sse(
                    &mut sender,
                    &format!("streaming error from upstream: {error}"),
                )
                .await;
            }
            return;
        };

        line_buffer.push_str(&String::from_utf8_lossy(&chunk));
        process_complete_lines(&mut line_buffer, &mut sender, &mut state).await;

        if line_buffer.contains("data: [DONE]") {
            break;
        }
    }

    let _ = send_stop_sequence(&mut sender, &state).await;
}

fn log_stream_read_error(error: &reqwest::Error) {
    if error.is_timeout() {
        error!(
            phase = "upstream_stream_timeout",
            "Streaming interrupted by upstream read timeout"
        );
        return;
    }

    error!(
        phase = "upstream_stream_error",
        "Streaming interrupted while reading upstream body: {error}"
    );
}

fn message_id() -> String {
    format!(
        "msg_{}",
        Uuid::new_v4()
            .simple()
            .to_string()
            .chars()
            .take(24)
            .collect::<String>()
    )
}

async fn process_complete_lines(
    line_buffer: &mut String,
    sender: &mut BodySender,
    state: &mut StreamState,
) {
    while let Some(newline_index) = line_buffer.find('\n') {
        let line: String = line_buffer.drain(..=newline_index).collect();
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            continue;
        }

        let Some(data_line) = line.strip_prefix("data: ") else {
            continue;
        };
        if data_line.trim() == "[DONE]" {
            break;
        }

        let Ok(parsed_chunk) = serde_json::from_str::<Value>(data_line) else {
            warn!("failed to parse upstream stream line as JSON: {data_line}");
            continue;
        };

        update_usage(&parsed_chunk, state);
        let Some(choice) = first_choice(&parsed_chunk) else {
            continue;
        };

        if handle_content_delta(choice, sender, state).await.is_err() {
            return;
        }
        if process_tool_deltas(choice, sender, state).await.is_err() {
            return;
        }
        update_finish_reason(choice, state);
    }
}

async fn handle_content_delta(
    choice: &Value,
    sender: &mut BodySender,
    state: &StreamState,
) -> std::io::Result<()> {
    let Some(content_delta) = content_delta(choice) else {
        return Ok(());
    };

    send_text_delta(sender, state, content_delta).await
}

async fn process_tool_deltas(
    choice: &Value,
    sender: &mut BodySender,
    state: &mut StreamState,
) -> std::io::Result<()> {
    let Some(tool_call_deltas) = tool_call_deltas(choice) else {
        return Ok(());
    };

    for tool_call_delta in tool_call_deltas {
        process_single_tool_delta(tool_call_delta, sender, state).await?;
    }
    Ok(())
}

async fn process_single_tool_delta(
    tool_call_delta: &Value,
    sender: &mut BodySender,
    state: &mut StreamState,
) -> std::io::Result<()> {
    let tool_call_index = tool_call_index(tool_call_delta);

    update_tool_identity(tool_call_delta, state, tool_call_index);
    maybe_start_tool_block(tool_call_index, sender, state).await?;
    send_tool_json_if_ready(tool_call_delta, sender, state, tool_call_index).await
}

async fn maybe_start_tool_block(
    tool_call_index: usize,
    sender: &mut BodySender,
    state: &mut StreamState,
) -> std::io::Result<()> {
    let can_start = state
        .tool_calls
        .get(&tool_call_index)
        .map(|tool| tool.id.is_some() && tool.name.is_some() && !tool.started)
        .unwrap_or(false);
    if !can_start {
        return Ok(());
    }

    state.tool_block_counter += 1;
    let claude_index = state.text_block_index + state.tool_block_counter;

    let tool_call_state = state
        .tool_calls
        .get_mut(&tool_call_index)
        .expect("tool call state should exist");
    tool_call_state.claude_index = Some(claude_index);
    tool_call_state.started = true;

    send_tool_block_start(sender, claude_index, &tool_call_state.id, &tool_call_state.name).await
}

async fn send_tool_json_if_ready(
    tool_call_delta: &Value,
    sender: &mut BodySender,
    state: &mut StreamState,
    tool_call_index: usize,
) -> std::io::Result<()> {
    let Some(arguments_delta) = tool_arguments_delta(tool_call_delta) else {
        return Ok(());
    };

    if !tool_started(state, tool_call_index) {
        return Ok(());
    }

    let snapshot = snapshot_json_state(state, tool_call_index, arguments_delta);
    let (json_sent, has_complete_json, claude_index, payload_json) = snapshot;

    if json_sent || !has_complete_json {
        return Ok(());
    }

    let Some(claude_index) = claude_index else {
        return Ok(());
    };

    send_tool_json_delta(sender, claude_index, &payload_json).await?;

    if let Some(tool_call_state) = state.tool_calls.get_mut(&tool_call_index) {
        tool_call_state.json_sent = true;
    }
    Ok(())
}
