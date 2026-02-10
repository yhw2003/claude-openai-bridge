use std::collections::HashMap;
use std::env;

#[derive(Clone, Debug)]
pub struct Config {
    pub openai_api_key: String,
    pub anthropic_api_key: Option<String>,
    pub openai_base_url: String,
    pub azure_api_version: Option<String>,
    pub host: String,
    pub port: u16,
    pub log_level: String,
    pub max_tokens_limit: u32,
    pub min_tokens_limit: u32,
    pub request_timeout: u64,
    pub request_body_max_size: usize,
    pub big_model: String,
    pub middle_model: String,
    pub small_model: String,
    pub custom_headers: HashMap<String, String>,
}

impl Config {
    pub fn from_env() -> Result<Self, String> {
        let openai_api_key = env::var("OPENAI_API_KEY")
            .map_err(|_| "OPENAI_API_KEY not found in environment variables".to_string())?;

        Ok(Self {
            openai_api_key,
            anthropic_api_key: env::var("ANTHROPIC_API_KEY").ok(),
            openai_base_url: env::var("OPENAI_BASE_URL")
                .unwrap_or_else(|_| "https://api.openai.com/v1".to_string()),
            azure_api_version: env::var("AZURE_API_VERSION").ok(),
            host: env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_string()),
            port: env_u16("PORT", 8082),
            log_level: env::var("LOG_LEVEL").unwrap_or_else(|_| "INFO".to_string()),
            max_tokens_limit: env_u32("MAX_TOKENS_LIMIT", 4096),
            min_tokens_limit: env_u32("MIN_TOKENS_LIMIT", 100),
            request_timeout: env_u64("REQUEST_TIMEOUT", 90),
            request_body_max_size: env_usize("REQUEST_BODY_MAX_SIZE", 16 * 1024 * 1024),
            big_model: env::var("BIG_MODEL").unwrap_or_else(|_| "gpt-4o".to_string()),
            middle_model: env::var("MIDDLE_MODEL").unwrap_or_else(|_| {
                env::var("BIG_MODEL").unwrap_or_else(|_| "gpt-4o".to_string())
            }),
            small_model: env::var("SMALL_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_string()),
            custom_headers: collect_custom_headers(),
        })
    }

    pub fn validate_openai_api_key_format(&self) -> bool {
        self.openai_api_key.starts_with("sk-")
    }

    pub fn validate_client_api_key(&self, provided_key: Option<&str>) -> bool {
        match self.anthropic_api_key.as_deref() {
            Some(expected) => provided_key.map(|key| key == expected).unwrap_or(false),
            None => true,
        }
    }
}

fn collect_custom_headers() -> HashMap<String, String> {
    let mut custom_headers = HashMap::new();
    for (env_key, env_value) in env::vars() {
        let Some(header_raw) = env_key.strip_prefix("CUSTOM_HEADER_") else {
            continue;
        };
        if header_raw.is_empty() {
            continue;
        }
        custom_headers.insert(header_raw.replace('_', "-"), env_value);
    }
    custom_headers
}

fn env_u16(key: &str, default: u16) -> u16 {
    env::var(key)
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(default)
}

fn env_u32(key: &str, default: u32) -> u32 {
    env::var(key)
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(default)
}

fn env_u64(key: &str, default: u64) -> u64 {
    env::var(key)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(default)
}

fn env_usize(key: &str, default: usize) -> usize {
    env::var(key)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(default)
}
