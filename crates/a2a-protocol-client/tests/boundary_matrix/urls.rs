// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! P1: one request target however the client was built.

use super::common::*;
use a2a_protocol_client::ClientBuilder;
use a2a_protocol_client::transport::RestTransport;

/// `SendMessage` crosses the wire as `POST {base}/message:send` however the
/// client was made: from the builder or from a card, with a root URL or one
/// under a base path, with or without a trailing slash.
#[tokio::test(flavor = "multi_thread")]
async fn rest_clients_send_to_the_same_target_however_they_were_built() {
    for base in ["", "/a2a"] {
        let upstream = serve((!base.is_empty()).then_some(base)).await;
        for slash in ["", "/"] {
            for how in ["builder", "card", "transport"] {
                let (proxy, targets) = recording_proxy(upstream).await;
                let url = format!("http://{proxy}{base}{slash}");
                let client = match how {
                    "builder" => ClientBuilder::new(&url)
                        .with_protocol_binding("HTTP+JSON")
                        .build()
                        .unwrap(),
                    "card" => ClientBuilder::from_card(&card_for(&url, "HTTP+JSON"))
                        .unwrap()
                        .build()
                        .unwrap(),
                    _ => ClientBuilder::new(&url)
                        .with_custom_transport(RestTransport::new(&url).unwrap())
                        .build()
                        .unwrap(),
                };
                let what = format!("{how} client, base {base:?}, trailing slash {slash:?}");
                send_ok(&client, &what).await;
                assert_eq!(
                    targets.lock().unwrap().as_slice(),
                    [format!("POST {base}/message:send")],
                    "{what}: the request target on the wire"
                );
            }
        }
    }
}
