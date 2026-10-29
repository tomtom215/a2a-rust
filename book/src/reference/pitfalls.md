# Pitfalls & Lessons Learned

A catalog of non-obvious problems met while building this SDK, many of them
found by the [dogfooding passes](../deployment/dogfooding-bugs.md). Each entry
documents a real issue and what to do instead. They apply as much to an agent, a custom store
or a custom transport you write on top of the SDK as to the SDK itself.

| Page | Read it when you are |
|---|---|
| [Serialization and validation](./pitfalls/serialization-and-validation.md) | Defining wire types, writing `serde` attributes, or validating what an executor emits |
| [HTTP, streaming and transports](./pitfalls/http-and-transports.md) | Working with `hyper` bodies, SSE, WebSocket, gRPC, or a REST dispatcher |
| [Async, cancellation and shutdown](./pitfalls/async-and-lifecycle.md) | Writing executors, background tasks, locks, or shutdown paths |
| [Security and resource limits](./pitfalls/security-and-resources.md) | Accepting webhook URLs, bounding stores and caches, or building SQL |
| [Pitfalls for contributors](./pitfalls/contributing.md) | Working in this repository: Cargo workspaces, tests, mutation testing, matching CI |
| [Fixed defects](./pitfalls/fixed.md) | Looking up a defect that has been fixed; kept as history |

## Next Steps

- **[Architecture Decision Records](./adrs.md)** — Design decisions behind these choices
- **[Configuration Reference](./configuration.md)** — All tunable parameters
