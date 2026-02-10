use serde_json::{json, Value};

use crate::constants::{CONTENT_IMAGE, CONTENT_TEXT, ROLE_USER};
use crate::models::ClaudeMessage;

pub fn convert_claude_user_message(message: &ClaudeMessage) -> Value {
    let Some(content) = &message.content else {
        return json!({ "role": ROLE_USER, "content": "" });
    };

    if let Some(text_content) = content.as_str() {
        return json!({ "role": ROLE_USER, "content": text_content });
    }

    let Some(blocks) = content.as_array() else {
        return json!({ "role": ROLE_USER, "content": "" });
    };

    let openai_content: Vec<Value> = blocks.iter().filter_map(convert_user_block).collect();

    if let Some(text) = single_text_content(&openai_content) {
        json!({ "role": ROLE_USER, "content": text })
    } else {
        json!({ "role": ROLE_USER, "content": openai_content })
    }
}

fn convert_user_block(block: &Value) -> Option<Value> {
    let block_type = block.get("type").and_then(Value::as_str)?;
    if block_type == CONTENT_TEXT {
        return block
            .get("text")
            .and_then(Value::as_str)
            .map(|text| json!({ "type": "text", "text": text }));
    }
    if block_type != CONTENT_IMAGE {
        return None;
    }

    let source = block.get("source")?.as_object()?;
    let source_type = source.get("type").and_then(Value::as_str).unwrap_or_default();
    let media_type = source
        .get("media_type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let data = source
        .get("data")
        .and_then(Value::as_str)
        .unwrap_or_default();

    if source_type != "base64" || media_type.is_empty() || data.is_empty() {
        return None;
    }

    Some(json!({
        "type": "image_url",
        "image_url": {"url": format!("data:{media_type};base64,{data}")}
    }))
}

fn single_text_content(openai_content: &[Value]) -> Option<&str> {
    if openai_content.len() != 1 {
        return None;
    }
    if openai_content
        .first()
        .and_then(|item| item.get("type"))
        .and_then(Value::as_str)
        != Some("text")
    {
        return None;
    }
    openai_content[0].get("text").and_then(Value::as_str)
}
