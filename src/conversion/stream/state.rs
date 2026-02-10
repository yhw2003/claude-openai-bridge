use std::collections::BTreeMap;

use serde_json::{json, Value};

use crate::models::StreamingToolCallState;

pub struct StreamState {
    pub text_block_index: usize,
    pub tool_block_counter: usize,
    pub tool_calls: BTreeMap<usize, StreamingToolCallState>,
    pub final_stop_reason: String,
    pub usage_data: Value,
}

impl StreamState {
    pub fn new() -> Self {
        Self {
            text_block_index: 0,
            tool_block_counter: 0,
            tool_calls: BTreeMap::new(),
            final_stop_reason: "end_turn".to_string(),
            usage_data: json!({"input_tokens": 0, "output_tokens": 0}),
        }
    }
}

pub fn started_tool_index(tool_call_state: &StreamingToolCallState) -> Option<usize> {
    if !tool_call_state.started {
        return None;
    }
    tool_call_state.claude_index
}
