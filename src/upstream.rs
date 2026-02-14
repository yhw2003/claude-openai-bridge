use reqwest::Client;
use reqwest::header::{
    AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue, USER_AGENT,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::borrow::Cow;
use std::time::Duration;
use tracing::{error, warn};

use crate::config::Config;
use crate::conversion::response::{OpenAiChatResponse, OpenAiResponsesResponse};
use crate::errors::{UpstreamError, classify_openai_error, extract_error_message_from_body};
use crate::utils::to_salvo_status;

#[derive(Clone, Debug)]
pub struct UpstreamClient {
    client: Client,
    config: Config,
}

impl UpstreamClient {
    pub fn new(config: Config) -> Result<Self, String> {
        let client = Client::builder()
            .build()
            .map_err(|error| format!("failed to initialize upstream HTTP client: {error}"))?;
        Ok(Self { client, config })
    }

    pub async fn chat_completion<T: Serialize + ?Sized>(
        &self,
        body: &T,
        session_id: &str,
    ) -> Result<OpenAiChatResponse, UpstreamError> {
        let response = self
            .send_request(
                "/chat/completions",
                body,
                session_id,
                Some(Duration::from_secs(self.config.request_timeout)),
                "non_stream",
            )
            .await?;
        parse_success_json_response::<OpenAiChatResponse>(response).await
    }

    pub async fn chat_completion_stream<T: Serialize + ?Sized>(
        &self,
        body: &T,
        session_id: &str,
    ) -> Result<reqwest::Response, UpstreamError> {
        let stream_timeout = self.config.stream_request_timeout.map(Duration::from_secs);
        self.send_request(
            "/chat/completions",
            body,
            session_id,
            stream_timeout,
            "stream",
        )
        .await
    }

    pub async fn responses<T: Serialize + ?Sized>(
        &self,
        body: &T,
        session_id: &str,
    ) -> Result<OpenAiResponsesResponse, UpstreamError> {
        let response = self
            .send_request(
                "/responses",
                body,
                session_id,
                Some(Duration::from_secs(self.config.request_timeout)),
                "non_stream",
            )
            .await?;

        parse_success_json_response::<OpenAiResponsesResponse>(response).await
    }

    pub async fn responses_stream<T: Serialize + ?Sized>(
        &self,
        body: &T,
        session_id: &str,
    ) -> Result<reqwest::Response, UpstreamError> {
        let stream_timeout = self.config.stream_request_timeout.map(Duration::from_secs);
        self.send_request("/responses", body, session_id, stream_timeout, "stream")
            .await
    }

    async fn send_request<T: Serialize + ?Sized>(
        &self,
        path: &str,
        body: &T,
        session_id: &str,
        timeout: Option<Duration>,
        request_kind: &'static str,
    ) -> Result<reqwest::Response, UpstreamError> {
        let url = format!(
            "{}{}",
            self.config.openai_base_url.trim_end_matches('/'),
            path
        );

        let mut request_builder = self
            .client
            .post(url)
            .headers(build_upstream_headers(&self.config, session_id))
            .json(body);

        if let Some(api_version) = self.config.azure_api_version.as_deref() {
            request_builder = request_builder.query(&[("api-version", api_version)]);
        }

        if let Some(duration) = timeout {
            request_builder = request_builder.timeout(duration);
        }

        let response = request_builder
            .send()
            .await
            .map_err(|error| build_send_error(error, timeout, request_kind))?;

        if response.status().is_success() {
            return Ok(response);
        }

        let status = to_salvo_status(response.status());
        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| "<missing>".to_string());
        let text = match response.text().await {
            Ok(value) => value,
            Err(error) => {
                warn!(
                    phase = "upstream_error_body_read_failed",
                    status = %status,
                    content_type = %content_type,
                    "failed to read upstream error response body: {error}"
                );
                String::new()
            }
        };
        let body_preview = preview_text(&text, BODY_PREVIEW_LIMIT);
        let raw_message = extract_error_message_from_body(&text);

        warn!(
            phase = "upstream_http_error",
            status = %status,
            content_type = %content_type,
            body_preview = %body_preview,
            "upstream returned non-success status"
        );

        Err(UpstreamError {
            status,
            message: classify_openai_error(&raw_message),
        })
    }
}

const BODY_PREVIEW_LIMIT: usize = 1024;

async fn parse_success_json_response<T: DeserializeOwned>(
    response: reqwest::Response,
) -> Result<T, UpstreamError> {
    let status = response.status();
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| "<missing>".to_string());
    let body = response.bytes().await.map_err(|error| UpstreamError {
        status: salvo::http::StatusCode::BAD_GATEWAY,
        message: classify_openai_error(&format!(
            "failed to read upstream response body (status: {status}, content-type: {content_type}): {error}"
        )),
    })?;

    decode_json_body::<T>(status, &content_type, &body)
}

fn decode_json_body<T: DeserializeOwned>(
    status: reqwest::StatusCode,
    content_type: &str,
    body: &[u8],
) -> Result<T, UpstreamError> {
    serde_json::from_slice::<T>(body).map_err(|error| {
        let body_preview = preview_bytes(body, BODY_PREVIEW_LIMIT);
        UpstreamError {
            status: salvo::http::StatusCode::BAD_GATEWAY,
            message: classify_openai_error(&format!(
                "failed to parse upstream JSON response (status: {status}, content-type: {content_type}, body-preview: {body_preview}): {error}"
            )),
        }
    })
}

fn preview_bytes(body: &[u8], limit: usize) -> String {
    match std::str::from_utf8(body) {
        Ok(text) => preview_text(text, limit).into_owned(),
        Err(_) => {
            let len = body.len().min(limit);
            let mut preview = String::with_capacity(len * 2 + 32);
            for byte in &body[..len] {
                use std::fmt::Write;
                let _ = write!(&mut preview, "{byte:02x}");
            }
            if body.len() > limit {
                preview.push_str("...(truncated)");
            }
            format!("<non-utf8 hex: {preview}>")
        }
    }
}

fn preview_text(text: &str, limit: usize) -> Cow<'_, str> {
    let mut iterator = text.chars();
    let preview: String = iterator.by_ref().take(limit).collect();
    if iterator.next().is_none() {
        Cow::Borrowed(text)
    } else {
        Cow::Owned(format!("{preview}...(truncated)"))
    }
}

fn build_send_error(
    error: reqwest::Error,
    timeout: Option<Duration>,
    request_kind: &'static str,
) -> UpstreamError {
    log_send_stage_error(&error, timeout, request_kind);
    UpstreamError {
        status: salvo::http::StatusCode::BAD_GATEWAY,
        message: classify_openai_error(&format!("upstream request failed: {error}")),
    }
}

fn log_send_stage_error(error: &reqwest::Error, timeout: Option<Duration>, request_kind: &str) {
    let timeout_secs = timeout.map(|value| value.as_secs());

    if error.is_timeout() {
        error!(
            phase = "upstream_connect_timeout",
            request_kind,
            timeout_secs = ?timeout_secs,
            "Upstream timeout before response headers"
        );
        return;
    }

    if error.is_connect() {
        error!(
            phase = "upstream_connect_error",
            request_kind, "Upstream connection failed before response headers: {error}"
        );
        return;
    }

    error!(
        phase = "upstream_request_error",
        request_kind, "Upstream request failed before response headers: {error}"
    );
}

fn build_upstream_headers(config: &Config, session_id: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(
        USER_AGENT,
        HeaderValue::from_static("claude-openai-bridge-rust/1.0.0"),
    );

    if let Ok(auth_value) = HeaderValue::from_str(&format!("Bearer {}", config.openai_api_key)) {
        headers.insert(AUTHORIZATION, auth_value);
    }

    for (header_name, header_value) in &config.custom_headers {
        let Ok(name) = HeaderName::from_bytes(header_name.as_bytes()) else {
            warn!("invalid custom header name ignored: {header_name}");
            continue;
        };
        let Ok(value) = HeaderValue::from_str(header_value) else {
            warn!("invalid custom header value ignored for {header_name}");
            continue;
        };
        headers.insert(name, value);
    }

    if let Ok(value) = HeaderValue::from_str(session_id) {
        headers.insert("session_id", value);
    }

    headers
}

#[cfg(test)]
mod tests {
    use super::{build_upstream_headers, decode_json_body, preview_bytes, preview_text};
    use crate::config::{Config, WireApi};
    use reqwest::StatusCode;
    use serde::Deserialize;
    use std::collections::HashMap;
    use uuid::Uuid;

    fn test_config() -> Config {
        Config {
            openai_api_key: "sk-test".to_string(),
            anthropic_api_key: None,
            openai_base_url: "https://api.openai.com/v1".to_string(),
            azure_api_version: None,
            host: "0.0.0.0".to_string(),
            port: 8082,
            log_level: "INFO".to_string(),
            request_timeout: 90,
            stream_request_timeout: None,
            request_body_max_size: 16 * 1024 * 1024,
            session_ttl_min_secs: 1800,
            session_ttl_max_secs: 86400,
            session_cleanup_interval_secs: 60,
            debug_tool_id_matching: false,
            wire_api: WireApi::Chat,
            big_model: "gpt-4o".to_string(),
            middle_model: "gpt-4o".to_string(),
            small_model: "gpt-4o-mini".to_string(),
            custom_headers: HashMap::new(),
        }
    }

    #[test]
    fn adds_session_id_header() {
        let session_id = Uuid::new_v4().to_string();
        let headers = build_upstream_headers(&test_config(), &session_id);

        let value = headers
            .get("session_id")
            .and_then(|raw| raw.to_str().ok())
            .expect("session_id header should exist");

        assert_eq!(value, session_id);
    }

    #[test]
    fn session_id_header_contains_valid_uuid() {
        let session_id = Uuid::new_v4().to_string();
        let headers = build_upstream_headers(&test_config(), &session_id);

        let value = headers
            .get("session_id")
            .and_then(|raw| raw.to_str().ok())
            .expect("session_id header should exist");

        assert!(Uuid::parse_str(value).is_ok());
    }

    #[derive(Debug, Deserialize)]
    struct TestPayload {
        value: String,
    }

    #[test]
    fn decodes_valid_json_payload() {
        let payload = decode_json_body::<TestPayload>(
            StatusCode::OK,
            "application/json",
            br#"{"value":"ok"}"#,
        )
        .expect("json should decode");

        assert_eq!(payload.value, "ok");
    }

    #[test]
    fn parse_error_includes_status_content_type_and_preview() {
        let error = decode_json_body::<TestPayload>(
            StatusCode::OK,
            "text/html",
            b"<html><body>upstream gateway failed</body></html>",
        )
        .expect_err("json should fail");

        assert_eq!(error.status, salvo::http::StatusCode::BAD_GATEWAY);
        assert!(error.message.contains("status: 200 OK"));
        assert!(error.message.contains("content-type: text/html"));
        assert!(
            error
                .message
                .contains("body-preview: <html><body>upstream gateway failed</body></html>")
        );
    }

    #[test]
    fn preview_text_truncates_long_text() {
        let preview = preview_text("abcdef", 3);
        assert_eq!(preview, "abc...(truncated)");
    }

    #[test]
    fn preview_bytes_formats_non_utf8_as_hex() {
        let preview = preview_bytes(&[0xff, 0x00, 0x7f], 8);
        assert_eq!(preview, "<non-utf8 hex: ff007f>");
    }
}
