// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The agent-card poll watcher reports a card that stays broken once, not at
//! every poll.
//!
//! A test binary of its own, with a global subscriber installed before the
//! watcher's `warn!` is ever reached. `tracing` caches each callsite's
//! interest globally on first use; in the library's unit-test binary other
//! hot-reload tests reach the same callsite with no subscriber, which caches
//! it as uninterested and left a thread-local capture there seeing 0 warnings
//! in some full-suite runs (see `handler/shutdown/tests/warning.rs` for the
//! same failure, found 2026-08-19).

#![cfg(feature = "tracing")]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, SystemTime};

use a2a_protocol_server::HotReloadAgentCardHandler;
use a2a_protocol_types::agent_card::{AgentCapabilities, AgentCard, AgentInterface};
use tracing_subscriber::layer::{Context, Layer, SubscriberExt};

static WARNINGS: AtomicUsize = AtomicUsize::new(0);

struct CountWarnings;

impl<S: tracing::Subscriber> Layer<S> for CountWarnings {
    fn on_event(&self, event: &tracing::Event<'_>, _: Context<'_, S>) {
        if *event.metadata().level() == tracing::Level::WARN {
            WARNINGS.fetch_add(1, Ordering::SeqCst);
        }
    }
}

fn card() -> AgentCard {
    AgentCard {
        url: None,
        name: "Hot Reload".into(),
        description: "A card on disk".into(),
        version: "1.0.0".into(),
        supported_interfaces: vec![AgentInterface {
            url: "https://agent.example.com/rpc".into(),
            protocol_binding: "JSONRPC".into(),
            protocol_version: "1.0.0".into(),
            tenant: None,
        }],
        default_input_modes: vec!["text/plain".into()],
        default_output_modes: vec!["text/plain".into()],
        skills: vec![],
        capabilities: AgentCapabilities::none(),
        provider: None,
        icon_url: None,
        documentation_url: None,
        security_schemes: None,
        security_requirements: None,
        signatures: None,
    }
}

/// The watcher remembers the mtime of the last failed reload and warns again
/// only when the file changes; every poll in between retries and fails.
#[tokio::test]
async fn a_card_that_stays_broken_is_reported_once() {
    tracing::subscriber::set_global_default(tracing_subscriber::registry().with(CountWarnings))
        .expect("the only test in this binary installs the subscriber");

    let dir = std::env::temp_dir().join(format!("a2a_poll_once_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("agent_card.json");
    let initial = card();
    std::fs::write(&file, serde_json::to_string(&initial).unwrap()).unwrap();

    let handler = HotReloadAgentCardHandler::new(initial);
    let handle = handler.spawn_poll_watcher(&file, Duration::from_millis(20));
    std::fs::write(&file, "{\"name\": \"broken").unwrap();
    std::fs::File::options()
        .write(true)
        .open(&file)
        .unwrap()
        .set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000))
        .unwrap();

    // Wait for the first warning, then for many more polls at 20 ms.
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while WARNINGS.load(Ordering::SeqCst) == 0 {
        assert!(
            std::time::Instant::now() < deadline,
            "the broken card was never reported"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    handle.abort();
    let _ = std::fs::remove_file(&file);
    let _ = std::fs::remove_dir(&dir);

    assert_eq!(
        WARNINGS.load(Ordering::SeqCst),
        1,
        "a file that stays broken at one mtime is warned about once"
    );
}
