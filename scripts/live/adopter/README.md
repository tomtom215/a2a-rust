<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# The adopter walk-through, as scripts

What a stranger does with 0.16.0, rerunnable: package the crates, build a project
outside the repository against the unpacked tarballs, type the README and the book
as written, run the examples and the CLI, and talk to a real model. The write-up is
`docs/audits/2026-11-07-adopter-walkthrough/`; its `evidence/` holds the output of one run.

None of this runs in CI. Every script sources `env.sh`, which needs `ADOPTER_SCRATCH`
(a directory for unpacked tarballs, projects and target directories; default
`${TMPDIR:-/tmp}/a2a-adopter`) and, for repository builds, `ADOPTER_REPO_TARGET`.
A stranger's project is built with **no `RUSTFLAGS`**; a build of this checkout is built with the
repository's `-D warnings`. Delete `$ADOPTER_SCRATCH/adopter-projects/*/target` between areas: the
disk allowance is the limit, not the CPU.

| Script | What it does | Needs |
|---|---|---|
| `01-package.sh` | `cargo package` of the four published crates (verifies each tarball) and unpacks them | stable cargo, crates.io, ~6 GB |
| `01b-all-features-check.sh` | builds the unpacked tarballs with every feature on, from a project outside the repository | `01-package.sh` done, ~2 GB |
| `tarball_audit.py`, `tarball_scan.py` | what is in each `.crate`: stray files, missing `include!` inputs, secrets, licence files | `01-package.sh` done (`target/package`) |
| `mkproject.py` | a stranger's project from a page's own `toml` and `rust` blocks, verbatim (`--show-hidden` keeps rustdoc's hidden lines) | the unpacked tarballs |
| `02-readme-quickstart.sh` | the README Quick Start, typed as written, built and run | `01-package.sh` done, port 3000 |
| `02-book-getting-started.sh` | Installation, Quick Start and Your First Agent as written; fails on a compiler warning | same |
| `book_blocks.py` | inventory of every fenced block of the README and the book | python3 |
| `mksweep.py`, `03-book-sweep.sh`, `sweep_classify.py` | every Rust block as a doctest outside the repository, at default features or all (`default`\|`all`) | `01-package.sh` done, ~4 GB |
| `02-commands-static.py` | every documented command and path, checked against the workspace without running anything | `cargo metadata --offline` |
| `07-doc-gates.sh` | the repository's documentation gates, `mdbook build` and the link checker | mdbook, python3 |
| `03-build-examples.sh` | builds the example and CLI binaries from the checkout (`-D warnings`) | crates.io, several GB |
| `03-examples-offline.sh` | `hello-agent`, `deploy-agent`, `echo-agent` as their READMEs say | `03-build-examples.sh` done, ports 3000 and 8080 |
| `03-hello-claims.sh` | the error texts `hello-agent`'s README quotes | `03-build-examples.sh` done, port 3000 |
| `cli_stub.py` | a misbehaving A2A peer (401, 429, 500, half-open, stalled and cut streams, a card naming another host) on nine loopback ports | python3 |
| `04-cli.sh`, `04b-cli-followup.sh` | every `a2a` subcommand, exit codes, bad input, no network, a 401, a 429, half-open and stalled peers, signals, large bodies | `03-build-examples.sh` done, openssl, curl, ports 3111 and 3401-3409 |
| `05-build-llama.sh` | builds `llama-server` at the newest `bNNNNN` tag and fetches two GGUFs, hashes checked against Hugging Face | cmake, ninja, git over the proxy, huggingface.co, ~6 GB then ~4 GB |
| `model_probe.py` | one timed chat completion against a local llama-server, thinking on or off | a llama-server on 11434 |
| `05-tool-loop-page.sh` | the tool-loop page's minimal agent, built from the page and run against the model | `01-package.sh`, `03-build-examples.sh`, a llama-server on 11434 |

Do not edit a script while it runs: bash reads it as it goes.
