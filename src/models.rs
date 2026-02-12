use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ClaudeMessage {
    pub role: String,
    #[serde(default)]
    pub content: Option<ClaudeContent>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum ClaudeContent {
    Text(String),
    Blocks(Vec<ClaudeContentBlock>),
    Other(Value),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type")]
pub enum ClaudeContentBlock {
    #[serde(rename = "text")]
    Text {
        text: String,
        #[serde(flatten)]
        extra: BTreeMap<String, Value>,
    },
    #[serde(rename = "image")]
    Image {
        source: Option<ClaudeImageSource>,
        #[serde(flatten)]
        extra: BTreeMap<String, Value>,
    },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: Option<String>,
        name: Option<String>,
        input: Option<Value>,
        #[serde(flatten)]
        extra: BTreeMap<String, Value>,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: Option<String>,
        content: Option<Value>,
        #[serde(flatten)]
        extra: BTreeMap<String, Value>,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ClaudeImageSource {
    #[serde(rename = "type")]
    pub source_type: Option<String>,
    pub media_type: Option<String>,
    pub data: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum ClaudeSystemContent {
    Text(String),
    Blocks(Vec<ClaudeSystemBlock>),
    Other(Value),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type")]
pub enum ClaudeSystemBlock {
    #[serde(rename = "text")]
    Text {
        text: String,
        #[serde(flatten)]
        extra: BTreeMap<String, Value>,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ClaudeToolDefinition {
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub input_schema: Option<Value>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum ClaudeToolChoice {
    Mode(String),
    Named(ClaudeNamedToolChoice),
    Other(Value),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ClaudeNamedToolChoice {
    #[serde(rename = "type")]
    pub choice_type: Option<String>,
    pub name: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ClaudeMessagesRequest {
    pub model: String,
    pub max_tokens: u32,
    pub messages: Vec<ClaudeMessage>,
    #[serde(default)]
    pub system: Option<ClaudeSystemContent>,
    #[serde(default)]
    pub stop_sequences: Option<Vec<String>>,
    #[serde(default)]
    pub stream: Option<bool>,
    #[serde(default)]
    pub temperature: Option<f64>,
    #[serde(default)]
    pub top_p: Option<f64>,
    #[serde(default)]
    pub tools: Option<Vec<ClaudeToolDefinition>>,
    #[serde(default)]
    pub tool_choice: Option<ClaudeToolChoice>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ClaudeTokenCountRequest {
    pub model: String,
    pub messages: Vec<ClaudeMessage>,
    #[serde(default)]
    pub system: Option<ClaudeSystemContent>,
}

#[derive(Debug, Default)]
pub struct StreamingToolCallState {
    pub id: Option<String>,
    pub name: Option<String>,
    pub args_buffer: String,
    pub json_sent: bool,
    pub claude_index: Option<usize>,
    pub started: bool,
}
