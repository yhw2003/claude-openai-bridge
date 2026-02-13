use dotenvy::dotenv;
use salvo::prelude::*;
use std::time::{Duration, Instant};
use tracing::{info, warn};

use crate::config::Config;
use crate::handlers;
use crate::state::{AppState, SessionManager, set_app_state};
use crate::upstream::UpstreamClient;
use crate::utils::init_tracing;

pub async fn run() {
    let _ = dotenv();
    let config = load_config_or_exit();
    init_tracing(&config.log_level);
    warn_if_validation_disabled(&config);

    let upstream = build_upstream_or_exit(config.clone());
    let sessions = SessionManager::new(
        config.session_ttl_min_secs,
        config.session_ttl_max_secs,
        config.session_cleanup_interval_secs,
    );
    spawn_session_cleanup_task(
        sessions.clone(),
        config.session_cleanup_interval_secs,
    );
    set_app_state(AppState {
        config: config.clone(),
        upstream,
        sessions,
    });

    info!(
        "Claude-to-OpenAI proxy starting on {}:{}",
        config.host, config.port
    );

    let acceptor = TcpListener::new((config.host.as_str(), config.port))
        .bind()
        .await;
    Server::new(acceptor).serve(handlers::router()).await;
}

fn load_config_or_exit() -> Config {
    match Config::load() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("Configuration Error: {error}");
            std::process::exit(1);
        }
    }
}

fn warn_if_validation_disabled(config: &Config) {
    if config.anthropic_api_key.is_none() {
        warn!("ANTHROPIC_API_KEY not set. Client API key validation is disabled.");
    }
}

fn build_upstream_or_exit(config: Config) -> UpstreamClient {
    match UpstreamClient::new(config) {
        Ok(upstream) => upstream,
        Err(error) => {
            eprintln!("Initialization Error: {error}");
            std::process::exit(1);
        }
    }
}

fn spawn_session_cleanup_task(sessions: SessionManager, interval_secs: u64) {
    tokio::spawn(async move {
        let interval = Duration::from_secs(interval_secs.max(1));
        loop {
            tokio::time::sleep(interval).await;
            let _ = sessions.cleanup_expired(Instant::now()).await;
        }
    });
}
