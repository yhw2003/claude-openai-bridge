use serde::Deserialize;
use serde_json::{Map, Value};

use crate::constants::TOOL_FUNCTION;
use crate::conversion::request::models::{
    OpenAiChatRequest, OpenAiFunctionDefinition, OpenAiToolChoice, OpenAiToolDefinition,
    supports_reasoning_effort,
};
use crate::models::{ClaudeMessagesRequest, ClaudeThinking, ClaudeToolChoice};

pub fn add_optional_request_fields(
    request: &ClaudeMessagesRequest,
    openai_request: &mut OpenAiChatRequest,
) {
    if let Some(stop_sequences) = &request.stop_sequences {
        openai_request.stop = Some(stop_sequences.clone());
    }
    if let Some(top_p) = request.top_p {
        openai_request.top_p = Some(top_p);
    }

    openai_request.reasoning_effort = derive_reasoning_effort(
        request.thinking.as_ref(),
        request.max_tokens,
        &openai_request.model,
    );
}

pub fn derive_reasoning_effort(
    thinking: Option<&ClaudeThinking>,
    max_tokens: u32,
    upstream_model: &str,
) -> Option<String> {
    if !supports_reasoning_effort(upstream_model) {
        return None;
    }

    let effort = thinking
        .and_then(|thinking| {
            if !is_thinking_requested(Some(thinking)) {
                return None;
            }

            Some(match thinking.budget_tokens {
                Some(budget_tokens) => {
                    let absolute_effort = effort_by_absolute_budget(budget_tokens);
                    let ratio_effort = effort_by_budget_ratio(budget_tokens, max_tokens);
                    higher_effort(absolute_effort, ratio_effort)
                }
                None => "medium",
            })
        })
        .unwrap_or("low");

    Some(effort.to_string())
}

pub fn is_thinking_requested(thinking: Option<&ClaudeThinking>) -> bool {
    let Some(thinking) = thinking else {
        return false;
    };

    thinking_enabled(
        thinking.thinking_type.as_deref(),
        thinking.budget_tokens.is_some(),
    )
}

fn thinking_enabled(mode: Option<&str>, has_budget_tokens: bool) -> bool {
    match mode.map(|value| value.trim().to_lowercase()) {
        Some(value) if matches!(value.as_str(), "disabled" | "off" | "none") => false,
        Some(value) if matches!(value.as_str(), "enabled" | "on" | "auto") => true,
        Some(_) => true,
        None => has_budget_tokens,
    }
}

fn effort_by_absolute_budget(budget_tokens: u32) -> &'static str {
    let clamped = budget_tokens.clamp(1, 65_536);
    if clamped <= 2_048 {
        "low"
    } else if clamped <= 8_192 {
        "medium"
    } else {
        "high"
    }
}

fn effort_by_budget_ratio(budget_tokens: u32, max_tokens: u32) -> &'static str {
    if max_tokens == 0 {
        return "medium";
    }

    let ratio = budget_tokens as f64 / max_tokens as f64;
    if ratio < 0.25 {
        "low"
    } else if ratio <= 0.6 {
        "medium"
    } else {
        "high"
    }
}

fn higher_effort(left: &'static str, right: &'static str) -> &'static str {
    if effort_rank(left) >= effort_rank(right) {
        left
    } else {
        right
    }
}

fn effort_rank(value: &str) -> u8 {
    match value {
        "high" => 3,
        "medium" => 2,
        "low" => 1,
        _ => 0,
    }
}

pub fn add_tools(request: &ClaudeMessagesRequest, openai_request: &mut OpenAiChatRequest) {
    let Some(tools) = &request.tools else {
        return;
    };

    let converted_tools: Vec<OpenAiToolDefinition> =
        tools.iter().filter_map(convert_single_tool).collect();
    if converted_tools.is_empty() {
        return;
    }
    openai_request.tools = Some(converted_tools);
}

fn convert_single_tool(tool: &crate::models::ClaudeToolDefinition) -> Option<OpenAiToolDefinition> {
    let name = tool.name.as_deref().unwrap_or_default().trim().to_string();
    if name.is_empty() {
        return None;
    }

    let description = tool.description.as_deref().unwrap_or_default().to_string();
    let parameters = tool
        .input_schema
        .clone()
        .unwrap_or_else(default_tool_parameters);

    Some(OpenAiToolDefinition {
        kind: TOOL_FUNCTION.to_string(),
        function: OpenAiFunctionDefinition {
            name,
            description,
            parameters,
        },
    })
}

pub fn add_tool_choice(request: &ClaudeMessagesRequest, openai_request: &mut OpenAiChatRequest) {
    let Some(tool_choice) = &request.tool_choice else {
        return;
    };

    openai_request.tool_choice = Some(match tool_choice {
        ClaudeToolChoice::Mode(choice_type) => match choice_type.as_str() {
            "auto" | "any" => OpenAiToolChoice::auto(),
            _ => OpenAiToolChoice::auto(),
        },
        ClaudeToolChoice::Named(named_choice) => match named_choice.choice_type.as_deref() {
            Some("tool") => create_tool_choice_payload(named_choice.name.as_deref()),
            Some("auto") | Some("any") => OpenAiToolChoice::auto(),
            _ => OpenAiToolChoice::auto(),
        },
        ClaudeToolChoice::Other(value) => create_tool_choice_from_value(value),
    });
}

fn create_tool_choice_payload(selected_name: Option<&str>) -> OpenAiToolChoice {
    match selected_name {
        Some(name) => OpenAiToolChoice::tool(name.to_string()),
        None => OpenAiToolChoice::auto(),
    }
}

fn create_tool_choice_from_value(value: &Value) -> OpenAiToolChoice {
    match serde_json::from_value::<LooseToolChoicePayload>(value.clone()) {
        Ok(parsed) => map_loose_tool_choice_payload(parsed),
        Err(_) => OpenAiToolChoice::auto(),
    }
}

fn map_loose_tool_choice_payload(payload: LooseToolChoicePayload) -> OpenAiToolChoice {
    match payload.choice_type.as_deref() {
        Some("auto") | Some("any") => OpenAiToolChoice::auto(),
        Some("tool") => create_tool_choice_payload(payload.name.as_deref()),
        _ => OpenAiToolChoice::auto(),
    }
}

fn default_tool_parameters() -> Value {
    let properties = Map::new();
    let mut object = Map::new();
    object.insert("type".to_string(), Value::String("object".to_string()));
    object.insert("properties".to_string(), Value::Object(properties));
    Value::Object(object)
}

#[derive(Debug, Deserialize)]
struct LooseToolChoicePayload {
    #[serde(rename = "type")]
    choice_type: Option<String>,
    name: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::derive_reasoning_effort;
    use crate::models::ClaudeThinking;

    #[test]
    fn defaults_to_low_when_thinking_missing() {
        let effort = derive_reasoning_effort(None, 4_096, "o3-mini");
        assert_eq!(effort.as_deref(), Some("low"));
    }

    #[test]
    fn defaults_to_low_when_thinking_disabled() {
        let thinking = ClaudeThinking {
            thinking_type: Some("disabled".to_string()),
            budget_tokens: Some(12_000),
        };
        let effort = derive_reasoning_effort(Some(&thinking), 4_096, "o3-mini");
        assert_eq!(effort.as_deref(), Some("low"));
    }

    #[test]
    fn maps_budget_to_high_effort() {
        let thinking = ClaudeThinking {
            thinking_type: Some("enabled".to_string()),
            budget_tokens: Some(10_000),
        };
        let effort = derive_reasoning_effort(Some(&thinking), 16_000, "o3-mini");
        assert_eq!(effort.as_deref(), Some("high"));
    }

    #[test]
    fn maps_missing_budget_to_medium_effort() {
        let thinking = ClaudeThinking {
            thinking_type: Some("enabled".to_string()),
            budget_tokens: None,
        };
        let effort = derive_reasoning_effort(Some(&thinking), 4_096, "o3-mini");
        assert_eq!(effort.as_deref(), Some("medium"));
    }

    #[test]
    fn skips_reasoning_effort_for_unsupported_models() {
        let thinking = ClaudeThinking {
            thinking_type: Some("enabled".to_string()),
            budget_tokens: Some(8_192),
        };
        let effort = derive_reasoning_effort(Some(&thinking), 8_192, "gpt-4o");
        assert!(effort.is_none());
    }
}
