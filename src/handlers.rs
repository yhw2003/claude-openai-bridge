use salvo::http::StatusCode;
use salvo::prelude::*;
use serde::de::{Deserializer, IgnoredAny};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::{debug, error, trace};

use crate::config::WireApi;
use crate::conversion::request::{
    OpenAiChatRequest, OpenAiMessage, OpenAiResponsesRequest, OpenAiUserMessage,
    convert_claude_to_openai, convert_claude_to_responses, is_thinking_requested,
};
use crate::conversion::response::{
    convert_openai_responses_to_claude_response, convert_openai_to_claude_response,
};
use crate::conversion::stream::{
    stream_openai_responses_to_claude_sse, stream_openai_to_claude_sse,
};
use crate::models::{ClaudeMessagesRequest, ClaudeTokenCountRequest};
use crate::state::app_state;
use crate::utils::now_timestamp_string;

pub fn router() -> Router {
    Router::new()
        .get(root)
        .push(Router::with_path("health").get(health_check))
        .push(Router::with_path("test-connection").get(test_connection))
        .push(
            Router::with_path("v1/messages")
                .post(create_message)
                .push(Router::with_path("count_tokens").post(count_tokens)),
        )
}

#[handler]
pub async fn create_message(req: &mut Request, res: &mut Response) {
    let state = app_state();
    if let Err(message) = validate_client_api_key_header(req) {
        unauthorized(res, &message);
        return;
    }

    let request = match parse_messages_request(req, res).await {
        Some(value) => value,
        None => return,
    };

    trace!(
        phase = "downstream_request_full",
        claude_request = %serde_json::to_string(&request).unwrap_or_default(),
        "Received downstream request (full)"
    );

    debug!(
        phase = "downstream_request_summary",
        claude_model = %request.model,
        stream = request.stream.unwrap_or(false),
        max_tokens = request.max_tokens,
        messages_len = request.messages.len(),
        has_system = request.system.is_some(),
        has_tools = request.tools.as_ref().map(|v| !v.is_empty()).unwrap_or(false),
        has_tool_choice = request.tool_choice.is_some(),
        "Received downstream request (summary)"
    );

    let thinking_requested = is_thinking_requested(request.thinking.as_ref());

    match state.config.wire_api {
        WireApi::Chat => handle_chat_message(res, request, thinking_requested).await,
        WireApi::Responses => handle_responses_message(res, request, thinking_requested).await,
    }
}

#[handler]
pub async fn count_tokens(req: &mut Request, res: &mut Response) {
    if let Err(message) = validate_client_api_key_header(req) {
        unauthorized(res, &message);
        return;
    }

    let max_size = app_state().config.request_body_max_size;
    let token_request = match req
        .parse_json_with_max_size::<ClaudeTokenCountRequest>(max_size)
        .await
    {
        Ok(value) => value,
        Err(error) => {
            bad_request(res, &format!("invalid request body: {error}"));
            return;
        }
    };

    trace!(
        phase = "downstream_token_count_full",
        claude_request = %serde_json::to_string(&token_request).unwrap_or_default(),
        "Token counting request (full)"
    );

    debug!(
        phase = "downstream_token_count_summary",
        claude_model = %token_request.model,
        messages_len = token_request.messages.len(),
        has_system = token_request.system.is_some(),
        "Token counting request (summary)"
    );

    let estimated_tokens = estimate_input_tokens(&token_request);
    res.render(Json(TokenCountResponse {
        input_tokens: estimated_tokens,
    }));
}

#[handler]
pub async fn health_check(res: &mut Response) {
    let config = &app_state().config;
    res.render(Json(HealthCheckResponse {
        status: "healthy".to_string(),
        timestamp: now_timestamp_string(),
        openai_api_configured: !config.openai_api_key.is_empty(),
        api_key_valid: config.validate_openai_api_key_format(),
        client_api_key_validation: config.anthropic_api_key.is_some(),
    }));
}

#[handler]
pub async fn test_connection(res: &mut Response) {
    let state = app_state();

    let upstream_result = match state.config.wire_api {
        WireApi::Chat => run_chat_connection_test(state).await,
        WireApi::Responses => run_responses_connection_test(state).await,
    };

    match upstream_result {
        Ok(response_id) => res.render(Json(ConnectionTestSuccessResponse {
            status: "success".to_string(),
            message: "Successfully connected to upstream OpenAI-compatible API".to_string(),
            model_used: state.config.small_model.clone(),
            timestamp: now_timestamp_string(),
            response_id,
        })),
        Err(error) => {
            error!("Connection test failed: {}", error.message);
            res.status_code(StatusCode::SERVICE_UNAVAILABLE);
            res.render(Json(ConnectionTestFailureResponse {
                status: "failed".to_string(),
                error_type: "API Error".to_string(),
                message: error.message,
                timestamp: now_timestamp_string(),
                suggestions: vec![
                    "Check OPENAI_API_KEY".to_string(),
                    "Verify model permissions".to_string(),
                    "Check provider rate limits".to_string(),
                ],
            }));
        }
    }
}

#[handler]
pub async fn root(res: &mut Response) {
    let config = &app_state().config;
    res.render(Json(RootResponse {
        message: "Claude-to-OpenAI API Proxy (Rust/Salvo)".to_string(),
        status: "running".to_string(),
        config: RootConfig {
            openai_base_url: config.openai_base_url.clone(),
            api_key_configured: !config.openai_api_key.is_empty(),
            client_api_key_validation: config.anthropic_api_key.is_some(),
            wire_api: wire_api_name(&config.wire_api),
            big_model: config.big_model.clone(),
            middle_model: config.middle_model.clone(),
            small_model: config.small_model.clone(),
        },
        endpoints: RootEndpoints {
            messages: "/v1/messages".to_string(),
            count_tokens: "/v1/messages/count_tokens".to_string(),
            health: "/health".to_string(),
            test_connection: "/test-connection".to_string(),
        },
    }));
}

async fn parse_messages_request(
    req: &mut Request,
    res: &mut Response,
) -> Option<ClaudeMessagesRequest> {
    let max_size = app_state().config.request_body_max_size;
    match req
        .parse_json_with_max_size::<ClaudeMessagesRequest>(max_size)
        .await
    {
        Ok(value) => Some(value),
        Err(error) => {
            bad_request(res, &format!("invalid request body: {error}"));
            None
        }
    }
}

async fn handle_chat_message(
    res: &mut Response,
    request: ClaudeMessagesRequest,
    thinking_requested: bool,
) {
    let state = app_state();
    let mut openai_request = convert_claude_to_openai(&request, &state.config);

    if request.stream.unwrap_or(false) {
        handle_chat_streaming_request(res, request, &mut openai_request, thinking_requested).await;
        return;
    }

    let openai_response = match state.upstream.chat_completion(&openai_request).await {
        Ok(value) => value,
        Err(error) => {
            upstream_failed(res, error.status, &error.message);
            return;
        }
    };

    match convert_openai_to_claude_response(&openai_response, &request) {
        Ok(value) => res.render(Json(value)),
        Err(message) => internal_error(res, &message),
    }
}

async fn handle_responses_message(
    res: &mut Response,
    request: ClaudeMessagesRequest,
    thinking_requested: bool,
) {
    let state = app_state();
    let mut responses_request = convert_claude_to_responses(&request, &state.config);

    if request.stream.unwrap_or(false) {
        handle_responses_streaming_request(
            res,
            request,
            &mut responses_request,
            thinking_requested,
        )
        .await;
        return;
    }

    let upstream_response = match state.upstream.responses(&responses_request).await {
        Ok(value) => value,
        Err(error) => {
            upstream_failed(res, error.status, &error.message);
            return;
        }
    };

    match convert_openai_responses_to_claude_response(&upstream_response, &request) {
        Ok(value) => res.render(Json(value)),
        Err(message) => internal_error(res, &message),
    }
}

async fn handle_chat_streaming_request(
    res: &mut Response,
    request: ClaudeMessagesRequest,
    openai_request: &mut OpenAiChatRequest,
    thinking_requested: bool,
) {
    openai_request.enable_stream_usage();
    let upstream_response = match app_state()
        .upstream
        .chat_completion_stream(openai_request)
        .await
    {
        Ok(value) => value,
        Err(error) => {
            render_streaming_error(res, error.status, error.message);
            return;
        }
    };

    set_sse_headers(res);
    let sender = res.channel();
    tokio::spawn(async move {
        stream_openai_to_claude_sse(
            upstream_response,
            sender,
            request.model.clone(),
            thinking_requested,
        )
        .await;
    });
}

async fn handle_responses_streaming_request(
    res: &mut Response,
    request: ClaudeMessagesRequest,
    responses_request: &mut OpenAiResponsesRequest,
    thinking_requested: bool,
) {
    responses_request.enable_stream();
    let upstream_response = match app_state()
        .upstream
        .responses_stream(responses_request)
        .await
    {
        Ok(value) => value,
        Err(error) => {
            render_streaming_error(res, error.status, error.message);
            return;
        }
    };

    set_sse_headers(res);
    let sender = res.channel();
    tokio::spawn(async move {
        stream_openai_responses_to_claude_sse(
            upstream_response,
            sender,
            request.model.clone(),
            thinking_requested,
        )
        .await;
    });
}

fn render_streaming_error(res: &mut Response, status: StatusCode, message: String) {
    error!("Streaming upstream error: {}", message);
    res.status_code(status);
    res.render(Json(StreamingErrorResponse {
        response_type: "error".to_string(),
        error: ErrorDetail {
            error_type: "api_error".to_string(),
            message,
        },
    }));
}

fn set_sse_headers(res: &mut Response) {
    res.status_code(StatusCode::OK);
    let _ = res.add_header("Cache-Control", "no-cache", true);
    let _ = res.add_header("Connection", "keep-alive", true);
    let _ = res.add_header("Access-Control-Allow-Origin", "*", true);
    let _ = res.add_header("Access-Control-Allow-Headers", "*", true);
    let _ = res.add_header("Content-Type", "text/event-stream; charset=utf-8", true);
}

async fn run_chat_connection_test(
    state: &crate::state::AppState,
) -> Result<String, crate::errors::UpstreamError> {
    let test_request = OpenAiChatRequest {
        model: state.config.small_model.clone(),
        messages: vec![OpenAiMessage::User(OpenAiUserMessage::from_text(
            "Hello".to_string(),
        ))],
        max_tokens: 5,
        temperature: 1.0,
        reasoning_effort: None,
        stream: false,
        stream_options: None,
        stop: None,
        top_p: None,
        tools: None,
        tool_choice: None,
    };

    let response = state.upstream.chat_completion(&test_request).await?;
    Ok(response.id().unwrap_or("unknown").to_string())
}

async fn run_responses_connection_test(
    state: &crate::state::AppState,
) -> Result<String, crate::errors::UpstreamError> {
    let test_request = serde_json::json!({
        "model": state.config.small_model.clone(),
        "input": "Hello",
        "max_output_tokens": 5,
        "stream": false
    });

    let response = state.upstream.responses(&test_request).await?;
    Ok(response.id().unwrap_or("unknown").to_string())
}

fn wire_api_name(wire_api: &WireApi) -> String {
    match wire_api {
        WireApi::Chat => "chat".to_string(),
        WireApi::Responses => "responses".to_string(),
    }
}

fn validate_client_api_key_header(req: &Request) -> Result<(), String> {
    let config = &app_state().config;
    if config.anthropic_api_key.is_none() {
        return Ok(());
    }

    let x_api_key = req
        .headers()
        .get("x-api-key")
        .and_then(|value| value.to_str().ok());

    let authorization = req
        .headers()
        .get("authorization")
        .and_then(|value| value.to_str().ok());

    let provided_key = x_api_key
        .or_else(|| authorization.and_then(|auth_header| auth_header.strip_prefix("Bearer ")));

    if config.validate_client_api_key(provided_key) {
        Ok(())
    } else {
        Err("Invalid API key. Please provide a valid Anthropic API key.".to_string())
    }
}

fn estimate_input_tokens(token_request: &ClaudeTokenCountRequest) -> usize {
    let mut total_chars: usize = 0;
    if let Some(system) = &token_request.system {
        total_chars += count_system_text_chars(system);
    }
    for message in &token_request.messages {
        if let Some(content) = &message.content {
            total_chars += count_message_text_chars(content);
        }
    }
    std::cmp::max(1, total_chars / 4)
}

fn count_system_text_chars(system: &crate::models::ClaudeSystemContent) -> usize {
    match system {
        crate::models::ClaudeSystemContent::Text(text) => text.len(),
        crate::models::ClaudeSystemContent::Blocks(blocks) => {
            blocks.iter().map(count_system_block_text_chars).sum()
        }
        crate::models::ClaudeSystemContent::Other(value) => count_text_chars_in_value(value),
    }
}

fn count_system_block_text_chars(block: &crate::models::ClaudeSystemBlock) -> usize {
    match block {
        crate::models::ClaudeSystemBlock::Text { text, .. } => text.len(),
        crate::models::ClaudeSystemBlock::Unknown => 0,
    }
}

fn count_message_text_chars(content: &crate::models::ClaudeContent) -> usize {
    match content {
        crate::models::ClaudeContent::Text(text) => text.len(),
        crate::models::ClaudeContent::Blocks(blocks) => {
            blocks.iter().map(count_message_block_text_chars).sum()
        }
        crate::models::ClaudeContent::Other(value) => count_text_chars_in_value(value),
    }
}

fn count_message_block_text_chars(block: &crate::models::ClaudeContentBlock) -> usize {
    match block {
        crate::models::ClaudeContentBlock::Text { text, .. } => text.len(),
        _ => serde_json::to_value(block)
            .ok()
            .as_ref()
            .map(count_text_chars_in_value)
            .unwrap_or(0),
    }
}

fn count_text_chars_in_value(value: &Value) -> usize {
    match value {
        Value::Null => 0,
        Value::String(text) => text.len(),
        Value::Array(items) => items.iter().map(count_text_chars_in_value).sum(),
        Value::Object(_) => serde_json::from_value::<LooseTextCarrier>(value.clone())
            .ok()
            .and_then(|payload| payload.text)
            .map_or_else(
                || count_text_chars_in_object_values(value),
                |text| text.len(),
            ),
        _ => 0,
    }
}

fn count_text_chars_in_object_values(value: &Value) -> usize {
    let Value::Object(object) = value else {
        return 0;
    };
    object.values().map(count_text_chars_in_value).sum()
}

fn deserialize_optional_string<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<LooseString>::deserialize(deserializer)?;
    Ok(value.and_then(LooseString::into_string))
}

fn unauthorized(res: &mut Response, message: &str) {
    res.status_code(StatusCode::UNAUTHORIZED);
    res.render(Json(DetailResponse {
        detail: message.to_string(),
    }));
}

fn bad_request(res: &mut Response, message: &str) {
    res.status_code(StatusCode::BAD_REQUEST);
    res.render(Json(DetailResponse {
        detail: message.to_string(),
    }));
}

fn internal_error(res: &mut Response, message: &str) {
    res.status_code(StatusCode::INTERNAL_SERVER_ERROR);
    res.render(Json(DetailResponse {
        detail: message.to_string(),
    }));
}

fn upstream_failed(res: &mut Response, status: StatusCode, message: &str) {
    error!("Upstream error: {message}");
    res.status_code(status);
    res.render(Json(DetailResponse {
        detail: message.to_string(),
    }));
}

#[derive(Debug, Serialize)]
struct DetailResponse {
    detail: String,
}

#[derive(Debug, Serialize)]
struct TokenCountResponse {
    input_tokens: usize,
}

#[derive(Debug, Serialize)]
struct HealthCheckResponse {
    status: String,
    timestamp: String,
    openai_api_configured: bool,
    api_key_valid: bool,
    client_api_key_validation: bool,
}

#[derive(Debug, Serialize)]
struct ConnectionTestFailureResponse {
    status: String,
    error_type: String,
    message: String,
    timestamp: String,
    suggestions: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ConnectionTestSuccessResponse {
    status: String,
    message: String,
    model_used: String,
    timestamp: String,
    response_id: String,
}

#[derive(Debug, Serialize)]
struct StreamingErrorResponse {
    #[serde(rename = "type")]
    response_type: String,
    error: ErrorDetail,
}

#[derive(Debug, Serialize)]
struct ErrorDetail {
    #[serde(rename = "type")]
    error_type: String,
    message: String,
}

#[derive(Debug, Serialize)]
struct RootResponse {
    message: String,
    status: String,
    config: RootConfig,
    endpoints: RootEndpoints,
}

#[derive(Debug, Serialize)]
struct RootConfig {
    openai_base_url: String,
    api_key_configured: bool,
    client_api_key_validation: bool,
    wire_api: String,
    big_model: String,
    middle_model: String,
    small_model: String,
}

#[derive(Debug, Serialize)]
struct RootEndpoints {
    messages: String,
    count_tokens: String,
    health: String,
    test_connection: String,
}

#[derive(Debug, Deserialize)]
struct LooseTextCarrier {
    #[serde(default, deserialize_with = "deserialize_optional_string")]
    text: Option<String>,
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
            Self::String(value) => Some(value),
            Self::Other(_) => None,
        }
    }
}
