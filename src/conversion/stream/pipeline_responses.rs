use futures_util::StreamExt;
use salvo::http::body::BodySender;
use serde_json::Value;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::conversion::stream::responses_helpers::{
    ResponsesStreamContext, event_error_message, event_type, has_tool_event, text_delta, tool_kind,
    update_from_completed,
};
use crate::conversion::stream::responses_tools::{
    handle_function_arguments_delta, handle_function_arguments_done, handle_output_item_added,
};
use crate::conversion::stream::sse::{
    send_error_sse, send_start_sequence, send_stop_sequence, send_text_delta,
    send_thinking_block_start, send_thinking_delta,
};
use crate::conversion::stream::state::{StreamState, StreamUsage};

pub async fn stream_openai_responses_to_claude_sse(
    upstream_response: reqwest::Response,
    mut sender: BodySender,
    original_model: String,
    thinking_requested: bool,
) -> StreamUsage {
    let mut state = StreamState::new(thinking_requested);
    let message_id = message_id();
    if send_start_sequence(&mut sender, &original_model, &message_id)
        .await
        .is_err()
    {
        return state.usage_data;
    }

    let mut context = ResponsesStreamContext::default();
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
            return state.usage_data;
        };

        line_buffer.push_str(&String::from_utf8_lossy(&chunk));
        let should_stop = process_lines(
            &mut line_buffer,
            &mut sender,
            &mut state,
            &mut context,
            &original_model,
            &message_id,
        )
        .await;
        if should_stop {
            break;
        }
    }

    let _ = send_stop_sequence(&mut sender, &state).await;
    state.usage_data
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

async fn process_lines(
    line_buffer: &mut String,
    sender: &mut BodySender,
    state: &mut StreamState,
    context: &mut ResponsesStreamContext,
    original_model: &str,
    message_id: &str,
) -> bool {
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
            return true;
        }

        let Ok(event) = serde_json::from_str::<Value>(data_line) else {
            warn!("failed to parse upstream stream line as JSON: {data_line}");
            continue;
        };

        let should_stop =
            handle_event(&event, sender, state, context, original_model, message_id).await;
        if should_stop {
            return true;
        }
    }

    false
}

async fn handle_event(
    event: &Value,
    sender: &mut BodySender,
    state: &mut StreamState,
    context: &mut ResponsesStreamContext,
    original_model: &str,
    message_id: &str,
) -> bool {
    let event_type = event_type(event);
    maybe_start_thinking_fallback(event_type, event, sender, state, original_model, message_id)
        .await;

    match event_type {
        Some("response.output_text.delta") | Some("response.refusal.delta") => {
            if let Some(delta) = text_delta(event) {
                let _ = send_text_delta(sender, state, delta).await;
            }
            false
        }
        Some("response.reasoning_text.delta")
        | Some("response.reasoning_summary_text.delta")
        | Some("response.reasoning.delta")
        | Some("response.reasoning_summary.delta") => {
            let _ = handle_thinking_delta(event, sender, state).await;
            false
        }
        Some("response.output_item.added") => {
            if tool_kind(event) == Some("function_call") {
                let _ = handle_output_item_added(event, sender, state, context).await;
            }
            false
        }
        Some("response.function_call_arguments.delta") => {
            let _ = handle_function_arguments_delta(event, sender, state, context).await;
            false
        }
        Some("response.function_call_arguments.done") => {
            let _ = handle_function_arguments_done(event, sender, state, context).await;
            false
        }
        Some("response.completed") => {
            update_from_completed(event, state);
            true
        }
        Some("response.failed") | Some("error") => {
            let message = event_error_message(event);
            let _ = send_error_sse(sender, &message).await;
            true
        }
        _ => false,
    }
}

async fn handle_thinking_delta(
    event: &Value,
    sender: &mut BodySender,
    state: &mut StreamState,
) -> std::io::Result<()> {
    let Some(delta) = text_delta(event) else {
        return Ok(());
    };
    if !state.thinking_started {
        start_thinking_block(sender, state).await?;
    }

    let Some(thinking_index) = state.thinking_block_index else {
        return Ok(());
    };
    state.saw_thinking_delta = true;
    send_thinking_delta(sender, thinking_index, delta).await
}

async fn maybe_start_thinking_fallback(
    event_type: Option<&str>,
    event: &Value,
    sender: &mut BodySender,
    state: &mut StreamState,
    original_model: &str,
    message_id: &str,
) {
    if !state.thinking_requested || state.thinking_started || state.saw_thinking_delta {
        return;
    }
    if matches!(
        event_type,
        Some("response.reasoning_text.delta")
            | Some("response.reasoning_summary_text.delta")
            | Some("response.reasoning.delta")
            | Some("response.reasoning_summary.delta")
    ) {
        return;
    }

    let has_content = text_delta(event).is_some();
    let has_tools = has_tool_event(event_type, event);
    let has_finish = matches!(event_type, Some("response.completed"));
    if !(has_content || has_tools || has_finish) {
        return;
    }

    if start_thinking_block(sender, state).await.is_ok() {
        info!(
            phase = "thinking_fallback_start",
            model = original_model,
            message_id,
            claude_index = state.thinking_block_index.unwrap_or(0),
            has_content_delta = has_content,
            has_tool_delta = has_tools,
            has_finish_reason = has_finish,
            "Upstream reasoning absent; emitting realtime empty thinking block"
        );
    }
}

async fn start_thinking_block(
    sender: &mut BodySender,
    state: &mut StreamState,
) -> std::io::Result<()> {
    state.tool_block_counter += 1;
    let claude_index = state.text_block_index + state.tool_block_counter;
    state.thinking_block_index = Some(claude_index);
    state.thinking_started = true;
    send_thinking_block_start(sender, claude_index).await
}
