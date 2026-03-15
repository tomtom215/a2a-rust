// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! A2A protocol 0.3.0 — umbrella SDK crate.
//!
//! Re-exports all three constituent crates so users who want everything can
//! depend on `a2a-sdk` alone.
//!
//! | Module | Source crate | Contents |
//! |---|---|---|
//! | [`types`] | `a2a-types` | All A2A wire types |
//! | [`client`] | `a2a-client` | HTTP client |
//! | [`server`] | `a2a-server` | Server framework |

#![warn(missing_docs)]
#![deny(unsafe_op_in_unsafe_fn)]
#![warn(clippy::all, clippy::pedantic, clippy::nursery)]
#![allow(clippy::module_name_repetitions)]

/// All A2A protocol wire types.
pub mod types {
    #[allow(unused_imports)]
    pub use a2a_types::*;
}

/// HTTP client for sending A2A requests.
pub mod client {
    #[allow(unused_imports)]
    pub use a2a_client::*;
}

/// Server framework for implementing A2A agents.
pub mod server {
    #[allow(unused_imports)]
    pub use a2a_server::*;
}
