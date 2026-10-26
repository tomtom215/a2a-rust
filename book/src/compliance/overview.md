# Compliance Overview

a2a-rust is a library. Most of what the EU AI Act, the GDPR and the Cyber
Resilience Act ask for falls on whoever builds and runs a system or a product
with it, not on the SDK. This section says, regime by regime, what the SDK
carries toward each obligation, what it leaves to you, and where it falls
short.

> **Not legal advice, and not a claim of compliance.** Nothing in this section
> says that the SDK, or anything built with it, complies with any law. It says
> what the SDK provides, with the test or CI step that proves each part, and
> what is left for you. Whether your system is in scope, and what you must do,
> is for you and your counsel.

## Where the SDK sits

- **EU AI Act.** The SDK is not an AI system and not a provider of one
  (Regulation (EU) 2024/1689, Articles 3(1) and 3(3)): it carries messages
  between agents and infers nothing. Whether the system *you* build is
  high-risk is yours to decide. See [EU AI Act](../deployment/ai-act-classification.md).
- **GDPR.** The SDK is neither a controller nor a processor. It is software
  that one of them runs. See [GDPR](./gdpr.md).
- **Cyber Resilience Act.** The SDK is a component a manufacturer builds into a
  product. The maintainer's position, in `SECURITY.md`, is that the crates are
  free software published by an individual and not monetised, which Recital 18
  treats as not made available in the course of a commercial activity, and
  that an open-source software steward is a legal person, which an individual
  is not. The project publishes the evidence a manufacturer would want from a
  supplier anyway. If that position changes, so does the page. See
  [Cyber Resilience Act](./cyber-resilience-act.md).

## Which page answers which question

| You need to… | Read |
|---|---|
| Decide whether your agent is a high-risk AI system | [EU AI Act](../deployment/ai-act-classification.md) |
| Keep tamper-evident logs of what agents did (AI Act Articles 12, 19, 26(6); GDPR Article 5(2)) | [Audit Trail](../deployment/audit.md) |
| Put a person between an agent and an action, or stop an agent (AI Act Article 14; GDPR Article 22) | [Human Oversight](../deployment/oversight.md) |
| Mark AI-generated content and sign what an agent sends (AI Act Article 50(2)) | [Provenance and Signatures](../deployment/provenance.md) |
| Store, protect, retain and erase personal data | [GDPR](./gdpr.md) |
| Show supply-chain and vulnerability-handling evidence for a product that embeds the SDK | [Cyber Resilience Act](./cyber-resilience-act.md) |
| Keep one tenant's data from another's | [Multi-Tenancy](../deployment/multi-tenancy.md) |
| See what is protected at rest, and the known gaps | [Security](../deployment/security.md) |

## What the control map holds

The [control map](https://github.com/tomtom215/a2a-rust/blob/main/docs/compliance/control-map.md)
traces 88 provisions to what the SDK provides. It is generated from
`docs/compliance/controls.toml`, and CI fails if a cited test does not exist
as a test, a cited file or symbol is missing, or the rendered map differs from
its source, so a row cannot outlive its evidence. It was last dated 2026-10-23.

| Framework | Provisions | Supported | Documented | Partial | Yours |
|---|---|---|---|---|---|
| EU AI Act (Regulation (EU) 2024/1689 as amended by 2026/1744) | 32 | 7 | 1 | 4 | 20 |
| Cyber Resilience Act (Regulation (EU) 2024/2847) | 25 | 4 | 3 | 13 | 5 |
| GDPR (Regulation (EU) 2016/679) | 22 | 2 | 0 | 7 | 13 |
| OWASP Top 10 for Agentic Applications (2026) | 7 | 1 | 0 | 5 | 1 |
| ISO/IEC 42001:2023 | 2 | 1 | 0 | 0 | 1 |
| **Total** | **88** | **15** | **4** | **29** | **40** |

What the statuses mean:

- **Supported.** The SDK carries its part, and the row cites a test or a CI
  step that proves it.
- **Documented.** The project's documentation carries it, and the row names the
  file.
- **Partial.** The SDK carries some of it. The row says what is left.
- **Yours.** The obligation is yours by nature, such as a notice to the people
  whose data you hold, or a risk assessment. The row says what the SDK offers
  toward it.

The map also has a **Gap** status, for a provision the SDK does not serve at
all. None is marked that way. Read that with the Partial column: 29 rows say
that part of an obligation is not met, and each says which part.

## Several protections are off until you switch them on

This is the point that most often separates what the SDK can do from what a
deployment does. These are the measures the map's rows name as off by default:

| Protection | How you turn it on | Why it matters |
|---|---|---|
| Audit trail | `RequestHandlerBuilder::with_audit` (the `audit` feature) | Without it nothing records who called what |
| Pseudonymised audit records | `AuditLog::with_privacy` (`audit` and `encryption`) | By default audit records name each caller in the clear |
| Encryption of stored tasks | `EncryptedTaskStore` (the `encryption` feature) | The SQL stores hold task content in the clear until you wrap them |
| Retention | Call `purge_expired` and `AuditLog::purge` | The SQLite and PostgreSQL task stores and the audit store keep everything until you purge |
| TLS on the server | `Server::with_tls`, which needs the `tls-rustls` feature (on by default in `a2a-protocol-sdk`, off in `a2a-protocol-server`), or terminate TLS in front of the server | The server's own listeners have no TLS until you enable it |
| Rate limiting and the failed-credential throttle | Install their interceptors | Neither applies until installed |
| `https`-only webhooks | An `https_only()` push sender | Push delivery accepts `http://` webhooks by default |
| Authentication | An authenticating interceptor, and a card that declares its security requirement | An agent whose card declares no requirement serves every caller |

What *is* on by default: message content stays out of spans (the executor span
carries none unless you opt in), audit records hold content only as digests,
the in-memory task store evicts terminal tasks after an hour, and a store that
cannot keep tenants apart refuses a tenant instead of sharing.

## Other frameworks

The map also covers two frameworks that are not law:

- **OWASP Top 10 for Agentic Applications (2026).** Six of the ten categories
  have rows (identity and privilege abuse, supply chain, inter-agent
  communication, cascading failures, human-agent trust, rogue agents), and four
  (ASI01, ASI02, ASI05, ASI06) are about the model and are marked yours. The
  identifiers and names were checked against three secondary summaries,
  because OWASP publishes the list as a download.
- **ISO/IEC 42001:2023.** A management-system standard: an organisation is
  certified against it, a library is not. The map has one row that the SDK
  supports, event logs (Annex A.6.2.8), and one that is yours, the management
  system itself. The Annex A numbers come from secondary sources that disagree
  on some of them, so check them against the published standard.

## Where to start

1. Decide your position under the AI Act with
   [EU AI Act](../deployment/ai-act-classification.md).
2. If you hold personal data, read [GDPR](./gdpr.md), then switch on what the
   table above says you need.
3. If you sell a product that embeds the SDK, read
   [Cyber Resilience Act](./cyber-resilience-act.md).
4. For any provision, open the
   [control map](https://github.com/tomtom215/a2a-rust/blob/main/docs/compliance/control-map.md):
   each row names the code, the test and what is left.
