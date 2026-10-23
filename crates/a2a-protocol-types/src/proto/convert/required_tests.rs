// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A field `a2a.proto` marks REQUIRED, absent on the gRPC wire, is refused
//! by the conversion as the JSON bindings refuse the missing key: each case
//! blanks one field of a valid message (proto3's absent `""`, `0` or
//! `None`), and also removes the key from the same object's JSON.

use crate::agent_card::AgentCard;
use crate::events::StreamResponse;
use crate::proto as pb;
use crate::task::Task;

use super::ConvertError;

fn task_json() -> serde_json::Value {
    serde_json::json!({
        "id": "t-1", "contextId": "c-1",
        "status": {"state": "TASK_STATE_WORKING",
            "message": {"messageId": "s-1", "role": "ROLE_AGENT", "parts": [{"text": "busy"}]}},
        "artifacts": [{"artifactId": "a-1", "parts": [{"text": "out"}]}],
        "history": [{"messageId": "m-1", "role": "ROLE_USER", "parts": [{"text": "in"}]}]
    })
}

fn card_json() -> serde_json::Value {
    serde_json::json!({
        "name": "a", "description": "d", "version": "1",
        "supportedInterfaces": [{"url": "https://a.example", "protocolBinding": "GRPC", "protocolVersion": "1.0"}],
        "provider": {"organization": "o", "url": "https://o.example"},
        "capabilities": {"streaming": true},
        "defaultInputModes": ["text/plain"], "defaultOutputModes": ["text/plain"],
        "skills": [{"id": "s", "name": "S", "description": "d", "tags": ["t"]}],
        "signatures": [{"protected": "p", "signature": "s"}],
        "securitySchemes": {
            "key": {"apiKeySecurityScheme": {"location": "header", "name": "X-Key"}},
            "http": {"httpAuthSecurityScheme": {"scheme": "bearer"}},
            "oidc": {"openIdConnectSecurityScheme": {"openIdConnectUrl": "https://i.example/.well-known/openid-configuration"}},
            "code": {"oauth2SecurityScheme": {"flows": {"authorizationCode": {
                "authorizationUrl": "https://i.example/a", "tokenUrl": "https://i.example/t", "scopes": {}}}}},
            "cc": {"oauth2SecurityScheme": {"flows": {"clientCredentials": {
                "tokenUrl": "https://i.example/t", "scopes": {}}}}},
            "device": {"oauth2SecurityScheme": {"flows": {"deviceCode": {
                "deviceAuthorizationUrl": "https://i.example/d", "tokenUrl": "https://i.example/t", "scopes": {}}}}}
        }
    })
}

fn pb_task() -> pb::Task {
    let task: Task = serde_json::from_value(task_json()).expect("task parses");
    task.try_into().expect("task converts")
}

fn pb_card() -> pb::AgentCard {
    let card: AgentCard = serde_json::from_value(card_json()).expect("card parses");
    card.try_into().expect("card converts")
}

/// Removes the key at `path` (`/`-separated, array indices numeric) from
/// `json`.
fn remove(json: &mut serde_json::Value, path: &str) {
    let (parent, key) = path.rsplit_once('/').expect("path has a key");
    let at = if parent.is_empty() {
        json
    } else {
        json.pointer_mut(parent).expect("parent exists")
    };
    at.as_object_mut()
        .expect("parent is an object")
        .remove(key)
        .expect("key exists");
}

fn refused(result: Result<impl std::fmt::Debug, ConvertError>, field: &str) {
    let err = result.expect_err(field);
    assert_eq!(err.field, field);
    assert_eq!(err.reason, "missing required field", "{field}");
}

#[test]
fn a_task_with_an_absent_required_field_is_refused_on_both_formats() {
    type Blank = fn(&mut pb::Task);
    let cases: [(&str, &str, Blank); 7] = [
        ("/id", "task.id", |t| t.id.clear()),
        ("/status/state", "status.state", |t| {
            t.status.as_mut().unwrap().state = 0;
        }),
        ("/status/message/messageId", "message.messageId", |t| {
            t.status
                .as_mut()
                .unwrap()
                .message
                .as_mut()
                .unwrap()
                .message_id
                .clear();
        }),
        ("/status/message/role", "message.role", |t| {
            t.status.as_mut().unwrap().message.as_mut().unwrap().role = 0;
        }),
        ("/history/0/messageId", "message.messageId", |t| {
            t.history[0].message_id.clear();
        }),
        ("/history/0/role", "message.role", |t| t.history[0].role = 0),
        ("/artifacts/0/artifactId", "artifact.artifactId", |t| {
            t.artifacts[0].artifact_id.clear();
        }),
    ];
    Task::try_from(pb_task()).expect("the unbroken task converts");
    for (path, field, blank) in cases {
        let mut proto = pb_task();
        blank(&mut proto);
        refused(Task::try_from(proto), field);
        let mut json = task_json();
        remove(&mut json, path);
        assert!(
            serde_json::from_value::<Task>(json).is_err(),
            "JSON accepts a task without {path}"
        );
    }
}

#[test]
fn an_update_without_its_task_or_context_id_is_refused() {
    use pb::stream_response::Payload;
    let status = || pb::TaskStatusUpdateEvent {
        task_id: "t".into(),
        context_id: "c".into(),
        status: Some(pb::TaskStatus {
            state: pb::TaskState::Working as i32,
            message: None,
            timestamp: None,
        }),
        metadata: None,
    };
    let artifact = || pb::TaskArtifactUpdateEvent {
        task_id: "t".into(),
        context_id: "c".into(),
        artifact: pb_task().artifacts.pop(),
        append: false,
        last_chunk: false,
        metadata: None,
    };
    let wrap = |payload| pb::StreamResponse {
        payload: Some(payload),
    };
    StreamResponse::try_from(wrap(Payload::StatusUpdate(status()))).expect("valid status");
    StreamResponse::try_from(wrap(Payload::ArtifactUpdate(artifact()))).expect("valid artifact");
    let mut e = status();
    e.task_id.clear();
    refused(
        StreamResponse::try_from(wrap(Payload::StatusUpdate(e))),
        "statusUpdate.taskId",
    );
    let mut e = status();
    e.context_id.clear();
    refused(
        StreamResponse::try_from(wrap(Payload::StatusUpdate(e))),
        "statusUpdate.contextId",
    );
    let mut e = artifact();
    e.task_id.clear();
    refused(
        StreamResponse::try_from(wrap(Payload::ArtifactUpdate(e))),
        "artifactUpdate.taskId",
    );
    let mut e = artifact();
    e.context_id.clear();
    refused(
        StreamResponse::try_from(wrap(Payload::ArtifactUpdate(e))),
        "artifactUpdate.contextId",
    );
}

/// Every REQUIRED string of the card graph, and `capabilities`.
#[test]
#[allow(clippy::too_many_lines)] // one flat table
fn a_card_with_an_absent_required_field_is_refused_on_both_formats() {
    use pb::o_auth_flows::Flow;
    use pb::security_scheme::Scheme;
    type Blank = fn(&mut pb::AgentCard);
    fn scheme<'a>(card: &'a mut pb::AgentCard, name: &str) -> &'a mut Scheme {
        card.security_schemes
            .get_mut(name)
            .and_then(|s| s.scheme.as_mut())
            .expect("scheme")
    }
    fn flow<'a>(card: &'a mut pb::AgentCard, name: &str) -> &'a mut Flow {
        match scheme(card, name) {
            Scheme::Oauth2SecurityScheme(s) => s.flows.as_mut().and_then(|f| f.flow.as_mut()),
            _ => None,
        }
        .expect("flow")
    }
    let cases: [(&str, &str, Blank); 22] = [
        ("/name", "agentCard.name", |c| c.name.clear()),
        ("/description", "agentCard.description", |c| {
            c.description.clear();
        }),
        ("/version", "agentCard.version", |c| c.version.clear()),
        ("/capabilities", "agentCard.capabilities", |c| {
            c.capabilities = None;
        }),
        ("/supportedInterfaces/0/url", "agentInterface.url", |c| {
            c.supported_interfaces[0].url.clear();
        }),
        (
            "/supportedInterfaces/0/protocolBinding",
            "agentInterface.protocolBinding",
            |c| {
                c.supported_interfaces[0].protocol_binding.clear();
            },
        ),
        (
            "/supportedInterfaces/0/protocolVersion",
            "agentInterface.protocolVersion",
            |c| {
                c.supported_interfaces[0].protocol_version.clear();
            },
        ),
        ("/provider/url", "agentProvider.url", |c| {
            c.provider.as_mut().unwrap().url.clear();
        }),
        (
            "/provider/organization",
            "agentProvider.organization",
            |c| {
                c.provider.as_mut().unwrap().organization.clear();
            },
        ),
        ("/skills/0/id", "agentSkill.id", |c| c.skills[0].id.clear()),
        ("/skills/0/name", "agentSkill.name", |c| {
            c.skills[0].name.clear();
        }),
        ("/skills/0/description", "agentSkill.description", |c| {
            c.skills[0].description.clear();
        }),
        ("/signatures/0/protected", "signature.protected", |c| {
            c.signatures[0].protected.clear();
        }),
        ("/signatures/0/signature", "signature.signature", |c| {
            c.signatures[0].signature.clear();
        }),
        (
            "/securitySchemes/key/apiKeySecurityScheme/name",
            "apiKey.name",
            |c| {
                if let Scheme::ApiKeySecurityScheme(s) = scheme(c, "key") {
                    s.name.clear();
                }
            },
        ),
        (
            "/securitySchemes/http/httpAuthSecurityScheme/scheme",
            "httpAuth.scheme",
            |c| {
                if let Scheme::HttpAuthSecurityScheme(s) = scheme(c, "http") {
                    s.scheme.clear();
                }
            },
        ),
        (
            "/securitySchemes/oidc/openIdConnectSecurityScheme/openIdConnectUrl",
            "openIdConnect.openIdConnectUrl",
            |c| {
                if let Scheme::OpenIdConnectSecurityScheme(s) = scheme(c, "oidc") {
                    s.open_id_connect_url.clear();
                }
            },
        ),
        (
            "/securitySchemes/code/oauth2SecurityScheme/flows/authorizationCode/authorizationUrl",
            "authorizationCode.authorizationUrl",
            |c| {
                if let Flow::AuthorizationCode(f) = flow(c, "code") {
                    f.authorization_url.clear();
                }
            },
        ),
        (
            "/securitySchemes/code/oauth2SecurityScheme/flows/authorizationCode/tokenUrl",
            "authorizationCode.tokenUrl",
            |c| {
                if let Flow::AuthorizationCode(f) = flow(c, "code") {
                    f.token_url.clear();
                }
            },
        ),
        (
            "/securitySchemes/cc/oauth2SecurityScheme/flows/clientCredentials/tokenUrl",
            "clientCredentials.tokenUrl",
            |c| {
                if let Flow::ClientCredentials(f) = flow(c, "cc") {
                    f.token_url.clear();
                }
            },
        ),
        (
            "/securitySchemes/device/oauth2SecurityScheme/flows/deviceCode/deviceAuthorizationUrl",
            "deviceCode.deviceAuthorizationUrl",
            |c| {
                if let Flow::DeviceCode(f) = flow(c, "device") {
                    f.device_authorization_url.clear();
                }
            },
        ),
        (
            "/securitySchemes/device/oauth2SecurityScheme/flows/deviceCode/tokenUrl",
            "deviceCode.tokenUrl",
            |c| {
                if let Flow::DeviceCode(f) = flow(c, "device") {
                    f.token_url.clear();
                }
            },
        ),
    ];
    AgentCard::try_from(pb_card()).expect("the unbroken card converts");
    for (path, field, blank) in cases {
        let mut proto = pb_card();
        blank(&mut proto);
        refused(AgentCard::try_from(proto), field);
        let mut json = card_json();
        remove(&mut json, path);
        assert!(
            serde_json::from_value::<AgentCard>(json).is_err(),
            "JSON accepts a card without {path}"
        );
    }
}
