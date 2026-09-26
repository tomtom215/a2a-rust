<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

> Appendix to [`deep-dive.md`](deep-dive.md) §1.1. Raw captures are in [`results/deep/params/raw/`](results/deep/params/raw/). The probe sources are in [`harness/sdk-params-matrix/`](harness/sdk-params-matrix/); `/opt/bench/sdks-matrix/` paths refer to the machine the probe ran on.

# GetExtendedAgentCard `params` handling across A2A SDKs (JSON-RPC binding)

Date: 2026-09-26. Every cell below comes from a run (EXECUTED). Nothing was only read from source.
Each server was sent `POST` with `A2A-Version: 1.0` and `Content-Type: application/json`, with bodies
`{"jsonrpc":"2.0","id":N,"method":"GetExtendedAgentCard"[,"params":X]}`. Every response was HTTP 200, so the
cells show only the JSON-RPC outcome. "OK" means the configured *extended* card came back (checked by its distinct name, e.g. `go-EXTENDED`).
Each client was pointed at a Python HTTP stub (`/opt/bench/sdks-matrix/capture.py`) that logs the body it receives and answers with a valid AgentCard result.

| SDK (version tested) | omitted | `null` | `{}` | `[]` | `{"tenant":""}` | own client sends | evidence |
|---|---|---|---|---|---|---|---|
| **a2a-rust** (a2a-protocol-server/-client 0.14.0; harness `agent-rust`) | OK | OK | OK | OK | OK | `"params":null` | EXECUTED (crates.io 0.14.0) |
| **a2a-rs** (a2a-server-lf 0.4.4 / a2a-client-lf 0.2.5; harness `agent-rs`) | **-32700** "invalid params: ... invalid type: null" | **-32700** | OK | **-32700** "invalid type: sequence" | OK | `"params":{}` | EXECUTED (crates.io) |
| **python** (a2a-sdk 1.1.5, PyPI latest) | OK | **-32602** "'NoneType' object is not iterable" | OK | OK | OK | `"params":{}` | EXECUTED (PyPI 1.1.5; git HEAD 0d5473c) |
| **go** (a2a-go/v2 v2.6.0, Go proxy latest) | **-32700** "parse error: unexpected end of JSON input" | OK | OK | **-32602** "cannot unmarshal array" | OK | `"params":{}` | EXECUTED (v2.6.0 = git HEAD ebf17c5) |
| **js** (@a2a-js/sdk 1.2.1, npm latest) | OK | OK | OK | OK | OK | `"params":{}` | EXECUTED (npm 1.2.1; git HEAD 11cbb29) |
| **java** (org.a2aproject.sdk 1.3.2.Final, Maven Central latest; Quarkus 3.39.1 reference-jsonrpc) | OK | OK | OK | **-32600** "Expect message object but got: []" (response has no `id`) | OK | `"params":{"tenant":""}` | EXECUTED (Central 1.3.2.Final; git HEAD c78c472 = 1.3.3.Final-SNAPSHOT) |
| **dotnet** (A2A / A2A.AspNetCore 1.0.0-preview2, NuGet latest; .NET SDK 10.0.401) | **-32602** "Invalid parameters" | **-32602** "Invalid parameters" | OK | **-32602** "'params' field must be an object or null." (echoes `id` as string `"4"`) | OK | `"params":{}` | EXECUTED (NuGet preview2 = commit 87fd448; git HEAD f9392e3 = unreleased 1.0.0-preview3) |

## Error-code correctness (JSON-RPC 2.0 says invalid params → -32602)

- **a2a-rust 0.14.0**: never errors. It also accepts `null` and `[]`, which JSON-RPC 2.0 §4.2 does not allow.
- **a2a-rs**: invalid params come back as **-32700 Parse error**, which is the wrong code. The source is `a2a-server-lf-0.4.4/src/jsonrpc.rs:78`: omitted params become `Value::Null` via `unwrap_or(Value::Null)`. Then `jsonrpc.rs:153-154` parses them for this method, and `jsonrpc.rs:238-243` (`parse_error`) sends `PARSE_ERROR` with the text "invalid params".
- **python**: `null` gives the correct **-32602**. The source is `a2a/server/routes/jsonrpc_dispatcher.py:291-297`: `body.get('params', {})` then `ParseDict` → `InvalidParamsError`. It accepts `[]`.
- **go**: `[]` gives the correct **-32602**. An *omitted* params member gives **-32700**, which is the wrong code for a well-formed request. The cause: `ServerRequest.Params` is `json.RawMessage` (`internal/jsonrpc/jsonrpc.go:221`), so it is empty when omitted. `json.Unmarshal` of the empty value fails at `a2asrv/jsonrpc.go:350-353`. The error is not an `UnmarshalTypeError`, so `handleUnmarshalError` (`jsonrpc.go:368-374`) maps it to ErrParseError. `null` works because unmarshalling `null` into a struct does nothing.
- **js**: never errors for this method. The params validation is skipped on purpose: `src/server/transports/jsonrpc/jsonrpc_transport_handler.ts:88` checks `method !== 'GetExtendedAgentCard'`, and line 196 uses `rpcRequest.params ?? {}`.
- **java**: `[]` gives **-32600 Invalid Request** where -32602 would be correct, and the error response leaves out `id`. It accepts omitted and `null` on purpose: `spec-grpc/.../JSONRPCUtils.java:257-267` (identical in the 1.3.2.Final sources jar) checks `paramsNode != null && !paramsNode.isJsonNull()`.
- **dotnet**: `[]`, `null` and omitted all get **-32602**, the correct code. But it rejects *omitted* params, which JSON-RPC allows. At commit 87fd448, `src/A2A/JsonRpc/JsonRpcRequestConverter.cs:171-187` turns Null/Undefined into a C# `null`, and `src/A2A.AspNetCore/A2AJsonRpcProcessor.cs:73` (`if (parameters == null)`) returns InvalidParams. Git HEAD f9392e3 has the same logic at `A2AJsonRpcProcessor.cs:88`; its diff against preview2 does not change params handling. On `[]` the error response echoes `id` as the string `"4"` rather than the number `4`.

## Setup notes

- **Auth:** only the JS SDK gates a static extended card on auth. `DefaultRequestHandler.getAuthenticatedExtendedAgentCard` returns the *public* card unless `context.user.isAuthenticated`. So the JS server used a `userBuilder` that treats `Authorization: Bearer probe` as authenticated, and the probes sent that header. The returned name `js-EXTENDED` confirms the extended card was served.
- **Java:** configured with `a2a.authorization.required=false`.
- **Other extended cards:** Python used `DefaultRequestHandler(extended_agent_card=...)`. Go used `a2asrv.WithExtendedAgentCard` plus `WithCapabilityChecks`. .NET used an `A2AServer` subclass that overrides `GetExtendedAgentCardAsync`; preview2 has no built-in extended-card option.
- **Tenant:** probe (e) was sent to every server. All of them accepted `{"tenant":""}`.
- **Java client:** the `{"tenant":""}` comes from `Client.getExtendedAgentCard()` with no tenant argument.
- **Rust clients:** tested with tiny binaries built against the pinned crates, in `/opt/bench/sdks-matrix/rustcli`. a2a-rust's `get_extended_agent_card()` sends `Value::Null` unless a tenant is configured: `a2a-protocol-client-0.14.0/src/methods/extended_card.rs`, `tenant_or_default(None).map_or(Value::Null, ...)`.
- **Ports:** the servers ran on 7601/7611 (Rust harness), 7621 (py), 7631 (go), 7641 (js), 7651 (java) and 7661 (.NET). The capture stubs ran on 7691-7697. All started processes were stopped.

## Raw captures (`raw/`)

`<sdk>.server.txt` holds the full request and response for bodies a-e. `<sdk>.client-capture.txt` holds the headers and body each client sent.
The harness scripts are in `/opt/bench/sdks-matrix/` (`probe.sh`, `capture.py`, and the per-SDK directories).

## Conclusion

The a2a-rust client's `"params":null` fails against 3 of the 6 other SDKs' servers: a2a-rs (-32700), Python (-32602) and .NET (-32602). It works against the Go, JS and Java servers and against a2a-rust's own.
The spec's own §9.4.8 example, with params omitted, is rejected by a2a-rs (-32700), Go (-32700) and .NET (-32602). Only a2a-rust, Python, JS and Java accept it, and `"params":{}` is the one form every server accepts.
