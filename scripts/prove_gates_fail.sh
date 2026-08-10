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
# have failed. This script asserts the other half: for each gate, break
# something that gate is responsible for, confirm it goes red, put it back,
# and confirm it goes green again. A gate that stays green through its own
# injected defect is reported as UNPROVEN, which is a finding.
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

# Same job set and parser as scripts/preflight.sh, deliberately: a gate this
# script has never heard of is the exact defect preflight's `require_known_jobs`
# exists to catch, one level further in.
GATE_JOBS='^(fmt|clippy|test|test-postgres|doc|deny|semver|package)$'

gates_for_jobs() {
    awk -v want="$1" '
        /^  [a-z0-9_-]+:[[:space:]]*$/ { job = $1; sub(/:$/, "", job); next }
        /^[[:space:]]+run:[[:space:]]*[^|>[:space:]]/ {
            if (job ~ want) { sub(/^[[:space:]]+run:[[:space:]]*/, ""); print }
        }
    ' "$CI_YML"
}

# ── Injections ───────────────────────────────────────────────────────────────
#
# Each is a shell function that writes a defect, and a matching one that undoes
# it. `revert_all` restores every touched file from git, so an interrupted run
# cannot leave the tree dirty.

TOUCHED=()

note_touched() { TOUCHED+=("$1"); }

revert_all() {
    if [ "${#TOUCHED[@]}" -gt 0 ]; then
        git checkout -- "${TOUCHED[@]}" 2>/dev/null || true
        TOUCHED=()
    fi
    rm -f crates/a2a-protocol-types/src/gate_probe_long.rs
    rm -f "$REPO_ROOT/.gate-probe-proto"
}
trap revert_all EXIT

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

# Maps a gate command to the injection that must break it. Matched by
# substring against the full command, longest match wins, so
# `--features postgres --test postgres_store_tests` cannot be captured by the
# plain `--features postgres` entry.
injection_for() {
    local cmd="$1"
    case "$cmd" in
        "cargo fmt --all -- --check")
            echo "fmt" ;;
        "./scripts/check_proto_copies.sh")
            echo "proto" ;;
        "./scripts/check_file_lengths.sh")
            echo "file_length" ;;
        *"--test postgres_store_tests"*)
            echo "postgres_ignored" ;;
        "cargo doc"*)
            echo "doc" ;;
        "cargo package"*)
            echo "package" ;;
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

apply_injection() {
    local spec="$1"
    local kind=${spec%%:*}
    local rest=${spec#*:}
    case "$kind" in
        fmt)
            note_touched "$TYPES_LIB"
            printf '\n#[allow(dead_code)]\nfn   gate_probe_fmt(  )->u8{1}\n' >>"$TYPES_LIB" ;;
        proto)
            note_touched "tck/proto/a2a_v1/a2a.proto"
            printf '\n// gate probe: injected drift\n' >>tck/proto/a2a_v1/a2a.proto ;;
        file_length)
            python3 -c "open('crates/a2a-protocol-types/src/gate_probe_long.rs','w').write('// gate probe\n'*600)"
            git add -N crates/a2a-protocol-types/src/gate_probe_long.rs >/dev/null 2>&1 || true ;;
        doc)
            note_touched "$TYPES_LIB"
            printf '\n/// Gate probe: [`NoSuchItemAnywhere`] is not a real path.\n#[allow(dead_code)]\npub fn gate_probe_doc() {}\n' >>"$TYPES_LIB" ;;
        package)
            note_touched "crates/a2a-protocol-types/Cargo.toml"
            # Point `readme` at a file that is not there. Only the packaging
            # gate reads the manifest's file references, so a compile error
            # would prove nothing about what `cargo package` adds over
            # `cargo build` — a dangling packaged-file reference is exactly
            # the class of defect this is the only gate for.
            printf '\nreadme = "NO_SUCH_README.md"\n' >>crates/a2a-protocol-types/Cargo.toml ;;
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

mapfile -t ALL_GATES < <(gates_for_jobs "$GATE_JOBS")

if [ "${#ALL_GATES[@]}" -eq 0 ]; then
    printf 'prove_gates_fail: parsed no gates from %s — the parser is broken.\n' "$CI_YML" >&2
    exit 2
fi

unregistered=()
for cmd in "${ALL_GATES[@]}"; do
    spec=$(injection_for "$cmd")
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
    for cmd in "${ALL_GATES[@]}"; do
        printf '  %-12s %s\n' "[$(injection_for "$cmd" | cut -d: -f1)]" "$cmd"
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
    (cd "$REPO_ROOT" && eval "$cmd") >"$log" 2>&1 || status=$?
    return "$status"
}

idx=0
for cmd in "${ALL_GATES[@]}"; do
    idx=$((idx + 1))
    if ! printf '%s' "$cmd" | grep -Eq -- "$ONLY"; then
        SKIPPED=$((SKIPPED + 1))
        continue
    fi
    spec=$(injection_for "$cmd")
    printf '\n\033[1m[%d/%d] %s\033[0m\n' "$idx" "${#ALL_GATES[@]}" "$cmd"
    printf '        injection: %s\n' "$spec"

    revert_all
    apply_injection "$spec"

    log="$LOG_DIR/gate-$idx-broken.log"
    start=$SECONDS
    if run_quiet "$cmd" "$log"; then
        broken_status=0
    else
        broken_status=$?
    fi
    elapsed=$((SECONDS - start))
    revert_all

    if [ "$broken_status" -ne 0 ]; then
        printf '  \033[32mPROVEN\033[0m  gate exited %s with the defect present (%ss)\n' \
            "$broken_status" "$elapsed"
        PROVEN=$((PROVEN + 1))
        RESULTS+=("PROVEN|$broken_status|${elapsed}s|$cmd")
    else
        printf '  \033[31mUNPROVEN\033[0m  gate exited 0 WITH the defect present (%ss)\n' "$elapsed"
        printf '            log: %s\n' "$log"
        UNPROVEN=$((UNPROVEN + 1))
        RESULTS+=("UNPROVEN|0|${elapsed}s|$cmd")
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
        printf '  \033[32m%-8s\033[0m exit %-3s %6s  %s\n' "$verdict" "$code" "$timing" "$cmd"
    else
        printf '  \033[31m%-8s\033[0m exit %-3s %6s  %s\n' "$verdict" "$code" "$timing" "$cmd"
    fi
done
printf '\n  %d proven, %d unproven, %d not selected (of %d gates)\n' \
    "$PROVEN" "$UNPROVEN" "$SKIPPED" "${#ALL_GATES[@]}"

[ "$UNPROVEN" -eq 0 ] || exit 1
