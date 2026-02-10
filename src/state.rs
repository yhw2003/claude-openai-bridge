use std::sync::OnceLock;

use crate::config::Config;
use crate::upstream::UpstreamClient;

#[derive(Clone, Debug)]
pub struct AppState {
    pub config: Config,
    pub upstream: UpstreamClient,
}

static APP_STATE: OnceLock<AppState> = OnceLock::new();

pub fn set_app_state(state: AppState) {
    APP_STATE
        .set(state)
        .expect("global state should only initialize once");
}

pub fn app_state() -> &'static AppState {
    APP_STATE
        .get()
        .expect("application state should be initialized before serving")
}
