use reqwest::Client;
use reqwest::header::{
    AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue, USER_AGENT,
};
use serde::Serialize;
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
        Ok(Self {
            client,
            config,
        })
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
        response
            .json::<OpenAiChatResponse>()
            .await
            .map_err(|error| UpstreamError {
                status: salvo::http::StatusCode::BAD_GATEWAY,
                message: classify_openai_error(&format!(
                    "failed to parse upstream JSON response: {error}"
                )),
            })
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

        response
            .json::<OpenAiResponsesResponse>()
            .await
            .map_err(|error| UpstreamError {
                status: salvo::http::StatusCode::BAD_GATEWAY,
                message: classify_openai_error(&format!(
                    "failed to parse upstream JSON response: {error}"
                )),
            })
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
        let text = response.text().await.unwrap_or_default();
        let raw_message = extract_error_message_from_body(&text);

        Err(UpstreamError {
            status,
            message: classify_openai_error(&raw_message),
        })
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
    use super::build_upstream_headers;
    use crate::config::{Config, WireApi};
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
}
