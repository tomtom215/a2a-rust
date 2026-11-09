// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! What a server that takes steering messages is set to: how many wait for a
//! running turn, and how large one may be.

/// How many steering messages may wait in one running turn's inbox, unless
/// [`SteeringConfig::with_inbox_capacity`] says otherwise.
///
/// Sixteen is room for a person, or a supervising agent, to correct course a
/// few times while a turn reads between steps, and small enough that a turn
/// which never reads its inbox holds at most sixteen messages of at most
/// [`DEFAULT_MAX_MESSAGE_SIZE`] bytes, about a mebibyte, for as long as it
/// runs.
pub const DEFAULT_INBOX_CAPACITY: usize = 16;

/// The largest steering message, in bytes of its JSON, unless
/// [`SteeringConfig::with_max_message_size`] says otherwise.
///
/// 64 KiB holds a long instruction and a few data parts. A steering message
/// is read by a model between steps, so one past this is a document, and a
/// document belongs in a task of its own. It is also what a message costs in
/// the store when it is handed to another replica.
pub const DEFAULT_MAX_MESSAGE_SIZE: usize = 64 * 1024;

/// Turns steering on, and sets its two bounds. See
/// [`RequestHandlerBuilder::with_steering`](crate::RequestHandlerBuilder::with_steering).
///
/// ```rust
/// use a2a_protocol_server::SteeringConfig;
///
/// let config = SteeringConfig::new()
///     .with_inbox_capacity(4)
///     .with_max_message_size(8 * 1024);
/// assert_eq!(config.inbox_capacity(), 4);
/// assert_eq!(config.max_message_size(), 8 * 1024);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct SteeringConfig {
    inbox_capacity: usize,
    max_message_size: usize,
    notifications: bool,
}

impl Default for SteeringConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl SteeringConfig {
    /// The defaults: [`DEFAULT_INBOX_CAPACITY`] messages of at most
    /// [`DEFAULT_MAX_MESSAGE_SIZE`] bytes.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            inbox_capacity: DEFAULT_INBOX_CAPACITY,
            max_message_size: DEFAULT_MAX_MESSAGE_SIZE,
            notifications: false,
        }
    }

    /// How many messages may wait in one turn's inbox. A message that finds
    /// the inbox full is refused with `503` and `Retry-After`, never dropped
    /// and never waited for. `build` refuses `0`: an inbox that holds nothing
    /// refuses every message.
    #[must_use]
    pub const fn with_inbox_capacity(mut self, capacity: usize) -> Self {
        self.inbox_capacity = capacity;
        self
    }

    /// The largest message accepted, in bytes of its JSON; one byte over is
    /// refused as too large. `build` refuses `0`.
    #[must_use]
    pub const fn with_max_message_size(mut self, bytes: usize) -> Self {
        self.max_message_size = bytes;
        self
    }

    /// Whether a replica that handed a steering message to another one
    /// through the store also tells it so at once, with PostgreSQL's
    /// `NOTIFY`, instead of leaving it to the other replica's next look at
    /// the store (every turn-claim renewal interval, one second by default).
    /// Off by default, and only the PostgreSQL claim store sends one.
    ///
    /// It holds a `LISTEN` session on a pooled connection, which breaks
    /// behind a connection pooler in transaction mode (`PgBouncer`'s usual
    /// setting): the pooler lends the session's connection to another client,
    /// and the next notification arrives in the middle of that client's
    /// statement. Measured against `PgBouncer` 1.26.1 on 2026-10-30, for the
    /// halt store, which listens the same way. Turn it on only with a direct
    /// connection to PostgreSQL. Nothing is lost by leaving it off: the
    /// look every renewal interval delivers every message.
    #[must_use]
    pub const fn with_notifications(mut self, on: bool) -> Self {
        self.notifications = on;
        self
    }

    /// How many messages may wait in one turn's inbox.
    #[must_use]
    pub const fn inbox_capacity(&self) -> usize {
        self.inbox_capacity
    }

    /// The largest message accepted, in bytes.
    #[must_use]
    pub const fn max_message_size(&self) -> usize {
        self.max_message_size
    }

    /// Whether a handed-off message is announced with `NOTIFY`.
    #[must_use]
    pub const fn notifications(&self) -> bool {
        self.notifications
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_are_the_documented_ones() {
        let config = SteeringConfig::new();
        assert_eq!(config.inbox_capacity(), 16);
        assert_eq!(config.max_message_size(), 65_536);
        assert_eq!(config.inbox_capacity(), DEFAULT_INBOX_CAPACITY);
        assert_eq!(config.max_message_size(), DEFAULT_MAX_MESSAGE_SIZE);
        assert!(!config.notifications(), "off until asked for");
    }

    #[test]
    fn each_setter_sets_its_own_bound_only() {
        let capacity = SteeringConfig::new().with_inbox_capacity(3);
        assert_eq!(capacity.inbox_capacity(), 3);
        assert_eq!(capacity.max_message_size(), DEFAULT_MAX_MESSAGE_SIZE);
        let size = SteeringConfig::new().with_max_message_size(7);
        assert_eq!(size.max_message_size(), 7);
        assert_eq!(size.inbox_capacity(), DEFAULT_INBOX_CAPACITY);
        let on = SteeringConfig::new().with_notifications(true);
        assert!(on.notifications());
        assert_eq!(on.inbox_capacity(), DEFAULT_INBOX_CAPACITY);
        assert_eq!(on.max_message_size(), DEFAULT_MAX_MESSAGE_SIZE);
        assert!(!on.with_notifications(false).notifications());
    }
}
