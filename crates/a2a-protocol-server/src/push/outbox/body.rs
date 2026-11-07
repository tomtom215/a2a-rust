// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! How an SQL outbox stores a row's event: its JSON, or, with a
//! [`PushConfigCipher`], sealed the way the push-config stores seal configs
//! (`push::cipher`), under a context naming the row, so a sealed body moved
//! to another row does not open.

use std::sync::Arc;

use a2a_protocol_types::error::{A2aError, A2aResult};
use a2a_protocol_types::events::StreamResponse;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;

use crate::push::PushConfigCipher;

/// Marks a sealed body.
const SEALED_PREFIX: &str = "a2a-sealed:v1:";

/// Sealed or plain, as the store was built.
#[derive(Clone, Default)]
pub struct Body {
    cipher: Option<Arc<dyn PushConfigCipher>>,
}

impl std::fmt::Debug for Body {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Body")
            .field("sealed", &self.cipher.is_some())
            .finish()
    }
}

/// The context a row's body is sealed under.
pub fn context(tenant: &str, caller: Option<&str>, task_id: &str, delivery_id: &str) -> Vec<u8> {
    serde_json::json!(["a2a-push-outbox/v1", tenant, caller, task_id, delivery_id])
        .to_string()
        .into_bytes()
}

impl Body {
    /// Seals every body from now on with `cipher`.
    pub fn with_cipher(cipher: Arc<dyn PushConfigCipher>) -> Self {
        Self {
            cipher: Some(cipher),
        }
    }

    /// The text stored for `event`.
    pub async fn encode(&self, event: &StreamResponse, context: &[u8]) -> A2aResult<String> {
        let json = serde_json::to_string(event)
            .map_err(|e| A2aError::internal(format!("push outbox: serialize: {e}")))?;
        match &self.cipher {
            None => Ok(json),
            Some(cipher) => {
                let sealed = cipher.seal(json.into_bytes(), context).await?;
                Ok(format!("{SEALED_PREFIX}{}", STANDARD.encode(sealed)))
            }
        }
    }

    /// The event stored as `stored`. A sealed body without a cipher, and a
    /// plain one with a cipher, are refused: a store that seals never serves
    /// a body it did not seal.
    pub async fn decode(&self, stored: &str, context: &[u8]) -> A2aResult<StreamResponse> {
        let json = match (stored.strip_prefix(SEALED_PREFIX), &self.cipher) {
            (Some(sealed), Some(cipher)) => {
                let sealed = STANDARD.decode(sealed).map_err(|e| {
                    A2aError::internal(format!("push outbox: a sealed body is not base64: {e}"))
                })?;
                cipher.open(sealed, context).await?
            }
            (None, None) => stored.as_bytes().to_vec(),
            (Some(_), None) => {
                return Err(A2aError::internal(
                    "push outbox: a body is sealed, and this outbox has no cipher",
                ));
            }
            (None, Some(_)) => {
                return Err(A2aError::internal(
                    "push outbox: a body is stored unsealed in an outbox that seals them",
                ));
            }
        };
        serde_json::from_slice(&json)
            .map_err(|e| A2aError::internal(format!("push outbox: deserialize: {e}")))
    }
}
