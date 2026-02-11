mod assistant;
mod models;
mod system;
mod tool_result;
mod tools;
mod user;

use std::cmp::{max, min};

use serde_json::{Value, json};
use tracing::{debug, trace};

use crate::config::Config;
use crate::constants::{ROLE_ASSISTANT, ROLE_SYSTEM, ROLE_USER};
use crate::models::{ClaudeMessage, ClaudeMessagesRequest};
use assistant::convert_claude_assistant_message;
use models::map_claude_model_to_openai;
use system::extract_system_text;
use tool_result::{convert_claude_tool_results, is_tool_result_user_message};
use tools::{add_optional_request_fields, add_tool_choice, add_tools};
use user::convert_claude_user_message;

pub fn convert_claude_to_openai(request: &ClaudeMessagesRequest, config: &Config) -> Value {
    let mapped_model = map_claude_model_to_openai(&request.model, config);
    debug!(
        phase = "model_routing",
        claude_model = %request.model,
        upstream_model = %mapped_model,
        "Model routing"
    );
    let mut openai_messages: Vec<Value> = Vec::new();

    push_system_message(request, &mut openai_messages);
    convert_message_list(&request.messages, &mut openai_messages);

    let mut openai_request = build_request_base(request, config, mapped_model, openai_messages);
    add_optional_request_fields(request, &mut openai_request);
    add_tools(request, &mut openai_request);
    add_tool_choice(request, &mut openai_request);

    trace!(
        phase = "upstream_request_full",
        openai_request = %openai_request,
        "Converted request for upstream (full)"
    );

    let messages_len = openai_request
        .get("messages")
        .and_then(|value| value.as_array())
        .map(|value| value.len())
        .unwrap_or(0);
    let tools_len = openai_request
        .get("tools")
        .and_then(|value| value.as_array())
        .map(|value| value.len())
        .unwrap_or(0);

    let upstream_model = openai_request
        .get("model")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let stream = openai_request
        .get("stream")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let max_tokens = openai_request
        .get("max_tokens")
        .and_then(|value| value.as_u64());
    let temperature = openai_request
        .get("temperature")
        .and_then(|value| value.as_f64());

    debug!(
        phase = "upstream_request_summary",
        upstream_model = %upstream_model,
        stream,
        max_tokens = ?max_tokens,
        temperature = ?temperature,
        messages_len,
        tools_len,
        has_tool_choice = openai_request.get("tool_choice").is_some(),
        "Converted request for upstream (summary)"
    );
    openai_request
}

fn push_system_message(request: &ClaudeMessagesRequest, openai_messages: &mut Vec<Value>) {
    let Some(system) = &request.system else {
        return;
    };
    let system_text = extract_system_text(system);
    if system_text.trim().is_empty() {
        return;
    }
    openai_messages.push(json!({"role": ROLE_SYSTEM, "content": system_text.trim()}));
}

fn convert_message_list(messages: &[ClaudeMessage], openai_messages: &mut Vec<Value>) {
    let mut index = 0usize;
    while index < messages.len() {
        push_message_with_tool_results(&messages[index], messages, &mut index, openai_messages);
        index += 1;
    }
}

fn push_message_with_tool_results(
    message: &ClaudeMessage,
    messages: &[ClaudeMessage],
    index: &mut usize,
    openai_messages: &mut Vec<Value>,
) {
    if message.role == ROLE_USER {
        openai_messages.push(convert_claude_user_message(message));
        return;
    }
    if message.role != ROLE_ASSISTANT {
        return;
    }

    openai_messages.push(convert_claude_assistant_message(message));
    if *index + 1 >= messages.len() || !is_tool_result_user_message(&messages[*index + 1]) {
        return;
    }

    openai_messages.extend(convert_claude_tool_results(&messages[*index + 1]));
    *index += 1;
}

fn build_request_base(
    request: &ClaudeMessagesRequest,
    config: &Config,
    mapped_model: String,
    openai_messages: Vec<Value>,
) -> Value {
    let bounded_tokens = min(
        max(request.max_tokens, config.min_tokens_limit),
        config.max_tokens_limit,
    );

    json!({
        "model": mapped_model,
        "messages": openai_messages,
        "max_tokens": bounded_tokens,
        "temperature": request.temperature.unwrap_or(1.0),
        "stream": request.stream.unwrap_or(false),
    })
}
