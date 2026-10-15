<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Issue draft for `a2aproject/A2A` — §5.7's non-empty required arrays and an empty `ListTasksResponse.tasks`

**Status: DRAFT, not sent.** Written 2026-10-15 against A2A v1.0.1 and its `a2a.proto`. Audit register, the §5.7 row.

---

Title: §5.7's "required arrays MUST contain at least one element" contradicts an empty `ListTasksResponse.tasks`

A2A v1.0.1 §5.7 (Field Presence and Optionality): "Fields marked with
`[(google.api.field_behavior) = REQUIRED]` indicate that the field **MUST**
be present and set in valid messages. [...] Arrays marked as required
**MUST** contain at least one element."

a2a.proto (v1.0.1), `ListTasksResponse`:

    // Array of tasks matching the specified criteria.
    repeated Task tasks = 1 [(google.api.field_behavior) = REQUIRED];

A `ListTasks` call whose filters match no task has no task to return. Read
together, the two texts make the only correct answer to such a call an
invalid message. An implementation must either break §5.7 (return an empty
`tasks`) or invent an error for "no matches", which §3.1.4 does not define.

a2a-rust returns an empty `tasks` and records the conflict. Could the
specification state that a required repeated field may be empty where the
operation's result is legitimately empty, or drop `REQUIRED` from
`ListTasksResponse.tasks` (as `next_page_token`'s comment already allows
"empty if there are no more results")?
