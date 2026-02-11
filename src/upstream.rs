use reqwest::header::{
    AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue, USER_AGENT,
};
use reqwest::Client;
use serde_json::Value;
use std::time::Duration;
use tracing::{error, warn};

use crate::config::Config;
use crate::errors::{classify_openai_error, extract_error_message_from_body, UpstreamError};
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

    pub async fn chat_completion(&self, body: &Value) -> Result<Value, UpstreamError> {
        let response = self
            .send_chat_request(
                body,
                Some(Duration::from_secs(self.config.request_timeout)),
                "non_stream",
            )
            .await?;
        response.json::<Value>().await.map_err(|error| UpstreamError {
            status: salvo::http::StatusCode::BAD_GATEWAY,
            message: classify_openai_error(&format!("failed to parse upstream JSON response: {error}")),
        })
    }

    pub async fn chat_completion_stream(
        &self,
        body: &Value,
    ) -> Result<reqwest::Response, UpstreamError> {
        let stream_timeout = self
            .config
            .stream_request_timeout
            .map(Duration::from_secs);
        self.send_chat_request(body, stream_timeout, "stream").await
    }

    async fn send_chat_request(
        &self,
        body: &Value,
        timeout: Option<Duration>,
        request_kind: &'static str,
    ) -> Result<reqwest::Response, UpstreamError> {
        let url = format!(
            "{}/chat/completions",
            self.config.openai_base_url.trim_end_matches('/')
        );

        let mut request_builder = self
            .client
            .post(url)
            .headers(build_upstream_headers(&self.config))
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
            request_kind,
            "Upstream connection failed before response headers: {error}"
        );
        return;
    }

    error!(
        phase = "upstream_request_error",
        request_kind,
        "Upstream request failed before response headers: {error}"
    );
}

fn build_upstream_headers(config: &Config) -> HeaderMap {
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

    headers
}
