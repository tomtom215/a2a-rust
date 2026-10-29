<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Client interceptors

A client interceptor runs around every call an `A2aClient` makes: `before` can
add headers or query parameters, `after` sees a success, and `on_error` sees a
failure. This page shows the trait and how to register one.

## The trait

Client interceptors modify outgoing requests and incoming responses:

```rust
# use std::future::Future;
# use a2a_protocol_sdk::client::{ClientRequest, ClientResponse, ClientResult};
use a2a_protocol_sdk::client::CallInterceptor;

struct RequestIdInterceptor;

impl CallInterceptor for RequestIdInterceptor {
    fn before<'a>(
        &'a self,
        req: &'a mut ClientRequest,
    ) -> impl Future<Output = ClientResult<()>> + Send + 'a {
        async move {
            // Add X-Request-Id header to outgoing requests
            req.extra_headers
                .insert("x-request-id".into(), uuid::Uuid::new_v4().to_string());
            Ok(())
        }
    }

    fn after<'a>(
        &'a self,
        resp: &'a ClientResponse,
    ) -> impl Future<Output = ClientResult<()>> + Send + 'a {
        async move {
            // Log the method that completed
            println!("{} completed", resp.method);
            Ok(())
        }
    }
}
```

`before` can also add to `req.query_params`, which the JSON-RPC and HTTP+JSON
transports append to the request URL; the gRPC and WebSocket transports refuse
a call that carries any, rather than send it without them.

`after` runs only when the call succeeds. To see failures, override
`on_error`, which has a no-op default: it gets the request as `before` left
it (its params have gone to the transport, so they read as `null`) and the
`ClientError`, runs in reverse registration order, and cannot change the
error the caller receives. `BearerAuthInterceptor` uses it to drop a token
the agent answered with `401`, so the next call fetches a new one.

## Adding client interceptors

Retries are a policy on the builder, not an interceptor:

```rust
# use std::future::Future;
# use a2a_protocol_sdk::client::{CallInterceptor, ClientRequest, ClientResponse, ClientResult};
# struct RequestIdInterceptor;
# impl CallInterceptor for RequestIdInterceptor {
#     fn before<'a>(&'a self, _: &'a mut ClientRequest) -> impl Future<Output = ClientResult<()>> + Send + 'a { async { Ok(()) } }
#     fn after<'a>(&'a self, _: &'a ClientResponse) -> impl Future<Output = ClientResult<()>> + Send + 'a { async { Ok(()) } }
# }
use a2a_protocol_sdk::client::{ClientBuilder, RetryPolicy};

let client = ClientBuilder::new("http://agent.example.com")
    .with_interceptor(RequestIdInterceptor)
    .with_retry_policy(RetryPolicy::default().with_max_retries(3))
    .build()
    .unwrap();
```
