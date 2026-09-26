// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

// Shared, SDK-independent code. Included verbatim via #[path] by BOTH agents
// (agent-rs and agent-rust) so the model-calling code cannot differ between
// the two SDK harnesses.

use std::time::Duration;

/// Where llama-server listens (OpenAI-compatible API).
pub fn endpoint() -> String {
    std::env::var("LLM_URL").unwrap_or_else(|_| "http://127.0.0.1:8080".into())
}

pub fn max_tokens() -> u32 {
    std::env::var("LLM_MAX_TOKENS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(48)
}

/// Streams a chat completion from llama-server. Each delta of generated text is
/// sent on the returned channel; the channel closes when generation ends.
/// An `Err` item means the model call failed.
pub fn stream_completion(prompt: String) -> tokio::sync::mpsc::Receiver<Result<String, String>> {
    let (tx, rx) = tokio::sync::mpsc::channel(64);
    tokio::spawn(async move {
        if let Err(e) = run(prompt, &tx).await {
            let _ = tx.send(Err(e)).await;
        }
    });
    rx
}

async fn run(
    prompt: String,
    tx: &tokio::sync::mpsc::Sender<Result<String, String>>,
) -> Result<(), String> {
    use futures::StreamExt;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|e| e.to_string())?;
    let body = serde_json::json!({
        "model": "qwen3-0.6b",
        "messages": [
            {"role": "system", "content": "You are a concise assistant. Answer in one or two sentences."},
            {"role": "user", "content": prompt}
        ],
        "stream": true,
        "max_tokens": max_tokens(),
        "temperature": 0.0,
        "seed": 42,
        "cache_prompt": false,
        "chat_template_kwargs": {"enable_thinking": false}
    });
    let resp = client
        .post(format!("{}/v1/chat/completions", endpoint()))
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("llm request: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("llm status {}", resp.status()));
    }
    let mut buf = Vec::<u8>::new();
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("llm stream: {e}"))?;
        buf.extend_from_slice(&chunk);
        while let Some(pos) = buf.windows(2).position(|w| w == b"\n\n") {
            let event: Vec<u8> = buf.drain(..pos + 2).collect();
            let text = String::from_utf8_lossy(&event);
            for line in text.lines() {
                let Some(data) = line.strip_prefix("data: ") else { continue };
                if data.trim() == "[DONE]" {
                    return Ok(());
                }
                let v: serde_json::Value =
                    serde_json::from_str(data).map_err(|e| format!("llm json: {e}"))?;
                if let Some(delta) = v["choices"][0]["delta"]["content"].as_str() {
                    if !delta.is_empty() && tx.send(Ok(delta.to_string())).await.is_err() {
                        return Ok(()); // consumer went away (e.g. cancel)
                    }
                }
            }
        }
    }
    Ok(())
}
