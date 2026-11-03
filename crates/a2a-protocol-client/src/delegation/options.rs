// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! What a delegation is given beyond its message, in one value (audit H18,
//! NEW-h-designs-5).
//!
//! A journal, a lease and a budget each had a constructor of their own
//! ([`Delegation::start_journaled`], [`Delegation::start_leased`],
//! [`Delegation::start_budgeted`]) and the three did not compose: a supervisor
//! that kept a journal to survive a restart and leased its children to survive
//! a crash could not ask for both, and after the restart nothing renewed the
//! leases of the children it picked up again, so the restart the journal
//! exists for killed the children it had recorded.
//! [`Delegation::start_with`] and [`Delegation::reattach_with`] take them all.

use std::sync::Arc;
use std::time::Duration;

use a2a_protocol_types::MessageSendParams;
use a2a_protocol_types::budget::ChildGrant;
use a2a_protocol_types::delegation_journal::DelegationJournal;
use a2a_protocol_types::idempotency;
use a2a_protocol_types::lease::Lease;

use super::renewal::Renewal;
use super::{Delegation, ResumePolicy, fresh_key, journal, refused};
use crate::client::A2aClient;
use crate::error::{ClientError, ClientResult};

/// The journal a delegation keeps a record in.
pub(super) enum JournalChoice {
    /// A new record, written before the message is sent.
    New {
        journal: Arc<dyn DelegationJournal>,
        owner: String,
        agent: String,
    },
    /// A record that exists already, kept up to date from a reattach.
    Existing {
        journal: Arc<dyn DelegationJournal>,
        record: String,
    },
}

/// What a delegation is given beyond its message: a lease, a budget, a
/// journal record, how a lost stream is resumed. Nothing is set by default,
/// which is [`Delegation::start`].
#[derive(Default)]
pub struct DelegationOptions {
    lease: Option<Duration>,
    budget: Option<ChildGrant>,
    journal: Option<JournalChoice>,
    resume: Option<ResumePolicy>,
    id_wait: Option<Duration>,
}

impl std::fmt::Debug for DelegationOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DelegationOptions")
            .field("lease", &self.lease)
            .field("budget", &self.budget.is_some())
            .field("journal", &self.journal.is_some())
            .field("resume", &self.resume)
            .field("id_wait", &self.id_wait)
            .finish()
    }
}

impl DelegationOptions {
    /// No lease, no budget, no journal, and the default resume policy.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Grants the child a lease of `ttl`, renewed every third of `ttl` until
    /// the handle ends (see [`Delegation::start_leased`]).
    #[must_use]
    pub const fn with_lease(mut self, ttl: Duration) -> Self {
        self.lease = Some(ttl);
        self
    }

    /// Gives the child `grant`, a budget carved out of the parent's (see
    /// [`Delegation::start_budgeted`]). A reattach cannot restore a grant.
    #[must_use]
    pub fn with_budget(mut self, grant: ChildGrant) -> Self {
        self.budget = Some(grant);
        self
    }

    /// Records the delegation in `journal` as a new record, owned by `owner`
    /// and naming `agent`, written before the message is sent (see
    /// [`Delegation::start_journaled`]).
    #[must_use]
    pub fn with_journal(
        mut self,
        journal: Arc<dyn DelegationJournal>,
        owner: impl Into<String>,
        agent: impl Into<String>,
    ) -> Self {
        self.journal = Some(JournalChoice::New {
            journal,
            owner: owner.into(),
            agent: agent.into(),
        });
        self
    }

    /// Keeps `record`, a record in `journal` that
    /// [`DelegationJournal::open`] listed, up to date: for
    /// [`Delegation::reattach_with`].
    #[must_use]
    pub fn with_journal_record(
        mut self,
        journal: Arc<dyn DelegationJournal>,
        record: impl Into<String>,
    ) -> Self {
        self.journal = Some(JournalChoice::Existing {
            journal,
            record: record.into(),
        });
        self
    }

    /// Sets how a lost stream is resumed (see [`ResumePolicy`]).
    #[must_use]
    pub const fn with_resume(mut self, policy: ResumePolicy) -> Self {
        self.resume = Some(policy);
        self
    }

    /// Sets how long a cancel waits for the child's id (see
    /// [`Delegation::with_id_wait`]).
    #[must_use]
    pub const fn with_id_wait(mut self, wait: Duration) -> Self {
        self.id_wait = Some(wait);
        self
    }
}

impl Delegation {
    /// [`start`](Self::start) with every part of `options` at once: a lease
    /// renewed until the handle ends, a budget settled when it does, a
    /// journal record written before the message is sent.
    ///
    /// # Errors
    ///
    /// As [`start`](Self::start); also the journal's error, as
    /// [`ClientError::Protocol`], when the record cannot be written (nothing
    /// is sent then), and [`ClientError::Config`] when `options` names an
    /// existing journal record, which only
    /// [`reattach_with`](Self::reattach_with) can keep. When the agent refuses
    /// the request, a budget reservation is returned and the record is
    /// settled `not_started`: no child exists.
    pub async fn start_with(
        client: Arc<A2aClient>,
        mut params: MessageSendParams,
        options: DelegationOptions,
    ) -> ClientResult<Self> {
        let DelegationOptions {
            lease,
            budget,
            journal,
            resume,
            id_wait,
        } = options;
        let new_record = new_record_of(journal)?;
        prepare(&client, &mut params, budget.as_ref(), lease);
        let tenant = params.tenant.clone();
        let record = begin_record(new_record, &params).await?;
        let mut this = match Self::start(Arc::clone(&client), params).await {
            Ok(this) => this,
            Err(e) => {
                if refused(&e) {
                    if let Some(grant) = budget {
                        grant.release();
                    }
                    if let Some((journal, id)) = &record {
                        let _ = journal.settle(id, "not_started", journal::now_ms()).await;
                    }
                }
                return Err(e);
            }
        };
        this.renewal = lease.map(|ttl| Renewal::spawn(client, tenant, ttl, &this.origin));
        this.budget = budget.map(|grant| (grant, None));
        if let Some((journal, id)) = record {
            this = this.with_journal(journal, id);
        }
        Ok(this.with_options(resume, id_wait))
    }

    /// [`reattach`](Self::reattach) with the lease, the journal record, the
    /// resume policy and the id wait of `options`: the child is picked up
    /// with its lease renewed again, from the moment the handle exists, and
    /// its record kept up to date.
    ///
    /// A lease the child was granted before the restart lapses after its time
    /// to live without a renewal; a supervisor that restarts must reattach, and
    /// so start renewing, inside it. A budget grant cannot be restored: the
    /// parent's meter died with the process, and the reservation was kept as
    /// spent.
    ///
    /// # Errors
    ///
    /// As [`reattach`](Self::reattach); also [`ClientError::Config`] when
    /// `options` carries a budget, or a new journal record (which is written
    /// before a message is sent, and a reattach sends none).
    pub async fn reattach_with(
        client: Arc<A2aClient>,
        tenant: Option<String>,
        task_id: impl Into<String> + Send,
        last_event_id: Option<String>,
        options: DelegationOptions,
    ) -> ClientResult<Self> {
        if options.budget.is_some() {
            return Err(ClientError::Config(
                "a budget grant cannot be restored by Delegation::reattach_with".to_owned(),
            ));
        }
        let existing = match options.journal {
            None => None,
            Some(JournalChoice::Existing { journal, record }) => Some((journal, record)),
            Some(JournalChoice::New { .. }) => {
                return Err(ClientError::Config(
                    "DelegationOptions::with_journal writes a new record before a message is \
                     sent; a reattach keeps an existing one: use with_journal_record"
                        .to_owned(),
                ));
            }
        };
        let mut this =
            Self::reattach(Arc::clone(&client), tenant.clone(), task_id, last_event_id).await?;
        if let Some(ttl) = options.lease {
            let renewal = Renewal::spawn(client, tenant, ttl, &this.origin);
            // The child is named already: renewal starts at the next tick.
            renewal
                .id
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone_from(&this.task_id);
            this.renewal = Some(renewal);
        }
        if let Some((journal, record)) = existing {
            this = this.with_journal(journal, record);
        }
        Ok(this.with_options(options.resume, options.id_wait))
    }

    /// [`start`](Self::start), granting the child a lease of `ttl` and
    /// renewing it every third of `ttl` until this handle settles, is
    /// cancelled, is detached or is dropped (see the [module docs](super)).
    /// The agent must grant leases (its card declares
    /// `a2a_protocol_types::lease::LEASE_EXTENSION_URI`); one that does not
    /// ignores the grant, and the child runs as with [`start`](Self::start).
    ///
    /// # Errors
    ///
    /// As [`start`](Self::start); an agent that grants leases also refuses
    /// a `ttl` outside its bounds.
    pub async fn start_leased(
        client: Arc<A2aClient>,
        params: MessageSendParams,
        ttl: Duration,
    ) -> ClientResult<Self> {
        Self::start_with(client, params, DelegationOptions::new().with_lease(ttl)).await
    }

    /// [`start`](Self::start) with `grant`, a budget carved out of the
    /// parent's by `BudgetHandle::grant`, and a lease of `lease` as
    /// [`start_leased`](Self::start_leased) grants one when it is `Some`.
    ///
    /// When the handle ends, the grant is settled. If the status update
    /// that settled the child (terminal or interrupted) carried a budget
    /// report, the parent is charged that spend instead of the reservation.
    /// Any other ending (a lost stream, a cancel answered by `CancelTask`,
    /// an unreachable child), a report-less update, or a handle detached or
    /// dropped keeps the whole reservation as spent. An agent that does not
    /// implement the extension reports nothing.
    ///
    /// # Errors
    ///
    /// As [`start`](Self::start). The reservation is returned only when the
    /// error shows the agent refused the request, so no child exists: an
    /// A2A error, an HTTP error status, an authentication challenge, or an
    /// endpoint the client cannot use.
    /// After a transport error or a timeout a child may be running, and the
    /// reservation is kept.
    pub async fn start_budgeted(
        client: Arc<A2aClient>,
        params: MessageSendParams,
        grant: ChildGrant,
        lease: Option<Duration>,
    ) -> ClientResult<Self> {
        let mut options = DelegationOptions::new().with_budget(grant);
        if let Some(ttl) = lease {
            options = options.with_lease(ttl);
        }
        Self::start_with(client, params, options).await
    }

    /// Applies the resume policy and the id wait of the options, where given.
    const fn with_options(
        mut self,
        resume: Option<ResumePolicy>,
        id_wait: Option<Duration>,
    ) -> Self {
        if let Some(policy) = resume {
            self.resume_policy = policy;
        }
        if let Some(wait) = id_wait {
            self.id_wait = wait;
        }
        self
    }
}

/// A new record to write: the journal, its owner, the agent it names.
type NewRecord = (Arc<dyn DelegationJournal>, String, String);

/// The new record `journal` asks for, if it does. A record that exists
/// cannot be started: only a reattach can keep one.
fn new_record_of(journal: Option<JournalChoice>) -> ClientResult<Option<NewRecord>> {
    match journal {
        None => Ok(None),
        Some(JournalChoice::New {
            journal,
            owner,
            agent,
        }) => Ok(Some((journal, owner, agent))),
        Some(JournalChoice::Existing { .. }) => Err(ClientError::Config(
            "DelegationOptions::with_journal_record names a record that exists: \
             use Delegation::reattach_with, or with_journal for a new one"
                .to_owned(),
        )),
    }
}

/// What goes into the message before it is sent: the budget, the lease and,
/// for an agent that honours one, an idempotency key the caller did not set.
fn prepare(
    client: &A2aClient,
    params: &mut MessageSendParams,
    budget: Option<&ChildGrant>,
    lease: Option<Duration>,
) {
    if let Some(grant) = budget {
        grant.budget().attach(&mut params.message);
    }
    if let Some(ttl) = lease {
        Lease::new(ttl).attach(&mut params.message);
    }
    if client.honours_idempotency() && matches!(idempotency::key_of(&params.message), Ok(None)) {
        // A fresh key is 64 hex digits, always valid, so this cannot fail.
        let _ = idempotency::set_key(&mut params.message, &fresh_key());
    }
}

/// Writes the new record, if there is one, before anything is sent, and
/// returns the journal with the record's id.
async fn begin_record(
    new_record: Option<NewRecord>,
    params: &MessageSendParams,
) -> ClientResult<Option<(Arc<dyn DelegationJournal>, String)>> {
    let Some((journal, owner, agent)) = new_record else {
        return Ok(None);
    };
    let key = idempotency::key_of(&params.message)
        .ok()
        .flatten()
        .map(str::to_owned);
    let record = journal::new_record(owner, agent, params.tenant.clone(), key);
    journal::begin(&*journal, &record).await?;
    Ok(Some((journal, record.id)))
}
