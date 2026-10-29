# Task & Config Stores

a2a-rust uses pluggable storage backends for tasks and push notification configs. The built-in in-memory stores work for development and testing. For production, use the SQLite or PostgreSQL stores that ship with the crate (features `sqlite` / `postgres`), or implement the traits for your own database.

## Which store

| Store | Use it for |
|---|---|
| [`InMemoryTaskStore`](./stores/in-memory.md), the default | Development, tests, and one process whose tasks need not survive a restart. Terminal tasks expire after an hour. |
| [`SqliteTaskStore`](./stores/sqlite.md), feature `sqlite` | One process, or processes on one host sharing the file, whose tasks must survive restarts. Tasks stay until you delete them. |
| [`PostgresTaskStore`](./stores/postgres.md), feature `postgres` | Replicas on several hosts sharing one store. Tasks stay until you delete them. |
| [`TenantAware…` variants](./stores/tenants.md) | Multi-tenant servers, and any server behind an authenticating interceptor: one partition per tenant, and per caller. |
| [Your own](./stores/custom.md) | A database the SDK does not ship a store for. |

Push-notification configs have their own store with the same choices; see
[Push-config stores](./stores/push-config-stores.md).

## In this chapter

- [In-memory task store](./stores/in-memory.md): limits, eviction and
  `TaskStoreConfig`.
- [SQLite task store](./stores/sqlite.md) and
  [PostgreSQL task store](./stores/postgres.md): setup, migrations, the
  connection pool, and what happens when the database is briefly unavailable.
- [Tenant-aware stores](./stores/tenants.md): partitions per tenant and per
  caller, and why an authenticating interceptor needs them.
- [Push-config stores](./stores/push-config-stores.md): the
  `PushConfigStore` trait and the in-memory store.
- [Implementing a store](./stores/custom.md): the `TaskStore` trait, errors,
  pagination and concurrency.
- [Retention, erasure and audit](./stores/retention.md): purging old tasks and
  erasing a caller's data.
- [Stores shared by replicas](./stores/replicas.md): terminal states, log
  positions and turn claims.

## Next steps

- **[Production Hardening](../deployment/production.md)** — Deployment checklist
- **[Configuration Reference](../reference/configuration.md)** — Every builder and store default
