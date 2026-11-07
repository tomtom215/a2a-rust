// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Outbound baggage: opt-in, by key, bounded, never over a caller's header.

#![cfg(feature = "otel")]

use std::collections::HashMap;

use a2a_protocol_types::trace_context::baggage::{BAGGAGE_HEADER, Member, parse};
use opentelemetry::KeyValue;
use opentelemetry::baggage::BaggageExt as _;

use super::*;
use crate::call_span::{CallSpan, Peer};

/// `f` with a `tracing-opentelemetry` layer recording every span.
fn with_otel<R>(f: impl FnOnce() -> R) -> R {
    use opentelemetry::trace::TracerProvider as _;
    use tracing_subscriber::layer::SubscriberExt as _;
    let provider = opentelemetry_sdk::trace::SdkTracerProvider::builder().build();
    let layer = tracing_opentelemetry::layer().with_tracer(provider.tracer("baggage-test"));
    crate::test_tracing::install();
    tracing::subscriber::with_default(tracing_subscriber::registry().with(layer), f)
}

/// The headers a call made under `baggage` writes, with a recording layer.
fn sent(baggage: &[(&str, &str)], policy: &BaggagePolicy, peer: &Peer) -> HashMap<String, String> {
    with_otel(|| {
        let context = opentelemetry::Context::new().with_baggage(
            baggage
                .iter()
                .map(|(k, v)| KeyValue::new((*k).to_owned(), (*v).to_owned())),
        );
        let _attached = context.attach();
        let call = CallSpan::open("SendMessage", peer, &serde_json::json!({}));
        let mut headers = HashMap::new();
        propagate(&call.span, peer.carries_headers, policy, &mut headers);
        headers
    })
}

fn jsonrpc() -> Peer {
    Peer::new("JSONRPC", "http://a:1", None)
}

fn members(headers: &HashMap<String, String>) -> Vec<(String, String)> {
    let mut got: Vec<_> = parse(&headers[BAGGAGE_HEADER], &BaggagePolicy::All)
        .into_iter()
        .map(|Member { key, value, .. }| (key, value))
        .collect();
    got.sort();
    got
}

#[test]
fn nothing_is_sent_by_default() {
    let headers = sent(&[("tenant", "acme")], &BaggagePolicy::Off, &jsonrpc());
    assert_eq!(headers, HashMap::new());
    assert_eq!(ClientConfig::default().baggage, BaggagePolicy::Off);
}

#[test]
fn only_the_allowed_keys_are_sent() {
    let headers = sent(
        &[
            ("tenant", "acme"),
            ("user.id", "42"),
            ("deploy", "blue green"),
        ],
        &BaggagePolicy::keys(["tenant", "deploy"]),
        &jsonrpc(),
    );
    assert_eq!(
        members(&headers),
        [
            ("deploy".to_owned(), "blue green".to_owned()),
            ("tenant".to_owned(), "acme".to_owned())
        ]
    );
}

#[test]
fn all_sends_every_entry() {
    let headers = sent(&[("a", "1"), ("b", "2")], &BaggagePolicy::All, &jsonrpc());
    assert_eq!(
        members(&headers),
        [
            ("a".to_owned(), "1".to_owned()),
            ("b".to_owned(), "2".to_owned())
        ]
    );
}

#[test]
fn nothing_is_sent_without_per_request_headers_or_without_baggage() {
    let custom = Peer::new("CUSTOM", "ws://a:1", None);
    assert_eq!(
        sent(&[("a", "1")], &BaggagePolicy::All, &custom),
        HashMap::new()
    );
    assert_eq!(sent(&[], &BaggagePolicy::All, &jsonrpc()), HashMap::new());
}

/// A `baggage` the caller set on the request, in any case, is theirs.
#[test]
fn a_callers_own_baggage_header_is_kept() {
    let headers = with_otel(|| {
        let context = opentelemetry::Context::new().with_baggage([KeyValue::new("a", "1")]);
        let _attached = context.attach();
        let peer = jsonrpc();
        let call = CallSpan::open("SendMessage", &peer, &serde_json::json!({}));
        let mut headers = HashMap::from([("Baggage".to_owned(), "mine=1".to_owned())]);
        propagate(&call.span, true, &BaggagePolicy::All, &mut headers);
        headers
    });
    assert_eq!(
        headers,
        HashMap::from([("Baggage".to_owned(), "mine=1".to_owned())])
    );
}

/// The setters set what they name.
#[test]
fn the_config_setters_set_the_policy_and_the_propagators() {
    let none = Propagators::from_env_value(Some("none"));
    let config = ClientConfig::default()
        .with_baggage_propagation(BaggagePolicy::keys(["tenant"]))
        .with_propagators(none.clone());
    assert_eq!(config.baggage, BaggagePolicy::keys(["tenant"]));
    assert_eq!(config.propagators, none);
}

/// What `tracing` recorded at `WARN`: the propagator named, then the message.
#[derive(Clone, Default)]
struct Warnings(std::sync::Arc<std::sync::Mutex<Vec<String>>>);

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for Warnings {
    fn on_event(&self, event: &tracing::Event<'_>, _: tracing_subscriber::layer::Context<'_, S>) {
        #[derive(Default)]
        struct Text {
            propagator: String,
            message: String,
        }
        impl tracing::field::Visit for Text {
            fn record_debug(&mut self, f: &tracing::field::Field, v: &dyn std::fmt::Debug) {
                match f.name() {
                    "propagator" => self.propagator = format!("{v:?}"),
                    "message" => self.message = format!("{v:?}"),
                    _ => {}
                }
            }
        }
        if *event.metadata().level() == tracing::Level::WARN {
            let mut text = Text::default();
            event.record(&mut text);
            self.0
                .lock()
                .expect("lock")
                .push(format!("{}: {}", text.propagator, text.message));
        }
    }
}

/// Each `OTEL_PROPAGATORS` name this crate does not carry is warned about,
/// by name, and a list of names it carries is not.
#[test]
fn each_unknown_propagator_is_warned_about_by_name() {
    use tracing_subscriber::layer::SubscriberExt as _;

    let warnings = Warnings::default();
    crate::test_tracing::install();
    tracing::subscriber::with_default(
        tracing_subscriber::registry().with(warnings.clone()),
        || {
            warn_unknown(&Propagators::from_env_value(Some("tracecontext,b3,jaeger")));
            warn_unknown(&Propagators::from_env_value(Some("baggage")));
        },
    );
    let said = warnings.0.lock().expect("lock").clone();
    assert_eq!(said.len(), 2, "{said:?}");
    assert!(
        said[0].starts_with("b3: OTEL_PROPAGATORS names a propagator"),
        "{said:?}"
    );
    assert!(
        said[1].starts_with("jaeger: OTEL_PROPAGATORS names a propagator"),
        "{said:?}"
    );
}
