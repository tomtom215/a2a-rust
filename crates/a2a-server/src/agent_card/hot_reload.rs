// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Hot-reload agent card handler.
//!
//! [`HotReloadAgentCardHandler`] wraps an [`AgentCard`] behind an
//! [`Arc<RwLock<_>>`](std::sync::Arc) so the card can be replaced at runtime
//! without restarting the server. The handler implements [`AgentCardProducer`]
//! and can therefore be used with [`DynamicAgentCardHandler`].
//!
//! Three reload strategies are provided:
//!
//! | Method | Platform | Mechanism |
//! |---|---|---|
//! | [`reload_from_file`](HotReloadAgentCardHandler::reload_from_file) | all | Reads a JSON file on demand |
//! | [`spawn_poll_watcher`](HotReloadAgentCardHandler::spawn_poll_watcher) | all | Polls file modification time at a configurable interval |
//! | [`spawn_signal_watcher`](HotReloadAgentCardHandler::spawn_signal_watcher) | unix | Reloads on `SIGHUP` |
//!
//! # Example
//!
//! ```no_run
//! use std::path::Path;
//! use std::sync::Arc;
//! use a2a_protocol_types::agent_card::AgentCard;
//! use a2a_protocol_server::agent_card::hot_reload::HotReloadAgentCardHandler;
//!
//! # fn example(card: AgentCard) {
//! let handler = HotReloadAgentCardHandler::new(card);
//!
//! // Periodic polling (cross-platform).
//! let handle = handler.spawn_poll_watcher(
//!     Path::new("/etc/a2a/agent.json"),
//!     std::time::Duration::from_secs(30),
//! );
//! // `handle` can be dropped or `.abort()`-ed to stop polling.
//! # }
//! ```

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime};

use a2a_protocol_types::agent_card::AgentCard;
use a2a_protocol_types::error::A2aResult;

use crate::agent_card::dynamic_handler::AgentCardProducer;
use crate::error::{ServerError, ServerResult};

/// An agent card handler that supports hot-reloading.
///
/// The current [`AgentCard`] is stored behind an [`Arc<RwLock<_>>`] so that it
/// can be atomically swapped while the server continues to serve requests.
///
/// This type implements [`AgentCardProducer`], so it can be plugged directly
/// into a [`DynamicAgentCardHandler`](super::DynamicAgentCardHandler) for
/// full HTTP caching support.
#[derive(Debug, Clone)]
pub struct HotReloadAgentCardHandler {
    card: Arc<RwLock<AgentCard>>,
}

impl HotReloadAgentCardHandler {
    /// Creates a new handler with the given initial [`AgentCard`].
    #[must_use]
    pub fn new(card: AgentCard) -> Self {
        Self {
            card: Arc::new(RwLock::new(card)),
        }
    }

    /// Returns a snapshot of the current [`AgentCard`].
    ///
    /// This acquires a short-lived read lock and clones the card.
    ///
    /// # Panics
    ///
    /// Panics if the internal `RwLock` is poisoned (another thread panicked
    /// while holding the write lock).
    #[must_use]
    pub fn current(&self) -> AgentCard {
        self.card
            .read()
            .expect("agent card RwLock poisoned")
            .clone()
    }

    /// Replaces the current agent card with `card`.
    ///
    /// All subsequent requests will see the new card immediately.
    ///
    /// # Panics
    ///
    /// Panics if the internal `RwLock` is poisoned.
    pub fn update(&self, card: AgentCard) {
        let mut guard = self.card.write().expect("agent card RwLock poisoned");
        *guard = card;
    }

    /// Reloads the agent card from a JSON file at `path`.
    ///
    /// The file is read synchronously (agent card files are expected to be
    /// small). On success the internal card is replaced atomically.
    ///
    /// # Errors
    ///
    /// Returns [`ServerError::Internal`] if the file cannot be read or parsed.
    pub fn reload_from_file(&self, path: &Path) -> ServerResult<()> {
        let contents = std::fs::read_to_string(path).map_err(|e| {
            ServerError::Internal(format!(
                "failed to read agent card file {}: {e}",
                path.display()
            ))
        })?;
        self.reload_from_json(&contents)
    }

    /// Reloads the agent card from a JSON string.
    ///
    /// On success the internal card is replaced atomically.
    ///
    /// # Errors
    ///
    /// Returns [`ServerError::Serialization`] if `json` is not valid agent card JSON.
    pub fn reload_from_json(&self, json: &str) -> ServerResult<()> {
        let card: AgentCard = serde_json::from_str(json)?;
        self.update(card);
        Ok(())
    }

    /// Spawns a background task that periodically checks whether the file at
    /// `path` has been modified and reloads the agent card when it has.
    ///
    /// The watcher compares the file's modification time on each tick and only
    /// re-reads the file when the timestamp changes. This is cross-platform
    /// and requires no OS-specific file notification APIs.
    ///
    /// Returns a [`tokio::task::JoinHandle`] that can be used to abort the
    /// watcher (via [`JoinHandle::abort`](tokio::task::JoinHandle::abort)).
    #[must_use]
    pub fn spawn_poll_watcher(
        &self,
        path: &Path,
        interval: Duration,
    ) -> tokio::task::JoinHandle<()> {
        let handler = self.clone();
        let path = path.to_path_buf();
        tokio::spawn(poll_watcher_loop(handler, path, interval))
    }

    /// Spawns a background task that reloads the agent card from `path`
    /// whenever the process receives `SIGHUP`.
    ///
    /// This is the traditional Unix mechanism for configuration reload and
    /// integrates well with process managers (systemd, supervisord, etc.).
    ///
    /// Returns a [`tokio::task::JoinHandle`] that can be used to abort the
    /// watcher (via [`JoinHandle::abort`](tokio::task::JoinHandle::abort)).
    ///
    /// # Panics
    ///
    /// Panics if the tokio signal handler cannot be registered (e.g. if the
    /// runtime was built without the `signal` feature).
    #[cfg(unix)]
    #[must_use]
    pub fn spawn_signal_watcher(&self, path: &Path) -> tokio::task::JoinHandle<()> {
        let handler = self.clone();
        let path = path.to_path_buf();
        tokio::spawn(signal_watcher_loop(handler, path))
    }
}

impl AgentCardProducer for HotReloadAgentCardHandler {
    fn produce<'a>(&'a self) -> Pin<Box<dyn Future<Output = A2aResult<AgentCard>> + Send + 'a>> {
        Box::pin(async move { Ok(self.current()) })
    }
}

/// Returns the modification time of a file, or `None` if the metadata cannot
/// be read.
fn file_mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).ok().and_then(|m| m.modified().ok())
}

/// Background loop that polls `path` for modification time changes and reloads
/// the agent card when a change is detected.
async fn poll_watcher_loop(handler: HotReloadAgentCardHandler, path: PathBuf, interval: Duration) {
    let mut last_mtime = file_mtime(&path);
    let mut tick = tokio::time::interval(interval);
    // The first tick completes immediately; consume it so we don't reload on
    // startup (the caller already loaded the initial card).
    tick.tick().await;

    loop {
        tick.tick().await;
        let current_mtime = file_mtime(&path);
        if current_mtime != last_mtime {
            last_mtime = current_mtime;
            if let Err(e) = handler.reload_from_file(&path) {
                // Log the error but keep polling. The file may be temporarily
                // unavailable during an atomic rename-based deploy.
                #[cfg(feature = "tracing")]
                tracing::warn!(
                    path = %path.display(),
                    error = %e,
                    "hot-reload: failed to reload agent card",
                );
                let _ = e;
            }
        }
    }
}

/// Background loop that reloads the agent card on `SIGHUP`.
#[cfg(unix)]
async fn signal_watcher_loop(handler: HotReloadAgentCardHandler, path: PathBuf) {
    use tokio::signal::unix::{signal, SignalKind};

    let mut stream = signal(SignalKind::hangup()).expect("failed to register SIGHUP handler");

    loop {
        stream.recv().await;
        if let Err(e) = handler.reload_from_file(&path) {
            #[cfg(feature = "tracing")]
            tracing::warn!(
                path = %path.display(),
                error = %e,
                "hot-reload: SIGHUP reload failed",
            );
            let _ = e;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_card::caching::tests::minimal_agent_card;

    #[test]
    fn new_handler_returns_initial_card() {
        let card = minimal_agent_card();
        let handler = HotReloadAgentCardHandler::new(card.clone());
        let current = handler.current();
        assert_eq!(current.name, card.name);
        assert_eq!(current.version, card.version);
    }

    #[test]
    fn update_replaces_card() {
        let card1 = minimal_agent_card();
        let handler = HotReloadAgentCardHandler::new(card1);

        let mut card2 = minimal_agent_card();
        card2.name = "Updated Agent".into();
        handler.update(card2);

        assert_eq!(handler.current().name, "Updated Agent");
    }

    #[test]
    fn reload_from_json_valid() {
        let card = minimal_agent_card();
        let handler = HotReloadAgentCardHandler::new(card);

        let mut new_card = minimal_agent_card();
        new_card.name = "JSON Reloaded".into();
        let json = serde_json::to_string(&new_card).unwrap();

        handler.reload_from_json(&json).unwrap();
        assert_eq!(handler.current().name, "JSON Reloaded");
    }

    #[test]
    fn reload_from_json_invalid() {
        let card = minimal_agent_card();
        let handler = HotReloadAgentCardHandler::new(card);

        let result = handler.reload_from_json("not valid json {{{");
        assert!(result.is_err());
        // Original card should be unchanged.
        assert_eq!(handler.current().name, "Test Agent");
    }

    #[test]
    fn reload_from_file_valid() {
        let card = minimal_agent_card();
        let handler = HotReloadAgentCardHandler::new(card);

        let dir = std::env::temp_dir().join("a2a_hot_reload_test");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("agent_card.json");

        let mut new_card = minimal_agent_card();
        new_card.name = "File Reloaded".into();
        std::fs::write(&file, serde_json::to_string(&new_card).unwrap()).unwrap();

        handler.reload_from_file(&file).unwrap();
        assert_eq!(handler.current().name, "File Reloaded");

        // Cleanup.
        let _ = std::fs::remove_file(&file);
        let _ = std::fs::remove_dir(&dir);
    }

    #[test]
    fn reload_from_file_missing() {
        let card = minimal_agent_card();
        let handler = HotReloadAgentCardHandler::new(card);

        let result = handler.reload_from_file(Path::new("/tmp/nonexistent_a2a_card.json"));
        assert!(result.is_err());
    }

    #[test]
    fn clone_shares_state() {
        let card = minimal_agent_card();
        let handler1 = HotReloadAgentCardHandler::new(card);
        let handler2 = handler1.clone();

        let mut new_card = minimal_agent_card();
        new_card.name = "Shared Update".into();
        handler1.update(new_card);

        // Both clones should see the update.
        assert_eq!(handler2.current().name, "Shared Update");
    }

    #[tokio::test]
    async fn producer_trait_returns_current_card() {
        let card = minimal_agent_card();
        let handler = HotReloadAgentCardHandler::new(card.clone());

        let produced = handler.produce().await.unwrap();
        assert_eq!(produced.name, card.name);
    }

    #[tokio::test]
    async fn poll_watcher_detects_change() {
        let dir = std::env::temp_dir().join("a2a_poll_watcher_test");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("agent_card.json");

        let initial = minimal_agent_card();
        std::fs::write(&file, serde_json::to_string(&initial).unwrap()).unwrap();

        let handler = HotReloadAgentCardHandler::new(initial);
        let handle = handler.spawn_poll_watcher(&file, Duration::from_millis(50));

        // Wait a moment, then write an updated card.
        tokio::time::sleep(Duration::from_millis(100)).await;

        let mut updated = minimal_agent_card();
        updated.name = "Poll Updated".into();
        std::fs::write(&file, serde_json::to_string(&updated).unwrap()).unwrap();

        // Give the poller time to detect the change.
        tokio::time::sleep(Duration::from_millis(200)).await;

        assert_eq!(handler.current().name, "Poll Updated");

        handle.abort();

        // Cleanup.
        let _ = std::fs::remove_file(&file);
        let _ = std::fs::remove_dir(&dir);
    }
}
