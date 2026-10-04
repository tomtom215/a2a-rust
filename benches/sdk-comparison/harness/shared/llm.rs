// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Identical in both agents (copied by build script, byte-for-byte checked):
//! stream text deltas from an OpenAI-compatible llama-server.
use futures::StreamExt;

pub fn llm_url() -> String {
    std::env::var("LLM_URL").unwrap_or_else(|_| "http://127.0.0.1:11434/v1/chat/completions".into())
}

/// Calls the model and forwards each text delta to `tx` as it arrives.
/// Returns the number of deltas forwarded.
pub async fn stream_completion(
    client: &reqwest::Client,
    prompt: &str,
    max_tokens: u32,
    tx: tokio::sync::mpsc::Sender<String>,
) -> Result<usize, String> {
    let body = serde_json::json!({
        "model": "qwen",
        "stream": true,
        "max_tokens": max_tokens,
        "temperature": 0.0,
        "seed": 42,
        "messages": [{"role": "user", "content": prompt}],
        "chat_template_kwargs": {"enable_thinking": false}
    });
    let resp = client.post(llm_url()).json(&body).send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("llm http {}", resp.status()));
    }
    let mut stream = resp.bytes_stream();
    let mut buf = String::new();
    let mut n = 0usize;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| e.to_string())?;
        buf.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(pos) = buf.find('\n') {
            let line = buf[..pos].trim().to_string();
            buf.drain(..=pos);
            let Some(data) = line.strip_prefix("data:") else { continue };
            let data = data.trim();
            if data == "[DONE]" {
                return Ok(n);
            }
            let Ok(v) = serde_json::from_str::<serde_json::Value>(data) else { continue };
            if let Some(t) = v["choices"][0]["delta"]["content"].as_str() {
                if !t.is_empty() {
                    n += 1;
                    if tx.send(t.to_string()).await.is_err() {
                        return Ok(n);
                    }
                }
            }
        }
    }
    Ok(n)
}
