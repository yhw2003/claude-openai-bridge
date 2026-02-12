use std::collections::BTreeMap;

use serde::Serialize;

use crate::models::StreamingToolCallState;

#[derive(Debug, Clone, Serialize)]
pub struct StreamUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_read_input_tokens: Option<u64>,
}

impl Default for StreamUsage {
    fn default() -> Self {
        Self {
            input_tokens: 0,
            output_tokens: 0,
            cache_read_input_tokens: None,
        }
    }
}

pub struct StreamState {
    pub text_block_index: usize,
    pub tool_block_counter: usize,
    pub tool_calls: BTreeMap<usize, StreamingToolCallState>,
    pub final_stop_reason: String,
    pub usage_data: StreamUsage,
}

impl StreamState {
    pub fn new() -> Self {
        Self {
            text_block_index: 0,
            tool_block_counter: 0,
            tool_calls: BTreeMap::new(),
            final_stop_reason: "end_turn".to_string(),
            usage_data: StreamUsage::default(),
        }
    }
}

pub fn started_tool_index(tool_call_state: &StreamingToolCallState) -> Option<usize> {
    if !tool_call_state.started {
        return None;
    }
    tool_call_state.claude_index
}
