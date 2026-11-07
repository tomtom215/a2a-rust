// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Turning steering on (ADR 0036).

use super::RequestHandlerBuilder;
use crate::error::{ServerError, ServerResult};
use crate::steering::SteeringConfig;

impl RequestHandlerBuilder {
    /// Takes steering messages: a `SendMessage` that names a running task and
    /// lists the steering extension
    /// ([`STEERING_EXTENSION_URI`](a2a_protocol_types::steering::STEERING_EXTENSION_URI))
    /// is put in that turn's inbox instead of being refused, and the agent
    /// card declares the extension. Off by default: a server that does not
    /// call this answers such a message as it always did, and its card does
    /// not declare the extension.
    ///
    /// An executor reads the inbox with
    /// [`RequestContext::inbox`](crate::RequestContext::inbox); one that never
    /// does is unaffected. `config` bounds the inbox and the message size; see
    /// [`SteeringConfig`] and [`crate::steering`] for what a steering message
    /// is checked against and how it can be refused.
    ///
    /// ```rust
    /// use a2a_protocol_server::{RequestHandlerBuilder, SteeringConfig};
    /// # use a2a_protocol_server::agent_executor;
    /// # struct Agent;
    /// # agent_executor!(Agent, |_ctx, _queue| async { Ok(()) });
    ///
    /// let handler = RequestHandlerBuilder::new(Agent)
    ///     .without_agent_card()
    ///     .with_steering(SteeringConfig::new().with_inbox_capacity(4))
    ///     .build()
    ///     .unwrap();
    /// # drop(handler);
    /// ```
    #[must_use]
    pub const fn with_steering(mut self, config: SteeringConfig) -> Self {
        self.steering = Some(config);
        self
    }

    /// Refuses a steering configuration that could never deliver anything.
    pub(super) fn check_steering(&self) -> ServerResult<()> {
        let Some(config) = &self.steering else {
            return Ok(());
        };
        if config.inbox_capacity() == 0 {
            return Err(ServerError::Config(
                "the steering inbox capacity must be at least 1: an inbox that holds nothing \
                 refuses every message"
                    .to_owned(),
            ));
        }
        if config.max_message_size() == 0 {
            return Err(ServerError::Config(
                "the steering message size limit must be at least 1 byte: no message fits \
                 under 0"
                    .to_owned(),
            ));
        }
        Ok(())
    }
}
