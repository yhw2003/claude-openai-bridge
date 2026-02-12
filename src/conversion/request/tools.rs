use serde::Deserialize;
use serde_json::{Map, Value};

use crate::constants::TOOL_FUNCTION;
use crate::conversion::request::models::{
    OpenAiChatRequest, OpenAiFunctionDefinition, OpenAiToolChoice, OpenAiToolDefinition,
};
use crate::models::{ClaudeMessagesRequest, ClaudeToolChoice};

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
