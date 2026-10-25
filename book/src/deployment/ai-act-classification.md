<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# EU AI Act: Is Your Agent High-Risk?

The EU AI Act's obligations fall on providers and deployers of AI systems,
and most of them only on high-risk ones. The [Compliance Overview](../compliance/overview.md)
puts this page beside the GDPR and the Cyber Resilience Act. This chapter says where the SDK sits,
how a system you build with it is classified, and which of the SDK's features
answer which obligations once it is. It cites Regulation (EU) 2024/1689 as
published in the Official Journal (OJ L, 2024/1689, 12.7.2024).
Regulation (EU) 2026/1744 amends Article 6; read the consolidated text
before you rely on a paragraph quoted here.

## Where the SDK sits

The SDK is not an AI system. Article 3(1) defines one as a machine-based
system that infers from its input how to generate outputs; the SDK carries A2A
messages between agents and infers nothing. It contains no general-purpose AI
model (Article 3(63)): no published crate ships model weights or depends on
an inference runtime, which `scripts/check_no_model_artifacts.py` checks on
every CI run. The tool loop (`server::tool_loop`) calls a model you choose
over its HTTP API; the model is yours, not the SDK's.

So whether anything here is high-risk is a question about the system you build,
and it is yours to answer and document.

## The two routes to high-risk (Article 6)

- **Article 6(1), Annex I.** Your system is a safety component of a product,
  or is itself a product, covered by the Union harmonisation legislation
  listed in Annex I, and that product must undergo a third-party conformity
  assessment under it.
- **Article 6(2), Annex III.** Your system is used in one of the areas Annex
  III lists.

Article 6(3) lets an Annex III system escape the label when it poses no
significant risk of harm to health, safety or fundamental rights: it performs
a narrow procedural task, improves the result of a completed human activity,
detects decision-making patterns without replacing human assessment, or
performs a preparatory task. **An Annex III system that profiles natural
persons is always high-risk.**

Article 6(4): if you decide an Annex III system is not high-risk, you document
that assessment before placing it on the market, register under Article
49(2), and give the documentation to national competent authorities when they
ask.

None of this can be decided from the code. It turns on the intended purpose
you give the system, which the SDK does not know.

## If it is high-risk: what the SDK gives you

These features help you meet the obligations. They do not meet them for you,
and nothing here is a conformity assessment.

| Obligation | What the SDK provides | Chapter |
|---|---|---|
| Article 12, record-keeping | With the `audit` feature, a tamper-evident audit trail of every call, task transition and oversight action | [Audit Trail](./audit.md) |
| Article 14, human oversight | Halting a tenant or server, approval before an action, a quorum of approvers (14(5)), cascading cancellation | [Human Oversight](./oversight.md) |
| Article 50(2), marking output (whatever the risk class) | A machine-readable AI-generated marker in A2A metadata, and signed content; not a watermark, and it does not travel with a saved file | [Provenance and Signatures](./provenance.md) |
| Article 15, accuracy, robustness and cybersecurity | Authentication, limits, refusal of malformed input, signed cards | [Security](./security.md) |

Article 50(2) binds providers of systems that generate synthetic audio,
image, video or text whether or not they are high-risk, from 2 August 2026
(2 December 2026 for a generative system already on the market then,
Article 111(4)). The control map rates the SDK's part Partial.

Each obligation is traced to the code and the test that proves the SDK's part
in the [control map](https://github.com/tomtom215/a2a-rust/blob/main/docs/compliance/control-map.md),
with what is left to you.

## What stays yours

The classification under Article 6 and any Article 6(4) assessment;
registration (Article 49); the conformity assessment (Article 43); the quality
management system (Article 17); and the risk management, data governance and
technical documentation the Regulation asks of a high-risk system's provider.
The SDK's records and controls are evidence you can use for these, not a
substitute for them.
