use serde::Serialize;
use serde_json::Value;

use crate::config::Config;
use crate::constants::{ROLE_ASSISTANT, ROLE_SYSTEM, ROLE_TOOL, ROLE_USER, TOOL_FUNCTION};

#[derive(Debug, Clone, Serialize)]
pub struct OpenAiChatRequest {
    pub model: String,
    pub messages: Vec<OpenAiMessage>,
    pub max_tokens: u32,
    pub temperature: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_options: Option<OpenAiStreamOptions>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<OpenAiToolDefinition>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<OpenAiToolChoice>,
}

impl OpenAiChatRequest {
    pub fn enable_stream_usage(&mut self) {
        self.stream = true;
        self.stream_options = Some(OpenAiStreamOptions {
            include_usage: true,
        });
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct OpenAiStreamOptions {
    pub include_usage: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum OpenAiMessage {
    System(OpenAiSystemMessage),
    User(OpenAiUserMessage),
    Assistant(OpenAiAssistantMessage),
    Tool(OpenAiToolMessage),
}

impl OpenAiMessage {
    #[cfg(test)]
    pub fn role(&self) -> &str {
        match self {
            Self::System(_) => ROLE_SYSTEM,
            Self::User(_) => ROLE_USER,
            Self::Assistant(_) => ROLE_ASSISTANT,
            Self::Tool(_) => ROLE_TOOL,
        }
    }

    pub fn assistant_tool_calls(&self) -> Option<&[OpenAiToolCall]> {
        let Self::Assistant(message) = self else {
            return None;
        };
        message.tool_calls.as_deref()
    }

    pub fn tool_call_id(&self) -> Option<&str> {
        let Self::Tool(message) = self else {
            return None;
        };
        Some(message.tool_call_id.as_str())
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct OpenAiSystemMessage {
    pub role: String,
    pub content: String,
}

impl OpenAiSystemMessage {
    pub fn from_text(content: String) -> Self {
        Self {
            role: ROLE_SYSTEM.to_string(),
            content,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct OpenAiUserMessage {
    pub role: String,
    pub content: OpenAiUserContent,
}

impl OpenAiUserMessage {
    pub fn from_text(content: String) -> Self {
        Self {
            role: ROLE_USER.to_string(),
            content: OpenAiUserContent::Text(content),
        }
    }

    pub fn from_parts(content: Vec<OpenAiUserContentPart>) -> Self {
        Self {
            role: ROLE_USER.to_string(),
            content: OpenAiUserContent::Parts(content),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum OpenAiUserContent {
    Text(String),
    Parts(Vec<OpenAiUserContentPart>),
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum OpenAiUserContentPart {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image_url")]
    ImageUrl { image_url: OpenAiImageUrl },
}

#[derive(Debug, Clone, Serialize)]
pub struct OpenAiImageUrl {
    pub url: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct OpenAiAssistantMessage {
    pub role: String,
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<OpenAiToolCall>>,
}

impl OpenAiAssistantMessage {
    pub fn from_text_and_tools(content: Option<String>, tool_calls: Vec<OpenAiToolCall>) -> Self {
        Self {
            role: ROLE_ASSISTANT.to_string(),
            content,
            tool_calls: if tool_calls.is_empty() {
                None
            } else {
                Some(tool_calls)
            },
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct OpenAiToolMessage {
    pub role: String,
    pub tool_call_id: String,
    pub content: String,
}

impl OpenAiToolMessage {
    pub fn new(tool_call_id: String, content: String) -> Self {
        Self {
            role: ROLE_TOOL.to_string(),
            tool_call_id,
            content,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct OpenAiToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub function: OpenAiFunctionCall,
}

impl OpenAiToolCall {
    pub fn function(id: String, name: String, arguments: String) -> Self {
        Self {
            id,
            kind: TOOL_FUNCTION.to_string(),
            function: OpenAiFunctionCall { name, arguments },
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct OpenAiFunctionCall {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct OpenAiToolDefinition {
    #[serde(rename = "type")]
    pub kind: String,
    pub function: OpenAiFunctionDefinition,
}

#[derive(Debug, Clone, Serialize)]
pub struct OpenAiFunctionDefinition {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum OpenAiToolChoice {
    Auto(String),
    Tool(OpenAiNamedToolChoice),
}

impl OpenAiToolChoice {
    pub fn auto() -> Self {
        Self::Auto("auto".to_string())
    }

    pub fn tool(name: String) -> Self {
        Self::Tool(OpenAiNamedToolChoice {
            kind: TOOL_FUNCTION.to_string(),
            function: OpenAiNamedToolFunction { name },
        })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct OpenAiNamedToolChoice {
    #[serde(rename = "type")]
    pub kind: String,
    pub function: OpenAiNamedToolFunction,
}

#[derive(Debug, Clone, Serialize)]
pub struct OpenAiNamedToolFunction {
    pub name: String,
}

pub fn map_claude_model_to_openai(claude_model: &str, config: &Config) -> String {
    if is_upstream_native_model(claude_model) {
        return claude_model.to_string();
    }

    let model_lower = claude_model.to_lowercase();
    if model_lower.contains("haiku") {
        config.small_model.clone()
    } else if model_lower.contains("sonnet") {
        config.middle_model.clone()
    } else {
        config.big_model.clone()
    }
}

fn is_upstream_native_model(model: &str) -> bool {
    let lowered = model.to_lowercase();
    lowered.starts_with("gpt-")
        || lowered.starts_with("o1-")
        || lowered.starts_with("ep-")
        || lowered.starts_with("doubao-")
        || lowered.starts_with("deepseek-")
}

pub fn supports_reasoning_effort(model: &str) -> bool {
    let lowered = model.to_lowercase();
    lowered.starts_with("o1")
        || lowered.starts_with("o3")
        || lowered.starts_with("o4")
        || lowered.starts_with("gpt-5")
}
