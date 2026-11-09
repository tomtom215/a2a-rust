<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Agent Skills

[Agent Skills](https://agentskills.io) is a format for giving an agent
reusable procedures: a directory with a `SKILL.md`, and optionally
`scripts/`, `references/` and `assets/`. The `skills` feature lets a
[tool loop](../tool-loop.md) use them
([ADR 0035](https://github.com/tomtom215/a2a-rust/blob/main/docs/adr/0035-agent-skills.md)):
the model is told each skill's name and description, loads a skill's
instructions when a task matches, and reads the files the skill bundles when
the instructions say to. **No script is ever run.**

An Agent Skill is not A2A's `AgentSkill`. That is an entry on an agent card,
which tells another agent what this one can do; an Agent Skill is something
this agent loads to know how to do it. The two meet in one place, an
[optional card entry](#advertising-skills-on-the-agent-card).

```toml
[dependencies]
a2a-protocol-sdk = { version = "0.16", features = ["skills"] }
```

The feature is off by default, implies `tool-loop`, and adds no crate to the
dependency graph.

## A skills directory

Each subdirectory of a skills directory that holds a `SKILL.md` is a skill. The
directory's name should be the skill's `name`:

```text
/etc/billing-agent/skills/
├── refunds/
│   ├── SKILL.md
│   ├── references/policy.md
│   └── scripts/refund.sh          listed to the model as present; never run
└── invoices/
    └── SKILL.md
```

```markdown
---
name: refunds
description: How to refund a payment. Use when a customer asks for their money back.
---
# Refunds

Read references/policy.md first, then look the payment up.
```

## The builder call

```rust,no_run
use a2a_protocol_sdk::server::skills::{SkillCatalog, SkillSource, SkillsConfig};
use a2a_protocol_sdk::server::tool_loop::{OpenAiChatModel, ToolLoop, ToolLoopExecutor, ToolSet};

# fn main() -> Result<(), Box<dyn std::error::Error>> {
let catalog = SkillCatalog::discover(
    &SkillsConfig::new().with_source(SkillSource::operator("/etc/billing-agent/skills")),
);
// A skill that is missing from the catalog is explained here and nowhere else.
for problem in catalog.diagnostics() {
    eprintln!("{problem}");
}

let model = OpenAiChatModel::new("http://127.0.0.1:8080/v1", "qwen3:1.7b")?;
let executor = ToolLoopExecutor::new(
    ToolLoop::new(model, ToolSet::new()).with_preamble("You answer billing questions."),
)
.with_skills(&catalog)?;
# drop(executor);
# Ok(())
# }
```

`discover` reads files, so call it at start-up (or from a blocking task). It
never fails and never panics: a directory that cannot be read, a malformed
`SKILL.md` and a name that is taken are each a `SkillDiagnostic`, and every
other skill is still loaded. `with_skills` is called on the executor, after the
loop has its preamble, and returns `DuplicateTool` if the loop already has a tool
called `activate_skill` or `read_skill_file`.

## What the model sees

The catalog goes at the end of the system prompt, after a blank line, in the form
the specification's client guide shows. A description is one line, at most 400
characters, and escaped, so it cannot close the element it is in:

```text
The following skills provide specialized instructions for specific tasks.
When a task matches a skill's description, call the activate_skill tool with the skill's name to load its full instructions before you proceed.
A skill may bundle files under references/ and assets/; read one with the read_skill_file tool, naming the skill and the path relative to its directory.
Skill scripts are never run by this agent.

<available_skills>
<skill>
<name>invoices</name>
<description>How to issue an invoice.</description>
</skill>
<skill>
<name>refunds</name>
<description>How to refund a payment. Use when a customer asks for their money back.</description>
</skill>
</available_skills>
```

It also gets two tools. `activate_skill` takes a `name` that is a JSON Schema
`enum` of the skills' names, so the model cannot ask for a path. It returns the
skill's instructions, then the files the skill bundles, then the scripts that are
present and not run:

```text
<skill_content name="refunds">
# Refunds

Read references/policy.md first, then look the payment up.

Paths in this skill are relative to the skill's directory; read a file with read_skill_file.
<skill_resources>
  <file>references/policy.md</file>
</skill_resources>
These scripts are present. This agent cannot run them; do not try to call them.
<skill_scripts>
  <file>scripts/refund.sh</file>
</skill_scripts>
</skill_content>
```

`read_skill_file` takes a `skill` (an enum of the skills that bundle files) and a
`path` such as `references/policy.md`, and returns the file's text. Neither tool
is added when there is nothing to offer: no skills, no catalog and no
`activate_skill`; no skill with a file, no `read_skill_file`.

Both are tools like any other. A call is an `execute_tool` span, is one of the
run's `gen_ai.invoke_agent.tool_calls`, counts against the loop's turn limit and
the caller's budget, and is shown in the `tool-trace` artifact. Both are
read-only, so asking twice runs twice.

## What is and is not executed

| | |
|---|---|
| `SKILL.md`'s instructions | Given to the model, as text. |
| `references/` and `assets/` | Read, one file at a time, when the model asks, as text. |
| `scripts/` | Listed as present. Never run, and never returned by `read_skill_file`. |
| `allowed-tools` | Recorded (`Skill::allowed_tools`) and never used. No skill adds a tool to the loop or approves one. |
| A binary file | Refused: a model cannot use one in a text result. |

Running scripts would need a sandbox this SDK does not have, and a directory
someone can write to is a way to put instructions in front of a model; the two
together are a way to run someone else's code. See
[Prompt-Injection Containment](https://github.com/tomtom215/a2a-rust/blob/main/docs/injection-containment.md).

## Trust and the security model

**An operator directory** (`SkillSource::operator`) is the operator's own text.

**A project directory** (`SkillSource::project`), such as a repository's
`.agents/skills`, is not read at all until the operator says so:

```rust,no_run
use a2a_protocol_sdk::server::skills::{SkillSource, SkillsConfig};

# fn main() {
let config = SkillsConfig::new()
    .with_source(SkillSource::operator("/etc/billing-agent/skills"))
    .with_source(SkillSource::project("/srv/checkout/.agents/skills").with_trusted(true));
# drop(config);
# }
```

A trusted project's skills are still text that anyone who can commit to the
repository wrote, and it still reaches the model. Nothing in the SDK can tell a
hijacked model's instruction from the operator's, so this module does not claim to
filter it. What it does is give that text no capability the operator's text has not:

- Operator directories are read first, so a project skill **never takes the name of
  an operator's**. (The specification's client guide says the project's wins, for
  a client that trusts its project.)
- A project skill is marked `<origin>project</origin>` in the catalog and
  `origin="project"` when activated. That is a label for the model and not a guard.
- `allowed-tools` is never honoured and `scripts/` are never run, for any scope.
- Whatever else the model can do is what you gave it, behind the controls the
  containment document lists: tools that only read declared `read_only`, idempotency
  keys, a token budget, an approval gate in front of anything a person must confirm.
  A skill cannot widen them.

**The reader.** `read_skill_file` checks the path as text (no NUL, backslash or
`..`, nothing absolute, and a first component of `references` or `assets`), resolves
it on disk and requires it to stay inside that directory, so a link that points
outside it, or into `scripts/` or `SKILL.md`, is refused. It then opens the file and
checks what it opened against the path (on Unix, that it is the same file). A file
over the size bound and a file that is not UTF-8 are refused, never truncated.

**What is left of a race.** Resolving a path and opening it are two steps, and
someone who can write inside the skill's directory could change it between them.
The check after the open closes that window on Unix. It stays open on a platform
without Unix file identities (Windows) for a link swapped in and out again within
that interval, which needs a privilege most processes lack; a hard link names any
file its maker can read; and a file replaced by a FIFO between the check and the
open holds one blocking-pool thread until something opens the other end. A skills
directory is expected to be written by its operator or its project's authors, not by
the process's other tenants.

**Links.** A symbolic link in a skills directory that points at a skill directory is
not followed unless the source says `with_follow_symlinks(true)`; a followed link
still cannot take a read out of the skill's own directory. A `references/`,
`assets/` or `scripts/` that is itself a symbolic link is neither listed to the model nor
readable, and discovery says so with a warning.

## Limits

Each is set on `SkillsConfig`, and a value over its bound is reported rather than
truncated into nonsense:

| Bound | Default | Over it |
|---|---|---|
| Skills in the catalog | 32 | The first 32 in name order (operator directories first) are read; the rest are not, with one diagnostic saying how many. |
| A description in the prompt | 400 characters | Cut, with an ellipsis. The whole description is in the activation result. |
| A skill's instructions | 64 KiB | The skill is skipped: half a procedure is worse than none. |
| A file `read_skill_file` returns | 64 KiB | Refused. |
| A frontmatter | 8 KiB | The skill is skipped, before the frontmatter is parsed. |
| Entries examined in one skills directory | 1,024 | The rest are not examined. |
| Files listed in each of `references/`, `assets/`, `scripts/` | 100, three directories deep | Listing stops. |

```rust,no_run
use a2a_protocol_sdk::server::skills::SkillsConfig;

# fn main() {
let small_model = SkillsConfig::new()
    .with_max_skills(8)
    .with_max_description_chars(200)
    .with_max_body_bytes(16 * 1024)
    .with_max_resource_bytes(16 * 1024);
# drop(small_model);
# }
```

## What discovery says

`SkillCatalog::diagnostics` lists what was noticed, each with a severity (`Info`,
`Warning` or `Error`), a `DiagnosticKind` to match on, the path it concerns and a
message. The lenient rules (ADR 0035, decision 2):

- **Used, with a warning:** a name that is not its directory's (the skill is used
  under its `name`), over 64 characters (up to 256), with an uppercase letter, an
  underscore or a dot, or a hyphen at an end or two together; a description over 1024
  characters; instructions over 500 lines; a plain value holding `: `; an optional
  field that is empty, too long or not a string (the field is ignored); a field the
  specification does not define; a duplicate field (the first is used).
- **Skipped, with the reason:** no frontmatter, or one that is not closed, is too
  large or is not `key: value` lines; a missing, empty or non-string `name` or
  `description`; a name with a character a prompt cannot carry (anything but
  letters, digits, `-`, `_` and `.`); instructions over the bound; a file that is not
  UTF-8 or not a regular file inside its directory; a name already taken.

The frontmatter is read as the subset of YAML the specification's reference parser
accepts: plain, quoted and block scalars, and one level of mapping for `metadata`. A
sequence, a flow collection, an anchor, an alias or a tag in a field is not read:
it costs nothing in a field the module does not use, and skips the skill if it is in
`name` or `description`. An alias is never expanded.

## Advertising skills on the agent card

Off unless you do it: a card is public and a skill's description is not
necessarily meant to be.

```rust,no_run
use a2a_protocol_sdk::prelude::*;
use a2a_protocol_sdk::server::skills::SkillCatalog;

# fn main() {
# let catalog: SkillCatalog = unimplemented!();
let mut card = AgentCard::new(
    "billing-agent",
    "1.0.0",
    AgentInterface::jsonrpc("http://127.0.0.1:3000"),
);
for skill in catalog.agent_skills() {
    card = card.with_skill(skill);
}
# drop(card);
# }
```

Each skill is an `AgentSkill` whose `id` and `name` are the skill's name, whose
`description` is the whole description, verbatim, and whose one tag is
`agent-skill`.

## What it does not do

- **Reload.** The catalog is a snapshot taken by `discover`, so the prompt and
  the tools' schemas are what the model was told. To pick up a change, discover
  again and build a new executor. A skill whose directory is no longer the one
  found has its file reads refused.
- **Run anything**, as above; or read outside `references/` and `assets/`.
- **Follow the specification past what it was read as.** The module follows the
  specification text as read on 2026-11-07 (`skills::SPEC_SNAPSHOT`); the format has
  no versioned releases, and a warning about a field it does not know says that
  date. It does not apply NFKC to names, treats an unknown field as a warning and
  not an error, and reads only `SKILL.md` in capitals.
- **Discover by convention.** It reads the directories you name, one level deep,
  not `~/.agents/skills` or the working directory's.

## Next steps

- **[The Tool Loop](../tool-loop.md)**: the loop skills go into
- **[Tools and idempotency](./tools.md)**: the controls that bound what a skill's text can make the model do
- **[Spans and metrics](./telemetry.md)**: where a skill call shows up
