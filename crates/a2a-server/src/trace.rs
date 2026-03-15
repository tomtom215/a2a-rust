// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Conditional tracing helpers.
//!
//! When the `tracing` feature is enabled, these macros delegate to the
//! [`tracing`] crate. When disabled, they expand to nothing — zero runtime
//! cost.

/// Emits an INFO-level event when the `tracing` feature is enabled.
macro_rules! trace_info {
    ($($arg:tt)*) => {
        #[cfg(feature = "tracing")]
        #[allow(clippy::used_underscore_binding)]
        { ::tracing::info!($($arg)*); }
    };
}

/// Emits a DEBUG-level event when the `tracing` feature is enabled.
macro_rules! trace_debug {
    ($($arg:tt)*) => {
        #[cfg(feature = "tracing")]
        #[allow(clippy::used_underscore_binding)]
        { ::tracing::debug!($($arg)*); }
    };
}

/// Emits a WARN-level event when the `tracing` feature is enabled.
macro_rules! trace_warn {
    ($($arg:tt)*) => {
        #[cfg(feature = "tracing")]
        #[allow(clippy::used_underscore_binding)]
        { ::tracing::warn!($($arg)*); }
    };
}

/// Emits an ERROR-level event when the `tracing` feature is enabled.
macro_rules! trace_error {
    ($($arg:tt)*) => {
        #[cfg(feature = "tracing")]
        #[allow(clippy::used_underscore_binding)]
        { ::tracing::error!($($arg)*); }
    };
}

#[allow(unused_imports)]
pub(crate) use trace_debug;
#[allow(unused_imports)]
pub(crate) use trace_error;
#[allow(unused_imports)]
pub(crate) use trace_info;
#[allow(unused_imports)]
pub(crate) use trace_warn;
