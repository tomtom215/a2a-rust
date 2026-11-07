// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The endpoints a balanced client chooses among, and their refresh from
//! its [`EndpointSource`].

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use tokio::time::Instant;

use super::{Endpoint, EndpointSource};
use crate::error::{ClientError, ClientResult};
use crate::origin::OriginState;
use crate::single_flight::SingleFlight;
use crate::transport::Transport;

/// Builds the transport for one endpoint, with its origin's state.
pub type Factory =
    Arc<dyn Fn(&Endpoint) -> ClientResult<(Box<dyn Transport>, Arc<OriginState>)> + Send + Sync>;

/// One endpoint of the set.
pub struct Member {
    pub endpoint: Endpoint,
    pub state: Arc<OriginState>,
    pub transport: Box<dyn Transport>,
    /// When this member was last chosen, on the balancer's own count: ties
    /// on calls in flight go to the one chosen longest ago.
    pub last_pick: AtomicU64,
}

impl std::fmt::Debug for Member {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Member")
            .field("endpoint", &self.endpoint)
            .field("state", &self.state)
            .finish_non_exhaustive()
    }
}

/// The set as it stands.
pub type Set = Arc<Vec<Arc<Member>>>;

struct Current {
    set: Set,
    /// When the source last answered (or failed); `None`: never asked.
    at: Option<Instant>,
    refreshing: bool,
}

/// The members, and how to refresh them.
pub struct Members {
    source: Arc<dyn EndpointSource>,
    factory: Factory,
    current: Mutex<Current>,
    flight: SingleFlight<Set>,
}

impl Members {
    pub fn new(source: Arc<dyn EndpointSource>, factory: Factory) -> Self {
        Self {
            source,
            factory,
            current: Mutex::new(Current {
                set: Arc::new(Vec::new()),
                at: None,
                refreshing: false,
            }),
            flight: SingleFlight::default(),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Current> {
        self.current.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The set to choose from. The first call asks the source and waits for
    /// it; a later call past the source's interval starts a refresh in the
    /// background and uses the set there is.
    pub async fn get(self: &Arc<Self>) -> ClientResult<Set> {
        let Some((set, due)) = self.current_set() else {
            return self.flight.run(|| self.refresh()).await;
        };
        if due {
            let members = Arc::clone(self);
            tokio::spawn(async move {
                // A failure keeps the set and is logged by `refresh`.
                let _ = members.flight.run(|| members.refresh()).await;
            });
        }
        Ok(set)
    }

    /// The set there is and whether it is due a refresh (claiming the
    /// refresh when it is); `None` before the source first answered.
    fn current_set(&self) -> Option<(Set, bool)> {
        let mut current = self.lock();
        let at = current.at?;
        let due = !current.refreshing
            && self
                .source
                .refresh_every()
                .is_some_and(|every| Instant::now().saturating_duration_since(at) >= every);
        if due {
            current.refreshing = true;
        }
        let set = Arc::clone(&current.set);
        drop(current);
        Some((set, due))
    }

    /// Asks the source and replaces the set; an error or an empty answer
    /// keeps the set there was, and fails only when there was none.
    async fn refresh(&self) -> ClientResult<Set> {
        let answer = self.source.endpoints().await;
        let mut current = self.lock();
        current.refreshing = false;
        current.at = Some(Instant::now());
        let endpoints = match answer {
            Ok(endpoints) if !endpoints.is_empty() => endpoints,
            Ok(_) => {
                return Self::keep(
                    &current,
                    ClientError::Config("the endpoint source returned no endpoints".into()),
                );
            }
            Err(e) => return Self::keep(&current, e),
        };
        let mut next = Vec::with_capacity(endpoints.len());
        let mut failures = Vec::new();
        for endpoint in endpoints {
            if next.iter().any(|m: &Arc<Member>| m.endpoint == endpoint) {
                continue;
            }
            if let Some(kept) = current.set.iter().find(|m| m.endpoint == endpoint) {
                next.push(Arc::clone(kept));
                continue;
            }
            match (self.factory)(&endpoint) {
                Ok((transport, state)) => next.push(Arc::new(Member {
                    endpoint,
                    state,
                    transport,
                    last_pick: AtomicU64::new(0),
                })),
                Err(e) => failures.push(format!(
                    "{}: {e}",
                    crate::redact::without_userinfo(endpoint.url())
                )),
            }
        }
        if next.is_empty() {
            return Self::keep(
                &current,
                ClientError::Config(format!(
                    "no endpoint of the set could be built: {}",
                    failures.join("; ")
                )),
            );
        }
        if !failures.is_empty() {
            trace_warn!(skipped = %failures.join("; "), "endpoints skipped: they could not be built");
        }
        current.set = Arc::new(next);
        Ok(Arc::clone(&current.set))
    }

    /// The set there was, or `error` when there was none.
    fn keep(current: &Current, error: ClientError) -> ClientResult<Set> {
        if current.set.is_empty() {
            return Err(error);
        }
        trace_warn!(error = %error, "endpoint refresh failed; keeping the endpoints there were");
        Ok(Arc::clone(&current.set))
    }

    /// The set now, without asking anything; for tests.
    #[cfg(test)]
    pub fn current(&self) -> Set {
        Arc::clone(&self.lock().set)
    }
}

impl std::fmt::Debug for Members {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let current = self.lock();
        f.debug_struct("Members")
            .field("set", &current.set)
            .field("at", &current.at)
            .finish_non_exhaustive()
    }
}

/// The next pick number.
pub fn next_pick(counter: &AtomicU64) -> u64 {
    counter.fetch_add(1, Ordering::Relaxed) + 1
}

#[cfg(test)]
mod tests;
