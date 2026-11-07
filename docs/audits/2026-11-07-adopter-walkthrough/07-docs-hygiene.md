<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# 7. Documentation hygiene

Scripts: `07-doc-gates.sh`, `07-links.py`, `02-commands-static.py`. Raw output: `evidence/07-*`.

## The repository's own gates (`scripts/preflight.sh --list` names them)

Run one by one, each with its exit code, on the tree as found (`evidence/07-doc-gates.log`):
`check_doc_versions.py`, `check_book_examples.py`, `check_feature_tables.py`,
`check_api_reference.py`, `check_doc_escapes.py`, `check_book_code.sh`,
`check_benchmark_prose.sh`, `gen_sitemap.py --check`, `check_readme_doctests.py`,
`check_compliance_map.py --check`, `check_panic_paths.py`, `mdbook build book`, and
`check_book_links.py` over the rendered book: **all exit 0**. None of them saw D-1 to D-5 or D-15
below; each was extended in this branch to see the one it should have (see `README.md`).

## Dead links

* The rendered book: `check_book_links.py` exit 0.
* The rest of the repository's Markdown (`07-links.py --all`: 669 relative links in 193 files,
  anchors included; text inside code spans and fences is not read): the README, CONTRIBUTING,
  STABILITY, ROADMAP, SECURITY, every crate, example, tool and deploy README resolve. Of the 30 the
  tool reports, 8 are a link quoted inside backticks in `docs/compliance/control-map.md` (a false
  positive: the tool read a code span), 11 are anchors in the table of contents of
  `docs/implementation/plan.md` (its headings end in `✅ COMPLETE`, so the slugs differ), 1 is an anchor
  in `spec/slimrpc_v1/slimrpc-collaborative-task.md:86` (`slimrpc.md#43-metadata`), and 10 are broken
  `./topics/*.md` links and anchors inside the vendored upstream A2A specification copy
  (`docs/implementation/a2a-v1.0.1-specification.md`, not this project's text). The 22 real ones are
  **D-16, polish**, all in design records and none on a stranger's first-hour path.

## Stale numbers and contradictions

| What | Where | State |
|---|---|---|
| Conformance badge 83/114, 5 failing | `README.md:23` | stale against the same file's 85 of 115, 2 failing; fixed (D-1) |
| `tls-rustls` default for the client and SDK only | installation.md, README, `gs-crate-map.mmd` | stale since 0.16.0 (D-3); fixed |
| `serve()` as the startup helper | introduction.md, README, server README, API reference | deprecated since 0.16.0 (D-2); fixed |
| "a2a-protocol-types ... 11 normal dependencies" | README | true: `cargo tree -p a2a-protocol-types -e normal` lists 11 besides itself |
| "20 alerts" | observability pages | true (14 + 6) |
| "about 50 lines" for hello-agent | README | true (48) |
| `STABILITY.md` §1: "The crates are at `0.15.0`, the newest release" | STABILITY.md | true until the tag; **it becomes false at the 0.16.0 tag** and the paragraph that follows lists what 0.16.0 removes; the release checklist should carry it (not edited here: it is the maintainer's release-day text) |
| `hello-agent` README quotes the Rust Display text of an error | hello-agent README, book | fixed |
| CLI exit-code table: "one `error:` line" | tools/a2a-cli/README.md, cli.md | stale (a log line comes first); fixed (D-8) |

## What was not checked

`docs/` as a whole beyond links (dated reviews and records), the ADRs, `ROADMAP.md`'s claims, every
number in the compliance pages, `book/src/reference/benchmarks.md` (a CI gate reads it), the
changelog.
