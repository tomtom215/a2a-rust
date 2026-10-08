// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Langfuse as the trace destination.
//!
//! Langfuse needs nothing vendor-specific from an agent: it ingests plain
//! OTLP/HTTP and maps the semantic conventions this crate's spans carry. What
//! it does need is configuration that is easy to get subtly wrong, and this
//! preset is that configuration, read from the three variables Langfuse's own
//! Python and JavaScript SDKs use — so an agent written in Rust is configured
//! exactly like the Python agents next to it.
//!
//! Every fact below was read from Langfuse, not assumed: the endpoint path
//! and transports from `langfuse.com/integrations/native/opentelemetry`, the
//! metrics and logs behaviour from the server source
//! (`web/src/pages/api/public/otel/v1/metrics/index.ts` accepts metrics and
//! discards them; there is no logs route), at `langfuse/langfuse@1a21a42`.

use base64::Engine as _;

use super::TelemetryError;

/// Where a self-hosted Langfuse listens when started from its own
/// `docker-compose.yml`; [`Langfuse::from_env`]'s default.
///
/// Langfuse's own SDKs default to its cloud service instead. This preset
/// does not, so that nothing is sent off the machine unless
/// `LANGFUSE_BASE_URL` says where.
pub const LANGFUSE_SELF_HOSTED: &str = "http://localhost:3000";

/// Where traces go, and the project keys that authorise them.
///
/// Applying it with
/// [`TelemetryBuilder::with_langfuse`](super::TelemetryBuilder::with_langfuse):
///
/// * sends **traces** to `{base_url}/api/public/otel/v1/traces` over
///   OTLP/HTTP with protobuf bodies — Langfuse does not accept OTLP/gRPC —
///   with `Authorization: Basic base64(public_key:secret_key)` and
///   `x-langfuse-ingestion-version: 4`, which Langfuse's documentation asks
///   for so that directly ingested spans are not delayed;
/// * turns **metrics and logs** off by default, because Langfuse discards
///   OTLP metrics and has no OTLP logs route. `OTEL_METRICS_EXPORTER=otlp`
///   or `OTEL_LOGS_EXPORTER=otlp` turns them back on, towards the general
///   `OTEL_EXPORTER_OTLP_*` endpoint — a collector, not Langfuse.
///
/// The preset's endpoint and headers are set in code, so for traces they win
/// over `OTEL_EXPORTER_OTLP_ENDPOINT` and `OTEL_EXPORTER_OTLP_HEADERS`; to
/// send traces somewhere else, do not apply it.
#[derive(Clone)]
pub struct Langfuse {
    base_url: String,
    public_key: String,
    secret_key: String,
}

impl std::fmt::Debug for Langfuse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Langfuse")
            .field("base_url", &self.base_url)
            .field("public_key", &self.public_key)
            .field("secret_key", &"<redacted>")
            .finish()
    }
}

impl Langfuse {
    /// A project on the Langfuse at `base_url` — a self-hosted instance such
    /// as [`LANGFUSE_SELF_HOSTED`], or Langfuse Cloud
    /// (`https://cloud.langfuse.com`, `https://us.cloud.langfuse.com`).
    pub fn new(
        base_url: impl Into<String>,
        public_key: impl Into<String>,
        secret_key: impl Into<String>,
    ) -> Self {
        Self {
            base_url: base_url.into(),
            public_key: public_key.into(),
            secret_key: secret_key.into(),
        }
    }

    /// Reads `LANGFUSE_PUBLIC_KEY`, `LANGFUSE_SECRET_KEY` and
    /// `LANGFUSE_BASE_URL`, the variables Langfuse's SDKs read. The base URL
    /// defaults to [`LANGFUSE_SELF_HOSTED`] — not to Langfuse Cloud, as
    /// theirs does.
    ///
    /// # Errors
    ///
    /// [`TelemetryError::Config`] naming the key that is missing. The error
    /// never contains a key's value.
    pub fn from_env() -> Result<Self, TelemetryError> {
        Self::from_lookup(&super::config::process_env)
    }

    pub(super) fn from_lookup(env: &super::config::Env) -> Result<Self, TelemetryError> {
        let required = |name: &str| {
            env(name).ok_or_else(|| {
                TelemetryError::config(name, "", "required for Langfuse, and not set")
            })
        };
        Ok(Self::new(
            env("LANGFUSE_BASE_URL").unwrap_or_else(|| LANGFUSE_SELF_HOSTED.to_owned()),
            required("LANGFUSE_PUBLIC_KEY")?,
            required("LANGFUSE_SECRET_KEY")?,
        ))
    }

    /// The OTLP/HTTP traces endpoint.
    pub(super) fn traces_endpoint(&self) -> String {
        format!(
            "{}/api/public/otel/v1/traces",
            self.base_url.trim_end_matches('/')
        )
    }

    /// The headers every export carries.
    pub(super) fn headers(&self) -> Vec<(String, String)> {
        let credentials = base64::engine::general_purpose::STANDARD
            .encode(format!("{}:{}", self.public_key, self.secret_key));
        vec![
            ("authorization".to_owned(), format!("Basic {credentials}")),
            ("x-langfuse-ingestion-version".to_owned(), "4".to_owned()),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn lookup(pairs: &'static [(&'static str, &'static str)]) -> Box<super::super::config::Env> {
        let map: HashMap<&str, &str> = pairs.iter().copied().collect();
        Box::new(move |k| map.get(k).map(|v| (*v).to_owned()))
    }

    #[test]
    fn endpoint_is_the_otel_traces_route_whatever_the_trailing_slash() {
        for base in ["http://localhost:3000", "http://localhost:3000/"] {
            assert_eq!(
                Langfuse::new(base, "pk", "sk").traces_endpoint(),
                "http://localhost:3000/api/public/otel/v1/traces"
            );
        }
    }

    #[test]
    fn headers_are_basic_auth_of_the_key_pair_and_ingestion_version_4() {
        let headers = Langfuse::new(LANGFUSE_SELF_HOSTED, "pk-lf-1", "sk-lf-2").headers();
        // base64("pk-lf-1:sk-lf-2"), computed independently with
        // `printf 'pk-lf-1:sk-lf-2' | base64`.
        assert_eq!(
            headers,
            vec![
                (
                    "authorization".to_owned(),
                    "Basic cGstbGYtMTpzay1sZi0y".to_owned()
                ),
                ("x-langfuse-ingestion-version".to_owned(), "4".to_owned()),
            ]
        );
    }

    #[test]
    fn from_env_defaults_the_base_url_and_names_a_missing_key_without_values() {
        let lf = Langfuse::from_lookup(&*lookup(&[
            ("LANGFUSE_PUBLIC_KEY", "pk"),
            ("LANGFUSE_SECRET_KEY", "sk"),
        ]))
        .unwrap();
        assert_eq!(lf.base_url, LANGFUSE_SELF_HOSTED);

        let err = Langfuse::from_lookup(&*lookup(&[("LANGFUSE_PUBLIC_KEY", "pk-visible")]))
            .unwrap_err()
            .to_string();
        assert!(err.contains("LANGFUSE_SECRET_KEY"), "{err}");
        assert!(!err.contains("pk-visible"), "{err}");
    }

    #[test]
    fn debug_never_prints_the_secret_key() {
        let shown = format!(
            "{:?}",
            Langfuse::new(LANGFUSE_SELF_HOSTED, "pk", "sk-very-secret")
        );
        assert!(!shown.contains("sk-very-secret"), "{shown}");
        // It still says what it is: the instance and the public key.
        assert!(shown.contains(LANGFUSE_SELF_HOSTED), "{shown}");
        assert!(shown.contains("\"pk\""), "{shown}");
        assert!(shown.contains("<redacted>"), "{shown}");
    }
}
