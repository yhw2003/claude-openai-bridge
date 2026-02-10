use std::time::{SystemTime, UNIX_EPOCH};

use salvo::http::StatusCode;
use tracing_subscriber::EnvFilter;

pub fn to_salvo_status(status: reqwest::StatusCode) -> StatusCode {
    StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY)
}

pub fn now_timestamp_string() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}

pub fn init_tracing(log_level: &str) {
    let normalized = log_level
        .split_whitespace()
        .next()
        .unwrap_or("info")
        .to_lowercase();

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(normalized));
    tracing_subscriber::fmt().with_env_filter(filter).init();
}
