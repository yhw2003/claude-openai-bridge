use serde_json::{Value, json};

use crate::config::Config;
use crate::constants::{ROLE_ASSISTANT, ROLE_USER, TOOL_FUNCTION};
use crate::models::ClaudeMessagesRequest;

use super::convert_claude_to_openai;
use super::models::{
    OpenAiMessage, OpenAiToolChoice, OpenAiToolDefinition, OpenAiUserContent, OpenAiUserContentPart,
};
use super::responses_models::{
    OpenAiResponsesRequest, ResponsesFunctionCallItem, ResponsesFunctionCallOutputItem,
    ResponsesInputItem, ResponsesMessageContent, ResponsesMessageContentPart, ResponsesMessageItem,
    ResponsesReasoning, ResponsesToolDefinition,
};

pub fn convert_claude_to_responses(
    request: &ClaudeMessagesRequest,
    config: &Config,
) -> OpenAiResponsesRequest {
    let chat_request = convert_claude_to_openai(request, config);
    convert_chat_request_to_responses(chat_request)
}

fn convert_chat_request_to_responses(
    chat_request: super::models::OpenAiChatRequest,
) -> OpenAiResponsesRequest {
    let mut input = Vec::new();
    let mut instructions = None;

    for message in chat_request.messages {
        convert_message_to_input_item(message, &mut input, &mut instructions);
    }

    OpenAiResponsesRequest {
        model: chat_request.model,
        input,
        instructions,
        max_output_tokens: Some(chat_request.max_tokens),
        temperature: Some(chat_request.temperature),
        top_p: chat_request.top_p,
        stop: chat_request.stop,
        reasoning: map_reasoning(chat_request.reasoning_effort),
        tools: map_tools(chat_request.tools),
        tool_choice: map_tool_choice(chat_request.tool_choice),
        stream: chat_request.stream,
    }
}

fn convert_message_to_input_item(
    message: OpenAiMessage,
    input: &mut Vec<ResponsesInputItem>,
    instructions: &mut Option<String>,
) {
    match message {
        OpenAiMessage::System(system_message) => {
            append_instruction(instructions, &system_message.content)
        }
        OpenAiMessage::User(user_message) => {
            input.push(ResponsesInputItem::Message(ResponsesMessageItem {
                role: ROLE_USER.to_string(),
                content: map_user_content(user_message.content),
            }));
        }
        OpenAiMessage::Assistant(assistant_message) => {
            push_assistant_text(input, assistant_message.content);
            push_assistant_tool_calls(input, assistant_message.tool_calls);
        }
        OpenAiMessage::Tool(tool_message) => {
            input.push(ResponsesInputItem::FunctionCallOutput(
                ResponsesFunctionCallOutputItem {
                    item_type: "function_call_output".to_string(),
                    call_id: tool_message.tool_call_id,
                    output: tool_message.content,
                },
            ));
        }
    }
}

fn append_instruction(instructions: &mut Option<String>, system_text: &str) {
    if system_text.trim().is_empty() {
        return;
    }

    match instructions {
        Some(existing) => {
            existing.push_str("\n\n");
            existing.push_str(system_text);
        }
        None => *instructions = Some(system_text.to_string()),
    }
}

fn map_user_content(content: OpenAiUserContent) -> ResponsesMessageContent {
    match content {
        OpenAiUserContent::Text(text) => ResponsesMessageContent::Text(text),
        OpenAiUserContent::Parts(parts) => {
            let mapped_parts = parts.into_iter().map(map_user_content_part).collect();
            ResponsesMessageContent::Parts(mapped_parts)
        }
    }
}

fn map_user_content_part(part: OpenAiUserContentPart) -> ResponsesMessageContentPart {
    match part {
        OpenAiUserContentPart::Text { text } => ResponsesMessageContentPart::InputText { text },
        OpenAiUserContentPart::ImageUrl { image_url } => ResponsesMessageContentPart::InputImage {
            image_url: image_url.url,
        },
    }
}

fn push_assistant_text(input: &mut Vec<ResponsesInputItem>, assistant_text: Option<String>) {
    let Some(text) = assistant_text.map(|value| value.trim().to_string()) else {
        return;
    };
    if text.is_empty() {
        return;
    }

    input.push(ResponsesInputItem::Message(ResponsesMessageItem {
        role: ROLE_ASSISTANT.to_string(),
        content: ResponsesMessageContent::Text(text),
    }));
}

fn push_assistant_tool_calls(
    input: &mut Vec<ResponsesInputItem>,
    tool_calls: Option<Vec<super::models::OpenAiToolCall>>,
) {
    let Some(tool_calls) = tool_calls else {
        return;
    };

    for tool_call in tool_calls {
        input.push(ResponsesInputItem::FunctionCall(
            ResponsesFunctionCallItem {
                item_type: "function_call".to_string(),
                call_id: tool_call.id,
                name: tool_call.function.name,
                arguments: tool_call.function.arguments,
            },
        ));
    }
}

fn map_reasoning(reasoning_effort: Option<String>) -> Option<ResponsesReasoning> {
    reasoning_effort.map(|effort| ResponsesReasoning { effort })
}

fn map_tool_choice(tool_choice: Option<OpenAiToolChoice>) -> Option<Value> {
    match tool_choice {
        Some(OpenAiToolChoice::Auto(_)) => Some(json!("auto")),
        Some(OpenAiToolChoice::Tool(named)) => Some(json!({
            "type": TOOL_FUNCTION,
            "name": named.function.name
        })),
        None => None,
    }
}

fn map_tools(tools: Option<Vec<OpenAiToolDefinition>>) -> Option<Vec<ResponsesToolDefinition>> {
    let tools = tools?;
    let converted: Vec<ResponsesToolDefinition> = tools.into_iter().map(map_single_tool).collect();
    if converted.is_empty() {
        None
    } else {
        Some(converted)
    }
}

fn map_single_tool(tool: OpenAiToolDefinition) -> ResponsesToolDefinition {
    ResponsesToolDefinition {
        kind: tool.kind,
        name: tool.function.name,
        description: tool.function.description,
        parameters: tool.function.parameters,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use crate::config::{Config, WireApi};
    use crate::models::{
        ClaudeContent, ClaudeContentBlock, ClaudeMessage, ClaudeMessagesRequest, ClaudeToolChoice,
        ClaudeToolDefinition,
    };

    use super::convert_claude_to_responses;

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
            wire_api: WireApi::Responses,
            big_model: "gpt-4o".to_string(),
            middle_model: "gpt-4o".to_string(),
            small_model: "gpt-4o-mini".to_string(),
            custom_headers: Default::default(),
        }
    }

    #[test]
    fn converts_tools_and_tool_choice() {
        let request = ClaudeMessagesRequest {
            model: "claude-3-5-sonnet-20241022".to_string(),
            max_tokens: 256,
            messages: vec![ClaudeMessage {
                role: "user".to_string(),
                content: Some(ClaudeContent::Text("hello".to_string())),
            }],
            thinking: None,
            system: Some(crate::models::ClaudeSystemContent::Text(
                "be brief".to_string(),
            )),
            stop_sequences: Some(vec!["stop".to_string()]),
            stream: Some(false),
            temperature: Some(0.5),
            top_p: Some(0.8),
            tools: Some(vec![ClaudeToolDefinition {
                name: Some("Bash".to_string()),
                description: Some("run shell".to_string()),
                input_schema: Some(serde_json::json!({"type":"object"})),
                extra: Default::default(),
            }]),
            tool_choice: Some(ClaudeToolChoice::Mode("auto".to_string())),
        };

        let converted = convert_claude_to_responses(&request, &test_config());

        assert_eq!(converted.instructions.as_deref(), Some("be brief"));
        assert_eq!(converted.max_output_tokens, Some(256));
        assert_eq!(converted.temperature, Some(0.5));
        assert_eq!(converted.stop, Some(vec!["stop".to_string()]));
        assert!(
            converted
                .tools
                .as_ref()
                .map(|v| !v.is_empty())
                .unwrap_or(false)
        );
        let payload = serde_json::to_value(&converted).expect("serialize request");
        let tools = payload
            .get("tools")
            .and_then(Value::as_array)
            .expect("tools array");
        assert_eq!(tools[0].get("name").and_then(Value::as_str), Some("Bash"));
        assert!(tools[0].get("function").is_none());
        assert_eq!(
            converted.tool_choice,
            Some(Value::String("auto".to_string()))
        );
    }

    #[test]
    fn converts_assistant_tool_calls_to_function_call_items() {
        let request = ClaudeMessagesRequest {
            model: "claude-3-5-sonnet-20241022".to_string(),
            max_tokens: 256,
            messages: vec![ClaudeMessage {
                role: "assistant".to_string(),
                content: Some(ClaudeContent::Blocks(vec![ClaudeContentBlock::ToolUse {
                    id: Some("call_abc".to_string()),
                    name: Some("Bash".to_string()),
                    input: Some(serde_json::json!({"command":"cargo check"})),
                    extra: Default::default(),
                }])),
            }],
            thinking: None,
            system: None,
            stop_sequences: None,
            stream: Some(false),
            temperature: Some(1.0),
            top_p: None,
            tools: None,
            tool_choice: None,
        };

        let converted = convert_claude_to_responses(&request, &test_config());
        let payload = serde_json::to_value(converted).expect("serialize request");
        let input = payload
            .get("input")
            .and_then(Value::as_array)
            .expect("input array");

        assert_eq!(input.len(), 1);
        assert_eq!(
            input[0].get("type").and_then(Value::as_str),
            Some("function_call")
        );
        assert_eq!(
            input[0].get("call_id").and_then(Value::as_str),
            Some("call_abc")
        );
    }
}
