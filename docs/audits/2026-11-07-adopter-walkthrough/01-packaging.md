<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# 1. Packaging as an adopter gets it

Scripts: `scripts/live/adopter/01-package.sh`, `tarball_audit.py`,
`tarball_scan.py`, `mkproject.py`, `03-book-sweep.sh`. Raw output:
`evidence/01-*.log`, `evidence/01-*.txt`.

## What was run

1. `cargo package -p a2a-protocol-types -p a2a-protocol-client -p a2a-protocol-server -p a2a-protocol-sdk --allow-dirty`
   (cargo 1.99.1, `RUSTFLAGS=-D warnings`, a clean `~/.cargo/registry/src` so no stale
   copy of an unpublished crate could be verified instead of the tree; the handoff's
   warning). One invocation, so cargo packaged in dependency order
   (types, server, client, sdk) and **verified every tarball by building it from its own
   contents** against the tarballs just written. `--no-verify` was not needed.
   Result: all four packaged and verified (exit 0).

   | Crate | Files | Uncompressed | Compressed |
   |---|---|---|---|
   | a2a-protocol-types | 157 | 1.5 MiB | 352 KiB |
   | a2a-protocol-client | 285 | 2.7 MiB | 660 KiB |
   | a2a-protocol-server | 957 | 9.8 MiB | 2.3 MiB |
   | a2a-protocol-sdk | 55 | 449 KiB | 107 KiB |

   The verification build uses each crate's default features only. It does not build
   `grpc`, `proto`, `websocket`, `sqlite` and the rest, which are where a missing `.proto`
   or `build.rs` input would show. Those were covered by step 3 below.
2. The tarballs were unpacked to a scratch directory and **a project outside the
   repository** (`mkproject.py`) was built against them: `[dependencies]` as the README
   gives them, `[patch.crates-io]` pointing at the unpacked packages so that the SDK's
   own `version = "0.16.0"` dependencies on its siblings resolve to the unpacked
   tarballs and not to crates.io (where 0.16.0 does not exist), no `RUSTFLAGS`, no
   lockfile from the repository. That is the README quick start, in `02-readme-and-book.md`.
3. `01b-all-features-check.sh`: a library crate outside the repository depending on the four unpacked tarballs with
   **every feature on** (`grpc`, `grpc-tls`, `websocket`, `sqlite`, `postgres`, `axum`, `signing`, `audit`,
   `tool-loop`, `auth-jwt`, `encryption`, `otlp-json`, `otel-prometheus`, `conformance`, `testing`, `proto`), and `cargo check`:
   **exit 0 in 5 m 35 s** (`evidence/01b-all-features-check.log`; 317 packages locked, 74 build-script directories, so the
   protobuf build scripts ran against the vendored `protoc` from the tarballs' own `build.rs` and `proto/`). A second, smaller
   all-feature set (`grpc`, `websocket`, `sqlite`, `tool-loop`, `axum`, `signing`, `auth-jwt`) also passed on rustc 1.94.0
   (`02c-msrv.sh`). `cargo tree -e normal,build -i openssl-sys` and `-i openssl`, `-i native-tls`, `-i reqwest` on that
   all-features project: none present, which is the README's "no OpenSSL" claim, checked against the tarballs.
4. `tarball_audit.py` and `tarball_scan.py` read the `.crate` files.

## What is in the tarballs that need not be

* **Tests and benches ship** (`tests/`: 113 files in client, 328 in server, 37 in types,
  49 in sdk; `benches/`; the `#[cfg(test)]` modules under `src/`). Conventional for Cargo
  (nothing sets `include`/`exclude`), harmless to a build, and 9.8 MiB uncompressed for the
  server crate. Not a defect. Whether the shipped tests run from an unpacked tarball was not
  checked.
* `Cargo.toml.orig`, `Cargo.lock`, `.cargo_vcs_info.json`: cargo's own.
* `codegen.toml` (35 KiB) in the types crate: the generator's input, not needed to build.
* No private keys other than a test file's deliberately garbage PEM
  (`tests/serve_tls.rs:163`), no tokens, no developer paths other than a fixture string
  (`/home/agent/a2a.db`), no `.env`, no binary blobs (`evidence/01-tarball-scan.txt`).

## What is missing

* **No `LICENSE` and no `NOTICE` file in any of the four tarballs** (finding P-1 in
  `README.md`). The manifests say `license = "Apache-2.0"`, and crates.io shows that, but
  the licence text and the project's `NOTICE` (third-party attributions) are in the
  repository root only; no crate directory contains them, so Cargo packages none.
  Apache-2.0 §4(a) asks a redistributor to give recipients a copy of the licence, and
  Linux-distribution packagers and `cargo-about`-style tools read the file from the
  tarball.
* Nothing a build needs. The protos (`proto/a2a_v1/a2a.proto` and the four
  `google/api/*.proto` files) are in each of the types, client and server tarballs
  (`check_proto_copies.sh` keeps them in step), the vendored `protoc` comes from the
  `protoc-bin-vendored` crate as an optional dependency, and every `include_str!` /
  `include!` / `build.rs` input named in the sources resolves inside its tarball
  (`tarball_audit.py` exit 0).

## Verdict

Pass for "a stranger can build from the tarballs"; one finding (no licence file).
