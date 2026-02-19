use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::Path;

use serde::Deserialize;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WireApi {
    Chat,
    Responses,
}

#[derive(Clone, Debug)]
pub struct Config {
    pub openai_api_key: String,
    pub anthropic_api_key: Option<String>,
    pub openai_base_url: String,
    pub azure_api_version: Option<String>,
    pub host: String,
    pub port: u16,
    pub log_level: String,
    pub request_timeout: u64,
    pub stream_request_timeout: Option<u64>,
    pub request_body_max_size: usize,
    pub session_ttl_min_secs: u64,
    pub session_ttl_max_secs: u64,
    pub session_cleanup_interval_secs: u64,
    pub debug_tool_id_matching: bool,
    pub wire_api: WireApi,
    pub big_model: String,
    pub middle_model: String,
    pub small_model: String,
    pub min_thinking_level: Option<String>,
    pub custom_headers: HashMap<String, String>,
}

#[derive(Debug, Default, Deserialize)]
struct TomlConfigRaw {
    openai_api_key: Option<String>,
    anthropic_api_key: Option<String>,
    openai_base_url: Option<String>,
    azure_api_version: Option<String>,
    host: Option<String>,
    port: Option<u16>,
    log_level: Option<String>,
    request_timeout: Option<u64>,
    stream_request_timeout: Option<u64>,
    request_body_max_size: Option<usize>,
    session_ttl_min_secs: Option<u64>,
    session_ttl_max_secs: Option<u64>,
    session_cleanup_interval_secs: Option<u64>,
    debug_tool_id_matching: Option<bool>,
    wire_api: Option<String>,
    big_model: Option<String>,
    middle_model: Option<String>,
    small_model: Option<String>,
    min_thinking_level: Option<String>,
    custom_headers: Option<HashMap<String, String>>,
}

impl Config {
    pub fn load() -> Result<Self, String> {
        let toml_config = read_toml_config("config.toml")?.unwrap_or_default();

        let openai_api_key = env::var("OPENAI_API_KEY")
            .ok()
            .or(toml_config.openai_api_key)
            .ok_or_else(|| {
                "OPENAI_API_KEY not found in environment variables and config.toml".to_string()
            })?;

        let anthropic_api_key = env::var("ANTHROPIC_API_KEY")
            .ok()
            .or(toml_config.anthropic_api_key);

        let openai_base_url = env::var("OPENAI_BASE_URL")
            .ok()
            .or(toml_config.openai_base_url)
            .unwrap_or_else(|| "https://api.openai.com/v1".to_string());

        let azure_api_version = env::var("AZURE_API_VERSION")
            .ok()
            .or(toml_config.azure_api_version);

        let host = env::var("HOST")
            .ok()
            .or(toml_config.host)
            .unwrap_or_else(|| "0.0.0.0".to_string());

        let port = env_u16_with_fallback("PORT", toml_config.port.unwrap_or(8082));
        let log_level = env::var("LOG_LEVEL")
            .ok()
            .or(toml_config.log_level)
            .unwrap_or_else(|| "INFO".to_string());

        let request_timeout =
            env_u64_with_fallback("REQUEST_TIMEOUT", toml_config.request_timeout.unwrap_or(90));

        let stream_request_timeout = env_optional_u64("STREAM_REQUEST_TIMEOUT")
            .or(toml_config.stream_request_timeout)
            .filter(|value| *value > 0);

        let request_body_max_size = env_usize_with_fallback(
            "REQUEST_BODY_MAX_SIZE",
            toml_config
                .request_body_max_size
                .unwrap_or(16 * 1024 * 1024),
        );

        let session_ttl_min_secs = env_u64_with_fallback(
            "SESSION_TTL_MIN_SECS",
            toml_config.session_ttl_min_secs.unwrap_or(1800),
        );
        let session_ttl_max_secs = env_u64_with_fallback(
            "SESSION_TTL_MAX_SECS",
            toml_config.session_ttl_max_secs.unwrap_or(86400),
        );
        let session_cleanup_interval_secs = env_u64_with_fallback(
            "SESSION_CLEANUP_INTERVAL_SECS",
            toml_config.session_cleanup_interval_secs.unwrap_or(60),
        );

        validate_session_config(
            session_ttl_min_secs,
            session_ttl_max_secs,
            session_cleanup_interval_secs,
        )?;

        let debug_tool_id_matching = env_bool_with_fallback(
            "DEBUG_TOOL_ID_MATCHING",
            toml_config.debug_tool_id_matching.unwrap_or(false),
        );

        let wire_api_raw = env::var("WIRE_API").ok().or(toml_config.wire_api);
        let wire_api = parse_wire_api(wire_api_raw.as_deref())?;

        let big_model = env::var("BIG_MODEL")
            .ok()
            .or(toml_config.big_model)
            .unwrap_or_else(|| "gpt-4o".to_string());

        let middle_model = env::var("MIDDLE_MODEL")
            .ok()
            .or(toml_config.middle_model)
            .unwrap_or_else(|| big_model.clone());

        let small_model = env::var("SMALL_MODEL")
            .ok()
            .or(toml_config.small_model)
            .unwrap_or_else(|| "gpt-4o-mini".to_string());

        let min_thinking_level_raw = env::var("MIN_THINKING_LEVEL")
            .ok()
            .or(toml_config.min_thinking_level);
        let min_thinking_level = parse_min_thinking_level(min_thinking_level_raw.as_deref())?;

        let mut custom_headers = toml_config.custom_headers.unwrap_or_default();
        custom_headers.extend(collect_custom_headers());

        Ok(Self {
            openai_api_key,
            anthropic_api_key,
            openai_base_url,
            azure_api_version,
            host,
            port,
            log_level,
            request_timeout,
            stream_request_timeout,
            request_body_max_size,
            session_ttl_min_secs,
            session_ttl_max_secs,
            session_cleanup_interval_secs,
            debug_tool_id_matching,
            wire_api,
            big_model,
            middle_model,
            small_model,
            min_thinking_level,
            custom_headers,
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

fn validate_session_config(min_secs: u64, max_secs: u64, cleanup_secs: u64) -> Result<(), String> {
    if min_secs == 0 {
        return Err("SESSION_TTL_MIN_SECS must be > 0".to_string());
    }
    if max_secs < min_secs {
        return Err("SESSION_TTL_MAX_SECS must be >= SESSION_TTL_MIN_SECS".to_string());
    }
    if cleanup_secs == 0 {
        return Err("SESSION_CLEANUP_INTERVAL_SECS must be > 0".to_string());
    }

    Ok(())
}

fn read_toml_config(path: &str) -> Result<Option<TomlConfigRaw>, String> {
    let config_path = Path::new(path);

    if !config_path.exists() {
        return Ok(None);
    }

    let content = fs::read_to_string(config_path)
        .map_err(|error| format!("Failed to read {}: {}", config_path.display(), error))?;

    let parsed = toml::from_str::<TomlConfigRaw>(&content)
        .map_err(|error| format!("Failed to parse {}: {}", config_path.display(), error))?;

    Ok(Some(parsed))
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

fn parse_wire_api(value: Option<&str>) -> Result<WireApi, String> {
    let Some(raw_value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(WireApi::Chat);
    };

    match raw_value.to_ascii_lowercase().as_str() {
        "chat" => Ok(WireApi::Chat),
        "responses" => Ok(WireApi::Responses),
        _ => Err(format!(
            "Invalid WIRE_API value '{raw_value}'. Supported values: chat, responses."
        )),
    }
}

fn parse_min_thinking_level(value: Option<&str>) -> Result<Option<String>, String> {
    let Some(raw_value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };

    let normalized = raw_value.to_ascii_lowercase();
    match normalized.as_str() {
        "low" | "medium" | "high" => Ok(Some(normalized)),
        _ => Err(format!(
            "Invalid MIN_THINKING_LEVEL value '{raw_value}'. Supported values: low, medium, high."
        )),
    }
}

fn env_u16_with_fallback(key: &str, fallback: u16) -> u16 {
    env::var(key)
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(fallback)
}

fn env_u64_with_fallback(key: &str, fallback: u64) -> u64 {
    env::var(key)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(fallback)
}

fn env_optional_u64(key: &str) -> Option<u64> {
    env::var(key)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
}

fn env_bool_with_fallback(key: &str, fallback: bool) -> bool {
    env::var(key)
        .ok()
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(fallback)
}

fn env_usize_with_fallback(key: &str, fallback: usize) -> usize {
    env::var(key)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(fallback)
}

#[cfg(test)]
mod tests {
    use super::parse_min_thinking_level;

    #[test]
    fn parse_min_thinking_level_accepts_valid_values_case_insensitive() {
        assert_eq!(
            parse_min_thinking_level(Some("  LOW ")).expect("should parse"),
            Some("low".to_string())
        );
        assert_eq!(
            parse_min_thinking_level(Some("Medium")).expect("should parse"),
            Some("medium".to_string())
        );
        assert_eq!(
            parse_min_thinking_level(Some("HIGH")).expect("should parse"),
            Some("high".to_string())
        );
    }

    #[test]
    fn parse_min_thinking_level_treats_empty_as_none() {
        assert_eq!(parse_min_thinking_level(None).expect("should parse"), None);
        assert_eq!(
            parse_min_thinking_level(Some("   ")).expect("should parse"),
            None
        );
    }

    #[test]
    fn parse_min_thinking_level_rejects_invalid_values() {
        let error = parse_min_thinking_level(Some("max")).expect_err("should fail");
        assert!(error.contains("Invalid MIN_THINKING_LEVEL value 'max'"));
    }
}
