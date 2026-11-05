// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The pin half of the push sender's DNS-rebinding defence: the name a
//! delivery's checked addresses are pinned under (`net::resolver`). Moved out
//! of `sender.rs`, which is over the 500-line limit.
//!
//! Until 0.16.0 an `http://` delivery was rewritten here to one checked
//! address per attempt, with the name in a `Host` header; it is pinned in the
//! resolver as `https://` is, so the proxy rules see the name (audit NEW-b-4).

use a2a_protocol_types::error::{A2aError, A2aResult};

/// The bare host name of a webhook URL: no port, no brackets. What a
/// delivery's pin is keyed by, as the connector asks the resolver for exactly
/// this.
pub(super) fn host_from_url(url: &str) -> A2aResult<String> {
    let uri: hyper::Uri = url
        .parse()
        .map_err(|e| A2aError::invalid_params(format!("invalid webhook URL: {e}")))?;
    uri.host()
        .map(str::to_owned)
        .ok_or_else(|| A2aError::invalid_params("webhook URL missing host"))
}

#[cfg(test)]
mod tests;
