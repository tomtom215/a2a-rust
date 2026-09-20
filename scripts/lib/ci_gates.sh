#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
# Shared ci.yml gate extraction. Sourced by scripts/preflight.sh and
# scripts/prove_gates_fail.sh; not executable on its own.
#
# This was duplicated between those two files until 2026-08-18 — 155 lines of
# awk, identical apart from one emit line, held in step by a comment reading
# "same job set and parser as scripts/preflight.sh, deliberately". Four changes
# in a single day had to be made twice, and nothing anywhere would have failed
# if one copy had been missed: the two scripts would simply have disagreed
# about what CI runs, silently, which is the exact class of defect both exist
# to catch. A convention that has to be remembered is not a guard.
#
# The job and step lists live here for the same reason. They were declared
# twice, and `prove_gates_fail.sh` carried a comment explaining that keeping
# them identical "is the point" — a thing better achieved by there being one.
#
# The caller must set CI_YML before sourcing.

# ── Job classification ───────────────────────────────────────────────────────
#
# Every job in ci.yml must be in exactly one of these two. `require_known_jobs`
# refuses to run if one is in neither.
GATE_JOBS='^(static-checks|clippy|features|test|test-postgres|doc|package|dogfood|example-surface|slimrpc-binding)$'

# Why each of these three is not a gate job, because an exemption with no
# recorded reason is the same shape as an oversight:
#
#   nightly  informational. It carries `continue-on-error: true` in ci.yml, so
#            nothing it reports can block a merge; there is no verdict to prove.
#
#   deny     BLOCKS, and is listed here anyway.
#   semver   BLOCKS, and is listed here anyway. ci.yml's own comment above the
#            job says so at length and is correct: neither job carries
#            `continue-on-error`.
#
# The reason the two blocking jobs are here is a parser limit and not a
# judgement that they do not matter: every step in them is a `uses:`, a
# marketplace action, and `gates_for_jobs` extracts `run:` commands. There is
# no command to copy and run, so filing them under GATE_JOBS would (correctly)
# make `require_nonempty_gate_jobs` refuse.
#
# Left at that, "has no `run:` key" silently meant "is not audited by
# anything" — which is how the binding's own `cargo-deny` step, the only thing
# auditing 379 transitive dependencies including a native C crypto build, came
# to be registered nowhere and missed by the unregistered-gate guard.
# `require_registered_actions` below closes that: every `uses:` step in ci.yml
# is either infrastructure or a named exemption carrying a reason, and an
# exemption whose step has gone is a failure like any other stale one.
NON_GATE_JOBS='^(nightly|deny|semver)$'

SKIP_STEPS='^(Install SPIRE|Install cargo-hack)$'

# Actions that prepare a runner and render no verdict about this repository:
# checking out, installing a toolchain, restoring a cache, installing Python.
# None of them can go red *about the code*, so none of them is a gate. Every
# other `uses:` in ci.yml must be registered in ACTION_EXEMPTIONS with a reason.
#
# Each alternative must still match a real `uses:`, for the reason
# `require_known_skips` gives about SKIP_STEPS: a classification that covers
# nothing reads as a decision and is an oversight.
SETUP_ACTIONS='^(actions/checkout|actions/setup-python|dtolnay/rust-toolchain|Swatinem/rust-cache)$'

# Resolved from this file's own location rather than from CI_YML, so it is
# right whichever script sourced it and from whatever working directory.
ACTION_EXEMPTIONS="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/action_gate_exemptions.txt"

# Emits one gate per line as "<prefix>\t<command>". The two fields are
# separate because prove_gates_fail.sh looks its injection up by the bare
# command — joining them would make every `"cargo run -p ..."*` pattern miss
# and report every gate as unregistered. preflight.sh, which wants them joined,
# strips the tab.
gates_for_jobs() {
    awk -v want="$1" -v skip="$SKIP_STEPS" '
        function shquote(s,   out) { out = s; gsub(/'\''/, "'\''\\'\'''\''", out); return "'\''" out "'\''" }
        # Encodes a block body as a single-line ANSI-C quoted string. Built a
        # character at a time rather than with gsub: the replacement text of
        # gsub has its own backslash rules on top of the string literal, and
        # the two layers together turned one backslash into six on the first
        # attempt. \047 is a single quote, which also keeps this program free
        # of the quote that would end it.
        function ansi_c(s,   i, c, out) {
            out = ""
            for (i = 1; i <= length(s); i++) {
                c = substr(s, i, 1)
                if (c == "\\")        out = out "\\\\"
                else if (c == "\047") out = out "\\\047"
                else if (c == "\n")   out = out "\\n"
                else                  out = out c
            }
            return "$\047" out "\047"
        }
        # Ends a pending block scalar, leaving it as the step command so the
        # next flush() emits it.
        # k newlines, for folded blocks where empty lines are not folded away.
        function nl(k,   s) { s = ""; while (k-- > 0) s = s "\n"; return s }
        # YAML folded-scalar (`>`) semantics, which are not "join with spaces":
        #   * a break between two ordinary lines folds to one space;
        #   * empty lines are NOT folded — k empty lines between content
        #     become k newlines, and the fold-space is dropped;
        #   * a *more-indented* line (one that still starts with a space after
        #     the block indent is removed) is kept literally, and the breaks
        #     on either side of it stay newlines rather than becoming spaces.
        # The third rule is the one worth stating: without it a folded block
        # containing an indented continuation silently loses its structure.
        function fold_lines(   i, L, kind, prevkind, pend, out, more) {
            out = ""; pend = 0; prevkind = ""
            for (i = 0; i < nblines; i++) {
                L = blines[i]
                if (L ~ /^[ \t]*$/) { pend++; continue }
                kind = (substr(L, 1, 1) == " ") ? "MORE" : "NORM"
                more = (prevkind == "MORE" || kind == "MORE")
                if (prevkind == "")        out = nl(pend) L
                # A break next to a more-indented line is never folded, so it
                # survives *in addition to* the newline each empty line
                # contributes: one blank line before an indented continuation
                # gives two newlines, not one. Measured against PyYAML, which
                # is the only reason this line is not `nl(pend)`.
                else if (more)             out = out nl(pend + 1) L
                else if (pend > 0)         out = out nl(pend) L
                else                       out = out " " L
                pend = 0
                prevkind = kind
            }
            return out
        }
        function end_block(   i, out) {
            if (!in_block) return
            in_block = 0
            if (block_folded) out = fold_lines()
            else {
                out = ""
                for (i = 0; i < nblines; i++) out = out (i ? "\n" : "") blines[i]
            }
            # Trailing newlines are dropped whatever the chomping indicator
            # says. `|`, `|-` and `|+` differ only in how many line breaks end
            # the scalar, and a trailing newline cannot change what a shell
            # command does — so chomping is parsed (below) but deliberately not
            # acted on, rather than being silently mishandled.
            sub(/\n+$/, "", out)
            nblines = 0
            if (job ~ want && out != "") cmd = "bash -e -c " ansi_c(out)
        }
        # Emits the pending step, if it belongs to a wanted job.
        function flush(   pair) {
            # TAB-separated: the injection lookup matches the bare command,
            # while execution needs the env prefixed. Joining them into one
            # string would make every `"cargo run -p ..."*` pattern in
            # `gate_kind_for` miss, and every gate would report as unregistered.
            if (cmd != "" && !(skip != "" && step_name ~ skip))
                printf "%s\t%s\n", wd_prefix job_env step_env, cmd
            cmd = ""; step_env = ""; wd_prefix = ""; env_indent = -1
        }
        # ── Block scalars: `run: |`, `|-`, `>`, `>-` ─────────────────────
        #
        # Held until something dedents, then emitted as one `bash -e -c
        # $\047...\047` command. One physical line because every consumer of this
        # function is line-oriented, and ANSI-C quoting so the newlines survive
        # as newlines: flattening them into `;` would turn a backslash
        # continuation into two broken commands, and an `if` into a syntax
        # error.
        #
        # `bash -e`, and deliberately not `-eo pipefail`. That is the shell
        # GitHub gives a `run:` step on Linux, and the gap between the two is
        # exactly how the official-TCK gate came to print REGRESSION and exit
        # 0. A harness that runs the block under a stricter shell than CI does
        # is not reproducing the gate, it is inventing a different one.
        in_block {
            if ($0 ~ /^[[:space:]]*$/) { blines[nblines++] = ""; next }
            ind = match($0, /[^ ]/) - 1
            # The block indent comes from the first non-empty line, unless the
            # header carried an explicit indentation indicator (`>2`), which
            # exists precisely for a first line that is itself more indented.
            if (block_indent < 0) block_indent = ind
            if (ind >= block_indent) {
                blines[nblines++] = substr($0, block_indent + 1)
                next
            }
            # Dedented: the block is over. No `next` — this same line is a new
            # step or a new job and the rules below have to see it.
            end_block()
        }
        # A job header (2-space key) ends the previous job and its env.
        /^  [a-z0-9_-]+:[[:space:]]*$/ {
            flush(); job = $1; sub(/:$/, "", job); job_env = ""; env_indent = -1
            pending_wd = ""; next
        }
        # A new list item ends the previous step.
        /^[[:space:]]*-[[:space:]]/ {
            flush()
            pending_wd = ""
            # flush() first, then rename: the step being emitted is the one
            # that just ended, not the one this line starts.
            step_name = ""
            if ($0 ~ /^[[:space:]]*-[[:space:]]+name:[[:space:]]*/) {
                line = $0
                sub(/^[[:space:]]*-[[:space:]]+name:[[:space:]]*/, "", line)
                sub(/[[:space:]]+$/, "", line)
                gsub(/^"|"$/, "", line)
                step_name = line
            }
        }
        # `working-directory:` appears *before* `run:` inside a step, and the
        # flush the run rule performs would clear it, so it is held here and
        # consumed there. Without this, commands from the out-of-workspace
        # binding are emitted bare and run from the repo root: `cargo fmt
        # --check`, `cargo clippy` and `cargo test` would check the workspace a
        # second time, never touch the binding, and report that as coverage of
        # it. No apostrophes in this block: the awk program is single-quoted,
        # and one closes it.
        /^[[:space:]]+working-directory:[[:space:]]*[^[:space:]]/ {
            line = $0
            sub(/^[[:space:]]+working-directory:[[:space:]]*/, "", line)
            sub(/[[:space:]]+$/, "", line)
            gsub(/^"|"$/, "", line)
            pending_wd = line
            next
        }
        /^[[:space:]]+run:[[:space:]]*[^|>[:space:]]/ {
            flush()
            if (pending_wd != "") { wd_prefix = "cd " pending_wd " && " }
            pending_wd = ""
            if (job ~ want) { line = $0; sub(/^[[:space:]]+run:[[:space:]]*/, "", line); cmd = line }
            next
        }
        /^[[:space:]]+run:[[:space:]]*[|>]/ {
            flush()
            if (pending_wd != "") { wd_prefix = "cd " pending_wd " && " }
            pending_wd = ""
            hdr = $0
            sub(/^[[:space:]]+run:[[:space:]]*/, "", hdr)
            block_folded = (substr(hdr, 1, 1) == ">")
            ind_digits = substr(hdr, 2)
            gsub(/[^0-9]/, "", ind_digits)
            in_block = 1; nblines = 0
            if (ind_digits != "")
                block_indent = (match($0, /[^ ]/) - 1) + int(ind_digits)
            else
                block_indent = -1
            next
        }
        # `env:` at 4 spaces is the job block; at 8 it belongs to the step. The
        # indent is recorded so only its own keys are read, and anything
        # shallower closes it.
        /^    env:[[:space:]]*$/  { env_indent = 4; next }
        /^        env:[[:space:]]*$/ { env_indent = 8; next }
        env_indent > 0 {
            indent = match($0, /[^ ]/) - 1
            if (indent <= env_indent) { env_indent = -1 }
            else if ($0 ~ /^[[:space:]]+[A-Za-z_][A-Za-z0-9_]*:/) {
                line = $0
                sub(/^[[:space:]]+/, "", line)
                key = line; sub(/:.*$/, "", key)
                val = line; sub(/^[^:]*:[[:space:]]*/, "", val)
                sub(/[[:space:]]+$/, "", val)
                gsub(/^"|"$/, "", val)
                gsub(/^'\''|'\''$/, "", val)
                if (env_indent == 4) job_env = job_env key "=" shquote(val) " "
                else step_env = step_env key "=" shquote(val) " "
                next
            }
        }
        END { end_block(); flush() }
    ' "$CI_YML"
}

require_known_skips() {
    local pat missing="" names
    names=$(awk '
        /^[[:space:]]*-[[:space:]]+name:[[:space:]]*/ {
            line = $0
            sub(/^[[:space:]]*-[[:space:]]+name:[[:space:]]*/, "", line)
            sub(/[[:space:]]+$/, "", line)
            gsub(/^"|"$/, "", line)
            print line
        }' "$CI_YML")
    # `while read`, not `for pat in $(...)`: step names contain spaces, and
    # word-splitting turned the single exemption "Install SPIRE" into two
    # patterns that matched nothing. The guard refused to run, correctly but
    # for entirely the wrong reason.
    while IFS= read -r pat; do
        [ -n "$pat" ] || continue
        if ! printf '%s\n' "$names" | grep -Fxq -- "$pat"; then
            missing="$missing$pat"$'\n'
        fi
    # `printf '%s\n'`, with the newline. Without it the final alternative has
    # no line terminator, `read` returns non-zero on it, and the loop body
    # never runs for the last pattern — which, with a single exemption, meant
    # this guard executed and checked nothing at all. Found by giving it a
    # deliberately stale pattern and watching it pass.
    done < <(printf '%s\n' "$SKIP_STEPS" | tr -d '^$()' | tr '|' '\n')
    if [ -n "$missing" ]; then
        printf '%s: SKIP_STEPS names step(s) that ci.yml no longer has:\n' "${0##*/}" >&2
        printf '%s' "$missing" | sed 's/^/      /' >&2
        printf '  Remove the exemption, or correct it to the new step name.\n' >&2
        exit 2
    fi
}

# The completeness guards — `require_known_jobs`, `require_nonempty_gate_jobs`
# and `require_registered_actions` — live beside this file and are sourced
# here, so that sourcing ci_gates.sh gets the whole picture rather than half of
# it. A guard a caller has to remember to source separately is a guard that
# gets forgotten, which is the argument this file's own header makes about the
# parser it used to be two copies of.
# shellcheck source=ci_gate_audit.sh
. "$(dirname "${BASH_SOURCE[0]}")/ci_gate_audit.sh"
