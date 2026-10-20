// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The genai client this example uses, with model names no hosted provider
//! claims sent to the OpenAI-compatible API on `http://localhost:11434/v1/`.
//!
//! genai routes such names to its Ollama adapter, which calls Ollama's own
//! `/api/chat`. Only Ollama serves that: llama.cpp's `llama-server` and vLLM
//! answer it `404`, and the run fell back to its mechanical answer with a
//! model listening (found 2026-10-20 against llama-server b11751). All three
//! serve `/v1/chat/completions`, so the OpenAI adapter pointed there reaches
//! any of them. Hosted model names keep genai's own routing.

use genai::adapter::AdapterKind;
use genai::resolver::{AuthData, Endpoint, ServiceTargetResolver};
use genai::{Client, ModelIden, ServiceTarget};

/// The OpenAI-compatible base URL a local model is served on.
pub const LOCAL_MODEL_ENDPOINT: &str = "http://localhost:11434/v1/";

/// A genai client that reaches a local model on [`LOCAL_MODEL_ENDPOINT`].
///
/// Fails only when genai cannot build its HTTP client.
pub fn client() -> genai::Result<Client> {
    let resolver = ServiceTargetResolver::from_resolver_fn(
        |target: ServiceTarget| -> Result<ServiceTarget, genai::resolver::Error> {
            if target.model.adapter_kind != AdapterKind::Ollama {
                return Ok(target);
            }
            Ok(ServiceTarget {
                endpoint: Endpoint::from_static(LOCAL_MODEL_ENDPOINT),
                // A local server takes any key; the OpenAI adapter needs one.
                auth: AuthData::from_single("local"),
                model: ModelIden::new(AdapterKind::OpenAI, target.model.model_name),
            })
        },
    );
    Client::builder()
        .with_service_target_resolver(resolver)
        .build()
}
