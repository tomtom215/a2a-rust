// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! How a [`ToolLoop`] is configured: the `with_*` setters and the strict profile.

#[cfg(feature = "otel")]
use super::GenAiMeter;
use super::{
    ArgumentCheck, TextCallRecovery, ToolLoop, ToolModel, ToolPolicy, ToolSet, TraceContent,
};

impl ToolLoop {
    /// Names the agent this loop is, recorded as `gen_ai.agent.name` on
    /// the run's metrics.
    #[must_use]
    pub fn with_agent_name(mut self, name: impl Into<String>) -> Self {
        self.agent_name = Some(name.into());
        self
    }

    /// Records the loop's metrics (`gen_ai.client.inference.duration` and
    /// the rest; ADR 0013) to `meter` rather than to the global provider's
    /// meter, which is looked up each time one is recorded.
    ///
    /// Takes the meter by reference since 0.16.0: the instruments are built
    /// from it once, here, and the meter itself is not kept (audit C-23).
    #[cfg(feature = "otel")]
    #[must_use]
    pub fn with_meter(mut self, meter: &opentelemetry::metrics::Meter) -> Self {
        self.meter = GenAiMeter::new(meter);
        self
    }

    /// Sets the secret idempotency keys are derived under. Each loop draws
    /// its own by default, so keys differ between processes; replicas that
    /// set the same secret give the same call in the same scope the same
    /// key, so a backend deduplicates a call one replica retries after
    /// another made it. Keep it as secret as a credential: with it, keys
    /// can be computed from task ids and arguments.
    #[must_use]
    pub const fn with_idempotency_secret(mut self, secret: [u8; 32]) -> Self {
        self.secret = secret;
        self.secret_set = true;
        self
    }

    /// Keeps each run's progress in `journal`, so a run begun again under the
    /// same key resumes instead of starting over, and a call a policy says
    /// needs approval can wait for it (adversarial review, S1 and S7). See
    /// [`StepJournal`](super::StepJournal) for what is written, what a resume
    /// promises, and what it does not.
    ///
    /// Set [`with_idempotency_secret`](Self::with_idempotency_secret) too if a
    /// call may be in flight when a replica dies: without it a call whose
    /// receipt was written and whose result was not cannot be re-run safely, and
    /// the resumed run fails with [`LoopError::Indeterminate`](super::LoopError::Indeterminate).
    #[must_use]
    pub fn with_journal(mut self, journal: std::sync::Arc<dyn super::StepJournal>) -> Self {
        self.journal = Some(journal);
        self
    }

    /// Whether a journal is installed.
    pub(in crate::tool_loop) fn has_journal(&self) -> bool {
        self.journal.is_some()
    }

    /// Adds a pin to every checkpoint: a policy version, a skill bundle digest,
    /// anything the deployment says a resumed run must still be running under.
    /// A run whose checkpoint carries other pins is refused on resume.
    #[must_use]
    pub fn with_pin(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.pin_labels.insert(name.into(), value.into());
        self
    }

    /// Sets the system preamble every turn is sent with.
    #[must_use]
    pub fn with_preamble(mut self, preamble: impl Into<String>) -> Self {
        self.preamble = preamble.into();
        self
    }

    /// Sets how many model turns a run may take. `0` is taken as `1`: a run
    /// asks the model at least once.
    #[must_use]
    pub const fn with_max_turns(mut self, max_turns: usize) -> Self {
        self.max_turns = if max_turns == 0 { 1 } else { max_turns };
        self
    }

    /// Sets how many tool calls one model reply may make; the first this many
    /// run and the rest are dropped, with a line in the trace saying so. `0` is
    /// taken as `1`. [`DEFAULT_MAX_CALLS_PER_TURN`](super::DEFAULT_MAX_CALLS_PER_TURN) when not set.
    #[must_use]
    pub const fn with_max_calls_per_turn(mut self, max_calls: usize) -> Self {
        self.max_calls_per_turn = if max_calls == 0 { 1 } else { max_calls };
        self
    }

    /// Whether a final reply that is exactly one JSON object naming a tool
    /// in the set — bare, in a code fence, or between `<tool_call>` tags —
    /// is run as that call rather than returned as the answer. On by
    /// default: small models write calls as text, and an answer of raw JSON
    /// helps nobody. `true` is [`TextCallRecovery::All`] and `false`
    /// [`TextCallRecovery::Off`]; [`with_text_call_recovery`](Self::with_text_call_recovery)
    /// also offers [`ReadOnly`](TextCallRecovery::ReadOnly).
    #[must_use]
    pub const fn with_recover_text_calls(mut self, on: bool) -> Self {
        self.text_recovery = if on {
            TextCallRecovery::All
        } else {
            TextCallRecovery::Off
        };
        self
    }

    /// Which tools a reply written as a call may run (adversarial review T3).
    #[must_use]
    pub const fn with_text_call_recovery(mut self, recovery: TextCallRecovery) -> Self {
        self.text_recovery = recovery;
        self
    }

    /// What the caller's trace carries for each call (adversarial review T1).
    #[must_use]
    pub const fn with_trace_content(mut self, content: TraceContent) -> Self {
        self.trace_content = content;
        self
    }

    /// Whether a call's arguments are checked against the tool's JSON Schema
    /// before the policy is asked and the call runs (adversarial review T2);
    /// see [`ArgumentCheck`] for what is checked and what is not.
    #[must_use]
    pub const fn with_argument_check(mut self, check: ArgumentCheck) -> Self {
        self.argument_check = check;
        self
    }

    /// Installs a policy asked before every call runs (adversarial review T2).
    #[must_use]
    pub fn with_policy(mut self, policy: impl ToolPolicy) -> Self {
        self.policy = Some(Box::new(policy));
        self
    }

    /// A loop for a deployment that takes a tool call to be an action with
    /// consequences: no call is lifted out of text
    /// ([`TextCallRecovery::Off`]), the caller's trace is a receipt
    /// ([`TraceContent::Receipt`]) and arguments are checked against the tool's
    /// schema ([`ArgumentCheck::Strict`]). Add a [`ToolPolicy`] with
    /// [`with_policy`](Self::with_policy) for per-action authorisation.
    #[must_use]
    pub fn strict(model: impl ToolModel, tools: ToolSet) -> Self {
        Self::new(model, tools)
            .with_text_call_recovery(TextCallRecovery::Off)
            .with_trace_content(TraceContent::Receipt)
            .with_argument_check(ArgumentCheck::Strict)
    }
}
