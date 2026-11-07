// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The URL the agent card advertises in `A2A_BIND_ADDR` mode (audit
//! NEW-a-l-5).
//!
//! Until 0.16.0 the card named the bind address, `http://{addr}`: with
//! `A2A_BIND_ADDR=0.0.0.0:8080` in a container every client that read the card
//! dialled `0.0.0.0:8080`, its own loopback, the trap `examples/deploy-agent`
//! exists to show. `A2A_PUBLIC_URL` names the URL callers use, as
//! deploy-agent's `AGENT_URL` does; unset, a bind to every address is
//! advertised as `localhost`, which is right for the local runs these examples
//! are for and says nothing false about a remote one. The same file is in each
//! of the five examples that read `A2A_BIND_ADDR`.

use std::net::SocketAddr;

/// The card's URL for a server bound at `bound`, from `A2A_PUBLIC_URL`.
pub fn card_url(bound: SocketAddr) -> String {
    card_url_from(std::env::var("A2A_PUBLIC_URL").ok(), bound)
}

/// [`card_url`] with the variable's value given.
fn card_url_from(public: Option<String>, bound: SocketAddr) -> String {
    match public
        .as_deref()
        .map(str::trim)
        .filter(|url| !url.is_empty())
    {
        Some(url) => url.trim_end_matches('/').to_owned(),
        None if bound.ip().is_unspecified() => format!("http://localhost:{}", bound.port()),
        None => format!("http://{bound}"),
    }
}

#[cfg(test)]
mod tests {
    use super::card_url_from;

    #[test]
    fn the_card_names_a_url_a_client_can_dial() {
        let at = |addr: &str| addr.parse().expect("an address");
        assert_eq!(
            card_url_from(None, at("0.0.0.0:8080")),
            "http://localhost:8080"
        );
        assert_eq!(
            card_url_from(None, at("[::]:8080")),
            "http://localhost:8080"
        );
        assert_eq!(
            card_url_from(None, at("127.0.0.1:3000")),
            "http://127.0.0.1:3000"
        );
        assert_eq!(card_url_from(None, at("[::1]:3000")), "http://[::1]:3000");
        assert_eq!(
            card_url_from(
                Some("https://agent.example.com/".into()),
                at("0.0.0.0:8080")
            ),
            "https://agent.example.com"
        );
        assert_eq!(
            card_url_from(Some("  ".into()), at("127.0.0.1:3000")),
            "http://127.0.0.1:3000",
            "an empty variable is unset"
        );
    }
}
