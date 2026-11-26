#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
#
# Regenerates every figure in docs/provenance-manifest.md.
#
# Why this exists as a script rather than a paragraph someone re-derives by
# hand: the figures it produces are the ones a downstream project's counsel is
# expected to check, and the last two attempts to state them by hand were both
# wrong — not by a rounding error, but by a factor of 2.3. Both times the cause
# was the same and neither run noticed it (see the shallow-clone guard below).
#
# The classification logic is a deliberate duplicate of
# `scripts/check_dco.sh`'s, which `.github/workflows/dco.yml` runs. That is not DRY, and it is intentional: the
# workflow gates *incoming* pull requests over a `base..head` range, this
# reports on *existing* history over all of it. Sharing one implementation
# would mean one of the two callers driving the other's shape. What keeps them
# honest instead is `--self-test`, which asserts the regex and the trailer
# pattern here still match the ones in the workflow file, and fails if the
# workflow drifts.
#
# Usage:
#   scripts/provenance_manifest.sh [REV]        aggregate tables (default HEAD)
#   scripts/provenance_manifest.sh --csv [REV]  one row per non-merge commit
#   scripts/provenance_manifest.sh --self-test  check this file against check_dco.sh
set -uo pipefail

# ── Classification, copied verbatim from scripts/check_dco.sh ────────────────
NON_HUMAN='^(noreply@anthropic\.com|.*\[bot\]@users\.noreply\.github\.com)$'
SIGNOFF_TMPL='^[[:space:]]*Signed-off-by:[[:space:]]+.+<%s>[[:space:]]*$'

REPO_ROOT=$(git rev-parse --show-toplevel 2>/dev/null) || {
  echo "not a git repository" >&2
  exit 2
}

# ── The guard that this whole file exists because of ─────────────────────────
#
# A shallow clone silently truncates the *oldest* history. That is precisely
# where this repository's non-compliant commits live — the sign-off policy
# starts near the tip — so a shallow measurement leaves the "passes" count
# untouched and understates every failure count. It does not look broken. It
# looks like good news.
#
# Measured, not asserted: PROVENANCE.md section 2.1 was written from a clone
# whose boundary hid 430 commits, and reported 120/282 (43%) passing where the
# true figure over the same ref is 120/641 (19%). The pass count was identical
# both times, which is exactly why nobody caught it.
#
# So: refuse to produce a number at all rather than produce that one.
require_full_clone() {
  if [ "$(git rev-parse --is-shallow-repository)" = "true" ]; then
    cat >&2 <<'MSG'
error: this is a shallow clone, so any count taken from it is a floor, not a total.

  A shallow clone drops the oldest commits. In this repository those are the
  ones that fail the DCO check, so a shallow run understates every failure
  category while leaving the pass count intact — it reads as good news.

  Fix it and re-run:

      git fetch --unshallow --tags origin

MSG
    exit 2
  fi
}

# Whether commit $1's message has a line matching $2. The message is read
# whole before grep sees it. Piped into `grep -q` under `pipefail`, a match in
# git's first write let grep exit before the second, git died of SIGPIPE, and
# the pipeline read as no match: 0b9f710e's 4,251-byte message is written as
# 4,096 + 155 bytes with its sign-off inside the first, which is how one run
# of 17 on 2026-11-18 counted 2130 passing commits where the others counted
# 2131 (ROADMAP *Release engineering* item 3; FND-09 step 5).
signed_off() {
  local body
  body=$(git log -1 --format='%B' "$1") || return 2
  grep -Eqi "$2" <<<"$body"
}

# Emits: sha <TAB> verdict <TAB> email <TAB> name <TAB> subject
# verdict ∈ { ok, nonhuman, nosignoff }
classify() {
  local rev="$1"
  while IFS=$'\t' read -r sha email name; do
    [ -n "$sha" ] || continue
    local subject
    subject=$(git log -1 --format='%s' "$sha")

    if printf '%s' "$email" | grep -Eqi "$NON_HUMAN"; then
      printf '%s\tnonhuman\t%s\t%s\t%s\n' "$sha" "$email" "$name" "$subject"
      continue
    fi

    local pattern
    # shellcheck disable=SC2059  # the template is ours, not user input
    pattern=$(printf "$SIGNOFF_TMPL" "${email//./\\.}")
    if signed_off "$sha" "$pattern"; then
      printf '%s\tok\t%s\t%s\t%s\n' "$sha" "$email" "$name" "$subject"
    else
      printf '%s\tnosignoff\t%s\t%s\t%s\n' "$sha" "$email" "$name" "$subject"
    fi
  done < <(git log --no-merges --format='%H%x09%ae%x09%an' "$rev")
}

# ── --self-test: the two copies of the rules must not drift apart ────────────
self_test() {
  # dco.yml runs scripts/check_dco.sh, so the rules are there. Until
  # 2026-11-26 this compared against dco.yml itself, which had stopped
  # holding them, and the two checks below failed on every run.
  local dco="$REPO_ROOT/scripts/check_dco.sh"
  local rc=0

  if [ ! -f "$dco" ]; then
    echo "FAIL  $dco not found" >&2
    return 1
  fi

  # The NON_HUMAN regex must appear in check_dco.sh byte-for-byte.
  if grep -qF "NON_HUMAN='$NON_HUMAN'" "$dco"; then
    echo "ok    NON_HUMAN regex matches check_dco.sh"
  else
    echo "FAIL  NON_HUMAN regex has drifted from check_dco.sh" >&2
    echo "      here:     NON_HUMAN='$NON_HUMAN'" >&2
    echo "      check_dco.sh:  $(grep -o "NON_HUMAN='.*'" "$dco" || echo '<not found>')" >&2
    rc=1
  fi

  # check_dco.sh interpolates the email inline; compare the invariant part.
  local core='^[[:space:]]*Signed-off-by:[[:space:]]+.+<'
  if grep -qF "$core" "$dco"; then
    echo "ok    Signed-off-by pattern matches check_dco.sh"
  else
    echo "FAIL  Signed-off-by pattern has drifted from check_dco.sh" >&2
    rc=1
  fi

  # A match in git's first write is still a match when git writes again
  # after grep has seen it (the SIGPIPE race signed_off's comment names). A
  # git that pauses between its two writes makes the race certain.
  if (
    git() {
      if [ "$*" = "log -1 --format=%B fake" ]; then
        printf 'Signed-off-by: A Person <a@example.com>\n'
        sleep 0.3
        printf 'trailing text\n'
      else
        command git "$@"
      fi
    }
    # shellcheck disable=SC2059  # the template is ours
    signed_off fake "$(printf "$SIGNOFF_TMPL" 'a@example\.com')"
  ); then
    echo "ok    a sign-off in git's first write is found however git writes the rest"
  else
    echo "FAIL  a sign-off in git's first write was missed (SIGPIPE under pipefail)" >&2
    rc=1
  fi

  # A classifier that cannot return all three verdicts is not classifying.
  local seen
  seen=$(classify HEAD | cut -f2 | sort -u | tr '\n' ' ')
  for v in nonhuman nosignoff ok; do
    case " $seen " in
      *" $v "*) echo "ok    verdict '$v' is reachable on this history" ;;
      *) echo "FAIL  verdict '$v' never produced — classifier or history changed" >&2; rc=1 ;;
    esac
  done

  return $rc
}

# ── main ─────────────────────────────────────────────────────────────────────
case "${1:---report}" in
  --self-test)
    require_full_clone
    self_test
    exit $?
    ;;
  --csv)
    require_full_clone
    REV="${2:-HEAD}"
    echo "sha,verdict,author_email,author_name,subject"
    classify "$REV" | while IFS=$'\t' read -r sha v email name subject; do
      printf '%s,%s,%s,"%s","%s"\n' "$sha" "$v" "$email" \
        "${name//\"/\"\"}" "${subject//\"/\"\"}"
    done
    exit 0
    ;;
  --report|-*) REV="HEAD" ;;
  *) REV="$1" ;;
esac
[ "${1:-}" = "--report" ] && REV="${2:-HEAD}"

require_full_clone

TSV=$(mktemp)
trap 'rm -f "$TSV"' EXIT
classify "$REV" > "$TSV"

total=$(git rev-list --count "$REV")
merges=$(git rev-list --count --merges "$REV")
nonmerge=$(wc -l < "$TSV" | tr -d ' ')
ok=$(awk -F'\t' '$2=="ok"' "$TSV" | wc -l | tr -d ' ')
nosign=$(awk -F'\t' '$2=="nosignoff"' "$TSV" | wc -l | tr -d ' ')
ai=$(awk -F'\t' '$2=="nonhuman" && $3=="noreply@anthropic.com"' "$TSV" | wc -l | tr -d ' ')
bot=$(awk -F'\t' '$2=="nonhuman" && $3!="noreply@anthropic.com"' "$TSV" | wc -l | tr -d ' ')

echo "provenance manifest — $(git rev-parse "$REV") ($(git log -1 --format=%ad --date=short "$REV"))"
echo
echo "commits reachable         $total"
echo "  merge commits           $merges   (dco.yml does not examine these)"
echo "  non-merge commits       $nonmerge"
echo
echo "dco.yml verdict over the $nonmerge non-merge commits:"
printf '  %-38s %5s  %s\n' "would pass" "$ok" "$(awk -v a="$ok" -v b="$nonmerge" 'BEGIN{printf "%.1f%%", 100*a/b}')"
printf '  %-38s %5s\n' "fail — author noreply@anthropic.com" "$ai"
printf '  %-38s %5s\n' "fail — author *[bot]@users.noreply" "$bot"
printf '  %-38s %5s\n' "fail — human author, no sign-off" "$nosign"
echo
echo "git author field, all $total commits (merges included):"
git log --format='%an <%ae>' "$REV" | sort | uniq -c | sort -rn | sed 's/^/  /'
echo
echo "earliest commit that fails dco.yml (sets the rewrite blast radius):"
earliest=$(awk -F'\t' '$2!="ok"{print $1}' "$TSV" | tail -1)
if [ -n "$earliest" ]; then
  git log -1 --format='  %h  %ad  %an <%ae>  %s' --date=short "$earliest"
  echo "  descendants rewritten if this commit is amended: $(git rev-list --count "$earliest..$REV")  (+ itself = $(( $(git rev-list --count "$earliest..$REV") + 1 )))"
else
  echo "  none — every non-merge commit passes"
fi
echo
echo "release tags, and whether a rewrite moves them:"
for t in $(git tag | sort -V); do
  typ=$(git cat-file -t "$t")
  if git merge-base --is-ancestor "$t^{commit}" "$REV" 2>/dev/null; then moved="yes"; else moved="n/a — not an ancestor"; fi
  printf '  %-8s %-8s %s  %s\n' "$t" "$typ" "$(git rev-parse --short "$t^{commit}")" "$moved"
done
