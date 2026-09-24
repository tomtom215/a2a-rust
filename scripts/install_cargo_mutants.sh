#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
#
# Installs the cargo-mutants the mutation gate runs, and the cargo-nextest it
# runs tests with, at pinned versions — then proves the installed cargo-mutants
# carries this repository's patch.
#
# Why a patched build. Stock cargo-mutants 27.1.0 makes no *viable* "replace
# the function body" mutant for two return-type shapes this codebase is full
# of, so those bodies were never graded (audit N9, N11):
#
#   - a `Result` alias not spelled `Result` (`ClientResult<T>`, `A2aResult<T>`):
#     it recognises a `Result` only by that exact last path segment, so it
#     emits `ClientResult::new()`-style replacements that never compile — 184
#     functions, measured 2026-09-23;
#   - `Pin<Box<dyn Future<Output = T> + ..>>`, the return type of every
#     object-safe async trait method (`TaskStore`, `ServerInterceptor`, …): it
#     has no case for it at all — 162 functions.
#
# `scripts/cargo-mutants/27.1.0-result-aliases-and-boxed-futures.patch` adds
# both cases to `src/fnvalue.rs`, and teaches `src/visit.rs` that a boxed
# future already yielding the replacement (`Box::pin(async { Ok(()) })`, an
# `after` hook's whole body) is the function unchanged, not a mutant — the
# stock text comparison missed it for differing only in `async move` or `()`.
# Unit tests for each, in cargo-mutants' own style.
# It is meant to go upstream; when a cargo-mutants release carries it, pin that
# release here, delete the patch, and keep the self-test below.
#
# Why not `cargo install cargo-mutants`: that installs whatever is newest, so
# the version the documents cite is an accident of when they were written
# (audit N4), and it cannot carry a patch. Why not a fork: a fork is a second
# repository to trust and keep current; a checksum-pinned crates.io tarball and
# a patch of under 200 lines in this repository are both reviewed here.
#
# Usage:
#   scripts/install_cargo_mutants.sh              install both, then self-test
#   scripts/install_cargo_mutants.sh --self-test BIN
#                                                 only check BIN for the patch
#
# Installs where `cargo install` does (`CARGO_INSTALL_ROOT`, else
# `CARGO_HOME`, else ~/.cargo). Exit 0 on success; non-zero, naming the step,
# on a checksum mismatch, a patch that does not apply exactly, a failed build,
# or a binary without the patch.

set -euo pipefail

CARGO_MUTANTS_VERSION=27.1.0
# The crates.io index's `cksum` for cargo-mutants 27.1.0: the SHA-256 of the
# `.crate` file. Checked 2026-09-23 against https://index.crates.io/ca/rg/cargo-mutants.
CARGO_MUTANTS_SHA256=07072e7bcdeb425d5e5fdbfd9f15a2c749e23cb2edf5ef40aee5876760ae1cf9
CARGO_NEXTEST_VERSION=0.9.146

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PATCH="$REPO_ROOT/scripts/cargo-mutants/${CARGO_MUTANTS_VERSION}-result-aliases-and-boxed-futures.patch"

# Lists the mutants of a small fixture and requires the replacements only the
# patched build makes (stock 27.1.0 lists neither), and no mutant of a boxed
# future whose body already is the replacement.
self_test() {
    local bin="$1" dir listing
    dir="$(mktemp -d)"
    mkdir -p "$dir/src"
    cat > "$dir/Cargo.toml" <<'TOML'
[package]
name = "cargo-mutants-self-test"
version = "0.0.0"
edition = "2021"
publish = false

[workspace]
TOML
    cat > "$dir/src/lib.rs" <<'RUST'
use std::future::Future;
use std::pin::Pin;

pub type ProbeResult<T> = Result<T, String>;

pub fn alias() -> ProbeResult<bool> {
    Ok(true)
}

pub fn boxed<'a>() -> Pin<Box<dyn Future<Output = bool> + Send + 'a>> {
    Box::pin(async { true })
}

pub fn idle<'a>() -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
    Box::pin(async {})
}
RUST
    # `--colors=never`: the listing is matched as text below, and CI sets
    # CARGO_TERM_COLOR=always, which cargo-mutants honours — the replacements
    # then arrive wrapped in escape codes and no plain match can find them.
    listing="$("$bin" mutants --list --colors=never --dir "$dir" 2>&1)" || {
        printf 'install_cargo_mutants: %s mutants --list failed:\n%s\n' "$bin" "$listing" >&2
        rm -rf "$dir"
        return 1
    }
    rm -rf "$dir"
    local missing=0
    if ! grep -qF 'replace alias -> ProbeResult<bool> with Ok(false)' <<<"$listing"; then
        printf 'install_cargo_mutants: %s makes no Ok(..) replacement for a Result alias (N9)\n' "$bin" >&2
        missing=1
    fi
    if ! grep -qF 'with Box::pin(async move{false})' <<<"$listing"; then
        printf 'install_cargo_mutants: %s makes no body replacement for a boxed future (N11)\n' "$bin" >&2
        missing=1
    fi
    if grep -qF 'replace idle ' <<<"$listing"; then
        printf 'install_cargo_mutants: %s mutates a boxed future into itself\n' "$bin" >&2
        missing=1
    fi
    if [ "$missing" -ne 0 ]; then
        printf 'The binary is not the patched build this gate requires. Its listing was:\n%s\n' "$listing" >&2
        return 1
    fi
    printf 'install_cargo_mutants: %s carries the patch\n' "$bin"
}

if [ "${1:-}" = "--self-test" ]; then
    [ $# -eq 2 ] || { echo "usage: $0 --self-test BIN" >&2; exit 2; }
    self_test "$2"
    exit $?
fi
[ $# -eq 0 ] || { echo "usage: $0 [--self-test BIN]" >&2; exit 2; }

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

crate="$work/cargo-mutants-${CARGO_MUTANTS_VERSION}.crate"
curl --proto '=https' --tlsv1.2 -sSfL -o "$crate" \
    "https://static.crates.io/crates/cargo-mutants/cargo-mutants-${CARGO_MUTANTS_VERSION}.crate"
if ! printf '%s  %s\n' "$CARGO_MUTANTS_SHA256" "$crate" | sha256sum --check --quiet; then
    echo "install_cargo_mutants: cargo-mutants-${CARGO_MUTANTS_VERSION}.crate does not match the pinned SHA-256" >&2
    exit 1
fi

tar -xzf "$crate" -C "$work"
src="$work/cargo-mutants-${CARGO_MUTANTS_VERSION}"
# `--fuzz=0`: the patch must apply exactly, or it is not the reviewed change.
if ! patch -d "$src" -p1 --forward --fuzz=0 --no-backup-if-mismatch --quiet < "$PATCH"; then
    echo "install_cargo_mutants: $PATCH does not apply exactly to cargo-mutants ${CARGO_MUTANTS_VERSION}" >&2
    exit 1
fi

cargo install --locked --force --path "$src"
cargo install --locked cargo-nextest --version "$CARGO_NEXTEST_VERSION"

bin_dir="${CARGO_INSTALL_ROOT:-${CARGO_HOME:-$HOME/.cargo}}/bin"
self_test "$bin_dir/cargo-mutants"
