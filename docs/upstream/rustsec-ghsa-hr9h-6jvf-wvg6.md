<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# RustSec advisory for GHSA-hr9h-6jvf-wvg6

**Target:** a pull request to [`rustsec/advisory-db`](https://github.com/rustsec/advisory-db)
adding `crates/a2a-protocol-server/RUSTSEC-0000-0000.md` (the maintainers
assign the number; `0000-0000` is the placeholder that project's template
uses).
**Status:** draft, not sent. The maintainer files it.
**Why:** `cargo audit` and `cargo deny` read the RustSec database, not GitHub
advisories on their own. Until this is filed, a project on
`a2a-protocol-server` 0.7.0 to 0.14.0 gets no warning from either tool, only
from GitHub's dependency alerts.

## What was checked, and what was not

Checked in this tree on 2026-10-24:

- The fix commit `5f0bbd08180aeb656015f1d9492cd81c660478e4` exists
  (`git cat-file -t` prints `commit`; author date 2026-09-26; subject "fix(server):
  take the path tenant from the URL, never a request header").
- `PathSegmentTenantResolver` is defined at
  `crates/a2a-protocol-server/src/tenant_resolver.rs:286`.
- The description below restates `CHANGELOG.md` `[0.14.1]` `### Security`; the
  affected-release nuance (0.3.0 to 0.6.0 exposed only application code that
  called the resolver itself) is that entry's, not new.

Not checked in this tree, taken from the maintainer's brief and the GitHub
advisory as published: the severity (high), the CVSS 3.1 vector and score
(8.1), CWE-639, the published date 2026-09-30, and the affected range
`>= 0.3.0, < 0.14.1` for both `a2a-protocol-server` and `a2a-protocol-sdk`.
The `v0.14.1` tag is not in this clone (no tags are fetched), so the patched
version rests on the changelog heading `## [0.14.1] - 2026-09-30` and on
crates.io, which was not queried.

## Decisions for the maintainer

1. **One crate or two.** The GitHub advisory lists `a2a-protocol-server` and
   `a2a-protocol-sdk`. RustSec files an advisory against the crate that holds
   the vulnerable code, here `a2a-protocol-server`, and `cargo audit` matches
   by the package name and version in `Cargo.lock`, so a project on the SDK
   facade also resolves a vulnerable server and is flagged through it. That
   matching rule is how `cargo-audit` is documented to work; it was not run
   against this advisory. If the maintainers prefer, a second file
   `crates/a2a-protocol-sdk/RUSTSEC-0000-0000.md` costs nothing.
2. **Category.** RustSec's categories are a closed list (`code-execution`,
   `crypto-failure`, `denial-of-service`, `file-disclosure`,
   `format-injection`, `memory-corruption`, `memory-exposure`,
   `privilege-escalation`). `privilege-escalation` is the closest to
   CWE-639 here (a caller acts as another tenant); there is no
   authorization-bypass category.
3. **`unaffected`.** The GitHub range starts at 0.3.0, so `< 0.3.0` is listed
   as unaffected. Whether crates.io holds any release below 0.3.0 was not
   checked; the line is harmless if none does.

## The file

````markdown
```toml
[advisory]
id = "RUSTSEC-0000-0000"
package = "a2a-protocol-server"
date = "2026-09-30"
url = "https://github.com/tomtom215/a2a-rust/security/advisories/GHSA-hr9h-6jvf-wvg6"
references = [
    "https://github.com/tomtom215/a2a-rust/commit/5f0bbd08180aeb656015f1d9492cd81c660478e4",
]
categories = ["privilege-escalation"]
keywords = ["multi-tenancy", "tenant-isolation", "authorization", "a2a"]
aliases = ["GHSA-hr9h-6jvf-wvg6"]
cvss = "CVSS:3.1/AV:N/AC:L/PR:L/UI:N/S:U/C:H/I:H/A:N"

[versions]
patched = [">= 0.14.1"]
unaffected = ["< 0.3.0"]
```

# `PathSegmentTenantResolver` lets a caller choose its own tenant

`PathSegmentTenantResolver` took the tenant from the request's `:path`
pseudo-header and, when that was absent, from an ordinary request header named
`path`. Only the WebSocket dispatcher set `:path`. On every other entry point
(JSON-RPC, HTTP+JSON, gRPC as a metadata entry, and the Axum adapter's
`A2aRouter`) the tenant therefore came from the `path` header, which any
client can send.

A server built with `.with_tenant_resolver(PathSegmentTenantResolver::new(..))`
ran a request carrying `path: /tenants/<other>/x` as tenant `<other>`, with
that tenant's tasks and push-notification configurations to read, list,
cancel, subscribe to and write. Path-based tenancy isolates tenants only when
something in front of the agent authorizes each caller against the URL; the
header defeated that control, so a caller admitted to one tenant's URLs could
act as another.

Affected: `a2a-protocol-server` 0.7.0 through 0.14.0, the releases in which the
request handler calls the configured resolver. 0.3.0 through 0.6.0 had the
same fallback, but only application code that called the resolver itself was
exposed.

Not affected: the WebSocket binding, `HeaderTenantResolver`,
`BearerTokenTenantResolver`, custom resolvers that do not read a `path`
header, and servers with no resolver.

Fixed in 0.14.1: the JSON-RPC and HTTP+JSON dispatchers record the request
URI's path under `:path` and the resolver reads `:path` only. The gRPC and
`A2aRouter` entry points pass no URL, so there the resolver now resolves no
tenant (the request is served from the default tenant, or rejected under
`require_resolved_tenant`); a deployment on those entry points should switch to
`BearerTokenTenantResolver`, or to `HeaderTenantResolver` behind a proxy that
sets the header and strips any inbound copy.

Workaround before upgrading: strip the inbound `path` header (for gRPC, the
`path` metadata key) at a proxy in front of the agent, or change resolver as
above.
````

## Filing

Fork `rustsec/advisory-db`, add the file above at
`crates/a2a-protocol-server/RUSTSEC-0000-0000.md`, open the pull request, and
leave the ID to the maintainers. Their CI lints the front matter; fix what it
reports.
