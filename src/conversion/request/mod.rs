mod assistant;
mod models;
mod system;
mod tool_result;
mod tools;
mod user;

pub use models::{OpenAiChatRequest, OpenAiMessage, OpenAiUserMessage};
pub use tools::is_thinking_requested;

use std::collections::HashSet;

use tracing::{debug, trace, warn};

use crate::config::Config;
use crate::constants::{ROLE_ASSISTANT, ROLE_USER};
use crate::models::{ClaudeMessage, ClaudeMessagesRequest};
use assistant::convert_claude_assistant_message;
use models::{OpenAiSystemMessage, map_claude_model_to_openai};
use system::extract_system_text;
use tool_result::{
    convert_claude_tool_results, has_non_tool_result_content, is_tool_result_user_message,
};
use tools::{add_optional_request_fields, add_tool_choice, add_tools, derive_reasoning_effort};
use user::convert_claude_user_message;

pub fn convert_claude_to_openai(
    request: &ClaudeMessagesRequest,
    config: &Config,
) -> OpenAiChatRequest {
    let mapped_model = map_claude_model_to_openai(&request.model, config);
    let thinking_type = request
        .thinking
        .as_ref()
        .and_then(|value| value.thinking_type.as_deref())
        .unwrap_or("none");
    let thinking_budget_tokens = request
        .thinking
        .as_ref()
        .and_then(|value| value.budget_tokens);
    let mapped_reasoning_effort =
        derive_reasoning_effort(request.thinking.as_ref(), request.max_tokens, &mapped_model);

    debug!(
        phase = "model_routing",
        claude_model = %request.model,
        upstream_model = %mapped_model,
        thinking_type,
        thinking_budget_tokens = ?thinking_budget_tokens,
        reasoning_effort = mapped_reasoning_effort.as_deref().unwrap_or("none"),
        "Model routing"
    );
    let mut openai_messages: Vec<OpenAiMessage> = Vec::new();

    push_system_message(request, &mut openai_messages);
    convert_message_list(
        &request.messages,
        &mut openai_messages,
        config.debug_tool_id_matching,
    );

    let mut openai_request = build_request_base(request, mapped_model, openai_messages);
    add_optional_request_fields(request, &mut openai_request);
    add_tools(request, &mut openai_request);
    add_tool_choice(request, &mut openai_request);

    trace!(
        phase = "upstream_request_full",
        openai_request = ?openai_request,
        "Converted request for upstream (full)"
    );

    let messages_len = openai_request.messages.len();
    let tools_len = openai_request
        .tools
        .as_ref()
        .map(|value| value.len())
        .unwrap_or(0);

    debug!(
        phase = "upstream_request_summary",
        upstream_model = %openai_request.model,
        stream = openai_request.stream,
        max_tokens = openai_request.max_tokens,
        temperature = openai_request.temperature,
        messages_len,
        tools_len,
        has_tool_choice = openai_request.tool_choice.is_some(),
        "Converted request for upstream (summary)"
    );

    openai_request
}

fn push_system_message(request: &ClaudeMessagesRequest, openai_messages: &mut Vec<OpenAiMessage>) {
    let Some(system) = &request.system else {
        return;
    };
    let system_text = extract_system_text(system);
    if system_text.trim().is_empty() {
        return;
    }
    openai_messages.push(OpenAiMessage::System(OpenAiSystemMessage::from_text(
        system_text.trim().to_string(),
    )));
}

fn convert_message_list(
    messages: &[ClaudeMessage],
    openai_messages: &mut Vec<OpenAiMessage>,
    debug_tool_id_matching: bool,
) {
    let mut seen_tool_call_ids = HashSet::new();

    for message in messages {
        if message.role == ROLE_USER {
            if is_tool_result_user_message(message) {
                for tool_message in convert_claude_tool_results(message) {
                    let Some(tool_call_id) = tool_message.tool_call_id() else {
                        warn!(
                            phase = "drop_tool_result",
                            reason = "missing_tool_call_id_in_converted_message",
                            "Dropping converted tool message"
                        );
                        continue;
                    };

                    let normalized_tool_call_id = tool_call_id.trim();
                    if !seen_tool_call_ids.contains(normalized_tool_call_id) {
                        if debug_tool_id_matching {
                            let mut known_tool_call_ids: Vec<&str> =
                                seen_tool_call_ids.iter().map(String::as_str).collect();
                            known_tool_call_ids.sort_unstable();

                            warn!(
                                phase = "drop_tool_result",
                                reason = "unknown_tool_call_id",
                                tool_call_id = normalized_tool_call_id,
                                known_ids_count = known_tool_call_ids.len(),
                                ?known_tool_call_ids,
                                "Dropping tool message with unknown tool_call_id"
                            );
                        } else {
                            warn!(
                                phase = "drop_tool_result",
                                reason = "unknown_tool_call_id",
                                tool_call_id = normalized_tool_call_id,
                                known_ids_count = seen_tool_call_ids.len(),
                                "Dropping tool message with unknown tool_call_id"
                            );
                        }
                        continue;
                    }

                    openai_messages.push(tool_message);
                }
            }

            if has_non_tool_result_content(message) {
                openai_messages.push(convert_claude_user_message(message));
            }
            continue;
        }

        if message.role == ROLE_ASSISTANT {
            let assistant_message = convert_claude_assistant_message(message);

            if let Some(tool_calls) = assistant_message.assistant_tool_calls() {
                for tool_call in tool_calls {
                    let normalized_tool_call_id = tool_call.id.trim();
                    if !normalized_tool_call_id.is_empty() {
                        seen_tool_call_ids.insert(normalized_tool_call_id.to_string());
                    }
                }
            }

            openai_messages.push(assistant_message);
        }
    }
}

fn build_request_base(
    request: &ClaudeMessagesRequest,
    mapped_model: String,
    openai_messages: Vec<OpenAiMessage>,
) -> OpenAiChatRequest {
    OpenAiChatRequest {
        model: mapped_model,
        messages: openai_messages,
        max_tokens: request.max_tokens,
        temperature: request.temperature.unwrap_or(1.0),
        reasoning_effort: None,
        stream: request.stream.unwrap_or(false),
        stream_options: None,
        stop: None,
        top_p: None,
        tools: None,
        tool_choice: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::models::{ClaudeContent, ClaudeContentBlock};
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
            debug_tool_id_matching: false,
            big_model: "gpt-4o".to_string(),
            middle_model: "gpt-4o".to_string(),
            small_model: "gpt-4o-mini".to_string(),
            custom_headers: Default::default(),
        }
    }

    fn make_request(messages: Vec<ClaudeMessage>) -> ClaudeMessagesRequest {
        ClaudeMessagesRequest {
            model: "claude-3-5-sonnet-20241022".to_string(),
            max_tokens: 256,
            messages,
            thinking: None,
            system: None,
            stop_sequences: None,
            stream: Some(false),
            temperature: Some(1.0),
            top_p: None,
            tools: None,
            tool_choice: None,
        }
    }

    #[test]
    fn preserves_tool_result_and_non_tool_user_content() {
        let request = make_request(vec![
            ClaudeMessage {
                role: ROLE_ASSISTANT.to_string(),
                content: Some(ClaudeContent::Blocks(vec![ClaudeContentBlock::ToolUse {
                    id: Some("call_test123".to_string()),
                    name: Some("Bash".to_string()),
                    input: Some(json!({"command": "cargo fmt"})),
                    extra: Default::default(),
                }])),
            },
            ClaudeMessage {
                role: ROLE_USER.to_string(),
                content: Some(ClaudeContent::Blocks(vec![
                    ClaudeContentBlock::ToolResult {
                        tool_use_id: Some("call_test123".to_string()),
                        content: Some(json!("ok")),
                        extra: Default::default(),
                    },
                    ClaudeContentBlock::Text {
                        text: "继续".to_string(),
                        extra: Default::default(),
                    },
                ])),
            },
        ]);

        let converted = convert_claude_to_openai(&request, &test_config());
        let messages = &converted.messages;

        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].role(), "assistant");
        assert_eq!(messages[1].role(), "tool");
        assert_eq!(messages[1].tool_call_id(), Some("call_test123"));
        assert_eq!(messages[2].role(), "user");
    }

    #[test]
    fn drops_assistant_tool_use_with_empty_id() {
        let request = make_request(vec![ClaudeMessage {
            role: ROLE_ASSISTANT.to_string(),
            content: Some(ClaudeContent::Blocks(vec![ClaudeContentBlock::ToolUse {
                id: Some("   ".to_string()),
                name: Some("Bash".to_string()),
                input: Some(json!({"command": "cargo fmt"})),
                extra: Default::default(),
            }])),
        }]);

        let converted = convert_claude_to_openai(&request, &test_config());
        let messages = &converted.messages;

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role(), "assistant");
        assert!(messages[0].assistant_tool_calls().is_none());
    }

    #[test]
    fn drops_tool_result_with_empty_tool_use_id() {
        let request = make_request(vec![
            ClaudeMessage {
                role: ROLE_ASSISTANT.to_string(),
                content: Some(ClaudeContent::Blocks(vec![ClaudeContentBlock::ToolUse {
                    id: Some("call_test123".to_string()),
                    name: Some("Bash".to_string()),
                    input: Some(json!({"command": "cargo fmt"})),
                    extra: Default::default(),
                }])),
            },
            ClaudeMessage {
                role: ROLE_USER.to_string(),
                content: Some(ClaudeContent::Blocks(vec![
                    ClaudeContentBlock::ToolResult {
                        tool_use_id: Some("   ".to_string()),
                        content: Some(json!("ok")),
                        extra: Default::default(),
                    },
                    ClaudeContentBlock::Text {
                        text: "继续".to_string(),
                        extra: Default::default(),
                    },
                ])),
            },
        ]);

        let converted = convert_claude_to_openai(&request, &test_config());
        let messages = &converted.messages;

        assert_eq!(messages.len(), 2);
        assert!(messages.iter().all(|message| message.role() != "tool"));
        assert_eq!(messages[1].role(), "user");
    }

    #[test]
    fn drops_tool_result_with_unknown_tool_call_id() {
        let request = make_request(vec![
            ClaudeMessage {
                role: ROLE_ASSISTANT.to_string(),
                content: Some(ClaudeContent::Blocks(vec![ClaudeContentBlock::ToolUse {
                    id: Some("call_known".to_string()),
                    name: Some("Bash".to_string()),
                    input: Some(json!({"command": "cargo fmt"})),
                    extra: Default::default(),
                }])),
            },
            ClaudeMessage {
                role: ROLE_USER.to_string(),
                content: Some(ClaudeContent::Blocks(vec![
                    ClaudeContentBlock::ToolResult {
                        tool_use_id: Some("call_unknown".to_string()),
                        content: Some(json!("ok")),
                        extra: Default::default(),
                    },
                ])),
            },
        ]);

        let converted = convert_claude_to_openai(&request, &test_config());
        let messages = &converted.messages;

        assert_eq!(messages.len(), 1);
        assert!(messages.iter().all(|message| message.role() != "tool"));
    }

    #[test]
    fn preserves_user_text_when_unknown_tool_result_filtered() {
        let request = make_request(vec![
            ClaudeMessage {
                role: ROLE_ASSISTANT.to_string(),
                content: Some(ClaudeContent::Blocks(vec![ClaudeContentBlock::ToolUse {
                    id: Some("call_known".to_string()),
                    name: Some("Bash".to_string()),
                    input: Some(json!({"command": "cargo fmt"})),
                    extra: Default::default(),
                }])),
            },
            ClaudeMessage {
                role: ROLE_USER.to_string(),
                content: Some(ClaudeContent::Blocks(vec![
                    ClaudeContentBlock::ToolResult {
                        tool_use_id: Some("call_unknown".to_string()),
                        content: Some(json!("ok")),
                        extra: Default::default(),
                    },
                    ClaudeContentBlock::Text {
                        text: "继续".to_string(),
                        extra: Default::default(),
                    },
                ])),
            },
        ]);

        let converted = convert_claude_to_openai(&request, &test_config());
        let messages = &converted.messages;

        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role(), "assistant");
        assert_eq!(messages[1].role(), "user");
    }
}
