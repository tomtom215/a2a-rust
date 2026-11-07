<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# ADR 0035: Agent Skills are adopted in the tool loop, without running their scripts

**Date:** 2026-11-07 (proposed 2026-11-01)
**Status:** Accepted (0.16.0). Built behind the off-by-default `skills`
feature of `a2a-protocol-server` (and the SDK's forward of it): the
`skills` module and `ToolLoopExecutor::with_skills`. The maintainer decided on
2026-11-01 (triage fork F9) to write the ADR and build a feature-gated module
with no script execution, in 0.17.0, and on 2026-11-07 moved it into 0.16.0.
The live-model run in *What would settle it* has not been made.
**Author:** Tom F.

---

## Context

[Agent Skills](https://agentskills.io) is a format for giving an agent
reusable procedures: a directory with a `SKILL.md` whose frontmatter carries a
`name` and a `description`, and optionally `scripts/`, `references/` and
`assets/`. A client reads only the name and description of every skill at
start-up, and reads a skill's body, then its files, when the task calls for it
(progressive disclosure). As recorded in the triage of 2026-11-01
(`docs/audits/2026-11-01-triage/DECISIONS.md`, F9, read from Anthropic's
announcement of 22 October 2026 and the Agentic AI Foundation's page): the
format was donated to the Agentic AI Foundation under the Linux Foundation, is
implemented by many tools, is maintained by individuals, has **no versioned
releases yet** (a roadmap proposes them), and marks `allowed-tools` as
experimental.

It is not A2A's `AgentSkill`. That is an entry on an agent card that
advertises what an agent can do to another agent. An Agent Skill is something
an agent loads to know how to do it. The two meet in one place: an agent that
has a skill can advertise it.

What this SDK has to build on:

* `ToolLoopExecutor` (`crates/a2a-protocol-server/src/tool_loop/executor.rs`)
  is the model-and-tools loop. Its system prompt and tool list are where a
  catalog and an `activate_skill` tool go.
* `Delegation` already runs a piece of work in another agent, which is what
  the format's "run a skill in a subagent" pattern is.
* `docs/injection-containment.md` records why text an agent reads from a place
  anyone can write is an instruction channel.

### The specification as read on 2026-11-07

Read from `https://agentskills.io/specification` and
`https://agentskills.io/client-implementation/adding-skills-support` (both
fetched that day; the page carries no version or date), and, for the reference
validator `skills-ref`, from a summary of its `validator.py` and `parser.py`
(not the source). Whether the repository `agentskills/agentskills` has tags or
releases was not checked: this environment had no access to it.

| Subject | The specification says |
|---|---|
| `name` | Required. 1 to 64 characters. "Unicode lowercase alphanumeric characters (`a-z`, `0-9`) and hyphens". Must not start or end with a hyphen, must not contain `--`, must match the parent directory's name. |
| `description` | Required. 1 to 1024 characters. Says what the skill does and when to use it. |
| `license` | Optional. A licence name or the name of a bundled licence file; short. |
| `compatibility` | Optional. 1 to 500 characters when given. Environment requirements. |
| `metadata` | Optional. A map from string keys to string values. |
| `allowed-tools` | Optional. A space-separated string of pre-approved tools. **Experimental**; support varies. |
| Body | Markdown after the frontmatter, no format restrictions. Recommended under 500 lines. |
| Progressive disclosure | Metadata about 100 tokens per skill, loaded at start-up; the body under 5,000 tokens, loaded on activation; resources as needed. Keep file references one level deep. |
| Optional directories | `scripts/` (executable code), `references/` (documentation), `assets/` (templates, images, data). |

The client guide (not the format) adds: scan directories for subdirectories
holding a file named exactly `SKILL.md`, skipping `.git` and `node_modules`,
with bounds on depth and count; when two skills share a name the project's
overrides the user's, and a shadowed one is logged; gate project-level skills
on trust, because a cloned repository may be untrusted; validate leniently
(warn on a name that does not match its directory or is over 64 characters and
load; skip a skill with no description or YAML that cannot be parsed; record
diagnostics); expect unquoted colons in values and accept them; put the
catalog (name, description, optionally location) in the system prompt or a
tool's description, at 50 to 100 tokens a skill; if there is a dedicated tool,
make its `name` parameter an enum of the valid names and register no tool when
there are no skills; return the body (frontmatter stripped or not), wrap it,
list the bundled files without reading them; protect it from context
compaction; deduplicate activations. The reference validator reads the
frontmatter with `strictyaml` (every scalar is a string; no flow collections,
anchors, aliases or tags), normalises names with NFKC, and reports an unknown
frontmatter key as an error.

## Decision

1. **Where.** A feature-gated `skills` module in `a2a-protocol-server` (feature
   `skills`, off by default, implies `tool-loop`), forwarded by the SDK. The
   alternative the 0.17.0 text named, its own crate, was for a dependency the
   server crate should not carry; decision 9 adds none, so the reason is gone.
   A fifth published crate would cost `RELEASING.md`'s dependency order, its
   lockfiles, `release.yml` and every version pin for code only the server side
   uses (ADR 0022 option 2 said the same of the tool loop).
2. **Discovery and validation are lenient, with diagnostics.**
   `SkillCatalog::discover(&SkillsConfig)` returns a catalog and a list of
   `SkillDiagnostic`s (a severity, a kind a program can match on, a path and a
   message). It never panics and never fails. A skill that deviates from the
   specification and can be used safely is loaded with a warning: a name that
   is not its directory's, over 64 characters (up to 256), with an uppercase
   letter, an underscore or a dot, a hyphen at an end or two together; a
   description over 1024 characters; a body over 500 lines; an optional field
   that is empty, too long or not a string (the field is ignored). A skill that
   cannot be used safely is skipped with the reason: no frontmatter, an unclosed
   or oversized one, a shape that is not `key: value` lines, a missing, empty
   or non-string `name` or `description`, a name with a character a prompt
   cannot carry (anything but letters, digits, `-`, `_`, `.`), a body over the
   bound, a file that is not UTF-8 or not a regular file inside its directory.
   Fields the specification does not define are named in a warning that says
   the specification was read on 2026-11-07 (decision 8).
3. **Catalog in the prompt, body on demand.** `ToolLoopExecutor::with_skills`
   appends the catalog to the loop's preamble and registers `activate_skill`,
   whose `name` parameter is a JSON Schema enum of the valid names, so a model
   cannot ask for a path. It returns the body (frontmatter stripped) in a
   `<skill_content name="...">` element, with the compatibility note, the files
   `read_skill_file` can return and the scripts that are present and not run.
   No catalog and no tool are added when there are no skills. The catalog is
   the specification's `<available_skills>` form with each description on one
   line, shortened to the configured length, and XML-escaped, so it cannot
   close the element it is in. The bodies are read at discovery, so an
   activation touches no file.
4. **Resources are read through a confined reader.** `read_skill_file` takes a
   skill (an enum of the skills that have files) and a path relative to its
   directory. The path is checked as text (no NUL, no backslash, no `..`,
   nothing absolute, first component `references` or `assets`), resolved on
   disk and required to stay inside that directory (so a link that leaves it, or
   goes to `scripts/` or `SKILL.md`, is refused), opened by its resolved path and
   then verified against what was opened (the path resolves to itself, and on
   Unix the open file is the file at the path). A file over the size bound and a
   file that is not UTF-8 are refused, never truncated; the bound is on the
   bytes read, not on the size the file had a moment earlier. The window that
   remains is written in `skills/reader.rs` and in the book: on a platform
   without Unix file identities a link swapped in and out between the open and
   the second resolution is not seen; a hard link names any file its maker can
   read; a regular file replaced by a FIFO between the check and the open
   blocks one blocking-pool thread until a writer appears. Reads run on tokio's
   blocking pool, and the call is abandoned when the run is cancelled.
5. **No script execution.** `scripts/` is listed in the diagnostics and to the
   model as present and not run, and `read_skill_file` does not return it. Running
   them needs a sandbox this SDK does not have, and a directory an operator writes
   to is a prompt-injection channel (`docs/injection-containment.md`); the two
   together are a remote code execution path. Revisit only with a design for the
   sandbox.
6. **Trust by scope.** A directory the operator configures
   (`SkillSource::operator`) is the operator's own text. A project directory
   (`SkillSource::project`) is not read at all until the operator marks it
   `with_trusted(true)`; the diagnostic for one that is not says nothing was read.
   A trusted project's text is still an instruction channel, and the module says
   so rather than claiming to filter it: nothing in the SDK can tell a hijacked
   model's instruction from the operator's, which is what the containment document
   records. What the module does instead is give such text no capability the
   operator's has not: operator directories are read first, so a project skill
   never takes the name of an operator's (the client guide says the opposite, for a
   client that trusts its project); `allowed-tools` is never acted on (decision 11);
   `scripts/` are never run; a project skill is marked `<origin>project</origin>`
   in the catalog and `origin="project"` in its activation, which is a label for
   the model and not a guard; and the tools that act are the integrator's, behind
   the controls the containment document lists (read-only declarations,
   idempotency keys, budgets, approval gates), which a skill cannot widen. A
   symbolic link to a skill directory is not followed unless the source says
   `with_follow_symlinks(true)`, in either scope; a followed link still cannot
   take a read out of the skill's own directory.
7. **Advertising.** `SkillCatalog::agent_skills()` returns each skill as an
   `AgentSkill` (id and name from the name, description whole and verbatim, one
   tag, `agent-skill`) for the integrator to put on the card. Nothing is
   advertised unless they do: a card is public and a skill's description is not
   necessarily meant to be.
8. **Pinned to a snapshot.** There is no version to pin, so
   `skills::SPEC_SNAPSHOT` (`2026-11-07`) is in the module docs and in the
   warning for each frontmatter field the module does not know.

Decisions on what the 0.17.0 text left open, made on 2026-11-07:

9. **The YAML frontmatter is read in-tree, as the subset the reference parser
   accepts.** Evidence, from `Cargo.lock` and crates.io on 2026-11-07: the
   lockfiles (workspace, `fuzz/`, `itk/`) hold no YAML crate. `serde_yaml` is
   archived (its last release is `0.9.34+deprecated`, 2024-03-25) and is not
   considered; `yaml-rust` last released on 2021-01-03; `serde_yml` and
   `serde_norway` are forks of it (the first last updated 2026-05-27, the second
   2024-12-21), not examined further. The maintained candidates are
   `yaml-rust2` 0.13.0 (2026-09-11, MIT OR Apache-2.0, MSRV 1.85; needs
   `arraydeque` and `hashlink` 0.12, where the lockfile has `hashlink` 0.11.1
   through `sqlx`, so it would add three crates or more) and `saphyr-parser`
   0.3.3 (2026-10-27, MIT OR Apache-2.0, MSRV 1.85; an event parser, which adds
   `arraydeque`, with `thiserror` 2 already in the graph, so two crates;
   pre-1.0). Both licences are in `deny.toml`'s allowlist; `cargo deny` was not
   run (it is not installed here), and no audit of either was looked for or is
   claimed. Why in-tree anyway, against the letter of the dependency policy (a
   standards-defined parser goes to a crate): the job is not YAML 1.2, it is the
   six fields of a frontmatter, in the subset the specification's own reference
   parser reads (`strictyaml`: every scalar a string, no anchors, aliases, tags or
   flow collections); the client guide asks for leniency a library would not give
   (a plain value holding `: ` is invalid YAML, common in the wild, and a
   library needs a re-quoting pass in front of it, which is a second parser); a
   hostile directory is the input, so the reader has no recursion, no alias and a
   size bound applied before it runs, and a library's panics on malformed input
   would have to be caught to keep "never panics"; and it adds nothing to the
   graph or the three lockfiles. What it reads: a top-level block mapping whose
   values are plain, single-quoted, double-quoted (with the YAML escapes and line
   folding) or `|` / `>` block scalars (with chomping and indentation
   indicators), and one level of nested mapping for `metadata`; anything else
   (sequences, flow collections, anchors, aliases, tags, deeper nesting) is named
   against its key and skipped by indentation, which costs nothing for an unknown
   field and skips the skill for `name` or `description`. It is fuzzed
   (`fuzz/fuzz_targets/skill_frontmatter.rs`). **What would reverse it:** a real
   skill the subset rejects that an operator needs; the answer then is
   `saphyr-parser`'s event API behind the same `skills::yaml` interface, two
   crates, with this reader kept as the fast path for what it accepts.
10. **The caps** (each is on `SkillsConfig`; each is exact at its bound and
    tested one under, at and one over): 32 skills, the first 32 in name order
    (operator directories first), the rest not read and one diagnostic saying how
    many; a description of at most 400 characters in the prompt (about 100
    tokens, the specification's figure), cut at a character with an ellipsis, the
    whole description in the activation result; a body of at most 64 KiB (about
    16,000 tokens, over three times the specification's 5,000), over which the
    skill is skipped, because half a procedure is worse than none; a file of at
    most 64 KiB from `read_skill_file`; a frontmatter of at most 8 KiB, checked
    before it is parsed; at most 1,024 entries of one directory examined; at most
    100 files listed in each of `references/`, `assets/` and `scripts/`, three
    directories deep. Thirty-two descriptions of 400 characters are at most 12,800
    characters of prompt; the arithmetic is an estimate of about 3,200 tokens, not
    a count.
11. **`allowed-tools` is recorded and never used.** It is experimental in the
    specification, it means "pre-approved" in the clients that honour it, and here
    that would let text from a skills directory approve a tool, which is the
    escalation the containment model exists to prevent. The field is kept
    (`Skill::allowed_tools`), reported at `Info`, and changes nothing: a test
    activates a skill that asks for a shell and the tool set is unchanged.
12. **No hot reload.** The catalog is a snapshot taken by `discover`: the prompt
    and the tools' schemas are fixed when the executor is built, so the model is
    never told one thing and offered another, and an activation cannot meet a file
    that changed after it was validated. Resources are read when asked, and a
    skill's directory that is no longer the one found is refused
    ("restart to load skills again"). To pick up a change, discover again and
    build a new executor.

## Alternatives

| Option | Why not |
|---|---|
| Validation only (`a2a skills validate`) | Useful, and a subset of this: `SkillCatalog::discover` and its diagnostics are it. A command-line front end is not built. |
| Run `scripts/` in a subprocess | No sandbox, and the directory is an injection channel. |
| Do not adopt | The format is becoming the common one for this; an agent built here would otherwise need an adapter per tool. Revisit if the standard stalls. |
| Treat Agent Skills as `AgentSkill` | They are different things; conflating them would put an agent's private procedures on its public card. |
| Its own crate | See decision 1: the dependency it was for is not needed. |
| A YAML crate | See decision 9. |
| The model reads `SKILL.md` through a general file-read tool | The specification allows it. This SDK has no file tool and gives the model none: a dedicated tool with an enum cannot be asked for a path. |
| Follow symbolic links by default, as other clients do | A link is how a directory someone else controls gets into this one; following is one builder call. |

## Consequences

* An `activate_skill` or `read_skill_file` call is a tool call like any other:
  an `execute_tool` span with `gen_ai.tool.name`, one of the run's
  `gen_ai.invoke_agent.tool_calls`, and a turn of the loop's `max_turns` and the
  caller's budget. Both tools are read-only, so asking twice runs twice. No new
  metric or `Metrics` callback was added, so
  `scripts/check_otel_metrics_coverage.py` has nothing to check.
* The catalog costs prompt tokens per skill; decision 10 bounds it.
* Skills are read at start-up and not again (decision 12). A skills directory on a
  slow or network file system slows start-up, not a request.
* The module adds no dependency and no package to any lockfile. The one lockfile
  line it changes is an edge in `fuzz/Cargo.lock`: the fuzz crate turns on `skills`,
  which implies `tool-loop`, which makes the server depend on `httpdate`, a package
  already in that lockfile through the client. Its CI legs are a clippy and a
  nextest run of the server crate with `--features skills` (`ci.yml`), and a fuzz
  target in `fuzz.yml`.
* `ToolLoop`'s `tools` and `preamble` fields and `ToolLoopExecutor`'s `tool_loop`
  field are `pub(super)` so the hook can live in its own file
  (`tool_loop/skills.rs`); nothing else of the tool loop changed.

## What would settle it

Each is a test that was run (the names are under `skills::` and
`tool_loop::skills::` in `a2a-protocol-server`, and `tests/tool_loop/skills.rs`):

* A skills directory with a valid skill, one with malformed frontmatter and one
  whose name disagrees with its directory: the first is loaded, the others are
  reported, and start-up succeeds
  (`malformed_frontmatter_is_skipped_with_the_reason_and_the_rest_loads`,
  `a_name_that_is_not_its_directorys_is_used_and_reported`). Also zero, one and
  300 skills, exact at the cap, a body over the bound, unicode names, duplicate
  names, a link to a skill directory, a loop of links, a directory that cannot be
  read, a file that is not a regular file, an alias bomb and a two-megabyte value.
* `activate_skill` rejects a name that is not in the catalog before any file is
  touched (`an_unknown_skill_is_refused_before_any_file_is_asked_for`); the reader
  refuses `../`, an absolute path and a symlink out of the directory
  (`a_parent_directory_an_absolute_path_and_scripts_are_each_refused`,
  `a_link_out_of_the_skill_is_refused`), and scripts, `SKILL.md`, a backslash, a
  NUL, an oversized file and a file that is not UTF-8.
* Through a real `RequestHandler`: a model activates a skill, reads one of its
  files and answers, and a model asked for a file outside the skill gets an error
  it reads and the task completes
  (`tests/tool_loop/skills.rs`).
* The calls are counted and timed as tool calls
  (`activations_and_reads_are_counted_and_timed_as_tool_calls`) and held to the
  turn limit (`skill_calls_are_held_to_the_turn_limit`).
* Mutation testing on the path confinement and the rest of the module: see the
  pull request.
* **Not run:** a live run, a small model (Qwen or Gemma through llama.cpp, as the
  `live-model` example does) given two skills and a request that matches one,
  activating that one and not the other, the run counted. It is the maintainer's
  to run on the merged tree.

## Built

* `skills::SkillCatalog`, `SkillsConfig`, `SkillSource`, `SkillScope`, `Skill`,
  `SkillDiagnostic`, `Severity`, `DiagnosticKind`, `SPEC_SNAPSHOT`, the four
  `DEFAULT_MAX_*` bounds, `ActivateSkillTool` and `ReadSkillFileTool`.
* `ToolLoopExecutor::with_skills`.
* The book page "Agent Skills" (`building-agents/tool-loop/skills.md`), with code
  that compiles in `book-tests`.
* The fuzz target `skill_frontmatter`.

## Not built

* Running a script, and a sandbox to do it in.
* Hot reload, and a command-line validator.
* Reading anything outside `references/` and `assets/`, or a binary file (a model
  cannot use one in a text result).
* Skill discovery deeper than a directory's children, from the user's home or the
  current directory by convention (`~/.agents/skills`), or from a remote
  registry: the operator names each directory.
* NFKC normalisation of names (the reference validator applies it): a name is
  compared as written, so a directory name in another normalisation form is
  reported as a mismatch and the skill is used.
* Unknown frontmatter keys as errors (the reference validator does): they are
  warnings.
* A `skill.md` in lower case (the reference parser accepts it): only `SKILL.md`.
* User-explicit activation (`/skill-name`), protection of a skill's text from
  context compaction (the loop does not compact), de-duplication of repeated
  activations, and running a skill in a subagent through `Delegation`.
* A model-visible description of `compatibility` beyond a line in the activation.

## Not verified

* A live model's use of the catalog and the tools (above).
* Windows and macOS: the tests that need a symbolic link are `#[cfg(unix)]` and the
  path tests are written to hold on both, but only Linux was run. The Windows
  branch of `identity` returns a constant, which is what the "remains" list
  above says; no Windows test exercises it.
* The repository's tags and releases (no access), so "no versioned releases" is
  the 2026-11-01 triage's statement plus the absence of a version on the pages
  read on 2026-11-07.
* `cargo deny` (not installed) for any crate named in decision 9.
