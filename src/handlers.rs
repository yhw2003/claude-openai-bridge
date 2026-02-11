use salvo::http::StatusCode;
use salvo::prelude::*;
use serde_json::{Value, json};
use tracing::{debug, error, trace};

use crate::constants::ROLE_USER;
use crate::conversion::request::convert_claude_to_openai;
use crate::conversion::response::convert_openai_to_claude_response;
use crate::conversion::stream::stream_openai_to_claude_sse;
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

    let mut openai_request = convert_claude_to_openai(&request, &state.config);
    if request.stream.unwrap_or(false) {
        handle_streaming_request(res, request, &mut openai_request).await;
        return;
    }

    let openai_response = match state.upstream.chat_completion(&openai_request).await {
        Ok(value) => value,
        Err(error) => {
            upstream_failed(res, error.status, &error.message);
            return;
        }
    };

    let claude_response = match convert_openai_to_claude_response(&openai_response, &request) {
        Ok(value) => value,
        Err(message) => {
            internal_error(res, &message);
            return;
        }
    };

    res.render(Json(claude_response));
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
    res.render(Json(json!({"input_tokens": estimated_tokens})));
}

#[handler]
pub async fn health_check(res: &mut Response) {
    let config = &app_state().config;
    res.render(Json(json!({
        "status": "healthy",
        "timestamp": now_timestamp_string(),
        "openai_api_configured": !config.openai_api_key.is_empty(),
        "api_key_valid": config.validate_openai_api_key_format(),
        "client_api_key_validation": config.anthropic_api_key.is_some(),
    })));
}

#[handler]
pub async fn test_connection(res: &mut Response) {
    let state = app_state();
    let test_request = json!({
        "model": state.config.small_model,
        "messages": [{"role": ROLE_USER, "content": "Hello"}],
        "max_tokens": 5,
        "stream": false,
    });

    let upstream_response = match state.upstream.chat_completion(&test_request).await {
        Ok(value) => value,
        Err(error) => {
            error!("Connection test failed: {}", error.message);
            res.status_code(StatusCode::SERVICE_UNAVAILABLE);
            res.render(Json(json!({
                "status": "failed",
                "error_type": "API Error",
                "message": error.message,
                "timestamp": now_timestamp_string(),
                "suggestions": [
                    "Check OPENAI_API_KEY",
                    "Verify model permissions",
                    "Check provider rate limits"
                ]
            })));
            return;
        }
    };

    res.render(Json(json!({
        "status": "success",
        "message": "Successfully connected to upstream OpenAI-compatible API",
        "model_used": state.config.small_model,
        "timestamp": now_timestamp_string(),
        "response_id": upstream_response.get("id").and_then(Value::as_str).unwrap_or("unknown"),
    })));
}

#[handler]
pub async fn root(res: &mut Response) {
    let config = &app_state().config;
    res.render(Json(json!({
        "message": "Claude-to-OpenAI API Proxy (Rust/Salvo)",
        "status": "running",
        "config": {
            "openai_base_url": config.openai_base_url,
            "api_key_configured": !config.openai_api_key.is_empty(),
            "client_api_key_validation": config.anthropic_api_key.is_some(),
            "big_model": config.big_model,
            "middle_model": config.middle_model,
            "small_model": config.small_model,
        },
        "endpoints": {
            "messages": "/v1/messages",
            "count_tokens": "/v1/messages/count_tokens",
            "health": "/health",
            "test_connection": "/test-connection"
        }
    })));
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

async fn handle_streaming_request(
    res: &mut Response,
    request: ClaudeMessagesRequest,
    openai_request: &mut Value,
) {
    openai_request["stream"] = Value::Bool(true);
    openai_request["stream_options"] = json!({ "include_usage": true });

    let upstream_response = match app_state()
        .upstream
        .chat_completion_stream(openai_request)
        .await
    {
        Ok(value) => value,
        Err(error) => {
            error!("Streaming upstream error: {}", error.message);
            res.status_code(error.status);
            res.render(Json(json!({
                "type": "error",
                "error": {"type": "api_error", "message": error.message}
            })));
            return;
        }
    };

    set_sse_headers(res);
    let sender = res.channel();
    tokio::spawn(async move {
        stream_openai_to_claude_sse(upstream_response, sender, request.model.clone()).await;
    });
}

fn set_sse_headers(res: &mut Response) {
    res.status_code(StatusCode::OK);
    let _ = res.add_header("Cache-Control", "no-cache", true);
    let _ = res.add_header("Connection", "keep-alive", true);
    let _ = res.add_header("Access-Control-Allow-Origin", "*", true);
    let _ = res.add_header("Access-Control-Allow-Headers", "*", true);
    let _ = res.add_header("Content-Type", "text/event-stream; charset=utf-8", true);
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
        total_chars += count_text_chars(system);
    }
    for message in &token_request.messages {
        if let Some(content) = &message.content {
            total_chars += count_text_chars(content);
        }
    }
    std::cmp::max(1, total_chars / 4)
}

fn count_text_chars(value: &Value) -> usize {
    match value {
        Value::Null => 0,
        Value::String(text) => text.len(),
        Value::Array(items) => items.iter().map(count_text_chars).sum(),
        Value::Object(object) => match object.get("text").and_then(Value::as_str) {
            Some(text) => text.len(),
            None => object.values().map(count_text_chars).sum(),
        },
        _ => 0,
    }
}

fn unauthorized(res: &mut Response, message: &str) {
    res.status_code(StatusCode::UNAUTHORIZED);
    res.render(Json(json!({ "detail": message })));
}

fn bad_request(res: &mut Response, message: &str) {
    res.status_code(StatusCode::BAD_REQUEST);
    res.render(Json(json!({ "detail": message })));
}

fn internal_error(res: &mut Response, message: &str) {
    res.status_code(StatusCode::INTERNAL_SERVER_ERROR);
    res.render(Json(json!({ "detail": message })));
}

fn upstream_failed(res: &mut Response, status: StatusCode, message: &str) {
    error!("Upstream error: {message}");
    res.status_code(status);
    res.render(Json(json!({ "detail": message })));
}
