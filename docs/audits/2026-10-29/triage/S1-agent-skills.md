<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Agent Skills: what the primary pages say

Read on 2026-10-31 by two web-fetch agents (a model's reading of the pages; the quotations are theirs and were not re-fetched by the lead). Pages: <https://www.anthropic.com/news/donating-agent-skills-to-the-agentic-ai-foundation>, <https://agentskills.io/home>, <https://agentskills.io/specification>, <https://github.com/agentskills/agentskills> (README, LICENSE, CONTRIBUTING). Not read: the `skills-ref` reference library, the Agentic AI Foundation's own page (the announcement links none), the MCP announcement. Decision X-6 (build it before 0.16.0 is tagged) is the maintainer's, in `DECISIONS.md`.

## The announcement

- Page metadata: "Published Oct 22, 2026".
- "Today, we're donating Agent Skills, the open standard that gives AI agents specialized knowledge and workflows, to the Agentic AI Foundation (AAIF)." Donated: "the Agent Skills name and logo, the GitHub organization, and the agentskills.io domain."
- The foundation: "the Agentic AI Foundation (AAIF) under the Linux Foundation, which we co-founded with Block and OpenAI last year, and to which we donated the Model Context Protocol (MCP)." This settles the question the first fetch left open (that fetch read only agentskills.io and the repository, neither of which says it).
- Governance: "Engineers from Anthropic, Google, OpenAI, and other companies will maintain the standard as individuals, with David Soria Parra and Mahesh Murag as Lead Maintainers." "As with MCP, the project will continue to prioritize community input and transparent decision-making."
- Versioning is a proposal, not a fact: "We've proposed starting with a roadmap covering versioned releases, a formal proposal process, and security guidance for people who write and install skills."
- Adoption: "More than 70 agent platforms, including ChatGPT, Cursor, GitHub Copilot, and VS Code, have integrated Agent Skills since we published the standard in December 2025."
- A2A, AGENTS.md: not seen on the announcement or the specification page.

## The format (agentskills.io/specification)

- A skill is a directory with a `SKILL.md`: YAML frontmatter, then Markdown. Optional `scripts/`, `references/`, `assets/`.
- Frontmatter: `name` (required; 1 to 64 characters; lowercase letters, digits and hyphens; no leading, trailing or consecutive hyphen; equal to the parent directory's name), `description` (required; 1 to 1024 characters; what the skill does and when to use it), `license`, `compatibility` (1 to 500 characters), `metadata` (a string-to-string map), `allowed-tools` (a space-separated string; "Experimental. Support for this field may vary between agent implementations").
- Progressive disclosure: name and description of every skill at startup (about 100 tokens each), the `SKILL.md` body on activation (under 5,000 tokens recommended; "Keep your main `SKILL.md` under 500 lines"), other files on demand. File references are relative to the skill root, one level deep.
- **No specification version or date is stated** on the specification page; the only "1.0" is an example value in a sample `metadata` block.
- The client guide (`agentskills.io/client-implementation/adding-skills-support`) is advisory: scan `<project>/.<client>/skills/`, `<project>/.agents/skills/`, `~/.<client>/skills/`, `~/.agents/skills/`; skip `.git` and `node_modules`; bound the scan (depth 4 to 6, at most 2,000 directories); a project skill overrides a user skill; "Consider gating project-level skill loading on a trust check"; parse leniently and warn ("Description is missing or empty: skip the skill"); a missing or unparseable frontmatter skips the skill.
- Scripts: the specification says only that `scripts/` "Contains executable code that agents can run" and that supported languages depend on the agent. **Not seen on any page read: guidance on sandboxing, approval, signing or vetting third-party skills or their scripts.** The announcement proposes "security guidance for people who write and install skills" as future work.

## Licence

README: "Code in this repository is licensed under Apache 2.0. Documentation is licensed under CC-BY-4.0." The announcement states none. `CONTRIBUTING.md` (read in the first fetch) says the specification is "still iterating" and that "large-scale redesigns are premature".

## What this means for the build

The format is real, donated and under a foundation, adopted widely; it has no version to pin, so pin the repository commit and re-check the specification text at each release of this SDK. The security gap (nothing says how to treat a downloaded skill's scripts) is the SDK's to close conservatively: parse and validate, never execute bundled scripts, load project-level skills only behind an explicit trust opt-in.
