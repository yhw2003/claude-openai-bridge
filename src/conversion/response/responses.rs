use serde::Deserialize;
use serde_json::Value;

use crate::constants::TOOL_FUNCTION;
use crate::models::ClaudeMessagesRequest;

use super::map_responses_incomplete_reason;
use super::types::{
    ClaudeContentBlock, ClaudeResponse, ClaudeUsage, build_claude_response, map_tool_use_block,
    maybe_push_text, maybe_push_thinking,
};

pub(crate) fn convert_openai_responses_to_claude_response(
    responses: &OpenAiResponsesResponse,
    original_request: &ClaudeMessagesRequest,
) -> Result<ClaudeResponse, String> {
    if responses.output.is_empty() && responses.output_text.is_none() {
        return Err("missing output in upstream responses payload".to_string());
    }

    let mut content_blocks = Vec::new();
    let mut saw_tool_use = false;

    for item in &responses.output {
        saw_tool_use |= append_output_item(item, &mut content_blocks);
    }
    append_output_text_fallback(responses, &mut content_blocks);

    let stop_reason = resolve_stop_reason(responses, saw_tool_use);
    Ok(build_claude_response(
        responses.id.clone(),
        original_request.model.clone(),
        content_blocks,
        stop_reason,
        usage_from_responses(responses.usage.as_ref()),
    ))
}

fn append_output_item(item: &Value, content_blocks: &mut Vec<ClaudeContentBlock>) -> bool {
    match item_type(item).unwrap_or_default() {
        "message" => {
            append_message_item(item, content_blocks);
            false
        }
        "reasoning" => {
            append_reasoning_item(item, content_blocks);
            false
        }
        "function_call" => append_function_call(item, content_blocks),
        _ => false,
    }
}

fn append_message_item(item: &Value, content_blocks: &mut Vec<ClaudeContentBlock>) {
    for part in content_parts(item) {
        let part_type = part.get("type").and_then(Value::as_str).unwrap_or_default();
        if matches!(part_type, "output_text" | "text" | "input_text") {
            maybe_push_text(content_blocks, part.get("text").and_then(Value::as_str));
            continue;
        }

        if part_type == "refusal" {
            let refusal_text = part
                .get("refusal")
                .and_then(Value::as_str)
                .or_else(|| part.get("text").and_then(Value::as_str));
            maybe_push_text(content_blocks, refusal_text);
        }
    }
}

fn append_reasoning_item(item: &Value, content_blocks: &mut Vec<ClaudeContentBlock>) {
    let signature = item.get("signature").and_then(Value::as_str);

    if let Some(summary) = item.get("summary").and_then(Value::as_array) {
        for summary_item in summary {
            let text = summary_item
                .get("text")
                .and_then(Value::as_str)
                .or_else(|| summary_item.get("summary").and_then(Value::as_str));
            maybe_push_thinking(content_blocks, text, signature);
        }
    }

    let text = item
        .get("text")
        .and_then(Value::as_str)
        .or_else(|| item.get("reasoning").and_then(Value::as_str));
    maybe_push_thinking(content_blocks, text, signature);
}

fn append_function_call(item: &Value, content_blocks: &mut Vec<ClaudeContentBlock>) -> bool {
    let arguments = item
        .get("arguments")
        .map(value_to_string)
        .unwrap_or_else(|| "{}".to_string());
    let block = map_tool_use_block(
        call_id(item).as_deref(),
        Some(TOOL_FUNCTION),
        item.get("name").and_then(Value::as_str),
        Some(arguments.as_str()),
    );

    if let Some(block) = block {
        content_blocks.push(block);
        true
    } else {
        false
    }
}

fn append_output_text_fallback(
    responses: &OpenAiResponsesResponse,
    content_blocks: &mut Vec<ClaudeContentBlock>,
) {
    if content_blocks
        .iter()
        .any(|block| matches!(block, ClaudeContentBlock::Text { .. }))
    {
        return;
    }

    maybe_push_text(content_blocks, responses.output_text.as_deref());
}

fn resolve_stop_reason(responses: &OpenAiResponsesResponse, saw_tool_use: bool) -> &'static str {
    if saw_tool_use {
        return crate::constants::STOP_TOOL_USE;
    }

    if responses.status.as_deref() == Some("incomplete") {
        return map_responses_incomplete_reason(incomplete_reason(&responses.incomplete_details));
    }

    crate::constants::STOP_END_TURN
}

fn usage_from_responses(usage: Option<&OpenAiResponsesUsage>) -> ClaudeUsage {
    ClaudeUsage {
        input_tokens: usage.and_then(|value| value.input_tokens).unwrap_or(0),
        output_tokens: usage.and_then(|value| value.output_tokens).unwrap_or(0),
    }
}

fn item_type(item: &Value) -> Option<&str> {
    item.get("type").and_then(Value::as_str)
}

fn content_parts(item: &Value) -> Vec<&Value> {
    item.get("content")
        .and_then(Value::as_array)
        .map(|value| value.iter().collect())
        .unwrap_or_default()
}

fn call_id(item: &Value) -> Option<String> {
    item.get("call_id")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .or_else(|| {
            item.get("id")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
}

fn incomplete_reason(details: &Option<Value>) -> Option<&str> {
    let details = details.as_ref()?;
    details
        .get("reason")
        .and_then(Value::as_str)
        .or_else(|| details.get("type").and_then(Value::as_str))
}

fn value_to_string(value: &Value) -> String {
    match value {
        Value::String(text) => text.to_string(),
        _ => value.to_string(),
    }
}

#[derive(Debug, Deserialize)]
pub struct OpenAiResponsesResponse {
    pub id: Option<String>,
    #[serde(default)]
    pub output: Vec<Value>,
    #[serde(default)]
    output_text: Option<String>,
    status: Option<String>,
    #[serde(default)]
    incomplete_details: Option<Value>,
    usage: Option<OpenAiResponsesUsage>,
}

impl OpenAiResponsesResponse {
    pub(crate) fn id(&self) -> Option<&str> {
        self.id.as_deref()
    }
}

#[derive(Debug, Deserialize)]
struct OpenAiResponsesUsage {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::{OpenAiResponsesResponse, convert_openai_responses_to_claude_response};
    use crate::models::ClaudeMessagesRequest;

    fn empty_request() -> ClaudeMessagesRequest {
        ClaudeMessagesRequest {
            model: "claude-3-5-sonnet-20241022".to_string(),
            max_tokens: 256,
            messages: vec![],
            thinking: None,
            system: None,
            stop_sequences: None,
            stream: Some(false),
            temperature: Some(1.0),
            top_p: None,
            tools: None,
            tool_choice: None,
        }
    }

    #[test]
    fn maps_function_call_to_tool_use() {
        let payload = json!({
            "id": "resp_2",
            "status": "completed",
            "output": [{
                "type": "function_call",
                "call_id": "call_abc",
                "name": "Bash",
                "arguments": "{\"command\":\"cargo check\"}"
            }]
        });

        let parsed: OpenAiResponsesResponse = serde_json::from_value(payload).expect("deserialize");
        let converted = convert_openai_responses_to_claude_response(&parsed, &empty_request())
            .expect("convert");
        let json = serde_json::to_value(converted).expect("serialize");
        let content = json
            .get("content")
            .and_then(Value::as_array)
            .expect("content array");

        assert_eq!(
            content[0].get("type").and_then(Value::as_str),
            Some("tool_use")
        );
    }

    #[test]
    fn maps_incomplete_reason_to_max_tokens() {
        let payload = json!({
            "id": "resp_3",
            "status": "incomplete",
            "incomplete_details": {"reason":"max_output_tokens"},
            "output": [{"type":"message","content":[{"type":"output_text","text":"partial"}]}]
        });

        let parsed: OpenAiResponsesResponse = serde_json::from_value(payload).expect("deserialize");
        let converted = convert_openai_responses_to_claude_response(&parsed, &empty_request())
            .expect("convert");
        let json = serde_json::to_value(converted).expect("serialize");

        assert_eq!(
            json.get("stop_reason").and_then(Value::as_str),
            Some("max_tokens")
        );
    }
}
