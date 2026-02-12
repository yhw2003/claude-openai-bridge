use salvo::http::body::BodySender;
use serde::Serialize;

use crate::constants::{
    CONTENT_TEXT, DELTA_INPUT_JSON, DELTA_TEXT, EVENT_CONTENT_BLOCK_DELTA,
    EVENT_CONTENT_BLOCK_START, EVENT_CONTENT_BLOCK_STOP, EVENT_MESSAGE_DELTA, EVENT_MESSAGE_START,
    EVENT_MESSAGE_STOP, EVENT_PING, ROLE_ASSISTANT,
};
use crate::conversion::stream::state::{StreamState, StreamUsage};

pub async fn send_start_sequence(
    sender: &mut BodySender,
    original_model: &str,
    message_id: &str,
) -> std::io::Result<()> {
    let start_event = MessageStartEvent {
        event_type: EVENT_MESSAGE_START,
        message: MessageStartPayload {
            id: message_id,
            message_type: "message",
            role: ROLE_ASSISTANT,
            model: original_model,
            content: vec![],
            stop_reason: None,
            stop_sequence: None,
            usage: UsageSnapshot {
                input_tokens: 0,
                output_tokens: 0,
            },
        },
    };

    send_sse(sender, EVENT_MESSAGE_START, &start_event).await?;

    let text_block_start = ContentBlockStartEvent {
        event_type: EVENT_CONTENT_BLOCK_START,
        index: 0,
        content_block: TextContentBlock {
            block_type: CONTENT_TEXT,
            text: "",
        },
    };

    send_sse(sender, EVENT_CONTENT_BLOCK_START, &text_block_start).await?;

    send_sse(
        sender,
        EVENT_PING,
        &TypeOnlyEvent {
            event_type: EVENT_PING,
        },
    )
    .await
}

pub async fn send_text_delta(
    sender: &mut BodySender,
    state: &StreamState,
    content_delta: &str,
) -> std::io::Result<()> {
    let event = ContentBlockDeltaEvent {
        event_type: EVENT_CONTENT_BLOCK_DELTA,
        index: state.text_block_index,
        delta: TextDeltaPayload {
            delta_type: DELTA_TEXT,
            text: content_delta,
        },
    };

    send_sse(sender, EVENT_CONTENT_BLOCK_DELTA, &event).await
}

pub async fn send_tool_block_start(
    sender: &mut BodySender,
    claude_index: usize,
    id: &Option<String>,
    name: &Option<String>,
) -> std::io::Result<()> {
    let event = ContentBlockStartEvent {
        event_type: EVENT_CONTENT_BLOCK_START,
        index: claude_index,
        content_block: ToolUseContentBlock {
            block_type: "tool_use",
            id,
            name,
            input: EmptyObject {},
        },
    };

    send_sse(sender, EVENT_CONTENT_BLOCK_START, &event).await
}

pub async fn send_tool_json_delta(
    sender: &mut BodySender,
    claude_index: usize,
    payload_json: &str,
) -> std::io::Result<()> {
    let event = ContentBlockDeltaEvent {
        event_type: EVENT_CONTENT_BLOCK_DELTA,
        index: claude_index,
        delta: JsonDeltaPayload {
            delta_type: DELTA_INPUT_JSON,
            partial_json: payload_json,
        },
    };

    send_sse(sender, EVENT_CONTENT_BLOCK_DELTA, &event).await
}

pub async fn send_stop_sequence(
    sender: &mut BodySender,
    state: &StreamState,
) -> std::io::Result<()> {
    send_sse(
        sender,
        EVENT_CONTENT_BLOCK_STOP,
        &TypeWithIndexEvent {
            event_type: EVENT_CONTENT_BLOCK_STOP,
            index: state.text_block_index,
        },
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
            &TypeWithIndexEvent {
                event_type: EVENT_CONTENT_BLOCK_STOP,
                index: claude_index,
            },
        )
        .await?;
    }

    let message_delta_event = MessageDeltaEvent {
        event_type: EVENT_MESSAGE_DELTA,
        delta: MessageDeltaPayload {
            stop_reason: state.final_stop_reason.as_str(),
            stop_sequence: None,
        },
        usage: &state.usage_data,
    };

    send_sse(sender, EVENT_MESSAGE_DELTA, &message_delta_event).await?;

    send_sse(
        sender,
        EVENT_MESSAGE_STOP,
        &TypeOnlyEvent {
            event_type: EVENT_MESSAGE_STOP,
        },
    )
    .await
}

pub async fn send_error_sse(sender: &mut BodySender, message: &str) -> std::io::Result<()> {
    let event = ErrorEvent {
        event_type: "error",
        error: ApiErrorPayload {
            error_type: "api_error",
            message,
        },
    };

    send_sse(sender, "error", &event).await
}

async fn send_sse<T: Serialize>(
    sender: &mut BodySender,
    event: &str,
    data: &T,
) -> std::io::Result<()> {
    let payload = format!(
        "event: {event}\ndata: {}\n\n",
        serde_json::to_string(data).unwrap_or_else(|_| "{}".to_string())
    );
    sender.send_data(payload).await
}

#[derive(Serialize)]
struct EmptyObject {}

#[derive(Serialize)]
struct UsageSnapshot {
    input_tokens: u64,
    output_tokens: u64,
}

#[derive(Serialize)]
struct MessageStartPayload<'a> {
    id: &'a str,
    #[serde(rename = "type")]
    message_type: &'static str,
    role: &'static str,
    model: &'a str,
    content: Vec<EmptyObject>,
    stop_reason: Option<String>,
    stop_sequence: Option<String>,
    usage: UsageSnapshot,
}

#[derive(Serialize)]
struct MessageStartEvent<'a> {
    #[serde(rename = "type")]
    event_type: &'static str,
    message: MessageStartPayload<'a>,
}

#[derive(Serialize)]
struct TypeOnlyEvent {
    #[serde(rename = "type")]
    event_type: &'static str,
}

#[derive(Serialize)]
struct TypeWithIndexEvent {
    #[serde(rename = "type")]
    event_type: &'static str,
    index: usize,
}

#[derive(Serialize)]
struct TextContentBlock<'a> {
    #[serde(rename = "type")]
    block_type: &'static str,
    text: &'a str,
}

#[derive(Serialize)]
struct ToolUseContentBlock<'a> {
    #[serde(rename = "type")]
    block_type: &'static str,
    id: &'a Option<String>,
    name: &'a Option<String>,
    input: EmptyObject,
}

#[derive(Serialize)]
struct ContentBlockStartEvent<T: Serialize> {
    #[serde(rename = "type")]
    event_type: &'static str,
    index: usize,
    content_block: T,
}

#[derive(Serialize)]
struct TextDeltaPayload<'a> {
    #[serde(rename = "type")]
    delta_type: &'static str,
    text: &'a str,
}

#[derive(Serialize)]
struct JsonDeltaPayload<'a> {
    #[serde(rename = "type")]
    delta_type: &'static str,
    partial_json: &'a str,
}

#[derive(Serialize)]
struct ContentBlockDeltaEvent<T: Serialize> {
    #[serde(rename = "type")]
    event_type: &'static str,
    index: usize,
    delta: T,
}

#[derive(Serialize)]
struct MessageDeltaPayload<'a> {
    stop_reason: &'a str,
    stop_sequence: Option<String>,
}

#[derive(Serialize)]
struct MessageDeltaEvent<'a> {
    #[serde(rename = "type")]
    event_type: &'static str,
    delta: MessageDeltaPayload<'a>,
    usage: &'a StreamUsage,
}

#[derive(Serialize)]
struct ApiErrorPayload<'a> {
    #[serde(rename = "type")]
    error_type: &'static str,
    message: &'a str,
}

#[derive(Serialize)]
struct ErrorEvent<'a> {
    #[serde(rename = "type")]
    event_type: &'static str,
    error: ApiErrorPayload<'a>,
}
