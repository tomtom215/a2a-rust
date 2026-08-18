#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
#
# Proves every CI gate can fail.
#
# A gate that cannot fail reports success. This repository has learned that
# the expensive way more than once: a mutation workflow that concluded
# "COMBINED MUTATION SCORE: 100%" from empty result files, a conformance job
# that exited green on a report with zero graded requirements, a preflight
# script that had never been told two of its jobs existed. In each case the
# gate ran, went green, and measured nothing.
#
# Running a gate and watching it pass says nothing about whether it *could*
# have failed. This script asserts the other half: for each gate, confirm it
# is green to begin with, break something that gate is responsible for,
# confirm it goes red *citing that break*, and put it back.
#
# Three ways a gate fails to be proven, all findings:
#
#   UNPROVEN      it stayed green through its own injected defect
#   INCONCLUSIVE  it went red, but its output never mentions the defect — so
#                 it died of something else and proves nothing
#   PRE-BROKEN    it was already failing before anything was injected
#
# The last of those was added on 2026-08-11, after this script reported the
# `doc` gate PROVEN while that gate had been red for hours on an unrelated
# broken intra-doc link: the injected link simply added one more error to a log
# that already had two. Until then the header above claimed a green baseline
# was confirmed and no code confirmed it, so "39 of 39 proven" meant less than
# it said. It costs a second run of every gate, which is what the guarantee is
# worth.
#
# The feature-gated injections are deliberately placed inside code compiled
# only under that feature. `cargo test --features sqlite` failing because of a
# defect in always-compiled code would prove nothing about whether the sqlite
# code is covered at all.
#
# Usage:
#   scripts/prove_gates_fail.sh              # every gate (slow: full rebuilds)
#   scripts/prove_gates_fail.sh --only fmt   # gates whose command matches a regex
#   scripts/prove_gates_fail.sh --list       # show the gate/injection pairing
#
# Exit codes: 0 every gate proven, 1 one or more UNPROVEN, 2 configuration
# drift (a gate with no registered injection, or vice versa).

set -Eeuo pipefail

REPO_ROOT=$(git rev-parse --show-toplevel)
cd "$REPO_ROOT"
CI_YML="$REPO_ROOT/.github/workflows/ci.yml"

# The ci.yml parser and the job/step lists, shared with scripts/preflight.sh
# instead of restated here. The comment this replaces said keeping the two
# copies identical "is the point" — which is better served by there being one.
# shellcheck source=lib/ci_gates.sh
. "$REPO_ROOT/scripts/lib/ci_gates.sh"
OTEL_RS="$REPO_ROOT/crates/a2a-protocol-server/src/otel/mod.rs"
LOG_DIR=$(mktemp -d "${TMPDIR:-/tmp}/a2a-provegates.XXXXXX")

ONLY='.'
LIST_ONLY=0
while [ $# -gt 0 ]; do
    case "$1" in
        --only) ONLY="$2"; shift 2 ;;
        --list) LIST_ONLY=1; shift ;;
        *) printf 'unknown argument %s\n' "$1" >&2; exit 2 ;;
    esac
done



# Every SKIP_STEPS alternative must still name a step that exists. An exemption
# for a step that has been renamed or deleted is an exemption that silently
# covers nothing, which is the same defect as an unlisted gate — this file has
# now had that shape three times (test-postgres and package unknown to
# preflight, slimrpc-binding unknown to both, deny and semver listed but empty).
# A folded block scalar (`run: >`) joins its lines with spaces; a literal one
# (`run: |`) keeps the newlines. `gates_for_jobs` treats both alike, so for `>`
# it would emit a *different command than CI runs*: YAML reads
# `run: >` / `echo one` / `two` as `echo one two`, and this parser would make
# `two` a command of its own. ci.yml uses only `|` today, so this refuses
# rather than implementing a fold nobody needs. A gate that runs the wrong
# command is worse than one that is missing, and refusing means a folded block
# cannot be introduced without someone deciding what it should mean.


# Each command carries the environment its ci.yml step runs under — the job's
# `env:` block then the step's, so a step-level value wins. Same parser as
# scripts/preflight.sh, and for the same reason it was added there on
# 2026-08-11: without it this script does not run the gate CI runs.
#
# Two gates proved that the hard way once the baseline check landed. The
# postgres suite reported PRE-BROKEN because `A2A_TEST_POSTGRES_URL` was unset
# and all 15 tests failed; and `cargo run -p incident-response --release`
# *hung the entire sweep*, because without `INCIDENT_EXIT_WHEN_DONE` the demo
# finishes its five acts and then parks on Ctrl+C waiting for a human. Neither
# was visible before: only the injected run happened, and it exits before
# reaching the park.

# ── Injections ───────────────────────────────────────────────────────────────
#
# Each is a shell function that writes a defect, and a matching one that undoes
# it. `revert_all` restores every touched file from git, so an interrupted run
# cannot leave the tree dirty.

TOUCHED=()

# Set by an injection that must run somewhere other than the repository root
# (only `package`, which cannot be tested in a dirty tree). Reset by
# `revert_all`, so it cannot leak into the next gate.
INJECT_WORKDIR=""
PACKAGE_CLONE=""

note_touched() { TOUCHED+=("$1"); }

revert_all() {
    if [ "${#TOUCHED[@]}" -gt 0 ]; then
        # `git checkout HEAD --`, not `git checkout --`. The latter restores
        # from the *index*, so if anything stages a file while a defect is
        # injected, every later revert faithfully restores the defect.
        #
        # That is not hypothetical. During the first full run of this script a
        # concurrent `git add -A && git commit` in the same working tree staged
        # the injected modules, and from that point the revert put them back:
        # `cargo test --workspace` then failed on a duplicate module rather
        # than on the injected panic, and the probe code reached three
        # commits. Restoring from HEAD makes the revert independent of
        # whatever the index happens to hold.
        #
        # No `|| true`: a revert that fails silently leaves a defect in the
        # tree, and every subsequent verdict is then about the wrong code.
        if ! git checkout HEAD -- "${TOUCHED[@]}" 2>&1; then
            printf '\nprove_gates_fail: FAILED TO REVERT %s\n' "${TOUCHED[*]}" >&2
            printf 'The working tree still contains an injected defect. Restore it by\n' >&2
            printf 'hand before doing anything else — do not commit.\n' >&2
        fi
        TOUCHED=()
    fi
    rm -f crates/a2a-protocol-types/src/gate_probe_long.rs
    git rm -q --cached --ignore-unmatch crates/a2a-protocol-types/src/gate_probe_long.rs 2>/dev/null || true
    if [ -n "$PACKAGE_CLONE" ]; then
        rm -rf "$PACKAGE_CLONE"
        PACKAGE_CLONE=""
    fi
    INJECT_WORKDIR=""
}
trap revert_all EXIT

# Refuse to start in a dirty tree.
#
# Every injection is "append, run, restore from HEAD", so an uncommitted change
# to a file this script touches would be destroyed by the first revert. It also
# makes the run ambiguous: a gate failing on someone else's work in progress
# proves nothing about the injected defect.
if [ -n "$(git status --porcelain)" ]; then
    printf 'prove_gates_fail: the working tree has uncommitted changes.\n\n' >&2
    git status --short >&2
    cat >&2 <<'EOF'

This script injects defects and restores files from HEAD, which would discard
the changes above. Commit or stash them first.

And while it runs, do not commit from this tree: a `git add` that catches an
injected defect stages it, which is how probe code reached three commits the
first time this ran.
EOF
    exit 2
fi

# Appends a line to a file, immediately after the anchor line.
inject_after() {
    local file="$1" anchor="$2" payload="$3"
    note_touched "$file"
    python3 - "$file" "$anchor" "$payload" <<'PY'
import sys, pathlib
path, anchor, payload = sys.argv[1], sys.argv[2], sys.argv[3]
p = pathlib.Path(path)
lines = p.read_text().splitlines(keepends=True)
for i, line in enumerate(lines):
    if anchor in line:
        lines.insert(i + 1, payload + "\n")
        p.write_text("".join(lines))
        sys.exit(0)
sys.exit(f"anchor {anchor!r} not found in {path}")
PY
}

# A failing test guarded by `feature`, so only a gate that compiles that
# feature can see it.
inject_failing_test() {
    local file="$1" feature="$2"
    note_touched "$file"
    {
        printf '\n#[cfg(all(test, feature = "%s"))]\n' "$feature"
        printf 'mod gate_probe_%s {\n' "$(printf '%s' "$feature" | tr -c 'a-z0-9' '_')"
        printf '    #[test]\n'
        printf '    fn gate_probe_must_fail() {\n'
        printf '        panic!("gate probe: injected failure for feature %s");\n' "$feature"
        printf '    }\n}\n'
    } >>"$file"
}

# A clippy denial guarded by `feature`. `let x = *&y;` trips
# clippy::deref_addrof, which is warn-by-default and denied by `-D warnings`.
inject_clippy_lint() {
    local file="$1" feature="$2" suffix="$3"
    note_touched "$file"
    {
        printf '\n#[cfg(feature = "%s")]\n' "$feature"
        printf '#[allow(dead_code)]\n'
        printf 'fn gate_probe_%s() -> u8 {\n' "$suffix"
        printf '    let y: u8 = 1;\n'
        printf '    *&y\n'
        printf '}\n'
    } >>"$file"
}

# The same, unconditional — for gates that compile everything anyway.
inject_clippy_lint_always() {
    local file="$1" suffix="$2"
    note_touched "$file"
    {
        printf '\n#[allow(dead_code)]\n'
        printf 'fn gate_probe_%s() -> u8 {\n' "$suffix"
        printf '    let y: u8 = 1;\n'
        printf '    *&y\n'
        printf '}\n'
    } >>"$file"
}

# A failing test that only an `--ignored` selection runs, in a named test
# file. `#[tokio::test]` because every suite these target is async.
inject_ignored_test() {
    local file="$1" where="$2"
    note_touched "$file"
    {
        printf '\n#[tokio::test]\n#[ignore = "gate probe"]\n'
        printf 'async fn gate_probe_must_fail() {\n'
        printf '    panic!("gate probe: injected failure in %s");\n' "$where"
        printf '}\n'
    } >>"$file"
}

inject_failing_test_always() {
    local file="$1" suffix="$2"
    note_touched "$file"
    {
        printf '\n#[cfg(test)]\n'
        printf 'mod gate_probe_%s {\n' "$suffix"
        printf '    #[test]\n'
        printf '    fn gate_probe_must_fail() {\n'
        printf '        panic!("gate probe: injected failure");\n'
        printf '    }\n}\n'
    } >>"$file"
}

TYPES_LIB=crates/a2a-protocol-types/src/lib.rs
CLIENT_LIB=crates/a2a-protocol-client/src/lib.rs
SERVER_LIB=crates/a2a-protocol-server/src/lib.rs

# The SLIMRPC binding is a separate cargo project with its own lockfile, so its
# gates are reached by the `cd` the parser now emits and its defects have to be
# injected into its own sources — a probe in crates/ is invisible to a build
# rooted in bindings/.
SLIMRPC_DIR=bindings/a2a-protocol-slimrpc
SLIMRPC_LIB=$SLIMRPC_DIR/src/lib.rs
SLIMRPC_BIN=$SLIMRPC_DIR/src/bin/slim_node.rs
SLIMRPC_TOML=$SLIMRPC_DIR/Cargo.toml
SLIMRPC_SPIFFE=$SLIMRPC_DIR/tests/spiffe.rs
MULTI_REPLICA=crates/a2a-protocol-server/tests/multi_replica.rs

# Maps a gate command to the injection that must break it. Matched by
# substring against the full command, longest match wins, so
# `--features postgres --test postgres_store_tests` cannot be captured by the
# plain `--features postgres` entry.
injection_for() {
    local cmd="$1" prefix="${2-}"
    # Routed on the working directory first, because several of the binding's
    # commands are within a word of a workspace one: its clippy gate is `cargo
    # clippy --all-targets -- -D warnings` and the workspace one is the same
    # plus `--workspace`. Matching on command text alone would eventually send
    # a bindings/ gate an injection written for crates/, and the gate would
    # report UNPROVEN while being perfectly capable of failing. The directory
    # is the thing that actually tells them apart.
    case "$prefix" in
        *"cd $SLIMRPC_DIR "*)
            case "$cmd" in
                # Before the generic `cargo test` arm: this gate runs only
                # `--ignored` tests in three named test files, so a panicking
                # unit test in lib.rs would never be selected and the gate
                # would pass with the defect sitting in the tree.
                *"--test spiffe"*) echo "spiffe_ignored:$SLIMRPC_SPIFFE" ;;
                "cargo fmt"*)     echo "fmt:$SLIMRPC_LIB" ;;
                "cargo clippy"*)  echo "clippy_always:$SLIMRPC_LIB" ;;
                "cargo build"*)   echo "build_bin:$SLIMRPC_BIN" ;;
                "cargo package"*) echo "package_manifest:$SLIMRPC_TOML" ;;
                "cargo test"*)    echo "test_always:$SLIMRPC_LIB" ;;
                *)                echo "" ;;
            esac
            return ;;
    esac
    case "$cmd" in
        "cargo fmt --all -- --check")
            echo "fmt:$TYPES_LIB" ;;
        "./scripts/check_proto_copies.sh")
            echo "proto" ;;
        "./scripts/check_file_lengths.sh")
            echo "file_length" ;;
        "./scripts/check_mutation_scope.sh")
            echo "mutation_scope" ;;
        "./scripts/check_benchmark_prose.sh")
            echo "benchmark_prose" ;;
        "./scripts/check_book_code.sh")
            echo "book_code" ;;
        *"check_api_reference.py"*)
            echo "api_reference" ;;
        *"check_otel_metrics_coverage.py"*)
            echo "otel_coverage" ;;
        *"check_package_excludes.py"*)
            echo "package_excludes" ;;
        *"prove_workflow_gates_fail.py"*)
            echo "workflow_gates" ;;
        *"check_block_scalars.py"*)
            echo "block_scalars:scripts/lib/ci_gates.sh" ;;
        *"--test postgres_store_tests"*)
            echo "postgres_ignored" ;;
        *"--test multi_replica"*)
            echo "ignored_suite:$MULTI_REPLICA:the multi-replica suite" ;;
        "cargo doc"*)
            echo "doc" ;;
        "cargo package"*)
            echo "package" ;;
        "cargo run -p agent-team"*)
            echo "dogfood" ;;
        # Before the general incident-response arm: `-- harden` runs Act 5
        # alone, and its defect is a hardening one, not a matrix one.
        "cargo run -p incident-response"*"harden"*)
            echo "example_hardening" ;;
        "cargo run -p echo-agent"*|"cargo run -p incident-response"*|\
        "cargo run -p genai-a2a-agent"*|"cargo run -p rig-a2a-agent"*|\
        "cargo run -p multi-lang-team"*)
            echo "example_surface" ;;
        "cargo clippy"*"--all-features"*)
            echo "clippy_always:$SERVER_LIB" ;;
        "cargo clippy"*"--features signing"*)
            echo "clippy:$TYPES_LIB:signing" ;;
        "cargo clippy"*"--features tracing"*)
            echo "clippy:$CLIENT_LIB:tracing" ;;
        "cargo clippy"*"--features tls-rustls"*)
            echo "clippy:$CLIENT_LIB:tls-rustls" ;;
        "cargo clippy"*"--features sqlite"*)
            echo "clippy:$SERVER_LIB:sqlite" ;;
        "cargo clippy"*"--features postgres"*)
            echo "clippy:$SERVER_LIB:postgres" ;;
        "cargo clippy"*"--features axum"*)
            echo "clippy:$SERVER_LIB:axum" ;;
        "cargo clippy"*"--features websocket"*)
            echo "clippy:$CLIENT_LIB:websocket" ;;
        "cargo clippy"*"--features grpc"*)
            echo "clippy:$CLIENT_LIB:grpc" ;;
        "cargo clippy"*"--features auth-jwt"*)
            echo "clippy:$SERVER_LIB:auth-jwt" ;;
        "cargo clippy"*)
            echo "clippy_always:$TYPES_LIB" ;;
        "cargo test"*"--all-features"*)
            echo "test_always:$SERVER_LIB" ;;
        "cargo test"*"--no-default-features"*)
            echo "test_always:$TYPES_LIB" ;;
        "cargo test"*"--features signing"*)
            echo "test:$TYPES_LIB:signing" ;;
        "cargo test"*"--features tracing"*)
            echo "test:$CLIENT_LIB:tracing" ;;
        "cargo test -p a2a-protocol-client --features tls-rustls")
            echo "test:$CLIENT_LIB:tls-rustls" ;;
        "cargo test -p a2a-protocol-server --features tls-rustls")
            echo "test:$SERVER_LIB:tls-rustls" ;;
        "cargo test"*"--features sqlite"*)
            echo "test:$SERVER_LIB:sqlite" ;;
        "cargo test"*"--features postgres"*)
            echo "test:$SERVER_LIB:postgres" ;;
        "cargo test"*"--features axum"*)
            echo "test:$SERVER_LIB:axum" ;;
        "cargo test"*"--features websocket"*)
            echo "test:$CLIENT_LIB:websocket" ;;
        "cargo test"*"--features grpc"*)
            echo "test:$CLIENT_LIB:grpc" ;;
        "cargo test"*"--features auth-jwt,tls-rustls"*)
            echo "test:$SERVER_LIB:auth-jwt" ;;
        "cargo test"*"--features auth-jwt"*)
            echo "test:$SERVER_LIB:auth-jwt" ;;
        "cargo test --workspace")
            echo "test_always:$TYPES_LIB" ;;
        *)
            echo "" ;;
    esac
}

# The string the gate's own output must contain for its failure to count as
# proof. Chosen to be something only *this* defect can produce, so a gate that
# died of ENOSPC, a lock timeout, or an unrelated compile error is reported
# INCONCLUSIVE instead of PROVEN.
expected_marker() {
    local kind=${1%%:*}
    case "$kind" in
        fmt)              echo "gate_probe_fmt" ;;
        proto)            echo "DRIFT    tck/proto/a2a_v1/a2a.proto" ;;
        file_length)      echo "gate_probe_long.rs" ;;
        mutation_scope)   echo "MUTATION SCOPE GAP" ;;
        benchmark_prose)  echo "DRIFT" ;;
        book_code)        echo "GREW" ;;
        api_reference)    echo "are not defined in crates/" ;;
        otel_coverage)    echo "the bundled exporter drops" ;;
        package_excludes) echo "not excluded" ;;
        workflow_gates)   echo "UNPROVEN" ;;
        block_scalars)    echo "MISMATCH" ;;
        doc)              echo "NoSuchItemAnywhere" ;;
        package)          echo "NO_SUCH_README.md" ;;
        package_manifest) echo "NO_SUCH_README.md" ;;
        build_bin)        echo "GateProbeNoSuchType" ;;
        dogfood)          echo "CLAIM TABLE DRIFT" ;;
        example_surface)  echo "matrix cell(s) never ran" ;;
        example_hardening) echo "partitions leak" ;;
        postgres_ignored) echo "gate probe: injected failure in the ignored postgres suite" ;;
        ignored_suite)    echo "gate probe: injected failure in ${1##*:}" ;;
        spiffe_ignored)   echo "gate probe: injected failure in the SPIFFE suite" ;;
        clippy|clippy_always) echo "deref_addrof" ;;
        test|test_always) echo "gate probe: injected failure" ;;
        *)                echo "__no_marker_defined__" ;;
    esac
}

apply_injection() {
    local spec="$1"
    local kind=${spec%%:*}
    local rest=${spec#*:}
    case "$kind" in
        fmt)
            note_touched "$rest"
            printf '\n#[allow(dead_code)]\nfn   gate_probe_fmt(  )->u8{1}\n' >>"$rest" ;;
        # `cargo build` denies nothing, so a lint probe would compile and the
        # gate would go green. It takes a genuine compile error, and one in the
        # binary target rather than the library: `--bin slim-node` is what the
        # step names.
        build_bin)
            note_touched "$rest"
            {
                printf '\n#[allow(dead_code)]\n'
                printf 'fn gate_probe_build() {\n'
                printf '    let _x: GateProbeNoSuchType = unimplemented!();\n'
                printf '}\n'
            } >>"$rest" ;;
        # The binding packages with `--allow-dirty`, so unlike the workspace
        # `package` gate above this needs no clone — the manifest is edited in
        # place and restored from HEAD like every other injection. It has no
        # `readme` key of its own, so one is inserted rather than repointed;
        # inserting where one already exists is the TOML duplicate-key mistake
        # that arm documents.
        package_manifest)
            note_touched "$rest"
            python3 - "$rest" <<'PY'
import re, sys, pathlib
p = pathlib.Path(sys.argv[1])
s = p.read_text()
if re.search(r'(?m)^readme\s*=', s):
    sys.exit("manifest already declares `readme`; repoint it instead of inserting")
s, n = re.subn(r'(?m)^(name\s*=\s*"a2a-protocol-slimrpc"\s*)$',
               r'\1\nreadme = "NO_SUCH_README.md"', s, count=1)
if n != 1:
    sys.exit("expected exactly one `name` key to anchor to; found %d" % n)
p.write_text(s)
PY
            ;;
        proto)
            note_touched "tck/proto/a2a_v1/a2a.proto"
            printf '\n// gate probe: injected drift\n' >>tck/proto/a2a_v1/a2a.proto ;;
        file_length)
            python3 -c "open('crates/a2a-protocol-types/src/gate_probe_long.rs','w').write('// gate probe\n'*600)"
            git add -N crates/a2a-protocol-types/src/gate_probe_long.rs >/dev/null 2>&1 || true ;;
        book_code)
            # Append an `ignore`d block: the exact move that would defeat the
            # book-tests crate, since a block nothing compiles cannot fail.
            # Injecting an *uncompilable* block instead would prove nothing —
            # `ignore` means the compiler never sees it, so the ratchet, not
            # the compiler, is what has to notice.
            note_touched "book/src/concepts/streaming.md"
            printf '\n```rust,ignore\nlet _: GateProbeNonexistentType = todo!();\n```\n' \
                >>book/src/concepts/streaming.md
            ;;
        api_reference)
            # Rename a type on the page and leave the code alone — the exact
            # decay this gate exists for. A hand-written listing goes stale the
            # moment something is renamed, and it goes stale silently, in the
            # page a reader trusts precisely because they do not yet know the
            # API well enough to catch it. `sed` rather than a heredoc so this
            # arm stays a one-liner like its neighbours.
            note_touched "book/src/reference/api-reference.md"
            sed -i 's/`TaskVersion`/`TaskRevision`/' \
                book/src/reference/api-reference.md
            ;;
        otel_coverage)
            # Delete one callback override from the exporter. This is the
            # defect verbatim: `on_persistence_error` and `on_push_delivery`
            # shipped wired to every call site and exported by nothing, because
            # a `Metrics` method the impl omits silently inherits the trait's
            # empty default. No compile error, and no failing test either — the
            # exporter's own tests run against a noop meter, which a no-op
            # override satisfies perfectly. `perl -0` rather than a heredoc so
            # this arm stays a one-liner like its neighbours.
            note_touched "$OTEL_RS"
            perl -0pi -e 's/\n    fn on_push_delivery\(.*?\n    \}\n/\n/s' "$OTEL_RS"
            ;;
        package_excludes)
            # Drop one `publish = false` member from ci.yml's exclude list.
            # This is the defect verbatim: `hello-agent`, `deploy-agent` and
            # `a2a-book-tests` each joined the workspace without joining the
            # list, and `cargo package --workspace` fails on the first one it
            # reaches — inside the *release* workflow, after the tag is pushed.
            note_touched ".github/workflows/ci.yml"
            sed -i 's/--exclude a2a-book-tests //' .github/workflows/ci.yml
            ;;
        benchmark_prose)
            # The defect is the historical one, restored verbatim: the
            # connection-reuse sentence as it shipped from v0.5.0 to v0.8.0,
            # while the tables beside it said 312.5 µs vs 189.0 µs.
            #
            # Injecting into the *prose* rather than the tables is deliberate.
            # Breaking a table would also break the sentence derived from it,
            # so the gate would go red without proving it can tell the two
            # apart — and prose drifting away from correct tables is the
            # failure that actually happened.
            note_touched "book/src/reference/benchmarks.md"
            python3 - <<'PY'
import pathlib
p = pathlib.Path("book/src/reference/benchmarks.md")
s = p.read_text()
s = s.replace(
    "Connection reuse saves 123.5 µs (39.5%) on loopback",
    "Connection reuse saves ~140µs (9%) on loopback",
)
p.write_text(s)
PY
            ;;
        mutation_scope)
            # The defect is the historical one, restored verbatim: drop the
            # `:(glob)` prefix from the mutation gate's scoping pathspec and it
            # silently stops matching every file directly under a crate's src/.
            #
            # An injection that added a source file instead would prove nothing
            # — the fixed pathspec matches new files fine, and the bug was never
            # about which files exist. It was about which files the pattern can
            # reach, so the pattern is what has to be broken.
            note_touched ".github/workflows/mutants.yml"
            python3 - <<'PY'
import pathlib, sys
p = pathlib.Path(".github/workflows/mutants.yml")
s = p.read_text()
old = "-- ':(glob)crates/*/src/**/*.rs' > pr-src.diff"
new = "-- 'crates/*/src/**/*.rs' > pr-src.diff"
if s.count(old) != 1:
    sys.exit(f"expected exactly one mutation scoping pathspec; found {s.count(old)}")
p.write_text(s.replace(old, new))
PY
            ;;
        workflow_gates)
            # This gate is itself a prover, so the defect has to be a broken
            # *gate* rather than broken code — and the most faithful one is the
            # real defect it was written for: strip `set -o pipefail` from the
            # official-TCK conformance gate and the step's exit status reverts
            # to tee's, i.e. always 0.
            #
            # The marker is "UNPROVEN", so this passes only if the prover names
            # that step as unproven. A prover that crashed, found no gates, or
            # died on a missing dependency exits non-zero too, and every one of
            # those would otherwise read as success here.
            note_touched ".github/workflows/official-tck.yml"
            python3 - <<'PY'
import pathlib, sys
p = pathlib.Path(".github/workflows/official-tck.yml")
s = p.read_text()
# Anchored on the pairing, not on `set -o pipefail` alone: the suite-run steps
# in the same file carry that line too, and removing one of those would prove
# something else entirely.
old = '          set -o pipefail\n          python3 tck/scripts/check_conformance.py \\\n'
new = '          python3 tck/scripts/check_conformance.py \\\n'
if s.count(old) != 1:
    sys.exit(f"expected exactly one conformance-gate pipefail pairing; found {s.count(old)}")
p.write_text(s.replace(old, new))
PY
            ;;
        doc)
            note_touched "$TYPES_LIB"
            printf '\n/// Gate probe: [`NoSuchItemAnywhere`] is not a real path.\n#[allow(dead_code)]\npub fn gate_probe_doc() {}\n' >>"$TYPES_LIB" ;;
        example_surface)
            # Stop recording one method, without breaking any call.
            #
            # A failing call would prove only that the examples propagate
            # errors. What this gate adds is the *completeness* check: the
            # matrix must catch a method that quietly stopped being driven
            # while everything still returned success. Dropping the
            # `ListTasks` recording is exactly that shape — every call still
            # succeeds, and the run must still go red.
            #
            # Injected in the shared harness so both examples are affected,
            # which also demonstrates that the two jobs read the same scorer.
            note_touched "examples/harness/src/sweep.rs"
            python3 - <<'PY2'
p = "examples/harness/src/sweep.rs"
s = open(p).read()
needle = 'Ok(resp) => ok!(Method::ListTasks, format!("{} task(s)", resp.tasks.len())),'
if s.count(needle) != 1:
    raise SystemExit(
        f"gate probe: expected exactly one anchor in {p}; found {s.count(needle)}"
    )
s = s.replace(needle, "Ok(resp) => { let _ = resp; }")
open(p, "w").write(s)
PY2
            ;;
        example_hardening)
            # Remove the tenant resolver, which is the exact regression Act 5's
            # isolation check was written for: with no resolver the handler
            # trusts the client's `params.tenant` verbatim, so any caller reads
            # or writes any partition by naming it.
            #
            # This is a defect that leaves every call succeeding — the demo's
            # first four acts stay green, every request returns 200, and only
            # the isolation check notices. The marker is "partitions leak", the
            # message that check emits when one tenant can see another's task;
            # a build error, a bind failure or a hung run all exit non-zero too,
            # and every one of those would otherwise read as proof.
            note_touched "examples/incident-response/src/hardening/tenancy.rs"
            python3 - <<'PY3'
p = "examples/incident-response/src/hardening/tenancy.rs"
s = open(p).read()
needle = "        .with_tenant_resolver(HeaderTenantResolver::default())\n"
if s.count(needle) != 1:
    raise SystemExit(
        f"gate probe: expected exactly one anchor in {p}; found {s.count(needle)}"
    )
# `HeaderTenantResolver` becomes unused, and the example is built with
# warnings allowed here, so the import can stay.
s = s.replace(needle, "        // gate probe: tenant resolver removed\n")
s = s.replace(
    "use a2a_protocol_server::tenant_resolver::HeaderTenantResolver;\n",
    "#[allow(unused_imports)]\nuse a2a_protocol_server::tenant_resolver::HeaderTenantResolver;\n",
)
open(p, "w").write(s)
PY3
            ;;
        dogfood)
            # Inject claim-table drift rather than a failing assertion.
            #
            # A test failure would prove only that `if failed > 0` still
            # exits 1, which was never the broken part. What *was* broken is
            # that the "SDK FEATURES EXERCISED" summary was a hardcoded list
            # printed as `[x]` with no link to the results — it stayed green
            # through fifteen failing tests. The drift check is the guard that
            # replaced it, so the drift check is what has to be proven.
            #
            # Naming a test that does not exist is the cheapest defect that
            # reaches it: it compiles, every test still passes, and the run
            # must still exit non-zero because the table now describes
            # something the suite does not contain.
            note_touched "examples/agent-team/src/features.rs"
            python3 - <<'PY'
p = "examples/agent-team/src/features.rs"
s = open(p).read()
needle = 'c("CancellationToken checking", &["cancel-task"]),'
if s.count(needle) != 1:
    raise SystemExit(
        f"gate probe: expected exactly one anchor in {p}; found {s.count(needle)}"
    )
s = s.replace(
    needle,
    'c("CancellationToken checking", &["cancel-task", "gate-probe-no-such-test"]),',
)
open(p, "w").write(s)
PY
            ;;
        package)
            # `cargo package` refuses outright on a dirty working tree:
            #
            #   error: 1 files in the working directory contain changes that
            #   were not yet committed into git
            #
            # So no in-place injection can reach it. Any edit produces that
            # same generic error, which proves nothing about packaging — it
            # was reported INCONCLUSIVE for exactly this reason before this
            # comment existed.
            #
            # The defect therefore has to be committed, and it must not be
            # committed here. A `--shared` clone gets an isolated tree with
            # its own history, running the gate's command verbatim; only the
            # directory differs. CARGO_TARGET_DIR is inherited so the clone
            # reuses the main build cache instead of compiling the world.
            PACKAGE_CLONE=$(mktemp -d "${TMPDIR:-/tmp}/a2a-pkgprobe.XXXXXX")
            git clone -q --shared "$REPO_ROOT" "$PACKAGE_CLONE/repo"
            # A `readme` pointing at a file that is not in the package. Only
            # the packaging gate reads the manifest's file references, so a
            # compile error here would prove nothing about what
            # `cargo package` adds over `cargo build`.
            python3 - "$PACKAGE_CLONE/repo/crates/a2a-protocol-types/Cargo.toml" <<'PY'
import re, sys, pathlib
p = pathlib.Path(sys.argv[1])
s = p.read_text()
# Repoint the existing key. Two earlier spellings were wrong in instructive
# ways: appending to the end of the file puts a bare key in whatever table
# happens to be last, and inserting one under [package] collides with the
# `readme` already declared there — cargo then reports a TOML duplicate-key
# error, not a missing file, and the gate fails for the wrong reason.
s, n = re.subn(r'(?m)^readme(\s*)=.*$', 'readme = "NO_SUCH_README.md"', s, count=1)
if n != 1:
    sys.exit("expected exactly one `readme` key in the manifest; found %d" % n)
p.write_text(s)
PY
            git -C "$PACKAGE_CLONE/repo" -c user.name=probe -c user.email=probe@invalid \
                commit -q -am "probe: dangling readme reference"
            INJECT_WORKDIR="$PACKAGE_CLONE/repo" ;;
        # Break folding itself, not the checker: this gate exists to notice a
        # parser regression, so the defect has to be one. Widening the fold
        # separator to two spaces is invisible to every other gate and turns
        # every folded case in the comparison red.
        block_scalars)
            note_touched "$rest"
            python3 - "$rest" <<'PY'
import re, sys, pathlib
p = pathlib.Path(sys.argv[1])
s = p.read_text()
s, n = re.subn(r'out = out " " L', 'out = out "  " L', s, count=1)
if n != 1:
    sys.exit("expected exactly one fold-space assignment; found %d" % n)
p.write_text(s)
PY
            ;;
        postgres_ignored)
            # This gate runs only `#[ignore]`d tests in one file, so the
            # defect has to be an ignored test in that file. A failure
            # anywhere else would never be selected.
            note_touched "crates/a2a-protocol-server/tests/postgres_store_tests.rs"
            {
                printf '\n#[tokio::test]\n#[ignore = "gate probe"]\n'
                printf 'async fn gate_probe_must_fail() {\n'
                printf '    panic!("gate probe: injected failure in the ignored postgres suite");\n'
                printf '}\n'
            } >>crates/a2a-protocol-server/tests/postgres_store_tests.rs ;;
        # Both of these gates select tests by file *and* by `--ignored`, so the
        # probe has to be an ignored test in one of the files they name. The
        # always-compiled probe every other test gate uses would be filtered
        # out before it ran, and the gate would go green with the defect in
        # place — proving nothing, in the script whose whole purpose is
        # noticing that.
        ignored_suite)
            local file=${rest%%:*}
            local where=${rest#*:}
            inject_ignored_test "$file" "$where" ;;
        spiffe_ignored)
            inject_ignored_test "$rest" "the SPIFFE suite" ;;
        clippy)
            local file=${rest%%:*}
            local feature=${rest#*:}
            inject_clippy_lint "$file" "$feature" "$(printf '%s' "$feature" | tr -c 'a-z0-9' '_')" ;;
        clippy_always)
            inject_clippy_lint_always "$rest" "always" ;;
        test)
            local file=${rest%%:*}
            local feature=${rest#*:}
            inject_failing_test "$file" "$feature" ;;
        test_always)
            inject_failing_test_always "$rest" "always" ;;
        "test")
            : ;;
        *)
            printf 'no injection implemented for spec %s\n' "$spec" >&2
            return 2 ;;
    esac
}

# ── Drift guard ──────────────────────────────────────────────────────────────

require_known_skips
mapfile -t ALL_GATES < <(gates_for_jobs "$GATE_JOBS")

if [ "${#ALL_GATES[@]}" -eq 0 ]; then
    printf 'prove_gates_fail: parsed no gates from %s — the parser is broken.\n' "$CI_YML" >&2
    exit 2
fi

unregistered=()
for entry in "${ALL_GATES[@]}"; do
    cmd=${entry#*$'\t'}
    spec=$(injection_for "$cmd" "${entry%%$'\t'*}")
    if [ -z "$spec" ]; then
        unregistered+=("$cmd")
    fi
done

if [ "${#unregistered[@]}" -gt 0 ]; then
    printf '\nprove_gates_fail: %d CI gate(s) have no registered defect injection:\n\n' \
        "${#unregistered[@]}" >&2
    printf '    %s\n' "${unregistered[@]}" >&2
    cat >&2 <<'EOF'

Every gate must have something that proves it can fail. A gate nobody has
tried to break is a gate nobody knows works. Add an injection to
`injection_for` above, or explain in this script why the gate is exempt.
EOF
    exit 2
fi

if [ "$LIST_ONLY" -eq 1 ]; then
    printf 'Gate / injection pairing (%d gates):\n\n' "${#ALL_GATES[@]}"
    for entry in "${ALL_GATES[@]}"; do
        cmd=${entry#*$'\t'}
        printf '  %-12s %s\n' "[$(injection_for "$cmd" "${entry%%$'\t'*}" | cut -d: -f1)]" "${entry%%$'\t'*}${cmd}"
    done
    exit 0
fi

# ── Environment parity with CI ───────────────────────────────────────────────
#
# Read from ci.yml's top-level `env:` block rather than restated here, for the
# reason scripts/preflight.sh gives: matching the commands but not the
# environment is not parity. This script learned that the hard way — it set
# RUSTFLAGS and CARGO_PROFILE_DEV_DEBUG by hand and missed
# `RUSTDOCFLAGS: -D warnings`, so `cargo doc` came back UNPROVEN against a
# broken intra-doc link that CI does reject. The gate was fine; the harness
# was not.
apply_ci_env() {
    local line key value
    while IFS= read -r line; do
        key=${line%%=*}
        value=${line#*=}
        if [ -z "${!key-}" ]; then
            export "$key=$value"
        fi
    done < <(awk '
        /^env:[[:space:]]*$/ { in_env = 1; next }
        /^[^[:space:]#]/     { in_env = 0 }
        in_env && /^  [A-Za-z_][A-Za-z0-9_]*:/ {
            line = $0
            sub(/^  /, "", line)
            key = line; sub(/:.*$/, "", key)
            val = line; sub(/^[^:]*:[[:space:]]*/, "", val)
            gsub(/^"|"$/, "", val)
            print key "=" val
        }
    ' "$CI_YML")
}

apply_ci_env

# Incremental state off, and not to match CI (which does not set it either
# way) — for disk. This sweep compiles the workspace under a dozen distinct
# feature permutations, and each keeps its own incremental artifacts:
# `target/debug/incremental` reached 13 GB partway through a run and filled
# the device, after which gates failed on ENOSPC instead of on their injected
# defect. Those failures are caught as INCONCLUSIVE now, but a sweep that
# cannot finish is not much better than one that lies.
export CARGO_INCREMENTAL="${CARGO_INCREMENTAL:-0}"

# So the `package` gate's isolated clone reuses this tree's build cache
# instead of compiling every dependency from scratch.
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$REPO_ROOT/target}"

for required in RUSTFLAGS RUSTDOCFLAGS; do
    if [ -z "${!required-}" ]; then
        printf 'prove_gates_fail: %s is unset — ci.yml no longer exports it, or the\n' \
            "$required" >&2
        printf 'env parser above stopped matching. Gates would run under a laxer\n' >&2
        printf 'environment than CI, so their verdicts would not transfer.\n' >&2
        exit 2
    fi
done

# ── Run ──────────────────────────────────────────────────────────────────────

PROVEN=0
UNPROVEN=0
SKIPPED=0
RESULTS=()

run_quiet() {
    local cmd="$1" log="$2" status=0
    # `|| status=$?`, not `if ...; then return 0; fi; return $?`. After an
    # `if` whose branch is not taken, bash sets `$?` to 0, so that spelling
    # reports every failing gate as exit 0 — which is precisely the
    # status-swallowing defect this script exists to catch, in the script
    # that catches it. It was written that way first, and every gate came
    # back UNPROVEN until this line changed.
    (cd "${INJECT_WORKDIR:-$REPO_ROOT}" && eval "$cmd") >"$log" 2>&1 || status=$?
    return "$status"
}

idx=0
for entry in "${ALL_GATES[@]}"; do
    idx=$((idx + 1))
    gate_env=${entry%%$'\t'*}
    cmd=${entry#*$'\t'}
    # The gate as CI runs it: env prefix plus command.
    full="${gate_env}${cmd}"
    if ! printf '%s' "$cmd" | grep -Eq -- "$ONLY"; then
        SKIPPED=$((SKIPPED + 1))
        continue
    fi
    spec=$(injection_for "$cmd" "${entry%%$'\t'*}")
    printf '\n\033[1m[%d/%d] %s\033[0m\n' "$idx" "${#ALL_GATES[@]}" "$full"
    printf '        injection: %s\n' "$spec"

    revert_all

    # Baseline: the gate must pass with nothing injected.
    #
    # Without this the script cannot tell "went red because of the defect"
    # from "was already red", and the header's promise to "put it back and
    # confirm it goes green again" was never kept in code. That gap is not
    # theoretical: on 2026-08-11 CI's `doc` gate had been failing for hours on
    # a broken intra-doc link, the injected link added one more error to the
    # same log, the marker matched, and the gate was reported PROVEN. A gate
    # that can never pass is not proven able to fail — it is just failing.
    #
    # Run first rather than after the injection: a broken gate is reported
    # without paying for an injected run that could not mean anything.
    log="$LOG_DIR/gate-$idx-baseline.log"
    start=$SECONDS
    # `|| baseline_status=$?`, for the reason `run_quiet` documents above: after
    # an `if !` whose branch *is* taken, bash has already consumed the status
    # and `$?` reads 0. Written that way first, and the PRE-BROKEN verdict duly
    # reported "gate exited 0 with nothing injected" — a status-swallowing bug
    # inside the check for status-swallowing bugs.
    baseline_status=0
    run_quiet "$full" "$log" || baseline_status=$?
    if [ "$baseline_status" -ne 0 ]; then
        elapsed=$((SECONDS - start))
        printf '  \033[31mPRE-BROKEN\033[0m  gate exited %s with nothing injected — it is\n' \
            "$baseline_status"
        printf '                already failing, so no injection can prove anything\n'
        printf '                about it (%ss)\n' "$elapsed"
        printf '                log: %s\n' "$log"
        UNPROVEN=$((UNPROVEN + 1))
        RESULTS+=("PRE-BROKEN|$baseline_status|${elapsed}s|$full")
        continue
    fi
    baseline_elapsed=$((SECONDS - start))

    apply_injection "$spec"

    log="$LOG_DIR/gate-$idx-broken.log"
    start=$SECONDS
    if run_quiet "$full" "$log"; then
        broken_status=0
    else
        broken_status=$?
    fi
    elapsed=$((SECONDS - start))
    revert_all

    if [ "$broken_status" -ne 0 ]; then
        # A non-zero exit is necessary and not sufficient. A gate that dies
        # because the disk filled, a lock timed out, or a dependency failed
        # to resolve also exits non-zero, and counting that as proof is the
        # same error this script exists to find — one level up. The first
        # full run of this script did exactly that: the target directory
        # filled during the sweep, nine `cargo test` gates failed in 0s on
        # ENOSPC, and every one was reported PROVEN.
        #
        # So the log has to show the gate reacting to *this* defect.
        marker=$(expected_marker "$spec")
        if grep -qF -- "$marker" "$log"; then
            printf '  \033[32mPROVEN\033[0m  gate exited %s citing the injected defect (%ss)\n' \
                "$broken_status" "$elapsed"
            PROVEN=$((PROVEN + 1))
            RESULTS+=("PROVEN|$broken_status|${elapsed}s|$full")
        else
            printf '  \033[31mINCONCLUSIVE\033[0m  gate exited %s but its output never mentions\n' \
                "$broken_status"
            printf '                the injected defect (%s) — it failed for some other\n' "$marker"
            printf '                reason, which proves nothing (%ss)\n' "$elapsed"
            printf '                log: %s\n' "$log"
            UNPROVEN=$((UNPROVEN + 1))
            RESULTS+=("INCONCLUSIVE|$broken_status|${elapsed}s|$full")
        fi
    else
        printf '  \033[31mUNPROVEN\033[0m  gate exited 0 WITH the defect present (%ss)\n' "$elapsed"
        printf '            log: %s\n' "$log"
        UNPROVEN=$((UNPROVEN + 1))
        RESULTS+=("UNPROVEN|0|${elapsed}s|$full")
    fi
done

printf '\n\033[1m── prove_gates_fail summary ──\033[0m\n'
for row in "${RESULTS[@]-}"; do
    [ -n "$row" ] || continue
    verdict=${row%%|*}
    code=${row#*|}; code=${code%%|*}
    timing=${row#*|*|}; timing=${timing%%|*}
    cmd=${row#*|*|*|}
    if [ "$verdict" = PROVEN ]; then
        printf '  \033[32m%-12s\033[0m exit %-3s %6s  %s\n' "$verdict" "$code" "$timing" "$cmd"
    else
        printf '  \033[31m%-12s\033[0m exit %-3s %6s  %s\n' "$verdict" "$code" "$timing" "$cmd"
    fi
done
printf '\n  %d proven, %d unproven, %d not selected (of %d gates)\n' \
    "$PROVEN" "$UNPROVEN" "$SKIPPED" "${#ALL_GATES[@]}"

# The tree must be exactly as it was found. A script that injects defects and
# leaves one behind is worse than no script: the next commit carries it.
revert_all
if [ -n "$(git status --porcelain)" ]; then
    printf '\nprove_gates_fail: the tree is dirty after the run — an injection was\n' >&2
    printf 'not reverted. Inspect and restore before committing:\n\n' >&2
    git status --short >&2
    exit 2
fi

[ "$UNPROVEN" -eq 0 ] || exit 1
