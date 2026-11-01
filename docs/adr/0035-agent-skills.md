<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# ADR 0035: Agent Skills are adopted in the tool loop, without running their scripts

**Date:** 2026-11-01
**Status:** Proposed. Nothing here is built or run. The maintainer decided on
2026-11-01 (triage fork F9): write the ADR now and build a feature-gated module
in 0.17.0, with no script execution.
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
  catalog and an `activate_skill` tool would go.
* `Delegation` already runs a piece of work in another agent, which is what
  the format's "run a skill in a subagent" pattern is.
* `docs/injection-containment.md` records why text an agent reads from a place
  anyone can write is an instruction channel.

## Decision

1. **Where.** A feature-gated `skills` module in the server crate (its own
   feature, off by default), built in 0.17.0. A separate crate is the
   alternative if it needs dependencies the server crate should not carry; a
   YAML frontmatter parser is the likely one, and the dependency policy
   (`CONTRIBUTING.md`) decides then.
2. **Discovery and validation are lenient, with diagnostics.** A skill whose
   frontmatter is malformed or whose name does not match its directory is
   reported and, where it can be used safely, still used; one that cannot is
   skipped with the reason. The result is a catalog of `(name, description,
   location)` and a list of diagnostics, never a panic and never a refusal to
   start.
3. **Catalog in the prompt, body on demand.** The tool loop puts the catalog
   in the system prompt and registers an `activate_skill` tool whose `name`
   parameter is an **enum of the valid skill names**, so a model cannot ask for
   a path. The tool returns the skill's `SKILL.md` body.
4. **Resources are read through a confined reader.** A skill's `references/`
   and `assets/` files are read by a tool that takes a path relative to the
   skill's directory, resolves it, and refuses anything that leaves the
   directory (including through a symlink), a file over a size cap, and a
   non-UTF-8 file it was asked to return as text.
5. **No script execution.** `scripts/` is listed in the diagnostics as present
   and not run. Running them needs a sandbox this SDK does not have, and a
   directory an operator writes to is a prompt-injection channel
   (`docs/injection-containment.md`); the two together are a remote code
   execution path. Revisit only with a design for the sandbox.
6. **Trust by scope.** Skills from a directory the operator configured are
   trusted as the operator's own text. Skills from a project directory the
   process was pointed at (a repository's) are loaded only when the operator
   says so; their text goes through the same containment as any retrieved
   text.
7. **Advertising.** Optionally, each loaded skill is added to the agent card
   as an `AgentSkill` (id from the name, description verbatim). Off by default:
   a card is public and a skill's description is not necessarily meant to be.
8. **Pinned to a snapshot.** There is no version to pin, so the module names
   the date of the specification text it was written against in its docs and
   in the diagnostics it emits for fields it does not understand.

## Alternatives

| Option | Why not |
|---|---|
| Validation only (`a2a skills validate`) | Useful, and a subset of this; it does not give an agent the skills. Can ship first. |
| Run `scripts/` in a subprocess | No sandbox, and the directory is an injection channel. |
| Do not adopt | The format is becoming the common one for this; an agent built here would otherwise need an adapter per tool. Revisit if the standard stalls. |
| Treat Agent Skills as `AgentSkill` | They are different things; conflating them would put an agent's private procedures on its public card. |

## Consequences

* An `activate_skill` call is a tool call like any other: it is traced,
  counted and subject to the tool-loop's step limits.
* The catalog costs prompt tokens per skill. A cap on the number of skills and
  on each description's length is part of the module, so a directory of
  hundreds cannot fill the context.
* The module reads files at start-up and on activation. Reload is a later
  question.

## What would settle it (nothing is run)

* A skills directory with a valid skill, one with malformed frontmatter and one
  whose name disagrees with its directory: the first is loaded, the others are
  reported, and start-up succeeds.
* `activate_skill` rejects a name that is not in the catalog before any file
  is touched; the reader refuses `../`, an absolute path and a symlink out of
  the directory (one test each).
* A live run: a small model (Qwen or Gemma through llama.cpp, as the
  `live-model` example does) is given two skills and a request that matches
  one; it activates that one and not the other, and the run is counted.
* Mutation testing on the path confinement.

## Open

* Whether the module is in the server crate or its own.
* The caps (skills, description length, body size).
* Whether a skill can declare tools it needs (`allowed-tools` is experimental)
  and what the loop does with the declaration.
* Hot reload.
