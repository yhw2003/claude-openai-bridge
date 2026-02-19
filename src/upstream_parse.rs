use serde_json::Value;

use crate::conversion::response::OpenAiResponsesResponse;

pub fn parse_responses_body(
    body: &str,
    content_type: Option<&str>,
) -> Result<OpenAiResponsesResponse, String> {
    if let Ok(parsed) = serde_json::from_str::<OpenAiResponsesResponse>(body) {
        return Ok(parsed);
    }

    if is_event_stream(content_type, body) {
        return parse_sse_wrapped_response(body);
    }

    Err("expected JSON object payload".to_string())
}

fn is_event_stream(content_type: Option<&str>, body: &str) -> bool {
    content_type
        .map(|value| value.to_ascii_lowercase().contains("text/event-stream"))
        .unwrap_or(false)
        || body.contains("\nevent:")
        || body.starts_with("event:")
}

fn parse_sse_wrapped_response(body: &str) -> Result<OpenAiResponsesResponse, String> {
    let mut latest = None;

    for payload in iter_sse_data_payloads(body) {
        if payload == "[DONE]" {
            continue;
        }

        let value = serde_json::from_str::<Value>(&payload)
            .map_err(|error| format!("failed to parse SSE data JSON: {error}"))?;

        if let Some(response) = value.get("response") {
            if let Ok(parsed) = serde_json::from_value::<OpenAiResponsesResponse>(response.clone())
            {
                latest = Some(parsed);
            }
            continue;
        }

        if let Ok(parsed) = serde_json::from_value::<OpenAiResponsesResponse>(value) {
            latest = Some(parsed);
        }
    }

    latest.ok_or_else(|| "no response object found in SSE payload".to_string())
}

fn iter_sse_data_payloads(body: &str) -> Vec<String> {
    let mut payloads = Vec::new();
    let mut current = Vec::new();

    for line in body.lines() {
        let trimmed = line.trim_end_matches('\r');

        if trimmed.is_empty() {
            if !current.is_empty() {
                payloads.push(current.join("\n"));
                current.clear();
            }
            continue;
        }

        if let Some(data) = trimmed.strip_prefix("data:") {
            current.push(data.trim_start().to_string());
        }
    }

    if !current.is_empty() {
        payloads.push(current.join("\n"));
    }

    payloads
}

#[cfg(test)]
mod tests {
    use super::parse_responses_body;

    #[test]
    fn parses_standard_json_body() {
        let body = r#"{"id":"resp_1","status":"completed","output":[]}"#;
        let parsed = parse_responses_body(body, Some("application/json")).expect("parse");
        assert_eq!(parsed.id(), Some("resp_1"));
    }

    #[test]
    fn parses_sse_wrapped_response_body() {
        let body = concat!(
            "event: response.created\n",
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_a\",\"status\":\"in_progress\",\"output\":[]}}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_a\",\"status\":\"completed\",\"output\":[]}}\n\n",
            "data: [DONE]\n\n"
        );

        let parsed = parse_responses_body(body, Some("text/event-stream")).expect("parse sse");
        assert_eq!(parsed.id(), Some("resp_a"));
    }
}
