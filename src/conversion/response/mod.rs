mod chat;
mod responses;
mod types;

pub(crate) use chat::{OpenAiChatResponse, convert_openai_to_claude_response};
pub(crate) use responses::{OpenAiResponsesResponse, convert_openai_responses_to_claude_response};

use crate::constants::{STOP_END_TURN, STOP_MAX_TOKENS, STOP_TOOL_USE};

pub fn map_finish_reason(finish_reason: &str) -> &'static str {
    match finish_reason {
        "length" => STOP_MAX_TOKENS,
        "tool_calls" | "function_call" => STOP_TOOL_USE,
        _ => STOP_END_TURN,
    }
}

pub fn map_responses_incomplete_reason(reason: Option<&str>) -> &'static str {
    match reason {
        Some("max_output_tokens") => STOP_MAX_TOKENS,
        Some("tool_use") | Some("function_call") => STOP_TOOL_USE,
        _ => STOP_END_TURN,
    }
}
