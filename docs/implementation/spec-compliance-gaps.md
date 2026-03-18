# A2A v1.0.0 Spec Compliance Gaps

> **Historical Reference** — All gaps have been resolved (Phase 7.5). This document is retained as a record of the compliance verification process.

**Created:** 2026-03-15
**Verified against:**
- Proto: <https://a2a-protocol.org/latest/spec/a2a.proto>
- JSON Schema: <https://a2a-protocol.org/latest/spec/a2a.json>

**Methodology:** Field-by-field comparison of every Rust type against both the proto and JSON schema definitions. Each gap was confirmed in both sources unless noted.

---

## Community Landscape

Before this project, no Rust SDK targeting A2A v1.0.0 existed:

| Project | Version | Status | License | Notes |
|---|---|---|---|---|
| [a2a-rs](https://github.com/EmilLindfors/a2a-rs) | v0.3.0 | Active (81 stars) | — | Full v0.3.0 coverage; hexagonal architecture; no v1.0 support |
| [A2A](https://github.com/robert-at-pretension-io/A2A) | unspecified | Moderate (22 stars) | GPL-3.0 | Testing framework / validator; 358 commits |
| **a2a-rust** (this project) | **v1.0.0** | **Active** | Apache-2.0 | First Rust SDK targeting v1.0.0; gaps documented below |

---

## Gap Summary

| Severity | Count | Description |
|---|---|---|
| **CRITICAL** | 4 | Wire-format breaking — incorrect JSON on the wire |
| **HIGH** | 5 | Missing fields or types — incomplete API surface |
| **LOW** | 1 | Extra field not in spec (harmless) |
| Acceptable | 5 | Type differences that are correct for JSON binding |

---

## CRITICAL — Wire-format breaking

These produce or expect incorrect JSON and MUST be fixed before any public release.

### C1. `TaskState::Pending` should be `Submitted`

**File:** `crates/a2a-types/src/task.rs`

Our variant `Pending` serializes as `"TASK_STATE_PENDING"`. The proto and JSON schema both define value 1 as `TASK_STATE_SUBMITTED`.

```rust
// CURRENT (wrong):
#[serde(rename = "TASK_STATE_PENDING")]
Pending,

// CORRECT:
#[serde(rename = "TASK_STATE_SUBMITTED")]
Submitted,
```

**Impact:** Any task created with our SDK sends the wrong state string. Other SDKs will fail to deserialize it.

**Fix:** Rename variant back to `Submitted`, update serde rename, update all references and tests.

### C2. `SecurityRequirements` type structure is wrong

**File:** `crates/a2a-types/src/security.rs`

Our type alias:
```rust
pub type SecurityRequirements = Vec<HashMap<String, Vec<String>>>;
```

The proto defines:
```protobuf
message SecurityRequirement {
    map<string, StringList> schemes = 1;
}
message StringList {
    repeated string list = 1;
}
```

**Wire format difference:**

| Source | JSON |
|---|---|
| Ours | `[{"oauth2": ["read", "write"]}]` |
| Spec | `[{"schemes": {"oauth2": {"list": ["read", "write"]}}}]` |

**Impact:** Agent cards with security requirements will be unreadable by other SDKs, and vice versa.

**Fix:** Define proper `SecurityRequirement` and `StringList` structs. Replace the type alias.

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StringList {
    pub list: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecurityRequirement {
    pub schemes: HashMap<String, StringList>,
}
```

### C3. `AgentCard.security` field name → `securityRequirements`

**File:** `crates/a2a-types/src/agent_card.rs:188`

```rust
// CURRENT (serializes as "security"):
pub security: Option<SecurityRequirements>,

// CORRECT (serializes as "securityRequirements"):
pub security_requirements: Option<Vec<SecurityRequirement>>,
```

**Impact:** Agent card JSON has `"security"` instead of `"securityRequirements"`. Other SDKs ignore it; our SDK can't read their cards.

### C4. `AgentSkill.security` field name → `securityRequirements`

**File:** `crates/a2a-types/src/agent_card.rs:129`

Same issue as C3 but on `AgentSkill`.

```rust
// CURRENT (serializes as "security"):
pub security: Option<SecurityRequirements>,

// CORRECT:
pub security_requirements: Option<Vec<SecurityRequirement>>,
```

---

## HIGH — Missing fields or types

These represent incomplete API surface. Other SDKs can send values we can't parse, or we can't send values they expect.

### H1. `MessageRole` missing `ROLE_UNSPECIFIED`

**File:** `crates/a2a-types/src/message.rs`

Proto defines three values; we only have two:

```rust
// MISSING:
#[serde(rename = "ROLE_UNSPECIFIED")]
Unspecified,
```

**Impact:** Deserializing a message with `"role": "ROLE_UNSPECIFIED"` from another SDK will fail.

### H2. `ListTasksParams` missing `history_length`

**File:** `crates/a2a-types/src/params.rs`

Proto `ListTasksRequest` includes `optional int32 history_length = 6`. Our `ListTasksParams` does not have this field.

```rust
// ADD:
#[serde(skip_serializing_if = "Option::is_none")]
pub history_length: Option<u32>,
```

**Impact:** Clients cannot request truncated history when listing tasks.

### H3. `PasswordOAuthFlow` missing

**File:** `crates/a2a-types/src/security.rs`

Proto and JSON schema both define `PasswordOAuthFlow` (deprecated but present). Our `OAuthFlows` struct is missing the `password` field.

```rust
// ADD to OAuthFlows:
#[serde(skip_serializing_if = "Option::is_none")]
pub password: Option<PasswordOAuthFlow>,

// ADD new struct:
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PasswordOAuthFlow {
    pub token_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_url: Option<String>,
    pub scopes: HashMap<String, String>,
}
```

**Impact:** Cannot deserialize agent cards that use the password OAuth flow.

### H4. No `ListPushConfigsParams` struct

**File:** `crates/a2a-types/src/params.rs`

Proto defines `ListTaskPushNotificationConfigsRequest` with `task_id`, `page_size`, `page_token`, and `tenant`. Our client uses inline `serde_json::json!()` instead.

```rust
// ADD:
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListPushConfigsParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tenant: Option<String>,
    pub task_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_size: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_token: Option<String>,
}
```

### H5. `list_push_configs` return type lacks pagination

**File:** `crates/a2a-client/src/methods/push_config.rs`

Proto `ListTaskPushNotificationConfigsResponse` includes `next_page_token`. Our client returns `Vec<TaskPushNotificationConfig>`, discarding pagination.

**Fix:** Return `ListPushConfigsResponse` (or extend existing `TaskListResponse` pattern) that includes `next_page_token`.

---

## LOW — Extra field not in spec

### L1. `AgentCapabilities.state_transition_history` not in spec

**File:** `crates/a2a-types/src/agent_card.rs:63`

We added this in Phase 7 but it does not appear in the proto or JSON schema. With `skip_serializing_if = "Option::is_none"` it's harmless — it won't appear on the wire when `None`, and will be silently ignored by other SDKs if set. **Remove to avoid confusion.**

---

## Acceptable Differences

These were reviewed and are correct for the HTTP+JSON protocol binding.

| # | Difference | Why it's OK |
|---|---|---|
| A1 | `u32` instead of `int32` for page_size, history_length | Negative values are nonsensical; JSON numbers are the same either way |
| A2 | `serde_json::Value` instead of proto `Struct` | JSON is a superset; `Value` handles all JSON types. Proto `Struct` is specifically for JSON objects, but in practice agents send arbitrary JSON |
| A3 | `timestamp: Option<String>` instead of proto `Timestamp` | ProtoJSON serializes `google.protobuf.Timestamp` as an RFC 3339 string — same wire format |
| A4 | `OAuthFlows` as struct with optional fields instead of proto `oneof` | JSON schema defines OAuthFlows as an object with optional properties (not oneOf). Our struct matches the JSON binding correctly. Proto `oneof` is a gRPC-specific constraint |
| A5 | `TaskVersion` newtype exists but is not in spec | Internal convenience type; never appears on the wire |

---

## Implementation Plan

### Phase 7.5 — Spec Compliance Fixes

Recommended implementation order (each step is independently testable):

**Step 1: Fix `TaskState` naming (C1)**
- Rename `Pending` → `Submitted`, update serde rename to `TASK_STATE_SUBMITTED`
- Update all references in handler, tests, echo-agent
- Run tests — several will need updating

**Step 2: Fix `SecurityRequirement` type (C2)**
- Define `StringList` and `SecurityRequirement` structs
- Replace `SecurityRequirements` type alias
- Update `AgentCard` and `AgentSkill` field types

**Step 3: Fix field names (C3, C4)**
- Rename `AgentCard.security` → `security_requirements`
- Rename `AgentSkill.security` → `security_requirements`
- Update all call sites

**Step 4: Add missing enum variant (H1)**
- Add `MessageRole::Unspecified` with `ROLE_UNSPECIFIED` serde rename

**Step 5: Add missing fields (H2)**
- Add `history_length` to `ListTasksParams`

**Step 6: Add missing OAuth flow (H3)**
- Add `PasswordOAuthFlow` struct
- Add `password` field to `OAuthFlows`

**Step 7: Fix push config params/response (H4, H5)**
- Add `ListPushConfigsParams` struct
- Add `ListPushConfigsResponse` struct with `next_page_token`
- Update client method signature

**Step 8: Remove extra field (L1)**
- Remove `state_transition_history` from `AgentCapabilities`
- Update echo-agent and tests

**Step 9: Tests**
- Verify all serde roundtrips produce spec-compliant JSON
- Wire-format snapshot tests comparing against known-good JSON from other SDKs

**Estimated effort:** ~4-6 hours total across all steps.

---

## Verification Checklist

After all fixes, verify against these spec artifacts:

- [x] `TaskState` enum values match proto exactly
- [x] `SecurityRequirement` JSON matches `{"schemes":{"name":{"list":["scope"]}}}`
- [x] `AgentCard` JSON includes `"securityRequirements"` (not `"security"`)
- [x] `AgentSkill` JSON includes `"securityRequirements"` (not `"security"`)
- [x] `MessageRole` roundtrips `"ROLE_UNSPECIFIED"`
- [x] `ListTasksParams` includes `historyLength` in JSON
- [x] `OAuthFlows` accepts `"password"` flow in JSON
- [x] `ListPushConfigs` returns pagination tokens
- [x] `AgentCapabilities` does NOT include `stateTransitionHistory`
- [x] All 11 RPC methods work end-to-end with corrected types
- [x] Echo agent example still runs successfully
- [x] TCK wire format conformance tests validate all fixes (ADR 0007, `tck_wire_format.rs`)
