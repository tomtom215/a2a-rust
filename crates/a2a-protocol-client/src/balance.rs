// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Calls spread over several endpoints of one agent, with failing ones
//! ejected and idempotent calls optionally hedged (ADR 0050, audit H12).
//!
//! A client built with [`ClientBuilder::new(url)`](crate::ClientBuilder::new)
//! sends every call to `url`, and over `https://` all of them share one
//! HTTP/2 connection. Behind a layer-4 balancer, which picks a replica per
//! connection, they all reach one replica. With
//! [`ClientBuilder::with_balancing`](crate::ClientBuilder::with_balancing)
//! a client sends each call to the endpoint of a set with the fewest calls
//! in flight, keeps an endpoint that keeps failing out of rotation for a
//! while, and can send a slow idempotent call to a second endpoint.
//!
//! ```no_run
//! use std::time::Duration;
//! use a2a_protocol_client::ClientBuilder;
//! use a2a_protocol_client::balance::{BalancePolicy, DnsEndpoints, Hedge};
//!
//! # fn main() -> a2a_protocol_client::ClientResult<()> {
//! // A Kubernetes headless Service: one name, one address per pod.
//! let url = "https://agent.ns.svc.cluster.local:8443";
//! let client = ClientBuilder::new(url)
//!     .with_balancing(
//!         DnsEndpoints::new(url)?,
//!         BalancePolicy::new().with_hedge(Hedge::after(Duration::from_millis(200))),
//!     )
//!     .build()?;
//! # drop(client);
//! # Ok(())
//! # }
//! ```

mod members;
mod selection;
mod transport;

use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::time::Duration;

use a2a_protocol_types::AgentCard;

use crate::error::{ClientError, ClientResult};
use crate::retry::RetryBudget;

pub(crate) use transport::BalancedTransport;

/// How often [`DnsEndpoints`] resolves its name again, by default.
pub const DEFAULT_DNS_REFRESH: Duration = Duration::from_secs(30);

/// One endpoint of an agent: a URL, optionally pinned to a socket address.
///
/// A pinned endpoint is still named by its URL (the `Host` header, TLS
/// server name and certificate check use the URL's host); only the TCP
/// connection goes to the address. Pinning needs the `tls-rustls` feature.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Endpoint {
    url: String,
    addr: Option<SocketAddr>,
}

impl Endpoint {
    /// An endpoint at `url`, resolved the usual way.
    #[must_use]
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            addr: None,
        }
    }

    /// An endpoint named by `url` whose connections go to `addr`.
    #[must_use]
    pub fn pinned(url: impl Into<String>, addr: SocketAddr) -> Self {
        Self {
            url: url.into(),
            addr: Some(addr),
        }
    }

    /// The URL.
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }

    /// The address connections go to, when pinned.
    #[must_use]
    pub const fn addr(&self) -> Option<SocketAddr> {
        self.addr
    }
}

/// The boxed future an [`EndpointSource`] returns.
pub type EndpointsFuture<'a> =
    Pin<Box<dyn Future<Output = ClientResult<Vec<Endpoint>>> + Send + 'a>>;

/// Where a balanced client's endpoints come from.
///
/// [`endpoints`](Self::endpoints) is asked on the client's first call and
/// again once [`refresh_every`](Self::refresh_every) has passed since the
/// last answer, by one caller at a time while the others use the set they
/// have. An error, or an empty set, keeps the set there was.
pub trait EndpointSource: Send + Sync + 'static {
    /// The endpoints now.
    fn endpoints(&self) -> EndpointsFuture<'_>;

    /// How long an answer holds; `None`: for ever (the default).
    fn refresh_every(&self) -> Option<Duration> {
        None
    }
}

/// A fixed set of endpoints.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StaticEndpoints {
    endpoints: Vec<Endpoint>,
}

impl StaticEndpoints {
    /// The replicas at `urls`.
    #[must_use]
    pub fn new<I, S>(urls: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            endpoints: urls.into_iter().map(Endpoint::new).collect(),
        }
    }

    /// Exactly these endpoints, pinned or not.
    #[must_use]
    pub const fn from_endpoints(endpoints: Vec<Endpoint>) -> Self {
        Self { endpoints }
    }

    /// The URLs of `card`'s interfaces of `binding` (matched as the builder
    /// matches bindings: ignoring case, `REST` for `HTTP+JSON`) for this
    /// SDK's protocol major, with the tenant of the first of them; for an
    /// agent that publishes one interface per replica.
    ///
    /// # Errors
    ///
    /// [`ClientError::Config`] when the card has no such interface.
    pub fn from_card(card: &AgentCard, binding: &str) -> ClientResult<Self> {
        use crate::builder::selection::{canonical_binding, is_compatible};
        let wanted = canonical_binding(binding);
        let matching: Vec<_> = card
            .supported_interfaces
            .iter()
            .filter(|i| {
                wanted.is_some()
                    && canonical_binding(&i.protocol_binding) == wanted
                    && is_compatible(&i.protocol_version)
            })
            .collect();
        let Some(first) = matching.first() else {
            return Err(ClientError::Config(format!(
                "the agent card has no {binding} interface for this SDK's protocol major"
            )));
        };
        let tenant = first.tenant.clone();
        Ok(Self::new(
            matching
                .iter()
                .filter(|i| i.tenant == tenant)
                .map(|i| i.url.clone()),
        ))
    }

    /// The endpoints.
    #[must_use]
    pub fn endpoints(&self) -> &[Endpoint] {
        &self.endpoints
    }
}

impl EndpointSource for StaticEndpoints {
    fn endpoints(&self) -> EndpointsFuture<'_> {
        let endpoints = self.endpoints.clone();
        Box::pin(async move { Ok(endpoints) })
    }
}

/// Every address a URL's host resolves to, one pinned endpoint each,
/// resolved again every 30 s: a Kubernetes headless Service, or any name
/// with one address per replica.
///
/// Resolution is the system's (`getaddrinfo`), which reports no record
/// lifetimes; [`with_refresh_every`](Self::with_refresh_every) sets how long
/// an answer is kept. A host that is an IP address gives one endpoint. Over
/// a proxy, the proxy resolves the name, so every endpoint reaches whatever
/// replica the proxy picks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DnsEndpoints {
    url: String,
    host: String,
    port: u16,
    refresh: Duration,
}

impl DnsEndpoints {
    /// The addresses of `url`'s host.
    ///
    /// # Errors
    ///
    /// [`ClientError::InvalidEndpoint`] when `url` has no host.
    pub fn new(url: impl Into<String>) -> ClientResult<Self> {
        let url = url.into();
        let uri: hyper::Uri = url
            .parse()
            .map_err(|e| ClientError::InvalidEndpoint(format!("{url}: {e}")))?;
        let host = uri
            .host()
            .ok_or_else(|| ClientError::InvalidEndpoint(format!("{url} has no host")))?
            .trim_start_matches('[')
            .trim_end_matches(']')
            .to_owned();
        let port = uri.port_u16().unwrap_or_else(|| {
            if uri.scheme_str() == Some("https") {
                443
            } else {
                80
            }
        });
        Ok(Self {
            url,
            host,
            port,
            refresh: DEFAULT_DNS_REFRESH,
        })
    }

    /// How long an answer is kept before the name is resolved again.
    /// Default: [`DEFAULT_DNS_REFRESH`].
    #[must_use]
    pub const fn with_refresh_every(mut self, every: Duration) -> Self {
        self.refresh = every;
        self
    }
}

impl EndpointSource for DnsEndpoints {
    fn endpoints(&self) -> EndpointsFuture<'_> {
        Box::pin(async move {
            if self.host.parse::<std::net::IpAddr>().is_ok() {
                return Ok(vec![Endpoint::new(self.url.clone())]);
            }
            let addrs = tokio::net::lookup_host((self.host.as_str(), self.port))
                .await
                .map_err(|e| ClientError::HttpClient(format!("resolving {}: {e}", self.host)))?;
            let mut seen = Vec::new();
            for addr in addrs {
                if !seen.contains(&addr) {
                    seen.push(addr);
                }
            }
            Ok(seen
                .into_iter()
                .map(|addr| Endpoint::pinned(self.url.clone(), addr))
                .collect())
        })
    }

    fn refresh_every(&self) -> Option<Duration> {
        Some(self.refresh)
    }
}

/// When an endpoint is taken out of rotation (ADR 0050 decision 3), and, on
/// an [`HttpPool`](crate::HttpPool), when its circuit opens.
///
/// `consecutive_failures` failures in a row (a connection error, a timeout,
/// `502`, `503`, `504`) eject an endpoint for `base`; the n-th ejection in a
/// row lasts `base * 2^(n-1)`, at most `max`. When an ejection ends, one call
/// is let through: its success returns the endpoint, its failure ejects it
/// again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Ejection {
    /// Failures in a row that eject. At least 1.
    pub consecutive_failures: u32,
    /// How long the first ejection lasts.
    pub base: Duration,
    /// The longest an ejection lasts.
    pub max: Duration,
}

impl Default for Ejection {
    /// Five failures in a row; 30 s, doubling to at most 300 s.
    fn default() -> Self {
        Self {
            consecutive_failures: 5,
            base: Duration::from_secs(30),
            max: Duration::from_secs(300),
        }
    }
}

impl Ejection {
    /// Ejection after `failures` in a row (at least 1), for `base` doubling
    /// up to `max` (at least `base`).
    #[must_use]
    pub fn new(failures: u32, base: Duration, max: Duration) -> Self {
        Self {
            consecutive_failures: failures.max(1),
            base,
            max: max.max(base),
        }
    }
}

/// Opt-in hedging: a second copy of a slow idempotent call (ADR 0050
/// decision 5).
///
/// A unary call that has had no answer after `delay` is sent to a second
/// endpoint as well, and the first answer that is not an error is returned;
/// the other call is dropped. Only idempotent methods are hedged (`GetTask`,
/// `ListTasks`, `CancelTask`, `GetExtendedAgentCard`, the push-config reads
/// and delete), and a `SendMessage` that carries an idempotency key to a
/// peer known to honour it; a send without a key never is, and nor is a
/// stream. Each hedge is paid from `budget`.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct Hedge {
    /// How long a call waits for an answer before its copy is sent.
    pub delay: Duration,
    /// What pays for the copies: by default 5% of calls, with a reserve of
    /// one.
    pub budget: RetryBudget,
}

impl Hedge {
    /// Hedge after `delay`, with the default budget.
    #[must_use]
    pub const fn after(delay: Duration) -> Self {
        Self {
            delay,
            budget: RetryBudget::new(0.05, 0.0).with_reserve(1.0),
        }
    }

    /// Pays for the copies from `budget`.
    #[must_use]
    pub const fn with_budget(mut self, budget: RetryBudget) -> Self {
        self.budget = budget;
        self
    }
}

/// How a balanced client chooses and ejects endpoints.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct BalancePolicy {
    /// When an endpoint is taken out of rotation; `None`: never. Default:
    /// [`Ejection::default`]. A client on an [`HttpPool`](crate::HttpPool)
    /// uses the pool's instead (`HttpPoolBuilder::with_ejection`).
    pub ejection: Option<Ejection>,
    /// The largest share of the set, in percent, kept out of rotation at
    /// once (rounded down); past it the endpoints whose ejection ends soonest
    /// are used. Default 50; 100 lets every endpoint be ejected, and calls
    /// then fail with [`ClientError::CircuitOpen`].
    pub max_ejected_percent: u8,
    /// Hedging; `None` (the default): off.
    pub hedge: Option<Hedge>,
    /// Whether [`with_ejection`](Self::with_ejection) or
    /// [`without_ejection`](Self::without_ejection) was called, which a
    /// pool's own policy conflicts with.
    ejection_set: bool,
}

impl Default for BalancePolicy {
    fn default() -> Self {
        Self::new()
    }
}

impl BalancePolicy {
    /// Ejection on with its defaults, at most half the set ejected, no
    /// hedging.
    #[must_use]
    pub fn new() -> Self {
        Self {
            ejection: Some(Ejection::default()),
            max_ejected_percent: 50,
            hedge: None,
            ejection_set: false,
        }
    }

    /// Ejects as `ejection` says.
    #[must_use]
    pub const fn with_ejection(mut self, ejection: Ejection) -> Self {
        self.ejection = Some(ejection);
        self.ejection_set = true;
        self
    }

    /// Never ejects.
    #[must_use]
    pub const fn without_ejection(mut self) -> Self {
        self.ejection = None;
        self.ejection_set = true;
        self
    }

    /// The largest share of the set kept out at once, in percent (at most
    /// 100).
    #[must_use]
    pub fn with_max_ejected_percent(mut self, percent: u8) -> Self {
        self.max_ejected_percent = percent.min(100);
        self
    }

    /// Hedges idempotent calls as `hedge` says.
    #[must_use]
    pub const fn with_hedge(mut self, hedge: Hedge) -> Self {
        self.hedge = Some(hedge);
        self
    }

    /// Whether the caller chose the ejection policy.
    pub(crate) const fn ejection_set(&self) -> bool {
        self.ejection_set
    }
}

#[cfg(test)]
mod tests;
