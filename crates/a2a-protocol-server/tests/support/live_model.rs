// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! What the real-model tests of the 2026-11-13 review share: the model client
//! for `llama-server`, a content cipher, and a SQLite file whose bytes a test
//! can search.

use std::sync::Arc;
use std::time::Duration;

use a2a_protocol_server::store::encrypted::{AES_256_KEY_LEN, Aes256GcmCipher, ContentCipher};
use a2a_protocol_server::tool_loop::OpenAiChatModel;
use serde_json::json;

pub fn model() -> OpenAiChatModel {
    let base = std::env::var("A2A_LIVE_MODEL_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:11434/v1".to_owned());
    let name = std::env::var("A2A_LIVE_MODEL").unwrap_or_else(|_| "qwen3:1.7b".to_owned());
    let mut extra = serde_json::Map::new();
    extra.insert(
        "chat_template_kwargs".to_owned(),
        json!({"enable_thinking": false}),
    );
    extra.insert("max_tokens".to_owned(), json!(200));
    extra.insert("temperature".to_owned(), json!(0));
    extra.insert("seed".to_owned(), json!(7));
    OpenAiChatModel::new(&base, name)
        .expect("a model client")
        .with_timeout(Duration::from_secs(300))
        .with_extra_body(extra)
}

pub fn cipher(key: u8) -> Arc<dyn ContentCipher> {
    Arc::new(Aes256GcmCipher::new(format!("k{key}"), [key; AES_256_KEY_LEN]).unwrap())
}

/// A database file of its own, removed when dropped.
pub struct DbFile(std::path::PathBuf);

impl DbFile {
    pub fn new(what: &str) -> Self {
        Self(std::env::temp_dir().join(format!("a2a-live-{what}-{}.db", uuid::Uuid::new_v4())))
    }

    pub fn url(&self) -> String {
        format!("sqlite://{}?mode=rwc", self.0.display())
    }

    /// The bytes of the database and of its write-ahead log, if any.
    fn bytes(&self) -> Vec<u8> {
        let mut out = std::fs::read(&self.0).unwrap();
        for suffix in ["-wal", "-journal"] {
            let mut p = self.0.clone().into_os_string();
            p.push(suffix);
            if let Ok(more) = std::fs::read(&p) {
                out.extend(more);
            }
        }
        out
    }

    pub fn holds(&self, text: &str) -> bool {
        self.bytes()
            .windows(text.len())
            .any(|w| w == text.as_bytes())
    }
}

impl Drop for DbFile {
    fn drop(&mut self) {
        for suffix in ["", "-wal", "-shm", "-journal"] {
            let mut p = self.0.clone().into_os_string();
            p.push(suffix);
            let _ = std::fs::remove_file(p);
        }
    }
}
