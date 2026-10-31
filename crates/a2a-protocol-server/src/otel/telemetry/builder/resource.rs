// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The resource a [`TelemetryBuilder`] gives every signal: service name,
//! instance id and detected attributes. Split out of `builder.rs` at the
//! 500-line ratchet.

use super::{Resource, TelemetryBuilder, config, detect};

impl TelemetryBuilder {
    #[cfg(test)]
    pub(in super::super) fn resource(&self) -> Resource {
        self.resource_and_notices().0
    }

    /// The resource, with `OTEL_RESOURCE_ATTRIBUTES` percent-decoded as the
    /// specification requires (`resource_env.rs`), and what could not be.
    pub(in super::super) fn resource_and_notices(&self) -> (Resource, Vec<String>) {
        let decoded = super::resource_env::decode(&*self.env);
        (
            super::resource_env::apply(self.sdk_resource(), decoded.attributes),
            decoded.notices,
        )
    }

    fn sdk_resource(&self) -> Resource {
        // `Resource::builder` reads `OTEL_SERVICE_NAME` and
        // `OTEL_RESOURCE_ATTRIBUTES` itself; the defaults are added only when
        // they name none, so they never overwrite them.
        let mut builder = Resource::builder();
        if let Some(name) = &self.default_service_name
            && !config::service_name_from_env(&*self.env)
        {
            builder = builder.with_service_name(name.clone());
        }
        // The conventions make `service.instance.id` the one attribute that
        // tells two replicas of a service apart (`service.instance.id`,
        // resource/service.md: "MUST be unique for each instance of the same
        // service.namespace,service.name pair"). Prometheus turns it into
        // `instance`, so without one every replica writes the same series.
        // A random UUID is what the conventions recommend when nothing
        // better names the process.
        if !config::resource_attribute_from_env(&*self.env, "service.instance.id") {
            builder = builder.with_attribute(opentelemetry::KeyValue::new(
                "service.instance.id",
                uuid::Uuid::new_v4().to_string(),
            ));
        }
        if self.detect_resource {
            for found in detect::attributes(&*self.env) {
                if !config::resource_attribute_from_env(&*self.env, found.key.as_str()) {
                    builder = builder.with_attribute(found);
                }
            }
        }
        builder.build()
    }
}
