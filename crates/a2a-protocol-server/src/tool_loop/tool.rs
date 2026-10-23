// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! [`Tool`], something a model may call, and [`ToolSet`], the catalogue the
//! loop offers it.

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use serde_json::Value;

use super::model::ToolSpec;

/// Why a call produced no result.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ToolError {
    /// The call failed in a way the model can act on: a bad argument, a name
    /// that does not exist, a record not found. The message goes back to the
    /// model as the call's result and the loop continues. This is the
    /// common case: "no service named 'billing'; call `list_services`" is how
    /// a model recovers.
    Failed(String),
    /// The call failed in a way no further turn can fix, such as the tool's
    /// own backend being down. The task ends `Failed`, class `internal`.
    Fatal(String),
}

impl fmt::Display for ToolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Failed(e) | Self::Fatal(e) => f.write_str(e),
        }
    }
}

impl std::error::Error for ToolError {}

/// One call, as a [`Tool`] is given it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ToolInvocation {
    /// The arguments, always a JSON object: the loop answers arguments that
    /// are not one itself, without calling the tool.
    pub arguments: Value,
    /// This call's idempotency key (ADR 0026): 64 lowercase hex digits,
    /// the same for the same tool and arguments within one run of the loop
    /// over one message, different otherwise, and not computable without
    /// the loop's secret. A tool with side effects passes it to its backend
    /// where the backend takes one (an `Idempotency-Key` header, a unique
    /// request id), so a retried request does its work once.
    pub idempotency_key: String,
}

impl ToolInvocation {
    /// An invocation with `arguments` and `idempotency_key`, for calling a
    /// tool outside a loop, as a test does.
    #[must_use]
    pub fn new(arguments: Value, idempotency_key: impl Into<String>) -> Self {
        Self {
            arguments,
            idempotency_key: idempotency_key.into(),
        }
    }
}

/// Something a model may call.
///
/// Object-safe, with a boxed future like [`AgentExecutor`](crate::AgentExecutor).
/// [`ToolSet`] holds each as an `Arc<dyn Tool>`.
pub trait Tool: Send + Sync + 'static {
    /// The name the model calls it by. Unique within a [`ToolSet`].
    fn name(&self) -> &str;

    /// What it does. The model chooses tools by this text, so say when to
    /// call it as well as what it returns.
    fn description(&self) -> &str;

    /// A JSON Schema for the arguments, an `object` schema.
    fn parameters(&self) -> Value;

    /// Whether a call changes nothing, so running it again is harmless:
    /// a lookup, a status query, a search. `false` unless a tool says so.
    ///
    /// Within one run, a call identical to one that already succeeded —
    /// same tool, same arguments — is not run again when the tool is not
    /// read-only: the model is sent the earlier result, marked as such
    /// (ADR 0026). A read-only tool is run each time, so a model can poll
    /// it for a change.
    fn read_only(&self) -> bool {
        false
    }

    /// Runs the call.
    ///
    /// # Errors
    ///
    /// [`ToolError::Failed`] to tell the model what went wrong;
    /// [`ToolError::Fatal`] to end the task.
    fn call<'a>(
        &'a self,
        invocation: ToolInvocation,
    ) -> Pin<Box<dyn Future<Output = Result<String, ToolError>> + Send + 'a>>;
}

/// Two tools in one [`ToolSet`] with the same name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuplicateTool(pub String);

impl fmt::Display for DuplicateTool {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "a tool named '{}' is already in this set", self.0)
    }
}

impl std::error::Error for DuplicateTool {}

/// The tools a [`ToolLoop`](super::ToolLoop) offers its model, in the order
/// they were added. Names are unique: a model asking for a name two tools
/// share could not be answered.
#[derive(Clone, Default)]
pub struct ToolSet {
    tools: Vec<Arc<dyn Tool>>,
}

impl fmt::Debug for ToolSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list()
            .entries(self.tools.iter().map(|t| t.name()))
            .finish()
    }
}

impl ToolSet {
    /// An empty set. A loop over it is a plain chat.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds `tool`.
    ///
    /// # Errors
    ///
    /// [`DuplicateTool`] when the set already holds a tool of that name; the
    /// set is unchanged.
    pub fn add(&mut self, tool: impl Tool) -> Result<(), DuplicateTool> {
        self.add_arc(Arc::new(tool))
    }

    /// Adds a shared `tool`, for one instance offered by several sets.
    ///
    /// # Errors
    ///
    /// [`DuplicateTool`] when the set already holds a tool of that name; the
    /// set is unchanged.
    pub fn add_arc(&mut self, tool: Arc<dyn Tool>) -> Result<(), DuplicateTool> {
        if self.get(tool.name()).is_some() {
            return Err(DuplicateTool(tool.name().to_owned()));
        }
        self.tools.push(tool);
        Ok(())
    }

    /// Adds `tool`, by value, for building a set in one expression.
    ///
    /// # Errors
    ///
    /// [`DuplicateTool`] when the set already holds a tool of that name.
    pub fn with(mut self, tool: impl Tool) -> Result<Self, DuplicateTool> {
        self.add(tool)?;
        Ok(self)
    }

    /// The tool named `name`.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Arc<dyn Tool>> {
        self.tools.iter().find(|t| t.name() == name)
    }

    /// The names, in the order the tools were added.
    #[must_use]
    pub fn names(&self) -> Vec<String> {
        self.tools.iter().map(|t| t.name().to_owned()).collect()
    }

    /// The catalogue as the model is told it.
    #[must_use]
    pub fn specs(&self) -> Vec<ToolSpec> {
        self.tools
            .iter()
            .map(|t| ToolSpec::new(t.name(), t.description(), t.parameters()))
            .collect()
    }

    /// How many tools the set holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    /// Whether the set holds none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Says nothing about itself beyond what the trait requires.
    struct Plain;

    impl Tool for Plain {
        fn name(&self) -> &'static str {
            "plain"
        }
        fn description(&self) -> &'static str {
            "Does something."
        }
        fn parameters(&self) -> Value {
            serde_json::json!({"type": "object"})
        }
        fn call<'a>(
            &'a self,
            _invocation: ToolInvocation,
        ) -> Pin<Box<dyn Future<Output = Result<String, ToolError>> + Send + 'a>> {
            Box::pin(async { Ok(String::new()) })
        }
    }

    /// A tool that does not say it is read-only is treated as changing
    /// something, so its repeated calls are not run again (ADR 0026). The
    /// mutation gate found the default untested: `true` survived.
    #[test]
    fn a_tool_is_not_read_only_unless_it_says_so() {
        assert!(!Plain.read_only());
    }

    /// The refusal names the tool.
    #[test]
    fn a_duplicate_names_the_tool() {
        assert_eq!(
            DuplicateTool("lookup".to_owned()).to_string(),
            "a tool named 'lookup' is already in this set"
        );
    }
}
