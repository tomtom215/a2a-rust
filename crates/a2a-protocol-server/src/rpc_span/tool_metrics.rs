// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The tool loop's metrics, as the OpenTelemetry `GenAI` conventions name
//! them — `docs/gen-ai/gen-ai-metrics.md`, `client-inference.md` and
//! `token.md` in `open-telemetry/semantic-conventions-genai` at `9ffc7e0`,
//! all `development`:
//!
//! | Instrument | Kind | Unit | Per |
//! |---|---|---|---|
//! | `gen_ai.client.inference.duration` | Histogram | `s` | model turn |
//! | `gen_ai.client.inference.usage.input_tokens` | Counter | `{token}` | model turn, when reported and non-zero |
//! | `gen_ai.client.inference.usage.output_tokens` | Counter | `{token}` | model turn, when reported and non-zero |
//! | `gen_ai.client.inference.operation.input_tokens` | Histogram | `{token}` | model turn, when reported |
//! | `gen_ai.client.inference.operation.output_tokens` | Histogram | `{token}` | model turn, when reported |
//! | `gen_ai.execute_tool.duration` | Histogram | `s` | tool call that ran |
//! | `gen_ai.invoke_agent.duration` | Histogram | `s` | run of the loop |
//! | `gen_ai.invoke_agent.inference_calls` | Histogram | `{inference_call}` | run of the loop |
//! | `gen_ai.invoke_agent.tool_calls` | Histogram | `{tool_call}` | run of the loop |
//!
//! Each histogram has the conventions' advisory buckets. A model turn's
//! instruments carry `gen_ai.operation.name` (`chat`), `gen_ai.provider.name`,
//! `gen_ai.request.model`, and when known `gen_ai.response.model`,
//! `server.address` and `server.port`; the usage counters add
//! `gen_ai.token.modality` = `text`, since a [`ChatRequest`] carries text
//! only. `gen_ai.client.operation.duration` is not reported: the conventions
//! say it "SHOULD NOT be reported for inference operations".
//!
//! `gen_ai.response.model` is whatever the provider says answered, so a
//! meter keeps the first [`MAX_RESPONSE_MODELS`] names it sees and records
//! any later one, or one over [`MAX_RESPONSE_MODEL_LEN`] bytes, as `_OTHER`:
//! 200 names made 200 series on each turn instrument before (live campaign
//! W2-LIVE-9, 2026-11-08). The span keeps the name as given.
//!
//! `error.type`, on every instrument that takes it, is one of: for a turn,
//! [`ModelError::error_type`] (the HTTP status, `timeout`, `transport`,
//! `invalid_response`, `response_too_large`, `configuration`, `_OTHER`); for
//! a call, `unknown_tool`, `invalid_arguments`, `tool_error`, `tool_fatal`;
//! for a run, the turn's type when the model failed, `tool_fatal`,
//! `turn_limit`, `budget_exhausted`, `empty_answer`, `awaiting_approval`,
//! `journal`, `pin_mismatch`, `indeterminate_call`, `ambiguous_calls`,
//! `too_large` (`LoopError::error_type`). `cancelled` on all three
//! is work abandoned before it ended: the token fired, or the future was
//! dropped.
//!
//! The instruments are built once per meter: for a meter given in code, when
//! it is given; for the global provider, at the first measurement after it is
//! installed (`otel::GlobalInstrument`), so a provider the process installs
//! after building its loop still receives them. Until 2026-11-04 every
//! measurement built its instrument again (audit C-23).
//!
//! [`ChatRequest`]: crate::tool_loop::ChatRequest
//! [`ModelError::error_type`]: crate::tool_loop::ModelError::error_type

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use opentelemetry::KeyValue;
use opentelemetry::metrics::{Counter, Histogram, Meter};

/// `gen_ai.client.inference.duration` and `gen_ai.execute_tool.duration`.
const DURATION_BUCKETS: [f64; 14] = [
    0.01, 0.02, 0.04, 0.08, 0.16, 0.32, 0.64, 1.28, 2.56, 5.12, 10.24, 20.48, 40.96, 81.92,
];

/// `gen_ai.invoke_agent.duration`.
const AGENT_DURATION_BUCKETS: [f64; 13] = [
    0.1, 0.2, 0.4, 0.8, 1.6, 3.2, 6.4, 12.8, 25.6, 51.2, 102.4, 204.8, 409.6,
];

/// `gen_ai.client.inference.operation.{input,output}_tokens`.
const TOKEN_BUCKETS: [f64; 14] = [
    1.0,
    4.0,
    16.0,
    64.0,
    256.0,
    1024.0,
    4096.0,
    16_384.0,
    65_536.0,
    262_144.0,
    1_048_576.0,
    4_194_304.0,
    16_777_216.0,
    67_108_864.0,
];

/// `gen_ai.invoke_agent.{inference,tool}_calls`.
const CALL_COUNT_BUCKETS: [f64; 8] = [1.0, 2.0, 4.0, 8.0, 16.0, 32.0, 64.0, 128.0];

/// How many distinct `gen_ai.response.model` values one meter records.
pub(crate) const MAX_RESPONSE_MODELS: usize = 16;
/// The longest `gen_ai.response.model` value recorded as given, in bytes.
pub(crate) const MAX_RESPONSE_MODEL_LEN: usize = 128;

/// Every instrument the loop records to, built from one meter.
struct Instruments {
    /// The `gen_ai.response.model` values recorded so far, at most
    /// [`MAX_RESPONSE_MODELS`].
    response_models: Mutex<std::collections::HashSet<String>>,
    inference_duration: Histogram<f64>,
    tool_duration: Histogram<f64>,
    /// `gen_ai.client.inference.operation.{input,output}_tokens`.
    operation_tokens: [Histogram<f64>; 2],
    /// `gen_ai.client.inference.usage.{input,output}_tokens`.
    usage_tokens: [Counter<u64>; 2],
    agent_duration: Histogram<f64>,
    inference_calls: Histogram<u64>,
    tool_calls: Histogram<u64>,
}

impl std::fmt::Debug for Instruments {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Instruments").finish_non_exhaustive()
    }
}

impl Instruments {
    /// `model` as a metric attribute value: as given while it is one of the
    /// first [`MAX_RESPONSE_MODELS`] seen and no longer than
    /// [`MAX_RESPONSE_MODEL_LEN`], `_OTHER` otherwise.
    fn response_model_label(&self, model: &str) -> String {
        if model.len() <= MAX_RESPONSE_MODEL_LEN
            && let Ok(mut seen) = self.response_models.lock()
            && (seen.contains(model) || seen.len() < MAX_RESPONSE_MODELS)
        {
            seen.insert(model.to_owned());
            return model.to_owned();
        }
        "_OTHER".to_owned()
    }

    fn build(meter: &Meter) -> Arc<Self> {
        let duration = |name: &'static str| {
            meter
                .f64_histogram(name)
                .with_unit("s")
                .with_boundaries(DURATION_BUCKETS.to_vec())
                .build()
        };
        let operation_tokens = |name: &'static str| {
            meter
                .f64_histogram(name)
                .with_unit("{token}")
                .with_boundaries(TOKEN_BUCKETS.to_vec())
                .build()
        };
        let usage_tokens =
            |name: &'static str| meter.u64_counter(name).with_unit("{token}").build();
        let calls = |name: &'static str, unit: &'static str| {
            meter
                .u64_histogram(name)
                .with_unit(unit)
                .with_boundaries(CALL_COUNT_BUCKETS.to_vec())
                .build()
        };
        Arc::new(Self {
            response_models: Mutex::default(),
            inference_duration: duration("gen_ai.client.inference.duration"),
            tool_duration: duration("gen_ai.execute_tool.duration"),
            operation_tokens: [
                operation_tokens("gen_ai.client.inference.operation.input_tokens"),
                operation_tokens("gen_ai.client.inference.operation.output_tokens"),
            ],
            usage_tokens: [
                usage_tokens("gen_ai.client.inference.usage.input_tokens"),
                usage_tokens("gen_ai.client.inference.usage.output_tokens"),
            ],
            agent_duration: meter
                .f64_histogram("gen_ai.invoke_agent.duration")
                .with_unit("s")
                .with_boundaries(AGENT_DURATION_BUCKETS.to_vec())
                .build(),
            inference_calls: calls("gen_ai.invoke_agent.inference_calls", "{inference_call}"),
            tool_calls: calls("gen_ai.invoke_agent.tool_calls", "{tool_call}"),
        })
    }
}

/// The global provider's instruments, under the `a2a.server` scope
/// `OtelMetricsBuilder` defaults to, built once per provider.
static GLOBAL: crate::otel::GlobalInstrument<Arc<Instruments>> =
    crate::otel::GlobalInstrument::new(Instruments::build);

/// The instruments the loop's metrics go to: those of the meter given, or the
/// global provider's as it is when asked.
#[derive(Clone, Default)]
pub struct GenAiMeter(Option<Arc<Instruments>>);

impl std::fmt::Debug for GenAiMeter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("GenAiMeter")
            .field(&if self.0.is_some() { "given" } else { "global" })
            .finish()
    }
}

impl GenAiMeter {
    /// Records to `meter` rather than the global provider.
    pub fn new(meter: &Meter) -> Self {
        Self(Some(Instruments::build(meter)))
    }

    fn get(&self) -> Arc<Instruments> {
        self.0.clone().unwrap_or_else(|| GLOBAL.get())
    }
}

/// What a turn or call is measuring.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Chat,
    Tool,
}

/// What the turn reported, filled in as it ends.
#[derive(Debug, Default)]
struct Outcome {
    error_type: Option<String>,
    response_model: Option<String>,
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
}

/// One turn's or one call's measurements, recorded when it is dropped.
#[derive(Debug)]
pub struct Measure {
    instruments: Arc<Instruments>,
    started: Instant,
    op: Op,
    attributes: Vec<KeyValue>,
    outcome: Mutex<Outcome>,
}

impl Measure {
    /// A model turn of `model` at `provider`, served at `server`.
    pub fn chat(
        meter: &GenAiMeter,
        provider: &str,
        model: &str,
        server: Option<(&str, u16)>,
    ) -> Self {
        let mut attributes = vec![
            KeyValue::new("gen_ai.operation.name", "chat"),
            KeyValue::new("gen_ai.provider.name", provider.to_owned()),
            KeyValue::new("gen_ai.request.model", model.to_owned()),
        ];
        if let Some((address, port)) = server {
            attributes.push(KeyValue::new("server.address", address.to_owned()));
            attributes.push(KeyValue::new("server.port", i64::from(port)));
        }
        Self::new(meter, Op::Chat, attributes)
    }

    /// A call to tool `name`.
    pub fn tool(meter: &GenAiMeter, name: &str) -> Self {
        let attributes = vec![
            KeyValue::new("gen_ai.tool.name", name.to_owned()),
            KeyValue::new("gen_ai.tool.type", "function"),
        ];
        Self::new(meter, Op::Tool, attributes)
    }

    fn new(meter: &GenAiMeter, op: Op, attributes: Vec<KeyValue>) -> Self {
        Self {
            instruments: meter.get(),
            started: Instant::now(),
            op,
            attributes,
            outcome: Mutex::new(Outcome::default()),
        }
    }

    /// The token counts and model the provider reported.
    pub fn answered(
        &self,
        input_tokens: Option<u64>,
        output_tokens: Option<u64>,
        response_model: Option<&str>,
    ) {
        if let Ok(mut outcome) = self.outcome.lock() {
            outcome.input_tokens = input_tokens;
            outcome.output_tokens = output_tokens;
            outcome.response_model = response_model.map(str::to_owned);
        }
    }

    /// It ended in `error_type`.
    pub fn failed(&self, error_type: &str) {
        if let Ok(mut outcome) = self.outcome.lock() {
            outcome.error_type = Some(error_type.to_owned());
        }
    }

    fn record(&self, outcome: &Outcome) {
        let seconds = self.started.elapsed().as_secs_f64();
        let mut attributes = self.attributes.clone();
        if let Some(model) = &outcome.response_model {
            let label = self.instruments.response_model_label(model);
            attributes.push(KeyValue::new("gen_ai.response.model", label));
        }
        let usage_attributes = attributes.clone();
        if let Some(error_type) = &outcome.error_type {
            attributes.push(KeyValue::new("error.type", error_type.clone()));
        }
        let duration = match self.op {
            Op::Chat => &self.instruments.inference_duration,
            Op::Tool => &self.instruments.tool_duration,
        };
        duration.record(seconds, &attributes);
        if self.op == Op::Chat {
            self.tokens(0, outcome.input_tokens, &attributes, &usage_attributes);
            self.tokens(1, outcome.output_tokens, &attributes, &usage_attributes);
        }
    }

    /// One direction's token instruments: the per-operation histogram for
    /// any reported count, and the usage counter for a positive one — "Each
    /// measurement MUST be a known, strictly positive modality
    /// contribution".
    /// `direction` is 0 for input, 1 for output.
    fn tokens(
        &self,
        direction: usize,
        count: Option<u64>,
        attributes: &[KeyValue],
        usage_attributes: &[KeyValue],
    ) {
        let Some(count) = count else { return };
        #[allow(clippy::cast_precision_loss)] // A token count below 2^53.
        self.instruments.operation_tokens[direction].record(count as f64, attributes);
        if count == 0 {
            return;
        }
        let mut usage_attributes = usage_attributes.to_vec();
        usage_attributes.push(KeyValue::new("gen_ai.token.modality", "text"));
        self.instruments.usage_tokens[direction].add(count, &usage_attributes);
    }
}

impl Drop for Measure {
    fn drop(&mut self) {
        let outcome = match self.outcome.get_mut() {
            Ok(outcome) => std::mem::take(outcome),
            Err(poisoned) => std::mem::take(poisoned.into_inner()),
        };
        self.record(&outcome);
    }
}

/// One run of the loop: an agent invocation. Recorded when dropped, as
/// `cancelled` unless [`ended`](Self::ended) said otherwise.
#[derive(Debug)]
pub struct AgentRun {
    instruments: Arc<Instruments>,
    started: Instant,
    attributes: Vec<KeyValue>,
    inference_calls: AtomicU64,
    tool_calls: AtomicU64,
    ended: Mutex<Ending>,
}

/// How far a run got.
#[derive(Debug)]
enum Ending {
    Running,
    Succeeded,
    Failed(String),
}

impl AgentRun {
    /// A run of an agent named `agent` over `model`.
    pub fn start(meter: &GenAiMeter, agent: Option<&str>, model: &str) -> Self {
        let mut attributes = vec![KeyValue::new("gen_ai.request.model", model.to_owned())];
        if let Some(agent) = agent {
            attributes.push(KeyValue::new("gen_ai.agent.name", agent.to_owned()));
        }
        Self {
            instruments: meter.get(),
            started: Instant::now(),
            attributes,
            inference_calls: AtomicU64::new(0),
            tool_calls: AtomicU64::new(0),
            ended: Mutex::new(Ending::Running),
        }
    }

    /// A model turn was asked for, whether or not it succeeded.
    pub fn inference_call(&self) {
        self.inference_calls.fetch_add(1, Ordering::Relaxed);
    }

    /// A tool call ran, whether or not it succeeded.
    pub fn tool_call(&self) {
        self.tool_calls.fetch_add(1, Ordering::Relaxed);
    }

    /// The run ended, failed with `error_type` or not.
    pub fn ended(&self, error_type: Option<&str>) {
        if let Ok(mut ended) = self.ended.lock() {
            *ended = error_type.map_or(Ending::Succeeded, |e| Ending::Failed(e.to_owned()));
        }
    }
}

impl Drop for AgentRun {
    fn drop(&mut self) {
        let ended = match self.ended.get_mut() {
            Ok(ended) => std::mem::replace(ended, Ending::Running),
            Err(poisoned) => std::mem::replace(poisoned.into_inner(), Ending::Running),
        };
        let error_type = match ended {
            Ending::Running => Some("cancelled".to_owned()),
            Ending::Succeeded => None,
            Ending::Failed(e) => Some(e),
        };
        // `agent.name` is the only attribute the two count histograms take.
        let counts: Vec<KeyValue> = self
            .attributes
            .iter()
            .filter(|kv| kv.key.as_str() == "gen_ai.agent.name")
            .cloned()
            .collect();
        let mut attributes = self.attributes.clone();
        if let Some(error_type) = error_type {
            attributes.push(KeyValue::new("error.type", error_type));
        }
        self.instruments
            .agent_duration
            .record(self.started.elapsed().as_secs_f64(), &attributes);
        self.instruments
            .inference_calls
            .record(self.inference_calls.load(Ordering::Relaxed), &counts);
        self.instruments
            .tool_calls
            .record(self.tool_calls.load(Ordering::Relaxed), &counts);
    }
}
