use crate::conversion::request::models::{
    OpenAiImageUrl, OpenAiMessage, OpenAiUserContentPart, OpenAiUserMessage,
};
use crate::models::{ClaudeContent, ClaudeContentBlock, ClaudeImageSource, ClaudeMessage};

pub fn convert_claude_user_message(message: &ClaudeMessage) -> OpenAiMessage {
    let Some(content) = &message.content else {
        return OpenAiMessage::User(OpenAiUserMessage::from_text(String::new()));
    };

    match content {
        ClaudeContent::Text(text_content) => {
            OpenAiMessage::User(OpenAiUserMessage::from_text(text_content.to_string()))
        }
        ClaudeContent::Blocks(blocks) => {
            let openai_content: Vec<OpenAiUserContentPart> =
                blocks.iter().filter_map(convert_user_block).collect();

            if let Some(text) = single_text_content(&openai_content) {
                OpenAiMessage::User(OpenAiUserMessage::from_text(text.to_string()))
            } else {
                OpenAiMessage::User(OpenAiUserMessage::from_parts(openai_content))
            }
        }
        ClaudeContent::Other(_) => OpenAiMessage::User(OpenAiUserMessage::from_text(String::new())),
    }
}

fn convert_user_block(block: &ClaudeContentBlock) -> Option<OpenAiUserContentPart> {
    match block {
        ClaudeContentBlock::Text { text, .. } => Some(OpenAiUserContentPart::Text {
            text: text.to_string(),
        }),
        ClaudeContentBlock::ToolResult { .. } => None,
        ClaudeContentBlock::Image { source, .. } => convert_image_source(source.as_ref()),
        _ => None,
    }
}

fn convert_image_source(source: Option<&ClaudeImageSource>) -> Option<OpenAiUserContentPart> {
    let source = source?;
    let source_type = source.source_type.as_deref().unwrap_or_default();
    let media_type = source.media_type.as_deref().unwrap_or_default();
    let data = source.data.as_deref().unwrap_or_default();

    if source_type != "base64" || media_type.is_empty() || data.is_empty() {
        return None;
    }

    Some(OpenAiUserContentPart::ImageUrl {
        image_url: OpenAiImageUrl {
            url: format!("data:{media_type};base64,{data}"),
        },
    })
}

fn single_text_content(openai_content: &[OpenAiUserContentPart]) -> Option<&str> {
    if openai_content.len() != 1 {
        return None;
    }

    match openai_content.first() {
        Some(OpenAiUserContentPart::Text { text }) => Some(text.as_str()),
        _ => None,
    }
}
