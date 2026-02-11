use salvo::http::body::BodySender;
use serde_json::{Value, json};

use crate::constants::{
    CONTENT_TEXT, DELTA_INPUT_JSON, DELTA_TEXT, EVENT_CONTENT_BLOCK_DELTA,
    EVENT_CONTENT_BLOCK_START, EVENT_CONTENT_BLOCK_STOP, EVENT_MESSAGE_DELTA, EVENT_MESSAGE_START,
    EVENT_MESSAGE_STOP, EVENT_PING, ROLE_ASSISTANT,
};
use crate::conversion::stream::state::StreamState;

pub async fn send_start_sequence(
    sender: &mut BodySender,
    original_model: &str,
    message_id: &str,
) -> std::io::Result<()> {
    send_sse(
        sender,
        EVENT_MESSAGE_START,
        &json!({
            "type": EVENT_MESSAGE_START,
            "message": {
                "id": message_id,
                "type": "message",
                "role": ROLE_ASSISTANT,
                "model": original_model,
                "content": [],
                "stop_reason": Value::Null,
                "stop_sequence": Value::Null,
                "usage": {"input_tokens": 0, "output_tokens": 0}
            }
        }),
    )
    .await?;

    send_sse(
        sender,
        EVENT_CONTENT_BLOCK_START,
        &json!({
            "type": EVENT_CONTENT_BLOCK_START,
            "index": 0,
            "content_block": {"type": CONTENT_TEXT, "text": ""}
        }),
    )
    .await?;

    send_sse(sender, EVENT_PING, &json!({"type": EVENT_PING})).await
}

pub async fn send_text_delta(
    sender: &mut BodySender,
    state: &StreamState,
    content_delta: &str,
) -> std::io::Result<()> {
    send_sse(
        sender,
        EVENT_CONTENT_BLOCK_DELTA,
        &json!({
            "type": EVENT_CONTENT_BLOCK_DELTA,
            "index": state.text_block_index,
            "delta": {"type": DELTA_TEXT, "text": content_delta}
        }),
    )
    .await
}

pub async fn send_tool_block_start(
    sender: &mut BodySender,
    claude_index: usize,
    id: &Option<String>,
    name: &Option<String>,
) -> std::io::Result<()> {
    send_sse(
        sender,
        EVENT_CONTENT_BLOCK_START,
        &json!({
            "type": EVENT_CONTENT_BLOCK_START,
            "index": claude_index,
            "content_block": {
                "type": "tool_use",
                "id": id,
                "name": name,
                "input": {}
            }
        }),
    )
    .await
}

pub async fn send_tool_json_delta(
    sender: &mut BodySender,
    claude_index: usize,
    payload_json: &str,
) -> std::io::Result<()> {
    send_sse(
        sender,
        EVENT_CONTENT_BLOCK_DELTA,
        &json!({
            "type": EVENT_CONTENT_BLOCK_DELTA,
            "index": claude_index,
            "delta": {"type": DELTA_INPUT_JSON, "partial_json": payload_json}
        }),
    )
    .await
}

pub async fn send_stop_sequence(
    sender: &mut BodySender,
    state: &StreamState,
) -> std::io::Result<()> {
    send_sse(
        sender,
        EVENT_CONTENT_BLOCK_STOP,
        &json!({"type": EVENT_CONTENT_BLOCK_STOP, "index": state.text_block_index}),
    )
    .await?;

    for tool_call_state in state.tool_calls.values() {
        let Some(claude_index) =
            crate::conversion::stream::state::started_tool_index(tool_call_state)
        else {
            continue;
        };

        send_sse(
            sender,
            EVENT_CONTENT_BLOCK_STOP,
            &json!({"type": EVENT_CONTENT_BLOCK_STOP, "index": claude_index}),
        )
        .await?;
    }

    send_sse(
        sender,
        EVENT_MESSAGE_DELTA,
        &json!({
            "type": EVENT_MESSAGE_DELTA,
            "delta": {
                "stop_reason": state.final_stop_reason,
                "stop_sequence": Value::Null,
            },
            "usage": state.usage_data,
        }),
    )
    .await?;

    send_sse(
        sender,
        EVENT_MESSAGE_STOP,
        &json!({"type": EVENT_MESSAGE_STOP}),
    )
    .await
}

pub async fn send_error_sse(sender: &mut BodySender, message: &str) -> std::io::Result<()> {
    send_sse(
        sender,
        "error",
        &json!({"type": "error", "error": {"type": "api_error", "message": message}}),
    )
    .await
}

async fn send_sse(sender: &mut BodySender, event: &str, data: &Value) -> std::io::Result<()> {
    let payload = format!(
        "event: {event}\ndata: {}\n\n",
        serde_json::to_string(data).unwrap_or_else(|_| "{}".to_string())
    );
    sender.send_data(payload).await
}
