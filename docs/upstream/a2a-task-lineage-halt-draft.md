<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Draft: a task's lineage, halting one job, and integers in extension metadata

**Status: draft, not sent.** Intended for a discussion in `a2aproject/A2A`.
Written 2026-11-05 against A2A v1.0.1 (`docs/implementation/a2a-v1.0.1-specification.md`).
Sending it is the maintainer's decision.

---

## Why

A2A has no link from a task to the tasks its agent starts on other agents. A
job (a tree of tasks across agents) cannot be named, so it cannot be stopped as
one: an operator can stop one task or one agent, not "everything this request
started". A search of v1.0.1 and `a2a.proto` for a parent task, a root or a
correlation id across agents finds none (`referenceTaskIds` names related
tasks in the *same* context, chosen by the client, and carries no order).

## What we built, as an extension

`https://a2a-rust.com/extensions/lineage/v1`, in `Message.metadata` under
`a2a-rust.com/lineage`:

```json
{ "root": "job-42", "depth": 2, "parentTaskId": "t-9" }
```

* `root` names the job; a task sent without a lineage is the root of its own
  job, named by its `contextId`. `depth` counts hops from the first task, capped
  at 16. `parentTaskId` is the delegating task's id on its own agent.
* A receiver fixes a task's lineage when the task is created, records it in
  `Task.metadata`, and refuses a continuation naming another root.
* An operator halts a root: the job's running tasks are stopped and its new
  sends refused, on every replica sharing a halt store, while the rest of the
  tenant runs on. The refusal carries `google.rpc.ErrorInfo` with reason
  `HALTED` in our own domain, `a2a-rust.com`.

Schema and conformance vectors:
`crates/a2a-protocol-types/extensions/lineage-v1/`. Wire contract for other
SDKs: the book page "Swarm Extensions: the Wire Contract". Interop: the
a2a-python and a2a-go clients carry a lineage to our server in
`itk/interop/`.

## What we are asking

1. Whether the project wants a parent-task link in the protocol itself (a
   `Message`/`Task` field, or a standard extension), so that tools can draw a
   job's tree across agents without each SDK inventing one. Our extension is
   offered as a starting point, not as the answer.
2. Whether "stopped by an operator" deserves a standard error reason. Today
   a halt is `-32603` / `UNAVAILABLE` with our own `HALTED` reason; a client
   cannot tell it from a fault without reading our domain, and may retry it.
3. A sentence of guidance for extension authors about numbers: `metadata` is a
   protobuf `Struct`, whose only number is a double, so an SDK that keeps
   metadata that way (a2a-python 1.2.3 does) sends `30000` as `30000.0`.
   Proposed text for the extensions section: "An extension that carries an
   integer in `metadata` SHOULD state that a receiver reads a JSON number with
   a whole value no larger than 2^53 as that integer, whether or not it is
   written with a fraction part." We found this when a lineage depth of `2`
   from a2a-python reached us as `2.0` and was refused; our lease and budget
   integers had the same fault.

## Known weaknesses

* A lineage is the caller's claim; it scopes halts and labels telemetry and
  must authorize nothing.
* Across a hop that shares no halt store, a halt travels only as the
  parent's cancels; an agent that does not cascade cancels ends it there.
