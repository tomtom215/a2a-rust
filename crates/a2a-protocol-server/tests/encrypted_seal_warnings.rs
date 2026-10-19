// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Release audit F7: the cipher warns as its seal budget runs down.
//!
//! In a binary of its own: the warning is read through a subscriber set for
//! this thread only, and `tracing` caches each callsite's interest across
//! threads. Another test reaching the same warning on another thread, with
//! no subscriber, could leave it cached as unwanted and drop a warning here
//! (seen on Windows, 2026-10-19, with `encrypted_seal_cost.rs`'s limit test).

#![cfg(all(feature = "encryption", feature = "tracing"))]

use std::sync::Arc;

use a2a_protocol_server::store::encrypted::{AES_256_KEY_LEN, Aes256GcmCipher, ContentCipher};

/// A warning at half the limit and one at nine tenths, and none otherwise:
/// the operator hears before the refusal. Limit 10: the 5th and the 9th.
#[test]
fn the_budget_warns_at_half_and_nine_tenths() {
    use std::sync::Mutex;
    use tracing_subscriber::layer::SubscriberExt as _;

    #[derive(Clone, Default)]
    struct Warnings(Arc<Mutex<Vec<String>>>);
    impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for Warnings {
        fn on_event(
            &self,
            event: &tracing::Event<'_>,
            _: tracing_subscriber::layer::Context<'_, S>,
        ) {
            if *event.metadata().level() == tracing::Level::WARN {
                struct Sealed(String);
                impl tracing::field::Visit for Sealed {
                    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
                        if field.name() == "sealed" {
                            self.0 = value.to_string();
                        }
                    }
                    fn record_debug(&mut self, _: &tracing::field::Field, _: &dyn std::fmt::Debug) {
                    }
                }
                let mut sealed = Sealed(String::new());
                event.record(&mut sealed);
                self.0.lock().unwrap().push(sealed.0);
            }
        }
    }

    let warnings = Warnings::default();
    let subscriber = tracing_subscriber::registry().with(warnings.clone());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("runtime");
    tracing::subscriber::with_default(subscriber, || {
        runtime.block_on(async {
            let cipher = Aes256GcmCipher::new("k1", [5; AES_256_KEY_LEN])
                .expect("cipher")
                .with_seal_limit(10);
            for _ in 0..10 {
                cipher
                    .seal(b"x".to_vec(), b"ctx")
                    .await
                    .expect("within the limit");
            }
        });
    });
    assert_eq!(*warnings.0.lock().unwrap(), ["5", "9"]);
}
