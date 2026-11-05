// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The name resolver behind the HTTPS client's TCP connector: the system's,
//! or one that pins one host name to one address (ADR 0050 decision 1).
//!
//! A pinned endpoint keeps its URL, so the `Host` header, the TLS server
//! name and the certificate check all use the host name, and only the TCP
//! connection goes to the address. Any other name (a proxy's) is resolved by
//! the system, so a pinned client still reaches its proxy.

use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use hyper_util::client::legacy::connect::dns::{GaiResolver, Name};

use super::dns::BoundedResolver;

/// The resolver of one hyper client.
#[derive(Clone, Debug)]
pub struct Resolver {
    system: GaiResolver,
    /// The host name answered with `addr`, lower-cased, without brackets.
    pin: Option<(Arc<str>, SocketAddr)>,
}

impl Resolver {
    /// The system's resolver.
    pub fn system() -> Self {
        Self {
            system: GaiResolver::new(),
            pin: None,
        }
    }

    /// `host` resolves to `addr`; every other name to what the system says.
    pub fn pinned(host: &str, addr: SocketAddr) -> Self {
        let host = host
            .trim_start_matches('[')
            .trim_end_matches(']')
            .to_ascii_lowercase();
        Self {
            system: GaiResolver::new(),
            pin: Some((Arc::from(host), addr)),
        }
    }

    /// The address `name` is pinned to, if it is the pinned one.
    fn pinned_for(&self, name: &str) -> Option<SocketAddr> {
        self.pin
            .as_ref()
            .filter(|(host, _)| host.eq_ignore_ascii_case(name))
            .map(|(_, addr)| *addr)
    }
}

/// The TCP connector under the HTTPS client: the system's resolver, or one
/// that pins the endpoint's host to an address, either way with the lookup
/// bounded by the connect timeout (audit B-28). A proxy's own name is always
/// resolved by the system.
pub type Tcp = hyper_util::client::legacy::connect::HttpConnector<BoundedResolver<Resolver>>;

type Addrs = std::vec::IntoIter<SocketAddr>;

impl tower_service::Service<Name> for Resolver {
    type Response = Addrs;
    type Error = std::io::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Addrs, std::io::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.system.poll_ready(cx)
    }

    fn call(&mut self, name: Name) -> Self::Future {
        if let Some(addr) = self.pinned_for(name.as_str()) {
            return Box::pin(std::future::ready(Ok(vec![addr].into_iter())));
        }
        let lookup = self.system.call(name);
        Box::pin(async move { Ok(lookup.await?.collect::<Vec<_>>().into_iter()) })
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use tower_service::Service;

    use super::*;

    async fn resolve(resolver: &mut Resolver, name: &str) -> Vec<SocketAddr> {
        resolver
            .call(Name::from_str(name).expect("name"))
            .await
            .expect("resolves")
            .collect()
    }

    #[tokio::test]
    async fn the_pinned_name_resolves_to_its_address_only() {
        let addr: SocketAddr = "192.0.2.7:8443".parse().unwrap();
        let mut resolver = Resolver::pinned("Agent.Example", addr);
        assert_eq!(resolve(&mut resolver, "agent.example").await, vec![addr]);
        assert_eq!(resolve(&mut resolver, "AGENT.EXAMPLE").await, vec![addr]);
    }

    #[tokio::test]
    async fn another_name_goes_to_the_system() {
        let addr: SocketAddr = "192.0.2.7:8443".parse().unwrap();
        let mut resolver = Resolver::pinned("agent.example", addr);
        let localhost = resolve(&mut resolver, "localhost").await;
        assert_ne!(localhost.len(), 0);
        assert!(
            localhost.iter().all(|a| a.ip().is_loopback()),
            "{localhost:?}"
        );
        let mut system = Resolver::system();
        assert!(
            resolve(&mut system, "localhost")
                .await
                .iter()
                .all(|a| a.ip().is_loopback())
        );
    }

    #[test]
    fn brackets_are_not_part_of_the_name() {
        let addr: SocketAddr = "[::1]:443".parse().unwrap();
        let resolver = Resolver::pinned("[::1]", addr);
        assert_eq!(resolver.pinned_for("::1"), Some(addr));
        assert_eq!(resolver.pinned_for("::2"), None);
        assert_eq!(Resolver::system().pinned_for("::1"), None);
    }
}
