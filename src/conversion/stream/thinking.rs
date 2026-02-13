use std::io;

use salvo::http::body::BodySender;
use tracing::info;

use crate::conversion::stream::helpers::{
    StreamChoice, content_delta, thinking_delta, thinking_signature_delta, tool_call_deltas,
};
use crate::conversion::stream::sse::{
    send_signature_delta, send_thinking_block_start, send_thinking_delta,
};
use crate::conversion::stream::state::StreamState;

pub struct ThinkingFallbackContext<'a> {
    pub model: &'a str,
    pub message_id: &'a str,
}

pub async fn handle_thinking_delta(
    choice: &StreamChoice,
    sender: &mut BodySender,
    state: &mut StreamState,
) -> io::Result<()> {
    maybe_start_thinking_block_from_delta(choice, sender, state).await?;
    maybe_send_thinking_delta(choice, sender, state).await?;
    maybe_send_signature_delta(choice, sender, state).await
}

pub async fn maybe_emit_realtime_fallback(
    choice: &StreamChoice,
    sender: &mut BodySender,
    state: &mut StreamState,
    context: &ThinkingFallbackContext<'_>,
) -> io::Result<()> {
    if !should_emit_realtime_fallback(choice, state) {
        return Ok(());
    }

    start_thinking_block(sender, state).await?;
    log_fallback_start(choice, state, context);
    Ok(())
}

fn should_emit_realtime_fallback(choice: &StreamChoice, state: &StreamState) -> bool {
    if !state.thinking_requested || state.thinking_started || state.saw_thinking_delta {
        return false;
    }

    if thinking_delta(choice).is_some() || thinking_signature_delta(choice).is_some() {
        return false;
    }

    let has_content = content_delta(choice).is_some();
    let has_tools = tool_call_deltas(choice)
        .map(|value| !value.is_empty())
        .unwrap_or(false);
    let has_finish = choice.finish_reason.is_some();
    has_content || has_tools || has_finish
}

async fn maybe_start_thinking_block_from_delta(
    choice: &StreamChoice,
    sender: &mut BodySender,
    state: &mut StreamState,
) -> io::Result<()> {
    if state.thinking_started || thinking_delta(choice).is_none() {
        return Ok(());
    }

    start_thinking_block(sender, state).await
}

async fn start_thinking_block(sender: &mut BodySender, state: &mut StreamState) -> io::Result<()> {
    state.tool_block_counter += 1;
    let claude_index = state.text_block_index + state.tool_block_counter;
    state.thinking_block_index = Some(claude_index);
    state.thinking_started = true;
    send_thinking_block_start(sender, claude_index).await
}

async fn maybe_send_thinking_delta(
    choice: &StreamChoice,
    sender: &mut BodySender,
    state: &mut StreamState,
) -> io::Result<()> {
    let Some(claude_index) = state.thinking_block_index else {
        return Ok(());
    };
    let Some(payload) = thinking_delta(choice) else {
        return Ok(());
    };

    state.saw_thinking_delta = true;
    send_thinking_delta(sender, claude_index, payload).await
}

async fn maybe_send_signature_delta(
    choice: &StreamChoice,
    sender: &mut BodySender,
    state: &mut StreamState,
) -> io::Result<()> {
    let Some(claude_index) = state.thinking_block_index else {
        return Ok(());
    };
    let Some(payload) = thinking_signature_delta(choice) else {
        return Ok(());
    };

    state.saw_thinking_delta = true;
    send_signature_delta(sender, claude_index, payload).await
}

fn log_fallback_start(
    choice: &StreamChoice,
    state: &StreamState,
    context: &ThinkingFallbackContext<'_>,
) {
    info!(
        phase = "thinking_fallback_start",
        model = context.model,
        message_id = context.message_id,
        claude_index = state.thinking_block_index.unwrap_or(0),
        stop_reason = state.final_stop_reason,
        has_content_delta = content_delta(choice).is_some(),
        has_tool_delta = tool_call_deltas(choice)
            .map(|value| !value.is_empty())
            .unwrap_or(false),
        has_finish_reason = choice.finish_reason.is_some(),
        started_tool_calls = state
            .tool_calls
            .values()
            .filter(|tool| tool.started)
            .count(),
        "Upstream reasoning absent; emitting realtime empty thinking block"
    );
}
