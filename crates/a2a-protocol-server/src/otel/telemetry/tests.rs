// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The builder's decisions, with the environment injected. Export itself is
//! tested end to end against stand-in collectors in `tests/telemetry_*.rs`.

use std::collections::HashMap;

use super::*;

fn with_env(pairs: &[(&str, &str)]) -> TelemetryBuilder {
    let map: HashMap<String, String> = pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
    Telemetry::builder()
        .install_globally(false)
        .with_env(move |k| map.get(k).cloned())
}

#[test]
fn self_targets_match_whole_path_segments_only() {
    assert!(is_self_target("hyper"));
    assert!(is_self_target("hyper::proto::h1"));
    assert!(is_self_target("opentelemetry_sdk::trace"));
    assert!(!is_self_target("hyperlane"));
    assert!(!is_self_target("my_agent::hyper"));
    assert!(!is_self_target("a2a_protocol_server::rpc"));
}

#[test]
fn sdk_disabled_builds_no_provider_and_no_thread() {
    let t = with_env(&[
        ("OTEL_SDK_DISABLED", "true"),
        ("OTEL_TRACES_EXPORTER", "otlp"),
    ])
    .build()
    .unwrap();
    assert!(t.tracer_provider().is_none());
    assert!(t.meter_provider().is_none());
    assert!(t.logger_provider().is_none());
    assert!(t.runtime.is_none());
}

#[test]
fn every_signal_is_on_by_default_and_each_variable_turns_one_off() {
    let t = with_env(&[]).build().unwrap();
    assert!(
        t.tracer_provider().is_some()
            && t.meter_provider().is_some()
            && t.logger_provider().is_some()
    );
    let t = with_env(&[("OTEL_METRICS_EXPORTER", "none")])
        .build()
        .unwrap();
    assert!(
        t.tracer_provider().is_some()
            && t.meter_provider().is_none()
            && t.logger_provider().is_some()
    );
}

/// `with_export_where_configured`: a signal is built only when an endpoint
/// names where it goes — the general variable, the signal's own variable,
/// or code — and the per-signal exporter variables still win.
#[test]
fn export_where_configured_builds_only_the_signals_with_an_endpoint() {
    let providers = |t: &Telemetry| {
        (
            t.tracer_provider().is_some(),
            t.meter_provider().is_some(),
            t.logger_provider().is_some(),
        )
    };
    let none = with_env(&[])
        .with_export_where_configured()
        .build()
        .unwrap();
    assert_eq!(providers(&none), (false, false, false));
    let all = with_env(&[("OTEL_EXPORTER_OTLP_ENDPOINT", "http://127.0.0.1:9")])
        .with_export_where_configured()
        .build()
        .unwrap();
    assert_eq!(providers(&all), (true, true, true));
    let metrics_only = with_env(&[("OTEL_EXPORTER_OTLP_METRICS_ENDPOINT", "http://127.0.0.1:9")])
        .with_export_where_configured()
        .build()
        .unwrap();
    assert_eq!(providers(&metrics_only), (false, true, false));
    let in_code = with_env(&[])
        .with_otlp_endpoint("http://127.0.0.1:9")
        .with_export_where_configured()
        .build()
        .unwrap();
    assert_eq!(providers(&in_code), (true, true, true));
    let langfuse = with_env(&[])
        .with_langfuse(Langfuse::new("http://127.0.0.1:9", "pk", "sk"))
        .with_export_where_configured()
        .build()
        .unwrap();
    assert_eq!(providers(&langfuse), (true, false, false));
    let turned_off = with_env(&[
        ("OTEL_EXPORTER_OTLP_ENDPOINT", "http://127.0.0.1:9"),
        ("OTEL_LOGS_EXPORTER", "none"),
    ])
    .with_export_where_configured()
    .with_metrics(false)
    .build()
    .unwrap();
    assert_eq!(providers(&turned_off), (true, false, false));
}

#[test]
fn langfuse_defaults_metrics_and_logs_off_and_the_environment_can_restore_them() {
    let lf = Langfuse::new("http://127.0.0.1:9", "pk", "sk");
    let t = with_env(&[]).with_langfuse(lf.clone()).build().unwrap();
    assert!(t.tracer_provider().is_some());
    assert!(t.meter_provider().is_none() && t.logger_provider().is_none());
    let t = with_env(&[("OTEL_LOGS_EXPORTER", "otlp")])
        .with_langfuse(lf)
        .build()
        .unwrap();
    assert!(t.logger_provider().is_some() && t.meter_provider().is_none());
}

#[test]
fn langfuse_traces_ignore_a_grpc_protocol_variable() {
    let b = with_env(&[("OTEL_EXPORTER_OTLP_PROTOCOL", "grpc")]).with_langfuse(Langfuse::new(
        "https://langfuse.internal.example/",
        "pk",
        "sk",
    ));
    let traces = b.target(Signal::Traces).unwrap();
    assert_eq!(traces.protocol, OtlpProtocol::HttpProtobuf);
    assert_eq!(
        traces.endpoint.as_deref(),
        Some("https://langfuse.internal.example/api/public/otel/v1/traces")
    );
    // The other signals still follow the environment.
    assert_eq!(
        b.target(Signal::Metrics).unwrap().protocol,
        OtlpProtocol::Grpc
    );
}

#[test]
fn a_bad_variable_fails_the_build_naming_it() {
    let err = with_env(&[("OTEL_LOGS_EXPORTER", "zipkin")])
        .build()
        .unwrap_err();
    assert!(err.to_string().contains("OTEL_LOGS_EXPORTER"), "{err}");
}

#[test]
fn compression_comes_from_the_environment_and_code_wins() {
    let t = |b: TelemetryBuilder| b.target(Signal::Traces).unwrap().compression;
    assert_eq!(t(with_env(&[])), None);
    assert_eq!(
        t(with_env(&[("OTEL_EXPORTER_OTLP_COMPRESSION", "gzip")])),
        Some(OtlpCompression::Gzip)
    );
    assert_eq!(
        t(with_env(&[("OTEL_EXPORTER_OTLP_COMPRESSION", "none")])
            .with_compression(OtlpCompression::Gzip)),
        Some(OtlpCompression::Gzip)
    );
}

#[test]
fn a_compression_this_crate_cannot_act_on_fails_the_build_naming_it() {
    for (var, value) in [
        ("OTEL_EXPORTER_OTLP_LOGS_COMPRESSION", "zstd"),
        ("OTEL_EXPORTER_OTLP_COMPRESSION", "brotli"),
    ] {
        let err = with_env(&[(var, value)]).build().unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains(var) && msg.contains(value), "{msg}");
    }
}

#[test]
fn an_unreadable_certificate_file_fails_the_build_naming_the_variable() {
    let err = with_env(&[("OTEL_EXPORTER_OTLP_CERTIFICATE", "/nonexistent/ca.pem")])
        .build()
        .unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("OTEL_EXPORTER_OTLP_CERTIFICATE") && msg.contains("/nonexistent/ca.pem"),
        "{msg}"
    );
}

#[test]
fn a_certificate_that_is_not_pem_is_refused() {
    let err = with_env(&[])
        .with_ca_certificate_pem("not a certificate")
        .build()
        .unwrap_err();
    assert!(matches!(err, TelemetryError::Tls(_)), "{err}");
}

#[test]
fn the_default_service_name_applies_only_when_the_environment_names_none() {
    let named = with_env(&[])
        .with_default_service_name("code-default")
        .resource();
    assert_eq!(
        named
            .get(&opentelemetry::Key::new("service.name"))
            .map(|v| v.to_string()),
        Some("code-default".to_owned())
    );
    // The injected environment says the service is named, so the default
    // must not be applied; the process environment, which `Resource` reads,
    // names none here, so the SDK's own fallback shows through.
    let deferred = with_env(&[("OTEL_SERVICE_NAME", "from-env")])
        .with_default_service_name("code-default")
        .resource();
    let name = deferred
        .get(&opentelemetry::Key::new("service.name"))
        .map(|v| v.to_string())
        .unwrap_or_default();
    assert_ne!(name, "code-default");
}

/// Two processes of one service report different `service.instance.id`s
/// unless the environment names one, which then wins.
#[test]
fn each_process_gets_an_instance_id_unless_the_environment_names_one() {
    let key = opentelemetry::Key::new("service.instance.id");
    let a = with_env(&[]).resource().get(&key).map(|v| v.to_string());
    let b = with_env(&[]).resource().get(&key).map(|v| v.to_string());
    let a = a.expect("an instance id by default");
    assert!(uuid::Uuid::parse_str(&a).is_ok(), "{a}");
    assert_ne!(Some(a), b, "two telemetries must not share an instance id");
    // The injected environment names one, so none is generated; the
    // process environment, which `Resource` reads, names none here.
    let named = with_env(&[("OTEL_RESOURCE_ATTRIBUTES", "service.instance.id=pod-7")]).resource();
    assert_eq!(named.get(&key), None);
}

#[test]
fn debug_output_never_contains_header_values() {
    let b = with_env(&[]).with_otlp_header("authorization", "Basic c2VjcmV0");
    assert!(!format!("{b:?}").contains("c2VjcmV0"));
}

/// The process environment is read as such, with an unset or empty value
/// treated as unset. Cargo and nextest both set `CARGO_PKG_NAME` for a test.
#[test]
fn process_env_reads_the_process_environment() {
    assert_eq!(
        config::process_env("CARGO_PKG_NAME").as_deref(),
        Some("a2a-protocol-server")
    );
    assert_eq!(config::process_env("A2A_TELEMETRY_TEST_SURELY_UNSET"), None);
}

/// `http/json` is a protocol the specification names, so without the
/// `otlp-json` feature it is refused for a reason of its own rather than as
/// unknown. With the feature, `tests/telemetry_export/` exports with it.
#[cfg(not(feature = "otlp-json"))]
#[test]
fn http_json_is_refused_as_not_compiled_in() {
    let err = with_env(&[("OTEL_EXPORTER_OTLP_PROTOCOL", "http/json")])
        .build()
        .unwrap_err()
        .to_string();
    assert!(err.contains("not compiled in"), "{err}");
}

#[test]
fn builder_debug_names_its_settings() {
    let shown = format!("{:?}", with_env(&[]).with_default_service_name("svc"));
    assert!(shown.starts_with("TelemetryBuilder"), "{shown}");
    assert!(shown.contains("\"svc\""), "{shown}");
}

/// A configuration error shows the value only when there is one.
#[test]
fn a_config_error_shows_its_value_only_when_set() {
    assert_eq!(
        TelemetryError::config("VAR", "", "required").to_string(),
        "VAR: required"
    );
    assert_eq!(
        TelemetryError::config("VAR", "v", "bad").to_string(),
        "VAR=\"v\": bad"
    );
}

/// The two errors that wrap another error give it as their source.
#[test]
fn wrapped_errors_are_their_source() {
    use std::error::Error as _;
    let exporter = TelemetryError::Exporter {
        signal: "traces",
        source: opentelemetry_otlp::ExporterBuildError::InvalidConfiguration("x".into()),
    };
    assert!(exporter.source().is_some());
    let runtime = TelemetryError::Runtime(std::io::Error::other("no thread"));
    assert_eq!(
        runtime.source().map(ToString::to_string).as_deref(),
        Some("no thread")
    );
    assert!(TelemetryError::Tls("x".into()).source().is_none());
}

/// Dropping the export runtime waits for it: when `drop` returns, the
/// runtime and every task on it have been dropped.
#[test]
fn dropping_the_export_runtime_waits_for_it_to_stop() {
    struct SetOnDrop(Arc<std::sync::atomic::AtomicBool>);
    impl Drop for SetOnDrop {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }
    let dropped = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let runtime = runtime::ExportRuntime::start().expect("start");
    let guard = SetOnDrop(Arc::clone(&dropped));
    runtime.handle().spawn(async move {
        let _guard = guard;
        std::future::pending::<()>().await;
    });
    drop(runtime);
    assert!(dropped.load(Ordering::SeqCst));
}

/// The documented bound on a collector's reply: 64 KiB.
#[test]
fn a_collectors_reply_is_read_up_to_64_kib() {
    assert_eq!(http::MAX_RESPONSE_BODY, 65_536);
}

/// The export client debugs as itself and its timeout — and nothing that
/// could carry a credential.
#[test]
fn the_export_client_debugs_as_its_timeout() {
    let runtime = runtime::ExportRuntime::start().expect("start");
    let tls = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .expect("protocol versions")
    .with_root_certificates(rustls::RootCertStore::empty())
    .with_no_client_auth();
    let client = http::ExportHttpClient::new(
        runtime.handle().clone(),
        &tls,
        std::time::Duration::from_secs(7),
        Arc::new(hyper_util::client::proxy::matcher::Matcher::builder().build()),
    );
    let shown = format!("{client:?}");
    assert!(shown.starts_with("ExportHttpClient"), "{shown}");
    assert!(shown.contains("7s"), "{shown}");
}

/// The client certificate and key the environment names are read from their
/// files and win over those given in code; a file that cannot be read fails
/// the build naming its variable.
#[test]
fn client_identity_files_are_read_and_win_over_code() {
    let dir = std::env::temp_dir().join(format!("a2a-otlp-identity-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (cert, key) = (dir.join("c.pem"), dir.join("k.pem"));
    std::fs::write(&cert, "CERT FROM FILE").unwrap();
    std::fs::write(&key, "KEY FROM FILE").unwrap();
    let (cert, key) = (cert.to_str().unwrap(), key.to_str().unwrap());
    let target = with_env(&[
        ("OTEL_EXPORTER_OTLP_CLIENT_CERTIFICATE", cert),
        ("OTEL_EXPORTER_OTLP_CLIENT_KEY", key),
    ])
    .with_client_certificate_pem("code cert", "code key")
    .target(Signal::Metrics)
    .unwrap();
    let identity = target.client_identity.expect("an identity");
    assert_eq!(identity.cert_pem, b"CERT FROM FILE");
    assert_eq!(identity.key_pem, b"KEY FROM FILE");

    let target = with_env(&[])
        .with_client_certificate_pem("code cert", "code key")
        .target(Signal::Logs)
        .unwrap();
    assert_eq!(
        target.client_identity.expect("an identity").key_pem,
        b"code key"
    );

    let missing = dir.join("absent.pem");
    let err = with_env(&[
        ("OTEL_EXPORTER_OTLP_CLIENT_CERTIFICATE", cert),
        (
            "OTEL_EXPORTER_OTLP_TRACES_CLIENT_KEY",
            missing.to_str().unwrap(),
        ),
    ])
    .target(Signal::Traces)
    .err()
    .expect("unreadable key");
    assert!(
        err.to_string()
            .contains("OTEL_EXPORTER_OTLP_TRACES_CLIENT_KEY"),
        "{err}"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A client certificate or key that is not PEM fails the build on either
/// transport, rather than every handshake.
#[test]
fn a_client_identity_that_is_not_pem_fails_the_build() {
    for protocol in [OtlpProtocol::Grpc, OtlpProtocol::HttpProtobuf] {
        let err = with_env(&[])
            .with_otlp_protocol(protocol)
            .with_otlp_endpoint("https://localhost:1")
            .with_client_certificate_pem("not a certificate", "not a key")
            .build()
            .unwrap_err();
        assert!(
            err.to_string().contains("client certificate"),
            "{protocol:?}: {err}"
        );
    }
}

/// OTLP/gRPC with no endpoint in code takes the environment's, as
/// `opentelemetry-otlp` reads it: the signal's variable before the general
/// one, a scheme added from `INSECURE` when it has none; OTLP/HTTP leaves an
/// unset endpoint to the exporter.
#[test]
fn a_grpc_endpoint_comes_from_the_environment_and_an_http_one_is_left_unset() {
    let endpoint =
        |pairs: &[(&str, &str)]| with_env(pairs).target(Signal::Metrics).unwrap().endpoint;
    let grpc = ("OTEL_EXPORTER_OTLP_PROTOCOL", "grpc");
    let general = ("OTEL_EXPORTER_OTLP_ENDPOINT", "collector:4317");
    assert_eq!(
        endpoint(&[grpc, general]).as_deref(),
        Some("https://collector:4317")
    );
    assert_eq!(
        endpoint(&[grpc, general, ("OTEL_EXPORTER_OTLP_INSECURE", " TRUE ")]).as_deref(),
        Some("http://collector:4317")
    );
    assert_eq!(
        endpoint(&[
            grpc,
            ("OTEL_EXPORTER_OTLP_ENDPOINT", "http://c.example:4317")
        ])
        .as_deref(),
        Some("http://c.example:4317")
    );
    assert_eq!(
        endpoint(&[
            grpc,
            general,
            (
                "OTEL_EXPORTER_OTLP_METRICS_ENDPOINT",
                "https://metrics.example:4317"
            ),
        ])
        .as_deref(),
        Some("https://metrics.example:4317")
    );
    assert_eq!(endpoint(&[grpc]), None);
    // OTLP/HTTP leaves it to the exporter, which reads the variable itself
    // (and needs a scheme: `a_schemeless_http_endpoint_is_refused_…`).
    assert_eq!(
        endpoint(&[
            ("OTEL_EXPORTER_OTLP_PROTOCOL", "http/protobuf"),
            ("OTEL_EXPORTER_OTLP_ENDPOINT", "http://collector:4318"),
        ]),
        None
    );
}
