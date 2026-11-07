// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The `with_*` setters of [`ClientConfig`], one per field. Kept apart from the
//! struct so the file that documents the fields stays under the 500-line limit.

use std::time::Duration;

use super::{ClientConfig, TlsConfig};

impl ClientConfig {
    /// Sets [`preferred_bindings`](Self::preferred_bindings), which nothing
    /// reads; see the field.
    #[must_use]
    #[deprecated(
        since = "0.15.0",
        note = "nothing reads `preferred_bindings`: `from_card` follows the card's interface \
                order (spec §8.3.2); pass a ranking to `ClientBuilder::from_card_preferring`"
    )]
    #[allow(deprecated)]
    pub fn with_preferred_bindings(mut self, bindings: Vec<String>) -> Self {
        self.preferred_bindings = bindings;
        self
    }

    /// Sets the MIME types advertised in `acceptedOutputModes`.
    #[must_use]
    pub fn with_accepted_output_modes(mut self, modes: Vec<String>) -> Self {
        self.accepted_output_modes = modes;
        self
    }

    /// Sets the history length requested; `None` uses the agent's default.
    #[must_use]
    pub const fn with_history_length(mut self, length: Option<u32>) -> Self {
        self.history_length = length;
        self
    }

    /// Sets whether `send_message` returns as soon as the task is submitted.
    #[must_use]
    pub const fn with_return_immediately(mut self, val: bool) -> Self {
        self.return_immediately = val;
        self
    }

    /// Sets [`blocking_over_stream`](Self::blocking_over_stream).
    #[must_use]
    pub const fn with_blocking_over_stream(mut self, val: bool) -> Self {
        self.blocking_over_stream = val;
        self
    }

    /// Sets the per-request timeout for non-streaming calls.
    #[must_use]
    pub const fn with_request_timeout(mut self, timeout: Duration) -> Self {
        self.request_timeout = timeout;
        self
    }

    /// Sets [`send_timeout`](Self::send_timeout).
    #[must_use]
    pub const fn with_send_timeout(mut self, timeout: Duration) -> Self {
        self.send_timeout = timeout;
        self
    }

    /// Sets the timeout for establishing a stream (headers, or an error
    /// body). See [`stream_connect_timeout`](Self::stream_connect_timeout).
    #[must_use]
    pub const fn with_stream_connect_timeout(mut self, timeout: Duration) -> Self {
        self.stream_connect_timeout = timeout;
        self
    }

    /// Sets how long an established stream may wait for its first data. See
    /// [`stream_first_event_timeout`](Self::stream_first_event_timeout).
    #[must_use]
    pub const fn with_stream_first_event_timeout(mut self, timeout: Duration) -> Self {
        self.stream_first_event_timeout = timeout;
        self
    }

    /// Sets the TCP connection timeout (DNS + handshake).
    #[must_use]
    pub const fn with_connection_timeout(mut self, timeout: Duration) -> Self {
        self.connection_timeout = timeout;
        self
    }

    /// Sets how long an established stream may receive nothing before it is
    /// failed with a timeout; `None` disables the bound. See
    /// [`stream_idle_timeout`](Self::stream_idle_timeout).
    #[must_use]
    pub const fn with_stream_idle_timeout(mut self, timeout: Option<Duration>) -> Self {
        self.stream_idle_timeout = timeout;
        self
    }

    /// Sets the buffered response body cap. See
    /// [`max_response_size`](Self::max_response_size) for which transports
    /// it reaches.
    #[must_use]
    pub const fn with_max_response_size(mut self, max_bytes: usize) -> Self {
        self.max_response_size = max_bytes;
        self
    }

    /// Sets the largest single stream event accepted. See
    /// [`max_event_size`](Self::max_event_size).
    #[must_use]
    pub const fn with_max_event_size(mut self, max_bytes: usize) -> Self {
        self.max_event_size = max_bytes;
        self
    }

    /// Sets the TLS configuration.
    // Const only without `tls-rustls`: with it `TlsConfig` holds an `Arc`,
    // whose destructor a const fn cannot run.
    #[cfg_attr(not(feature = "tls-rustls"), allow(clippy::missing_const_for_fn))]
    #[must_use]
    pub fn with_tls(mut self, tls: TlsConfig) -> Self {
        self.tls = tls;
        self
    }

    /// Sets the default tenant; `None` sends none unless a request names one.
    #[must_use]
    pub fn with_tenant(mut self, tenant: Option<String>) -> Self {
        self.tenant = tenant;
        self
    }
}
