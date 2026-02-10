use serde_json::Value;

use crate::constants::CONTENT_TEXT;

pub fn extract_system_text(system: &Value) -> String {
    match system {
        Value::String(text) => text.to_string(),
        Value::Array(blocks) => {
            let text_parts: Vec<String> = blocks
                .iter()
                .filter_map(extract_system_block_text)
                .collect();
            text_parts.join("\n\n")
        }
        _ => String::new(),
    }
}

fn extract_system_block_text(block: &Value) -> Option<String> {
    if block.get("type").and_then(Value::as_str) != Some(CONTENT_TEXT) {
        return None;
    }

    block
        .get("text")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}
