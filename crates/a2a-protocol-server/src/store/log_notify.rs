// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! `LISTEN`/`NOTIFY` for the `PostgreSQL` task stores' event logs: an append
//! announces the task it was for, and a listener wakes the streams waiting on
//! it (audit E-F25 (b), ADR 0057).
//!
//! This is the design of the halt store's listener
//! ([`PostgresHaltStore`](crate::store::PostgresHaltStore), ADR 0024), including
//! what it says about poolers, because the failure is the same one: `LISTEN`
//! belongs to a session, and a pooler in transaction mode lends the session to
//! someone else. See [`LogListen`].
//!
//! Three differences from the halt store, each on purpose:
//!
//! * **Opt-in.** The halt store listens unless told not to, and is built by an
//!   operator who names its pool. A task store is built by everyone, most of
//!   them behind a pooler, and its appends are the hottest write there is: a
//!   default that put a `NOTIFY` in every append, and a `LISTEN` on every
//!   replica, would change what an upgrade does to a deployment that did not
//!   ask. The statement an unannounced append runs is the one it ran before.
//! * **Lazy.** The listener starts with the first stream that waits, so a
//!   replica that serves no cross-replica stream holds no connection for it.
//! * **One notification per append, named by task.** The payload is the key
//!   of [`log_wake::key`](super::log_wake): a task and its partition, which
//!   the listener looks up as it arrives. A task whose key does not fit is
//!   not announced and is polled, as it was.

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use sqlx::postgres::{PgListener, PgPool};

use super::LogWatch;
use super::log_wake::{LogWakers, key};

/// The channel an append is announced on. The payload is the task's key.
pub const CHANNEL: &str = "a2a_task_log";

/// How long the listener waits before reconnecting after an error.
const RECONNECT_DELAY: Duration = Duration::from_secs(1);

/// Where the listener of a store's log notifications gets its session
/// ([`PostgresTaskStore::with_log_notifications`](crate::store::PostgresTaskStore::with_log_notifications)).
///
/// # Behind a connection pooler
///
/// `LISTEN` belongs to a session. A pooler in transaction mode (`PgBouncer`'s
/// `pool_mode = transaction`, the usual setting) runs it on whichever server
/// connection it lends, then lends that connection to a client that is not
/// listening, and the next notification lands in the middle of that client's
/// statement: sqlx fails it with `unexpected message: NotificationResponse`
/// (seen live through `PgBouncer` 1.26.1, 2026-10-30, as the halt store's
/// failure). So behind such a pooler, listen on a connection straight to
/// `PostgreSQL`: [`Direct`](Self::Direct). A store built without
/// `with_log_notifications` listens to nothing, and its streams poll.
#[derive(Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum LogListen {
    /// A connection taken from the store's own pool, held for as long as the
    /// store lives: size the pool one connection larger. For a pool that
    /// reaches `PostgreSQL` directly.
    Pool,
    /// A connection of its own to this URL, past any pooler. The URL is a
    /// credential and is not printed by `Debug`.
    Direct(String),
}

impl std::fmt::Debug for LogListen {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pool => f.write_str("Pool"),
            Self::Direct(_) => f.write_str("Direct(<url>)"),
        }
    }
}

/// The listening task, aborted when the last store clone is dropped.
#[derive(Debug)]
struct Listener(tokio::task::JoinHandle<()>);

impl Drop for Listener {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// What a store that announces and listens holds: the waiters, the means to
/// listen for them and the listener, once a stream has waited.
#[derive(Debug)]
pub struct LogNotify {
    wakers: LogWakers,
    pool: PgPool,
    listen: LogListen,
    listener: OnceLock<Listener>,
}

impl LogNotify {
    /// Announcing and listening, over `pool` or as `listen` says.
    pub fn new(pool: PgPool, listen: LogListen) -> Arc<Self> {
        Arc::new(Self {
            wakers: LogWakers::new(),
            pool,
            listen,
            listener: OnceLock::new(),
        })
    }

    /// A watch on `task` in `partition`, registered now, with the listener
    /// started if this is the first. `None` for a task whose key does not fit
    /// a notification (it is polled) and outside a runtime (the listener
    /// cannot run).
    pub fn watch(&self, partition: &str, task: &str) -> Option<LogWatch> {
        let key = key(partition, task)?;
        tokio::runtime::Handle::try_current().ok()?;
        let watch = self.wakers.watch(key);
        self.listener.get_or_init(|| {
            Listener(tokio::spawn(listen(
                self.pool.clone(),
                self.listen.clone(),
                self.wakers.clone(),
            )))
        });
        Some(watch)
    }
}

/// The key an append is announced under, for a store that announces
/// (`log` is `Some`) and a task whose key fits a notification.
pub fn announcement(log: Option<&Arc<LogNotify>>, partition: &str, task: &str) -> Option<String> {
    log.and_then(|_| key(partition, task))
}

/// A watch on `task` in `partition` for a store that has opted in (`log` is
/// `Some`); see [`LogNotify::watch`].
pub fn watch(log: Option<&Arc<LogNotify>>, partition: &str, task: &str) -> Option<LogWatch> {
    log?.watch(partition, task)
}

/// The append statement for a store that announces `key`, or the plain one.
pub const fn append_sql(
    key: Option<&str>,
    plain: &'static str,
    announcing: &'static str,
) -> &'static str {
    if key.is_some() { announcing } else { plain }
}

/// Binds the announcement, the channel and the key, to an announcing append,
/// and nothing to a plain one.
pub fn announce<'q>(
    query: sqlx::query::Query<'q, sqlx::Postgres, sqlx::postgres::PgArguments>,
    key: Option<&'q str>,
) -> sqlx::query::Query<'q, sqlx::Postgres, sqlx::postgres::PgArguments> {
    match key {
        Some(key) => query.bind(CHANNEL).bind(key),
        None => query,
    }
}

/// Forwards notifications to the waiters until the task is aborted. Every
/// waiter is woken once the listener is listening, and again after a lost
/// connection is back: whatever was announced before `LISTEN` took effect, or
/// while the connection was down, is gone, and a read recovers it.
async fn listen(pool: PgPool, on: LogListen, wakers: LogWakers) {
    loop {
        let connected = match &on {
            LogListen::Pool => PgListener::connect_with(&pool).await,
            LogListen::Direct(url) => PgListener::connect(url).await,
        };
        let mut listener = match connected {
            Ok(l) => l,
            Err(_e) => {
                trace_warn!(error = %_e, "task log listener: cannot connect; retrying");
                tokio::time::sleep(RECONNECT_DELAY).await;
                continue;
            }
        };
        if let Err(_e) = listener.listen(CHANNEL).await {
            trace_warn!(error = %_e, "task log listener: LISTEN failed; retrying");
            tokio::time::sleep(RECONNECT_DELAY).await;
            continue;
        }
        wakers.resync();
        loop {
            match listener.try_recv().await {
                Ok(Some(n)) => wakers.notify(n.payload()),
                // The connection was lost and is being re-established.
                Ok(None) => wakers.resync(),
                Err(_e) => {
                    trace_warn!(error = %_e, "task log listener: receive failed; reconnecting");
                    tokio::time::sleep(RECONNECT_DELAY).await;
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
