use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use tokio::sync::RwLock;
use uuid::Uuid;

use crate::config::Config;
use crate::upstream::UpstreamClient;

const SESSION_TTL_TOKEN_K: f64 = 50_000.0;

#[derive(Clone, Debug)]
pub struct AppState {
    pub config: Config,
    pub upstream: UpstreamClient,
    pub sessions: SessionManager,
}

#[derive(Clone, Debug)]
pub struct SessionManager {
    inner: Arc<RwLock<SessionStore>>,
    ttl_min: Duration,
    ttl_max: Duration,
    cleanup_interval: Duration,
}

#[derive(Debug)]
struct SessionStore {
    sessions: HashMap<String, SessionEntry>,
    last_cleanup: Instant,
}

#[derive(Debug)]
struct SessionEntry {
    session_id: String,
    last_seen: Instant,
    total_tokens: u64,
}

impl SessionManager {
    pub fn new(ttl_min_secs: u64, ttl_max_secs: u64, cleanup_interval_secs: u64) -> Self {
        let now = Instant::now();
        Self {
            inner: Arc::new(RwLock::new(SessionStore {
                sessions: HashMap::new(),
                last_cleanup: now,
            })),
            ttl_min: Duration::from_secs(ttl_min_secs),
            ttl_max: Duration::from_secs(ttl_max_secs),
            cleanup_interval: Duration::from_secs(cleanup_interval_secs),
        }
    }

    pub async fn resolve_session_id(&self, identity_key: &str) -> String {
        let now = Instant::now();
        let mut store = self.inner.write().await;
        self.maybe_cleanup_locked(&mut store, now);

        if let Some(entry) = store.sessions.get_mut(identity_key) {
            entry.last_seen = now;
            return entry.session_id.clone();
        }

        let session_id = Uuid::new_v4().to_string();
        store.sessions.insert(
            identity_key.to_string(),
            SessionEntry {
                session_id: session_id.clone(),
                last_seen: now,
                total_tokens: 0,
            },
        );
        session_id
    }

    #[allow(dead_code)]
    pub async fn touch(&self, identity_key: &str) {
        let now = Instant::now();
        let mut store = self.inner.write().await;
        if let Some(entry) = store.sessions.get_mut(identity_key) {
            entry.last_seen = now;
        }
    }

    pub async fn add_usage(&self, identity_key: &str, tokens: u64) {
        let now = Instant::now();
        let mut store = self.inner.write().await;
        if let Some(entry) = store.sessions.get_mut(identity_key) {
            entry.total_tokens = entry.total_tokens.saturating_add(tokens);
            entry.last_seen = now;
            return;
        }

        store.sessions.insert(
            identity_key.to_string(),
            SessionEntry {
                session_id: Uuid::new_v4().to_string(),
                last_seen: now,
                total_tokens: tokens,
            },
        );
    }

    pub async fn cleanup_expired(&self, now: Instant) -> usize {
        let mut store = self.inner.write().await;
        let removed = self.cleanup_expired_locked(&mut store, now);
        store.last_cleanup = now;
        removed
    }

    fn maybe_cleanup_locked(&self, store: &mut SessionStore, now: Instant) {
        let elapsed = now
            .checked_duration_since(store.last_cleanup)
            .unwrap_or_default();
        if elapsed < self.cleanup_interval {
            return;
        }

        self.cleanup_expired_locked(store, now);
        store.last_cleanup = now;
    }

    fn cleanup_expired_locked(&self, store: &mut SessionStore, now: Instant) -> usize {
        let before = store.sessions.len();
        store
            .sessions
            .retain(|_, entry| !self.is_expired(entry, now));
        before.saturating_sub(store.sessions.len())
    }

    fn is_expired(&self, entry: &SessionEntry, now: Instant) -> bool {
        let ttl = self.dynamic_ttl(entry.total_tokens);
        now.checked_duration_since(entry.last_seen)
            .unwrap_or_default()
            > ttl
    }

    fn dynamic_ttl(&self, total_tokens: u64) -> Duration {
        let min_secs = self.ttl_min.as_secs() as f64;
        let max_secs = self.ttl_max.as_secs() as f64;
        if max_secs <= min_secs {
            return self.ttl_min;
        }

        let usage = total_tokens as f64;
        let factor = usage / (usage + SESSION_TTL_TOKEN_K);
        let ttl_secs = min_secs + (max_secs - min_secs) * factor;
        let bounded = ttl_secs.clamp(min_secs, max_secs);
        Duration::from_secs(bounded as u64)
    }
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

#[cfg(test)]
mod tests {
    use super::{SessionEntry, SessionManager};
    use std::time::{Duration, Instant};

    #[tokio::test]
    async fn reuses_session_for_same_identity() {
        let manager = SessionManager::new(10, 100, 60);
        let first = manager.resolve_session_id("identity-a").await;
        let second = manager.resolve_session_id("identity-a").await;
        assert_eq!(first, second);
    }

    #[tokio::test]
    async fn creates_distinct_session_for_distinct_identity() {
        let manager = SessionManager::new(10, 100, 60);
        let first = manager.resolve_session_id("identity-a").await;
        let second = manager.resolve_session_id("identity-b").await;
        assert_ne!(first, second);
    }

    #[test]
    fn adaptive_ttl_is_bounded_and_monotonic() {
        let manager = SessionManager::new(600, 7200, 60);

        let ttl_zero = manager.dynamic_ttl(0).as_secs();
        let ttl_mid = manager.dynamic_ttl(50_000).as_secs();
        let ttl_high = manager.dynamic_ttl(50_000_000).as_secs();

        assert!(ttl_zero >= 600);
        assert!(ttl_high <= 7200);
        assert!(ttl_zero <= ttl_mid);
        assert!(ttl_mid <= ttl_high);
        assert!(ttl_high >= 7190);
    }

    #[tokio::test]
    async fn cleanup_removes_expired_but_keeps_active() {
        let manager = SessionManager::new(60, 3600, 60);
        let now = Instant::now();

        {
            let mut store = manager.inner.write().await;
            store.sessions.insert(
                "expired".to_string(),
                SessionEntry {
                    session_id: "s1".to_string(),
                    last_seen: now - Duration::from_secs(120),
                    total_tokens: 0,
                },
            );
            store.sessions.insert(
                "active".to_string(),
                SessionEntry {
                    session_id: "s2".to_string(),
                    last_seen: now - Duration::from_secs(30),
                    total_tokens: 0,
                },
            );
        }

        let removed = manager.cleanup_expired(now).await;
        assert_eq!(removed, 1);

        let store = manager.inner.read().await;
        assert!(!store.sessions.contains_key("expired"));
        assert!(store.sessions.contains_key("active"));
    }
}
