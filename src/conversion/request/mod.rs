mod assistant;
mod models;
mod system;
mod tool_result;
mod tools;
mod user;

use serde_json::{Value, json};
use tracing::{debug, trace};

use crate::config::Config;
use crate::constants::{ROLE_ASSISTANT, ROLE_SYSTEM, ROLE_USER};
use crate::models::{ClaudeMessage, ClaudeMessagesRequest};
use assistant::convert_claude_assistant_message;
use models::map_claude_model_to_openai;
use system::extract_system_text;
use tool_result::{
    convert_claude_tool_results, has_non_tool_result_content, is_tool_result_user_message,
};
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

    let mut openai_request = build_request_base(request, mapped_model, openai_messages);
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
    for message in messages {
        if message.role == ROLE_USER {
            if is_tool_result_user_message(message) {
                openai_messages.extend(convert_claude_tool_results(message));
            }

            if has_non_tool_result_content(message) {
                openai_messages.push(convert_claude_user_message(message));
            }
            continue;
        }

        if message.role == ROLE_ASSISTANT {
            openai_messages.push(convert_claude_assistant_message(message));
        }
    }
}

fn build_request_base(
    request: &ClaudeMessagesRequest,
    mapped_model: String,
    openai_messages: Vec<Value>,
) -> Value {
    json!({
        "model": mapped_model,
        "messages": openai_messages,
        "max_tokens": request.max_tokens,
        "temperature": request.temperature.unwrap_or(1.0),
        "stream": request.stream.unwrap_or(false),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use serde_json::json;

    fn test_config() -> Config {
        Config {
            openai_api_key: "sk-test".to_string(),
            anthropic_api_key: None,
            openai_base_url: "https://api.openai.com/v1".to_string(),
            azure_api_version: None,
            host: "127.0.0.1".to_string(),
            port: 8082,
            log_level: "INFO".to_string(),
            request_timeout: 90,
            stream_request_timeout: None,
            request_body_max_size: 16 * 1024 * 1024,
            big_model: "gpt-4o".to_string(),
            middle_model: "gpt-4o".to_string(),
            small_model: "gpt-4o-mini".to_string(),
            custom_headers: Default::default(),
        }
    }

    #[test]
    fn preserves_tool_result_and_non_tool_user_content() {
        let request = ClaudeMessagesRequest {
            model: "claude-3-5-sonnet-20241022".to_string(),
            max_tokens: 256,
            messages: vec![
                ClaudeMessage {
                    role: ROLE_ASSISTANT.to_string(),
                    content: Some(json!([
                        {
                            "type": "tool_use",
                            "id": "call_test123",
                            "name": "Bash",
                            "input": {"command": "cargo fmt"}
                        }
                    ])),
                },
                ClaudeMessage {
                    role: ROLE_USER.to_string(),
                    content: Some(json!([
                        {
                            "type": "tool_result",
                            "tool_use_id": "call_test123",
                            "content": "ok"
                        },
                        {
                            "type": "text",
                            "text": "继续"
                        }
                    ])),
                },
            ],
            system: None,
            stop_sequences: None,
            stream: Some(false),
            temperature: Some(1.0),
            top_p: None,
            tools: None,
            tool_choice: None,
        };

        let converted = convert_claude_to_openai(&request, &test_config());
        let messages = converted
            .get("messages")
            .and_then(Value::as_array)
            .expect("messages should be an array");

        assert_eq!(messages.len(), 3);
        assert_eq!(
            messages[0].get("role").and_then(Value::as_str),
            Some("assistant")
        );
        assert_eq!(
            messages[1].get("role").and_then(Value::as_str),
            Some("tool")
        );
        assert_eq!(
            messages[1].get("tool_call_id").and_then(Value::as_str),
            Some("call_test123")
        );
        assert_eq!(
            messages[2].get("role").and_then(Value::as_str),
            Some("user")
        );
    }
}
