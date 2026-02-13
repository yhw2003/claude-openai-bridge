use salvo::http::StatusCode;
use serde::Deserialize;
use serde::de::{Deserializer, IgnoredAny};

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
    if let Ok(parsed) = serde_json::from_str::<UpstreamErrorEnvelope>(body) {
        if let Some(message) = parsed.error.and_then(UpstreamErrorField::into_message) {
            return message;
        }
        if let Some(message) = parsed.message {
            return message;
        }
    }

    if body.trim().is_empty() {
        "upstream API returned an empty error response".to_string()
    } else {
        body.to_string()
    }
}

fn deserialize_optional_string<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<LooseString>::deserialize(deserializer)?;
    Ok(value.and_then(LooseString::into_string))
}

#[derive(Debug, Deserialize)]
struct UpstreamErrorEnvelope {
    #[serde(default)]
    error: Option<UpstreamErrorField>,
    #[serde(default, deserialize_with = "deserialize_optional_string")]
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum UpstreamErrorField {
    Payload(UpstreamErrorPayload),
    Other(IgnoredAny),
}

impl UpstreamErrorField {
    fn into_message(self) -> Option<String> {
        match self {
            UpstreamErrorField::Payload(payload) => payload.message,
            UpstreamErrorField::Other(_) => None,
        }
    }
}

#[derive(Debug, Deserialize)]
struct UpstreamErrorPayload {
    #[serde(default, deserialize_with = "deserialize_optional_string")]
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum LooseString {
    String(String),
    Other(IgnoredAny),
}

impl LooseString {
    fn into_string(self) -> Option<String> {
        match self {
            LooseString::String(value) => Some(value),
            LooseString::Other(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::extract_error_message_from_body;

    #[test]
    fn extracts_nested_error_message() {
        let body = r#"{"error":{"message":"nested"}}"#;
        assert_eq!(extract_error_message_from_body(body), "nested");
    }

    #[test]
    fn extracts_top_level_message() {
        let body = r#"{"message":"top"}"#;
        assert_eq!(extract_error_message_from_body(body), "top");
    }

    #[test]
    fn prefers_nested_error_message() {
        let body = r#"{"error":{"message":"nested"},"message":"top"}"#;
        assert_eq!(extract_error_message_from_body(body), "nested");
    }

    #[test]
    fn ignores_non_string_message_fields() {
        let body = r#"{"error":{"message":"nested"},"message":123}"#;
        assert_eq!(extract_error_message_from_body(body), "nested");
    }

    #[test]
    fn returns_default_message_for_empty_body() {
        assert_eq!(
            extract_error_message_from_body("   "),
            "upstream API returned an empty error response"
        );
    }

    #[test]
    fn returns_original_body_for_non_json() {
        assert_eq!(
            extract_error_message_from_body("gateway failed"),
            "gateway failed"
        );
    }
}
