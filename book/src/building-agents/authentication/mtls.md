<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Mutual TLS

Name the caller by the client certificate it presented in the TLS handshake. The certificate is verified by the listener's TLS configuration; `MutualTlsAuthInterceptor` turns it into a caller identity.

## The client certificate names the caller

With TLS on the listener and a rustls configuration whose client verifier
trusts your CAs (`rustls::server::WebPkiClientVerifier`, passed to
`Server::with_tls` or `WebSocketDispatcher::with_tls`; for gRPC,
`ServerTlsConfig::client_ca_root`), the certificate a client presents
reaches the interceptors as `CallContext::peer_certificate`, the DER of its
leaf. `MutualTlsAuthInterceptor` (`tls-rustls` or `grpc-tls`) names the
caller from it and refuses a call without one (`401`, `WWW-Authenticate:
MutualTLS`; gRPC `UNAUTHENTICATED`):

```rust,no_run
# fn card() -> a2a_protocol_types::agent_card::AgentCard {
#     use a2a_protocol_types::agent_card::{AgentCard, AgentInterface, AgentSkill};
#     AgentCard::new("my-agent", "1.0.0", AgentInterface::jsonrpc("http://localhost:3000"))
#         .with_description("An example agent")
#         .with_skill(AgentSkill::new("echo", "Echo", "Echoes text").with_tags(["echo"]))
# }
use a2a_protocol_server::auth::MutualTlsAuthInterceptor;
use a2a_protocol_types::security::{
    MutualTlsSecurityScheme, SecurityRequirement, SecurityScheme, StringList,
};
use std::collections::HashMap;
# use a2a_protocol_sdk::prelude::*;
# struct MyAgent;
# agent_executor!(MyAgent, |_ctx, _queue| async { Ok(()) });
# fn main() -> Result<(), Box<dyn std::error::Error>> {

// `mtls:sha256:<fingerprint>`: exact, needs no parser, changes on renewal.
let by_fingerprint = MutualTlsAuthInterceptor::new();

// `mtls:<name>` from your own mapping; `None` refuses the certificate. To
// name callers by subject, parse `der` with the X.509 crate you trust.
let by_name = MutualTlsAuthInterceptor::with_names(|der| {
    (der.len() > 0).then(|| "billing-agent".to_owned())
});
# let _ = by_fingerprint;

// Spec §13.3: the public card declares the scheme the interceptor checks.
let card = card()
    .with_security_schemes(HashMap::from([(
        "mtls".to_owned(),
        SecurityScheme::MutualTls(MutualTlsSecurityScheme { description: None }),
    )]))
    .with_security_requirements(vec![SecurityRequirement {
        schemes: HashMap::from([("mtls".to_owned(), StringList { list: vec![] })]),
    }]);
let handler = RequestHandlerBuilder::new(MyAgent)
    .with_agent_card(card)
    .with_interceptor(by_name)
    .build()?;
# let _ = handler;
# Ok(())
# }
```

The handshake verifies the certificate; the interceptor only names it. A
verifier that also admits clients without one
(`allow_unauthenticated()`, `client_auth_optional(true)`) leaves the refusal
to the interceptor, so the same listener can serve the agent card to anyone.
Behind a proxy that terminates TLS, the listener sees the proxy's handshake,
not the client's, and there is no client certificate to read.
