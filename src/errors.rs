use salvo::http::StatusCode;
use serde_json::Value;

#[derive(Debug)]
pub struct UpstreamError {
    pub status: StatusCode,
    pub message: String,
}

pub fn classify_openai_error(detail: &str) -> String {
    let lowered = detail.to_lowercase();

    if lowered.contains("unsupported_country_region_territory")
        || lowered.contains("country, region, or territory not supported")
    {
        return "OpenAI API is not available in your region. Consider using Azure OpenAI or a compatible regional provider.".to_string();
    }

    if lowered.contains("invalid_api_key") || lowered.contains("unauthorized") {
        return "Invalid API key. Please verify OPENAI_API_KEY configuration.".to_string();
    }

    if lowered.contains("rate_limit") || lowered.contains("quota") {
        return "Rate limit exceeded. Please retry later or upgrade your upstream quota."
            .to_string();
    }

    if lowered.contains("model")
        && (lowered.contains("not found") || lowered.contains("does not exist"))
    {
        return "Model not found. Please check BIG_MODEL / MIDDLE_MODEL / SMALL_MODEL mappings."
            .to_string();
    }

    if lowered.contains("billing") || lowered.contains("payment") {
        return "Billing issue detected. Please verify upstream account billing status."
            .to_string();
    }

    detail.to_string()
}

pub fn extract_error_message_from_body(body: &str) -> String {
    if let Ok(parsed) = serde_json::from_str::<Value>(body) {
        if let Some(message) = parsed
            .get("error")
            .and_then(|error| error.get("message"))
            .and_then(Value::as_str)
        {
            return message.to_string();
        }
        if let Some(message) = parsed.get("message").and_then(Value::as_str) {
            return message.to_string();
        }
    }

    if body.trim().is_empty() {
        "upstream API returned an empty error response".to_string()
    } else {
        body.to_string()
    }
}
