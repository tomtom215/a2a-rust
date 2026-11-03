<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# The conversion of the 26 wire structs

What converted every struct literal of the 26 generated wire structs that
became `#[non_exhaustive]` (audit G-10, fork F1) to `Type::new(..).with_*(..)`.
Kept so that the conversion can be read, checked and re-run when `main` moves
under a pull request that carries it; it is not part of any build or gate.

```bash
python3 -I scripts/codemod/wire_structs/apply.py . --dry-run   # count, change nothing
python3 -I scripts/codemod/wire_structs/apply.py .              # rewrite in place; then `cargo fmt --all`
python3 -I scripts/codemod/wire_structs/test_rust_literal.py    # the rewrite's 28 cases
python3 -I scripts/codemod/wire_structs/test_apply.py           # the file walk's 6 cases
```

- `specs.py` lists each of the 26 types: which fields `new` takes, which
  setter each optional field has, and which parameters are `impl Into<String>`.
- `rust_literal.py` reads Rust the way the compiler tokenises it (strings, raw
  strings, characters, nested block comments), tells a literal from a
  definition, an impl and a pattern, and rewrites. It leaves the prost types in
  `proto`/`pb` alone (they share names), and reports a literal that has a
  comment inside, or that it cannot split into fields.
- `apply.py` walks the tracked `.rs` and `.md` files: code outside the types
  crate's own `src/`, doc comments everywhere, and the fenced Rust blocks of the
  book and the READMEs. It skips the changelog, `docs/`, the upgrade guides
  (their old-API snippets are the point; the ones that are compiled were
  converted by hand) and `benches/sdk-comparison/` (other SDKs).

What it leaves for a person: a literal with a comment inside, a literal in
hidden doc lines (`# ` prefixed), and anything whose result the compiler then
rejects. On the conversion that produced this change that was 14 sites and
6 compiler or clippy findings, all fixed by hand.
