pub const ROLE_USER: &str = "user";
pub const ROLE_ASSISTANT: &str = "assistant";
pub const ROLE_SYSTEM: &str = "system";
pub const ROLE_TOOL: &str = "tool";

pub const CONTENT_TEXT: &str = "text";
#[allow(dead_code)]
pub const CONTENT_IMAGE: &str = "image";
#[allow(dead_code)]
pub const CONTENT_TOOL_USE: &str = "tool_use";
#[allow(dead_code)]
pub const CONTENT_TOOL_RESULT: &str = "tool_result";
pub const CONTENT_THINKING: &str = "thinking";

pub const TOOL_FUNCTION: &str = "function";

pub const STOP_END_TURN: &str = "end_turn";
pub const STOP_MAX_TOKENS: &str = "max_tokens";
pub const STOP_TOOL_USE: &str = "tool_use";

pub const EVENT_MESSAGE_START: &str = "message_start";
pub const EVENT_MESSAGE_STOP: &str = "message_stop";
pub const EVENT_MESSAGE_DELTA: &str = "message_delta";
pub const EVENT_CONTENT_BLOCK_START: &str = "content_block_start";
pub const EVENT_CONTENT_BLOCK_STOP: &str = "content_block_stop";
pub const EVENT_CONTENT_BLOCK_DELTA: &str = "content_block_delta";
pub const EVENT_PING: &str = "ping";

pub const DELTA_TEXT: &str = "text_delta";
pub const DELTA_INPUT_JSON: &str = "input_json_delta";
pub const DELTA_THINKING: &str = "thinking_delta";
pub const DELTA_SIGNATURE: &str = "signature_delta";
