use crate::config::Config;

pub fn map_claude_model_to_openai(claude_model: &str, config: &Config) -> String {
    if is_upstream_native_model(claude_model) {
        return claude_model.to_string();
    }

    let model_lower = claude_model.to_lowercase();
    if model_lower.contains("haiku") {
        config.small_model.clone()
    } else if model_lower.contains("sonnet") {
        config.middle_model.clone()
    } else {
        config.big_model.clone()
    }
}

fn is_upstream_native_model(model: &str) -> bool {
    model.starts_with("gpt-")
        || model.starts_with("o1-")
        || model.starts_with("ep-")
        || model.starts_with("doubao-")
        || model.starts_with("deepseek-")
}
