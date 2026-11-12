// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The connection metadata a binding hands a call: the peer's address and
//! certificate, as request extensions and as the headers-map keys they travel
//! under. Documented, and re-exported, at [`call_context`](crate::call_context).

use std::net::SocketAddr;

/// The headers-map key under which a binding records the address of the
/// connection a call arrived on, as `ip:port`; see [`CallContext::peer_addr`].
///
/// A transport of your own that calls the
/// [`RequestHandler`](crate::RequestHandler) directly sets it in the headers
/// map it passes.
pub const PEER_ADDR_KEY: &str = ":peer-addr";

/// The address of the connection a request arrived on, as an HTTP request
/// extension.
///
/// The SDK's servers insert it into each request before dispatching it, and
/// the dispatchers copy it to [`CallContext::peer_addr`]; see the
/// [module docs](crate::call_context).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PeerAddr(pub SocketAddr);

/// The headers-map key under which a binding records the client's
/// mutual-TLS certificate, standard base64 of its DER encoding; see
/// [`CallContext::peer_certificate`].
pub const PEER_CERTIFICATE_KEY: &str = ":peer-certificate";

/// The certificate a client presented in the TLS handshake of the
/// connection a request arrived on, DER-encoded, as an HTTP request
/// extension; see the [module docs](crate::call_context).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PeerCertificate(pub std::sync::Arc<[u8]>);
