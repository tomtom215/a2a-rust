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
//! `error.type`, on every instrument that takes it, is one of: for a turn,
//! [`ModelError::error_type`] (the HTTP status, `timeout`, `transport`,
//! `invalid_response`, `response_too_large`, `configuration`, `_OTHER`); for
//! a call, `unknown_tool`, `invalid_arguments`, `tool_error`, `tool_fatal`;
//! for a run, the turn's type when the model failed, `tool_fatal`,
//! `turn_limit`, `budget_exhausted`. `cancelled` on all three is work
//! abandoned before it ended: the token fired, or the future was dropped.
//!
//! The instruments are built from the meter at the moment each measurement
//! is taken, not at start-up, so a provider the process installs after
//! building its loop still receives them.
//!
//! [`ChatRequest`]: crate::tool_loop::ChatRequest
//! [`ModelError::error_type`]: crate::tool_loop::ModelError::error_type

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use opentelemetry::KeyValue;
use opentelemetry::metrics::Meter;

/// The instrumentation scope the loop's metrics are recorded under when no
/// meter is given: the one `OtelMetricsBuilder` defaults to.
pub const METER_NAME: &str = "a2a.server";

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

/// The meter the loop's metrics go to: the one given, or the global
/// provider's as it is when asked.
#[derive(Clone, Debug, Default)]
pub struct GenAiMeter(Option<Meter>);

impl GenAiMeter {
    /// Records to `meter` rather than the global provider.
    pub const fn new(meter: Meter) -> Self {
        Self(Some(meter))
    }

    fn get(&self) -> Meter {
        self.0
            .clone()
            .unwrap_or_else(|| opentelemetry::global::meter(METER_NAME))
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
    meter: Meter,
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
            meter: meter.get(),
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
            attributes.push(KeyValue::new("gen_ai.response.model", model.clone()));
        }
        let usage_attributes = attributes.clone();
        if let Some(error_type) = &outcome.error_type {
            attributes.push(KeyValue::new("error.type", error_type.clone()));
        }
        let duration = match self.op {
            Op::Chat => "gen_ai.client.inference.duration",
            Op::Tool => "gen_ai.execute_tool.duration",
        };
        self.meter
            .f64_histogram(duration)
            .with_unit("s")
            .with_boundaries(DURATION_BUCKETS.to_vec())
            .build()
            .record(seconds, &attributes);
        if self.op == Op::Chat {
            self.tokens(
                "input",
                outcome.input_tokens,
                &attributes,
                &usage_attributes,
            );
            self.tokens(
                "output",
                outcome.output_tokens,
                &attributes,
                &usage_attributes,
            );
        }
    }

    /// One direction's token instruments: the per-operation histogram for
    /// any reported count, and the usage counter for a positive one — "Each
    /// measurement MUST be a known, strictly positive modality
    /// contribution".
    fn tokens(
        &self,
        direction: &str,
        count: Option<u64>,
        attributes: &[KeyValue],
        usage_attributes: &[KeyValue],
    ) {
        let Some(count) = count else { return };
        #[allow(clippy::cast_precision_loss)] // A token count below 2^53.
        self.meter
            .f64_histogram(format!(
                "gen_ai.client.inference.operation.{direction}_tokens"
            ))
            .with_unit("{token}")
            .with_boundaries(TOKEN_BUCKETS.to_vec())
            .build()
            .record(count as f64, attributes);
        if count == 0 {
            return;
        }
        let mut usage_attributes = usage_attributes.to_vec();
        usage_attributes.push(KeyValue::new("gen_ai.token.modality", "text"));
        self.meter
            .u64_counter(format!("gen_ai.client.inference.usage.{direction}_tokens"))
            .with_unit("{token}")
            .build()
            .add(count, &usage_attributes);
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
    meter: Meter,
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
            meter: meter.get(),
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
        self.meter
            .f64_histogram("gen_ai.invoke_agent.duration")
            .with_unit("s")
            .with_boundaries(AGENT_DURATION_BUCKETS.to_vec())
            .build()
            .record(self.started.elapsed().as_secs_f64(), &attributes);
        for (name, unit, count) in [
            (
                "gen_ai.invoke_agent.inference_calls",
                "{inference_call}",
                &self.inference_calls,
            ),
            (
                "gen_ai.invoke_agent.tool_calls",
                "{tool_call}",
                &self.tool_calls,
            ),
        ] {
            self.meter
                .u64_histogram(name)
                .with_unit(unit)
                .with_boundaries(CALL_COUNT_BUCKETS.to_vec())
                .build()
                .record(count.load(Ordering::Relaxed), &counts);
        }
    }
}
