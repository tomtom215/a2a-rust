<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# HTTP+JSON (REST)

The HTTP+JSON binding (`protocolBinding: "HTTP+JSON"`, often called REST)
maps each A2A operation to an HTTP method and a resource path. This page covers
the route table, where a tenant can appear, content negotiation and the
dispatcher's built-in request checks. To serve it, see
[Dispatchers](../../building-agents/dispatchers/rest.md).

## Routes

The REST transport uses standard HTTP methods and URL paths:

| Operation | Method | Path |
|-----------|--------|------|
| Send message | `POST` | `/message:send` |
| Stream message | `POST` | `/message:stream` |
| Get task | `GET` | `/tasks/{id}` |
| List tasks | `GET` | `/tasks` |
| Cancel task | `POST` | `/tasks/{id}:cancel` |
| Subscribe | `GET\|POST` | `/tasks/{id}:subscribe` |
| Create push config | `POST` | `/tasks/{id}/pushNotificationConfigs` |
| Get push config | `GET` | `/tasks/{id}/pushNotificationConfigs/{configId}` |
| List push configs | `GET` | `/tasks/{id}/pushNotificationConfigs` |
| Delete push config | `DELETE` | `/tasks/{id}/pushNotificationConfigs/{configId}` |
| Extended card | `GET` | `/extendedAgentCard` |
| Agent card | `GET` | `/.well-known/agent-card.json` |

## Multi-tenant paths

The spec's proto binds every method twice: a primary pattern with no tenant in
the path (`/tasks/{id}`) and an `additional_bindings` pattern with the tenant
as the leading segment (`/{tenant}/tasks/{id}`). This SDK's client sends the
prefix form — the same one the official Python SDK's REST client sends, and
the only one the official Python server reads — with the segment
percent-encoded, and keeps the field in POST bodies as well.

The server accepts the tenant from every place the proto lets it arrive, in
this order of precedence:

1. the path prefix — `/{tenant}/…` (canonical) or this SDK's explicit
   `/tenants/{tenant-id}/…` form, percent-decoded;
2. the `?tenant=` query parameter on `GET` and `DELETE` (§11.5's transcoding
   of the primary pattern);
3. the `tenant` field of a `POST` body (`message:send`, `message:stream`,
   `tasks/{id}:cancel`, `tasks/{id}:subscribe`, push-config create).

A path tenant is injected into a POST body that omits it, so
`POST /acme/message:send` with no `tenant` in the JSON lands in `acme`. When
the path and the body disagree the path wins, as a path variable does under
`google.api.http`.

## Content types

The REST dispatcher accepts both `application/json` and `application/a2a+json`.
A `POST` with any other `Content-Type` is refused with 400.

JSON-RPC responses are `application/json`, as §9.1 requires. HTTP+JSON
responses are negotiated, following §11.1's SHOULD of `application/a2a+json`
without breaking clients that cannot read it:

- an `Accept` header that names `application/a2a+json` with a `q` at least as
  high as `application/json`'s gets `application/a2a+json`;
- an `Accept` that prefers `application/json` gets `application/json`;
- otherwise, a request body labelled `application/a2a+json` gets that type;
- otherwise, a request whose `Accept` asks for `text/event-stream` gets
  `application/json`;
- anything else, including no `Accept` at all, gets `application/a2a+json`.

Successes and errors are labelled alike. The agent card (a §8.2 well-known
resource, not an HTTP+JSON operation) and `/health` and `/ready` keep their
own `application/json`. The event-stream rule is for released a2a-go
(v2.6.0): its streaming calls send `Accept: text/event-stream` and nothing
else, and it decodes an error body only under `application/json`, so a
refused Go stream would otherwise lose its error. Its unary calls send
`Accept: application/json`.

## Security

The REST dispatcher includes built-in protections:

- **Path traversal rejection** — a `.` or `..` path segment, raw or percent-encoded at any depth (`%2E%2E`, `%252E%252E`; a `\` counts as a separator), returns 400; an id that merely contains `..` (`v1..prod`) is served
- **Query string limits** — Query strings over 4 KiB return 414
- **Body size limits** — Request bodies over 4 MiB return 413
