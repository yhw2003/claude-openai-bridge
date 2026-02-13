use std::collections::BTreeMap;

use serde::Serialize;

use crate::models::StreamingToolCallState;

#[derive(Debug, Clone, Default, Serialize)]
pub struct StreamUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_read_input_tokens: Option<u64>,
}

impl StreamUsage {
    pub fn total_tokens(&self) -> u64 {
        self.input_tokens.saturating_add(self.output_tokens)
    }
}

pub struct StreamState {
    pub text_block_index: usize,
    pub thinking_block_index: Option<usize>,
    pub thinking_started: bool,
    pub thinking_requested: bool,
    pub saw_thinking_delta: bool,
    pub tool_block_counter: usize,
    pub tool_calls: BTreeMap<usize, StreamingToolCallState>,
    pub final_stop_reason: String,
    pub usage_data: StreamUsage,
}

impl StreamState {
    pub fn new(thinking_requested: bool) -> Self {
        Self {
            text_block_index: 0,
            thinking_block_index: None,
            thinking_started: false,
            thinking_requested,
            saw_thinking_delta: false,
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
