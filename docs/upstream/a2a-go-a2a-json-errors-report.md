<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Issue draft for `a2aproject/a2a-go` — HTTP+JSON errors labelled `application/a2a+json` lose their identity

**Status: DRAFT, held — not filed.** Written 2026-10-15 against a2a-go
v2.6.0, the latest release that day (`go list -m -versions
github.com/a2aproject/a2a-go/v2`), read from the module cache. A search of
the v2.6.0 Go sources for `a2a+json` finds nothing. Not checked: a2a-go's
`main` branch and its open issues. Ledger entry §11.1 (line 2750);
`crates/a2a-protocol-server/src/dispatch/rest/media_type.rs` rule 3.

---

Title: REST client drops error details when the server labels the error `application/a2a+json`

A2A v1.0.1 §11.1 (HTTP+JSON/REST Protocol Binding): "Content-Type:
`application/a2a+json` SHOULD be used for requests and responses".

`internal/rest/rest.go`, `FromRESTError` (v2.6.0, line 275):

```go
contentType := resp.Header.Get("Content-Type")
if !strings.HasPrefix(contentType, "application/json") {
    return a2a.ErrServerError
}
```

So a server that follows §11.1 and labels an error response
`application/a2a+json` has every error turned into `ErrServerError`: a
missing task is no longer `ErrTaskNotFound`, and the `google.rpc.Status`
details are not read.

The client's requests make this hard for a server to avoid.
`a2aclient/rest.go` (v2.6.0, lines 111-118) sends `Accept: application/json`
on a unary call, which a server can honour, but `Accept: text/event-stream`
and nothing else on `SendStreamingMessage` and `SubscribeToTask`. A refused
stream is answered with a JSON error body, and the request named no JSON
type, so a server following §11.1 picks `application/a2a+json`, and the
client cannot read it. `SubscribeToTask` sends no body either, so the server
has no request `Content-Type` to mirror.

Measured with a2a-rust's server on 2026-09-25 (its commit `dfc69ed2`,
reverted in `cf2a7e96`): labelling every HTTP+JSON response
`application/a2a+json` cost a2a-go the identity of every HTTP+JSON error.
a2a-rust now answers a request that asks for an event stream and names no
JSON type with `application/json`, for this reason only.

Suggested change, either or both:

1. `FromRESTError` accepts `application/a2a+json` as well as
   `application/json` (compare the media type's essence, case-insensitively,
   RFC 9110 §8.3.1).
2. Streaming requests send
   `Accept: text/event-stream, application/a2a+json, application/json;q=0.9`,
   so the server knows which error type the client reads.

The server side has the same gap in the other direction: `a2asrv/rest.go`
labels every response `application/json` (lines 63 and 493).
