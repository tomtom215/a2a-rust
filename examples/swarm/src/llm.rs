// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A non-streaming call to an OpenAI-compatible chat endpoint (llama.cpp's
//! `llama-server`, vLLM, Ollama). Plain HTTP only: the swarm's model leg is a
//! loopback measurement, not a production client.

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;

#[derive(Clone)]
pub struct Llm {
    client: Client<hyper_util::client::legacy::connect::HttpConnector, Full<Bytes>>,
    url: String,
}

impl Llm {
    /// `SWARM_LLM_URL`, e.g. `http://127.0.0.1:11434/v1/chat/completions`.
    /// `None` when unset, so the swarm runs without a model.
    pub fn from_env() -> Option<Self> {
        let url = std::env::var("SWARM_LLM_URL").ok()?;
        Some(Self {
            client: Client::builder(TokioExecutor::new()).build_http(),
            url,
        })
    }

    pub async fn complete(&self, prompt: &str, max_tokens: u32) -> Result<String, String> {
        let body = serde_json::json!({
            "model": "swarm", "max_tokens": max_tokens, "temperature": 0.0, "seed": 42,
            "messages": [{"role": "user", "content": prompt}],
            "chat_template_kwargs": {"enable_thinking": false},
        });
        let req = hyper::Request::post(&self.url)
            .header("content-type", "application/json")
            .body(Full::new(Bytes::from(body.to_string())))
            .map_err(|e| e.to_string())?;
        let res = self.client.request(req).await.map_err(|e| e.to_string())?;
        let status = res.status();
        let bytes = res
            .into_body()
            .collect()
            .await
            .map_err(|e| e.to_string())?
            .to_bytes();
        if !status.is_success() {
            return Err(format!("model endpoint answered {status}"));
        }
        let v: serde_json::Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        v["choices"][0]["message"]["content"]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| "model response had no content".to_owned())
    }
}
