// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Steering a running task: a message to a task that is still `working`,
//! delivered to the executor through an inbox (ADR 0036, the extension in
//! [`a2a_protocol_types::steering`]).
//!
//! Off by default. [`RequestHandlerBuilder::with_steering`] turns it on, the
//! agent card then declares the extension, and a `SendMessage` that names a
//! running task and lists the extension URI is put in that turn's [`Inbox`]
//! instead of being refused. An executor reads it with
//! [`RequestContext::inbox`](crate::RequestContext::inbox); one that never
//! does is unaffected. `ToolLoopExecutor::with_steering` reads it between
//! model steps (feature `tool-loop`).
//!
//! # What admission does
//!
//! A steering message passes through everything any send passes through before
//! it is looked at: the interceptors (authentication, authorization of the
//! named task, the rate limit), required extensions, the tenant and caller
//! partition, the message's own validation, and the halt check on the task's
//! job. Then, and only then, it is put in the inbox of the turn that runs the
//! task: on this replica, or handed through the store to the replica that
//! holds the task's turn claim ([`TurnClaimStore::hand_steering`]).
//!
//! It is refused, never dropped, when
//!
//! * the inbox is full: `503` with `Retry-After` ([`ServerError::Overloaded`]);
//! * the turn has ended, or the claim that held it has lapsed, before the
//!   message was delivered: `503` with `Retry-After`, because the retry meets
//!   whatever the task became;
//! * the message is larger than [`SteeringConfig::max_message_size`]: `413`.
//!
//! A message to a task that is not running is not steering: a parked task
//! (`input-required`, `auth-required`) is continued as the specification says,
//! and a finished one is refused as it always was.
//!
//! [`RequestHandlerBuilder::with_steering`]: crate::RequestHandlerBuilder::with_steering
//! [`TurnClaimStore::hand_steering`]: crate::store::TurnClaimStore::hand_steering
//! [`ServerError::Overloaded`]: crate::ServerError::Overloaded

mod config;
mod inbox;

pub use config::{DEFAULT_INBOX_CAPACITY, DEFAULT_MAX_MESSAGE_SIZE, SteeringConfig};
pub use inbox::{Inbox, Refused, Steered};
