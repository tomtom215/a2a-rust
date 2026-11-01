// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Constructors and builders for the protocol objects that are
//! `#[non_exhaustive]` (audit G-10).
//!
//! A struct marked `#[non_exhaustive]` cannot be written as a struct literal
//! outside its crate, so a field the A2A specification adds later is not a
//! breaking change to every caller that built one. These are the other way in:
//! `new` takes the fields the protocol requires, and a `with_*` method sets
//! each optional one. Each leaves every field it is not given at the value a
//! literal would have written (`None`, or empty), so converting a literal is
//! mechanical and changes no wire byte.
//!
//! `Message`, `AgentCard`, `AgentSkill` and `AgentInterface` had theirs
//! already, in their own modules; the rest are here.

use crate::agent_card::{AgentCapabilities, AgentInterface, AgentProvider};
use crate::artifact::{Artifact, ArtifactId};
use crate::events::{TaskArtifactUpdateEvent, TaskStatusUpdateEvent};
use crate::extensions::AgentExtension;
use crate::message::Message;
use crate::push::{AuthenticationInfo, TaskPushNotificationConfig};
use crate::task::{ContextId, Task, TaskId, TaskStatus};

impl TaskStatus {
    /// Sets the message that goes with the state (for `failed` or
    /// `input-required`, what the agent says about it).
    #[must_use]
    pub fn with_message(mut self, message: Message) -> Self {
        self.message = Some(message);
        self
    }

    /// Sets the timestamp to `timestamp` (ISO 8601, as the specification
    /// writes it) where [`TaskStatus::with_timestamp`] would take the current
    /// time.
    #[must_use]
    pub fn at(mut self, timestamp: impl Into<String>) -> Self {
        self.timestamp = Some(timestamp.into());
        self
    }
}

impl Task {
    /// Sets the message history.
    #[must_use]
    pub fn with_history(mut self, history: Vec<Message>) -> Self {
        self.history = Some(history);
        self
    }

    /// Sets the artifacts the task has produced.
    #[must_use]
    pub fn with_artifacts(mut self, artifacts: Vec<Artifact>) -> Self {
        self.artifacts = Some(artifacts);
        self
    }

    /// Sets the task's metadata.
    #[must_use]
    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = Some(metadata);
        self
    }
}

impl Artifact {
    /// Sets the artifact's human-readable name.
    #[must_use]
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Sets the artifact's description.
    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Sets the artifact's metadata.
    #[must_use]
    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = Some(metadata);
        self
    }

    /// Sets the extension URIs that apply to the artifact.
    #[must_use]
    pub fn with_extensions<I, S>(mut self, extensions: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.extensions = Some(extensions.into_iter().map(Into::into).collect());
        self
    }

    /// An artifact with the given id and parts, whatever the parts are: unlike
    /// [`Artifact::new`] it does not assert that there is at least one, so a
    /// test of [`Artifact::validate`] can build the artifact it expects to be
    /// refused.
    #[must_use]
    pub fn unchecked(id: impl Into<ArtifactId>, parts: Vec<crate::message::Part>) -> Self {
        let mut artifact = Self::new(id, vec![crate::message::Part::text(String::new())]);
        artifact.parts = parts;
        artifact
    }
}

impl TaskStatusUpdateEvent {
    /// An event reporting that task `task_id` of context `context_id` is now
    /// in `status`, with no metadata.
    #[must_use]
    pub fn new(
        task_id: impl Into<TaskId>,
        context_id: impl Into<ContextId>,
        status: TaskStatus,
    ) -> Self {
        Self {
            task_id: task_id.into(),
            context_id: context_id.into(),
            status,
            metadata: None,
        }
    }

    /// Sets the event's metadata.
    #[must_use]
    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = Some(metadata);
        self
    }
}

impl TaskArtifactUpdateEvent {
    /// An event carrying `artifact` for task `task_id` of context
    /// `context_id`, with `append`, `last_chunk` and metadata unset.
    #[must_use]
    pub fn new(
        task_id: impl Into<TaskId>,
        context_id: impl Into<ContextId>,
        artifact: Artifact,
    ) -> Self {
        Self {
            task_id: task_id.into(),
            context_id: context_id.into(),
            artifact,
            append: None,
            last_chunk: None,
            metadata: None,
        }
    }

    /// Says whether the parts are added to the artifact the client already
    /// has (`true`) or replace it (`false`).
    #[must_use]
    pub const fn with_append(mut self, append: bool) -> Self {
        self.append = Some(append);
        self
    }

    /// Says whether this is the artifact's last chunk.
    #[must_use]
    pub const fn with_last_chunk(mut self, last_chunk: bool) -> Self {
        self.last_chunk = Some(last_chunk);
        self
    }

    /// Sets the event's metadata.
    #[must_use]
    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = Some(metadata);
        self
    }
}

impl TaskPushNotificationConfig {
    /// A config for `url` that names no task: what a request body is before
    /// the task id is taken from the path or the method's parameters.
    #[must_use]
    pub fn for_url(url: impl Into<String>) -> Self {
        Self {
            tenant: None,
            id: None,
            task_id: None,
            url: url.into(),
            token: None,
            authentication: None,
        }
    }

    /// Sets the config's id (the server assigns one when it is absent).
    #[must_use]
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Sets the token the webhook receives to know a notification is for this
    /// config (spec §13.2).
    #[must_use]
    pub fn with_token(mut self, token: impl Into<String>) -> Self {
        self.token = Some(token.into());
        self
    }

    /// Sets the credentials the webhook asks to be sent.
    #[must_use]
    pub fn with_authentication(mut self, authentication: AuthenticationInfo) -> Self {
        self.authentication = Some(authentication);
        self
    }

    /// Sets the tenant the config belongs to.
    #[must_use]
    pub fn with_tenant(mut self, tenant: impl Into<String>) -> Self {
        self.tenant = Some(tenant.into());
        self
    }
}

impl AgentExtension {
    /// Sets the description of what the extension does.
    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Says whether a client must understand the extension to use the agent.
    #[must_use]
    pub const fn with_required(mut self, required: bool) -> Self {
        self.required = Some(required);
        self
    }

    /// Sets the extension's parameters.
    #[must_use]
    pub fn with_params(mut self, params: serde_json::Value) -> Self {
        self.params = Some(params);
        self
    }
}

impl AgentProvider {
    /// The organization that runs the agent, and its website.
    #[must_use]
    pub fn new(organization: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            organization: organization.into(),
        }
    }
}

impl AgentCapabilities {
    /// Sets the extensions the agent supports.
    #[must_use]
    pub fn with_extensions(mut self, extensions: Vec<AgentExtension>) -> Self {
        self.extensions = Some(extensions);
        self
    }
}

impl AgentInterface {
    /// Sets the protocol version the interface speaks, where
    /// [`AgentInterface::new`] writes the version this crate implements.
    #[must_use]
    pub fn with_protocol_version(mut self, version: impl Into<String>) -> Self {
        self.protocol_version = version.into();
        self
    }
}

#[cfg(test)]
#[path = "wire_builders_tests.rs"]
mod tests;
