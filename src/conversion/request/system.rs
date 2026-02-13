use crate::models::{ClaudeSystemBlock, ClaudeSystemContent};

pub fn extract_system_text(system: &ClaudeSystemContent) -> String {
    match system {
        ClaudeSystemContent::Text(text) => text.to_string(),
        ClaudeSystemContent::Blocks(blocks) => {
            let text_parts: Vec<String> = blocks
                .iter()
                .filter_map(extract_system_block_text)
                .collect();
            text_parts.join("\n\n")
        }
        ClaudeSystemContent::Other(_) => String::new(),
    }
}

fn extract_system_block_text(block: &ClaudeSystemBlock) -> Option<String> {
    match block {
        ClaudeSystemBlock::Text { text, .. } => Some(text.clone()),
        ClaudeSystemBlock::Unknown => None,
    }
}
