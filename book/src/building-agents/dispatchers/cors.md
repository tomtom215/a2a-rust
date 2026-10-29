<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# CORS

A browser refuses a cross-origin call unless the server answers with CORS
headers. This page shows how to turn them on for the HTTP dispatchers.

Both dispatchers support CORS for browser-based clients. It is off until
configured: without `with_cors`, no response carries CORS headers — an
`OPTIONS` preflight included — so a browser refuses the cross-origin call.

```rust
# fn card() -> a2a_protocol_types::agent_card::AgentCard {
#     use a2a_protocol_types::agent_card::{AgentCapabilities, AgentCard, AgentInterface, AgentSkill};
#     AgentCard::new("my-agent", "1.0.0", AgentInterface::jsonrpc("http://localhost:3000"))
#         .with_description("An example agent")
#         .with_skill(AgentSkill::new("echo", "Echo", "Echoes text").with_tags(["echo"]))
#         .with_capabilities(AgentCapabilities::none().with_streaming(true))
# }
# use a2a_protocol_sdk::prelude::*;
# struct MyAgent;
# agent_executor!(MyAgent, |_ctx, _queue| async { Ok(()) });
# fn main() -> Result<(), Box<dyn std::error::Error>> {
# let handler = std::sync::Arc::new(RequestHandlerBuilder::new(MyAgent).with_agent_card(card()).build()?);
use a2a_protocol_sdk::server::CorsConfig;

// Answers OPTIONS preflights, and adds the CORS headers to every response.
let dispatcher = JsonRpcDispatcher::new(handler)
    .with_cors(CorsConfig::new("https://my-app.example.com"));
# Ok(())
# }
```
