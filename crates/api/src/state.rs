use std::collections::HashMap;
use std::sync::Arc;

use lazyboy_control::SandboxProvider;
use lazyboy_sandbox::{DockerSandbox, FakeSandbox};
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;

use crate::auth::AuthConfig;
use crate::db::{Actor, Db};
use crate::mcp::McpHub;
use crate::memory::MemoryService;

#[derive(Clone, Default)]
pub struct CallRegistry {
    inner: Arc<std::sync::Mutex<HashMap<String, String>>>,
}

impl CallRegistry {
    /// Returns a lease that releases the slot when dropped, so a websocket
    /// upgrade that never completes cannot leave the bot marked as busy.
    pub fn try_begin(&self, bot_id: &str, call_id: &str) -> Option<CallLease> {
        let mut map = self.lock();
        if map.contains_key(bot_id) {
            return None;
        }
        map.insert(bot_id.to_string(), call_id.to_string());
        drop(map);
        Some(CallLease {
            registry: self.clone(),
            bot_id: bot_id.to_string(),
            call_id: call_id.to_string(),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, String>> {
        self.inner.lock().unwrap_or_else(|error| error.into_inner())
    }
}

pub struct CallLease {
    registry: CallRegistry,
    bot_id: String,
    call_id: String,
}

impl Drop for CallLease {
    fn drop(&mut self) {
        let mut map = self.registry.lock();
        if map
            .get(&self.bot_id)
            .is_some_and(|held| *held == self.call_id)
        {
            map.remove(&self.bot_id);
        }
    }
}

/// Capacity of the wake channel. A wake carries only a thread id, so a full
/// channel means a reader stopped draining: it degrades to that reader's
/// fallback poll rather than dropping a message, because the `events` table is
/// still the source of truth for order and replay.
const WAKE_CAPACITY: usize = 512;

/// Signals the session event stream that a thread's cursor moved. A wake only
/// says "read it now"; it is what turns the browser's event feed from a poll
/// into a push, which is what makes a chat reply feel instant.
#[derive(Clone)]
pub struct WakeBus {
    sender: tokio::sync::broadcast::Sender<String>,
}

impl WakeBus {
    fn with_capacity(capacity: usize) -> Self {
        let (sender, _) = tokio::sync::broadcast::channel(capacity);
        Self { sender }
    }

    /// Never blocks and never fails a request: nobody listening, or a reader too
    /// slow to keep up, is a latency concern only.
    pub fn wake(&self, thread_id: &str) {
        let _ = self.sender.send(thread_id.to_string());
    }

    pub fn subscribe(&self) -> WakeSubscription {
        WakeSubscription {
            receiver: self.sender.subscribe(),
        }
    }
}

impl Default for WakeBus {
    fn default() -> Self {
        Self::with_capacity(WAKE_CAPACITY)
    }
}

pub struct WakeSubscription {
    receiver: tokio::sync::broadcast::Receiver<String>,
}

impl WakeSubscription {
    /// Resolves when `thread_id` moves. Every branch either returns or waits,
    /// and `recv` is cancel safe, so a `select!` that drops this future cannot
    /// swallow a wake: the message stays queued for the next call.
    pub async fn wait(&mut self, thread_id: &str) {
        loop {
            match self.receiver.recv().await {
                Ok(received) if received == thread_id => return,
                Ok(_) => continue,
                // A lagged reader already missed wakes, so let the caller re-read
                // the database rather than wait for a signal it cannot see.
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => return,
                // A closed channel can never signal again: parking here keeps the
                // caller on its fallback poll instead of spinning on a future
                // that resolves immediately.
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    std::future::pending::<()>().await
                }
            }
        }
    }

    /// Resolves on a wake for any thread, for a caller that only cares that
    /// something happened. Each subscriber owns its own receiver, so this never
    /// steals a wake from a thread-scoped one.
    pub async fn wait_any(&mut self) {
        loop {
            match self.receiver.recv().await {
                Ok(_) => return,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => return,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    std::future::pending::<()>().await
                }
            }
        }
    }
}

#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    pub sandbox: Arc<dyn SandboxProvider>,
    pub data_dir: String,
    pub auth: AuthConfig,
    pub memory: MemoryService,
    pub mcp: McpHub,
    pub calls: CallRegistry,
    pub wakes: WakeBus,
}

impl AppState {
    pub async fn connect(database_url: &str) -> Result<Self, String> {
        let pool = PgPoolOptions::new()
            .max_connections(10)
            .connect(database_url)
            .await
            .map_err(|error| error.to_string())?;
        sqlx::migrate!("../../migrations")
            .run(&pool)
            .await
            .map_err(|error| error.to_string())?;
        let sandbox = sandbox_from_env();
        Ok(Self {
            db: Db { pool },
            sandbox,
            data_dir: std::env::var("DATA_DIR").unwrap_or_else(|_| "./data".into()),
            auth: AuthConfig::from_env(),
            memory: MemoryService::from_env(),
            mcp: McpHub::new(),
            calls: CallRegistry::default(),
            wakes: WakeBus::default(),
        })
    }

    pub async fn bootstrap(&self) -> Result<Actor, sqlx::Error> {
        self.db.ensure_local_actor().await
    }

    pub fn pool(&self) -> &PgPool {
        &self.db.pool
    }
}

fn sandbox_from_env() -> Arc<dyn SandboxProvider> {
    match std::env::var("SANDBOX_PROVIDER")
        .unwrap_or_else(|_| "docker".into())
        .as_str()
    {
        "fake" => Arc::new(FakeSandbox::new()),
        _ => {
            let url = std::env::var("SANDBOX_SUPERVISOR_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:7091".into());
            let token = std::env::var("SANDBOX_SUPERVISOR_TOKEN")
                .expect("SANDBOX_SUPERVISOR_TOKEN must be set");
            assert!(
                token.len() >= 32 && token != "dev-token",
                "SANDBOX_SUPERVISOR_TOKEN must be a non-default value of at least 32 characters"
            );
            Arc::new(DockerSandbox::new(url, token))
        }
    }
}

#[cfg(test)]
mod wake_bus_tests {
    use super::WakeBus;
    use std::time::Duration;

    #[tokio::test]
    async fn every_subscription_for_a_thread_observes_the_wake() {
        let bus = WakeBus::with_capacity(4);
        let mut first = bus.subscribe();
        let mut second = bus.subscribe();
        bus.wake("thread-1");
        for subscription in [&mut first, &mut second] {
            tokio::time::timeout(Duration::from_secs(1), subscription.wait("thread-1"))
                .await
                .expect("every subscriber sees the wake");
        }
    }

    #[tokio::test]
    async fn another_threads_wake_does_not_wake_me() {
        let bus = WakeBus::with_capacity(4);
        let mut mine = bus.subscribe();
        bus.wake("someone-else");
        tokio::time::timeout(Duration::from_millis(50), mine.wait("mine"))
            .await
            .expect_err("an unrelated thread stays silent");
        bus.wake("mine");
        tokio::time::timeout(Duration::from_secs(1), mine.wait("mine"))
            .await
            .expect("the matching thread resolves");
    }

    #[tokio::test]
    async fn a_lagged_reader_is_released_so_the_database_can_be_re_read() {
        let bus = WakeBus::with_capacity(4);
        let mut slow = bus.subscribe();
        for index in 0..32 {
            bus.wake(&format!("thread-{index}"));
        }
        // The backlog overflowed, so the wait resolves instead of hanging on a
        // signal this reader can no longer reach.
        tokio::time::timeout(Duration::from_secs(1), slow.wait("never-sent"))
            .await
            .expect("lag releases the reader");
    }

    #[tokio::test]
    async fn wait_any_answers_for_a_thread_a_waiter_ignored() {
        let bus = WakeBus::with_capacity(4);
        let mut scoped = bus.subscribe();
        let mut any = bus.subscribe();
        bus.wake("somewhere-else");
        tokio::time::timeout(Duration::from_secs(1), any.wait_any())
            .await
            .expect("any-waker answers for the thread the scoped one skipped");
        // The scoped subscription has its own receiver and is still waiting.
        tokio::time::timeout(Duration::from_millis(50), scoped.wait("mine"))
            .await
            .expect_err("the scoped subscription is untouched");
    }
}
