// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! [`CardCache`]: one [`CachingCardResolver`] per card URL, shared by every
//! client that asks (ADR 0051, audit H8).

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use a2a_protocol_types::AgentCard;

use super::{AGENT_CARD_PATH, CachingCardResolver, CardFetchOptions, build_card_url};
use crate::error::ClientResult;

/// The default bound on the URLs a [`CardCache`] keeps.
pub const DEFAULT_MAX_ENTRIES: usize = 1024;

/// The refresh jitter a [`CardCache`]'s resolvers use by default.
const DEFAULT_JITTER: f64 = 0.1;

/// A cache of agent cards for many agents: one [`CachingCardResolver`] per
/// card URL, each with this cache's settings.
///
/// A supervisor that calls fifty peers holds one of these instead of fifty
/// resolvers, and every lookup of one URL, from any clone and any task,
/// shares that URL's stored card and its single fetch in flight. Clones share
/// the cache.
///
/// ```no_run
/// use std::time::Duration;
/// use a2a_protocol_client::ClientBuilder;
/// use a2a_protocol_client::discovery::CardCache;
///
/// # async fn demo() -> a2a_protocol_client::ClientResult<()> {
/// let cards = CardCache::new()
///     .with_default_freshness(Duration::from_secs(300))
///     .with_stale_if_error(Duration::from_secs(600));
/// for peer in ["https://alpha.example", "https://beta.example"] {
///     let client = ClientBuilder::from_card(&cards.resolve(peer).await?)?.build()?;
///     # drop(client);
/// }
/// # Ok(())
/// # }
/// ```
///
/// Every resolver of one cache sends the same request headers (its
/// [`CardFetchOptions`]), so a card fetched under one credential is never
/// returned for another (RFC 9111 §4.1): a second credential needs a second
/// cache. Each `with_*` call starts a new, empty cache.
///
/// Past [`with_max_entries`](Self::with_max_entries) URLs (default 1024), the
/// least recently used one is dropped; a resolver already handed out keeps
/// working, alone.
#[derive(Clone)]
pub struct CardCache {
    inner: Arc<Inner>,
}

#[derive(Clone)]
struct Settings {
    options: CardFetchOptions,
    #[cfg(feature = "tls-rustls")]
    pool: Option<crate::HttpPool>,
    path: String,
    always_revalidate: bool,
    default_freshness: Option<Duration>,
    stale_if_error: Option<Duration>,
    jitter: f64,
    max_entries: usize,
}

struct Inner {
    settings: Settings,
    entries: Mutex<Entries>,
}

#[derive(Default)]
struct Entries {
    /// Card URL to its resolver and the tick it was last used at.
    map: HashMap<String, (CachingCardResolver, u64)>,
    tick: u64,
}

impl Default for CardCache {
    fn default() -> Self {
        Self::new()
    }
}

impl CardCache {
    /// A cache with the default options, the well-known card path, a 10%
    /// refresh jitter and room for 1024 URLs.
    #[must_use]
    pub fn new() -> Self {
        Self::from_settings(Settings {
            options: CardFetchOptions::default(),
            #[cfg(feature = "tls-rustls")]
            pool: None,
            path: AGENT_CARD_PATH.to_owned(),
            always_revalidate: false,
            default_freshness: None,
            stale_if_error: None,
            jitter: DEFAULT_JITTER,
            max_entries: DEFAULT_MAX_ENTRIES,
        })
    }

    fn from_settings(settings: Settings) -> Self {
        Self {
            inner: Arc::new(Inner {
                settings,
                entries: Mutex::new(Entries::default()),
            }),
        }
    }

    /// A new, empty cache with `change` applied to this one's settings.
    fn with(&self, change: impl FnOnce(&mut Settings)) -> Self {
        let mut settings = self.inner.settings.clone();
        change(&mut settings);
        Self::from_settings(settings)
    }

    /// Every fetch sends and checks `options`
    /// ([`CachingCardResolver::with_options`]).
    #[must_use]
    pub fn with_options(self, options: CardFetchOptions) -> Self {
        self.with(|s| s.options = options)
    }

    /// Every fetch goes through `pool` ([`CachingCardResolver::with_pool`]).
    #[cfg(feature = "tls-rustls")]
    #[must_use]
    pub fn with_pool(self, pool: &crate::HttpPool) -> Self {
        self.with(|s| s.pool = Some(pool.clone()))
    }

    /// The path of the card under each base URL. Default:
    /// [`AGENT_CARD_PATH`].
    #[must_use]
    pub fn with_path(self, path: impl Into<String>) -> Self {
        let path = path.into();
        self.with(|s| s.path = path)
    }

    /// [`CachingCardResolver::with_always_revalidate`] for every URL.
    #[must_use]
    pub fn with_always_revalidate(self, always: bool) -> Self {
        self.with(|s| s.always_revalidate = always)
    }

    /// [`CachingCardResolver::with_default_freshness`] for every URL.
    #[must_use]
    pub fn with_default_freshness(self, lifetime: Duration) -> Self {
        self.with(|s| s.default_freshness = Some(lifetime))
    }

    /// [`CachingCardResolver::with_stale_if_error`] for every URL.
    #[must_use]
    pub fn with_stale_if_error(self, window: Duration) -> Self {
        self.with(|s| s.stale_if_error = Some(window))
    }

    /// [`CachingCardResolver::with_refresh_jitter`] for every URL. Default
    /// here: `0.1`.
    #[must_use]
    pub fn with_refresh_jitter(self, fraction: f64) -> Self {
        self.with(|s| s.jitter = fraction)
    }

    /// The most URLs kept; at least 1. Default: [`DEFAULT_MAX_ENTRIES`].
    #[must_use]
    pub fn with_max_entries(self, max: usize) -> Self {
        self.with(|s| s.max_entries = max.max(1))
    }

    /// The resolver for the card under `base_url`: the one this cache
    /// already holds for that URL, or a new one with the cache's settings.
    ///
    /// # Errors
    ///
    /// [`ClientError::InvalidEndpoint`](crate::ClientError::InvalidEndpoint)
    /// when `base_url` is malformed.
    pub fn resolver(&self, base_url: &str) -> ClientResult<CachingCardResolver> {
        let s = &self.inner.settings;
        let url = build_card_url(base_url, &s.path)?;
        if let Some(resolver) = self.touch(&url, None) {
            return Ok(resolver);
        }
        // Built outside the lock: a resolver builds an HTTP client. Two
        // callers that race here keep whichever was stored first.
        let built = self.new_resolver(base_url)?;
        Ok(self.touch(&url, Some(built.clone())).unwrap_or(built))
    }

    /// The resolver stored for `url`, marked as just used; with `insert`,
    /// stores it first when none is, dropping the least recently used URL
    /// past the bound.
    fn touch(&self, url: &str, insert: Option<CachingCardResolver>) -> Option<CachingCardResolver> {
        let mut entries = self
            .inner
            .entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        entries.tick += 1;
        let tick = entries.tick;
        if let Some((resolver, used)) = entries.map.get_mut(url) {
            *used = tick;
            return Some(resolver.clone());
        }
        let resolver = insert?;
        entries.map.insert(url.to_owned(), (resolver.clone(), tick));
        if entries.map.len() > self.inner.settings.max_entries
            && let Some(oldest) = entries
                .map
                .iter()
                .min_by_key(|(_, (_, used))| *used)
                .map(|(url, _)| url.clone())
        {
            entries.map.remove(&oldest);
        }
        drop(entries);
        Some(resolver)
    }

    fn new_resolver(&self, base_url: &str) -> ClientResult<CachingCardResolver> {
        let s = &self.inner.settings;
        let resolver = CachingCardResolver::with_path(base_url, &s.path)?
            .with_options(s.options.clone())
            .with_always_revalidate(s.always_revalidate)
            .with_refresh_jitter(s.jitter);
        #[cfg(feature = "tls-rustls")]
        let resolver = match &s.pool {
            Some(pool) => resolver.with_pool(pool),
            None => resolver,
        };
        let resolver = match s.default_freshness {
            Some(lifetime) => resolver.with_default_freshness(lifetime),
            None => resolver,
        };
        Ok(match s.stale_if_error {
            Some(window) => resolver.with_stale_if_error(window),
            None => resolver,
        })
    }

    /// The card under `base_url`: [`resolver`](Self::resolver), then
    /// [`CachingCardResolver::resolve`].
    ///
    /// # Errors
    ///
    /// Those of the two calls.
    pub async fn resolve(&self, base_url: &str) -> ClientResult<AgentCard> {
        self.resolver(base_url)?.resolve().await
    }

    /// How many URLs the cache holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner
            .entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .map
            .len()
    }

    /// Whether the cache holds no URL.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl std::fmt::Debug for CardCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // URLs are left out: a card URL may carry a key in its query.
        let s = &self.inner.settings;
        let mut d = f.debug_struct("CardCache");
        d.field("entries", &self.len())
            .field("max_entries", &s.max_entries)
            .field("options", &s.options)
            .field("path", &s.path)
            .field("always_revalidate", &s.always_revalidate)
            .field("default_freshness", &s.default_freshness)
            .field("stale_if_error", &s.stale_if_error)
            .field("jitter", &s.jitter);
        #[cfg(feature = "tls-rustls")]
        d.field("pooled", &s.pool.is_some());
        d.finish()
    }
}
