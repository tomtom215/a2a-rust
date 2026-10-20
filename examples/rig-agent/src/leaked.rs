// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A tool call the model wrote as its answer's text instead of as a call.
//!
//! Small local models sometimes print the call — `{"name": "service_status",
//! "arguments": {"service": "billing"}}`, bare, in a code fence, or between
//! `<tool_call>` tags — and the server's tool-call parser does not lift it
//! out, so the reply arrives as text and the loop would hand the caller raw
//! JSON as the answer. Measured 2026-10-20 with Qwen3-1.7B-Q4_K_M on
//! llama-server b11751 (`--jinja`): 1 of 20 runs of the billing prompt.
//!
//! Only a reply that is nothing but one such object, naming a tool the agent
//! declared, is taken for a call; prose that mentions JSON, or an object
//! naming anything else, stays an answer.

use rig_core::completion::message::ToolFunction;
use serde_json::Value;

/// The call `text` is, if it is exactly one call to a tool in `known`.
pub fn leaked_call(text: &str, known: &[String]) -> Option<ToolFunction> {
    let mut body = text.trim();
    if let Some(inner) = body
        .strip_prefix("<tool_call>")
        .and_then(|b| b.strip_suffix("</tool_call>"))
    {
        body = inner.trim();
    }
    if let Some(inner) = body.strip_prefix("```").and_then(|b| b.strip_suffix("```")) {
        body = inner.strip_prefix("json").unwrap_or(inner).trim();
    }
    let Value::Object(call) = serde_json::from_str::<Value>(body).ok()? else {
        return None;
    };
    let name = call.get("name")?.as_str()?;
    if !known.iter().any(|k| k == name) {
        return None;
    }
    let arguments = match call.get("arguments").or_else(|| call.get("parameters")) {
        None => Value::Object(serde_json::Map::new()),
        Some(Value::Object(args)) => Value::Object(args.clone()),
        // Some templates write the arguments as a JSON string.
        Some(Value::String(s)) => match serde_json::from_str::<Value>(s).ok()? {
            args @ Value::Object(_) => args,
            _ => return None,
        },
        Some(_) => return None,
    };
    Some(ToolFunction {
        name: name.to_owned(),
        arguments,
    })
}

#[cfg(test)]
mod tests {
    use super::leaked_call;
    use serde_json::json;

    fn known() -> Vec<String> {
        vec!["service_status".to_owned(), "list_services".to_owned()]
    }

    #[test]
    fn a_bare_fenced_or_tagged_call_is_recovered() {
        for text in [
            r#"{"name": "service_status", "arguments": {"service": "billing"}}"#,
            "```json\n{\"name\": \"service_status\", \"arguments\": {\"service\": \"billing\"}}\n```",
            "<tool_call>\n{\"name\": \"service_status\", \"arguments\": {\"service\": \"billing\"}}\n</tool_call>",
            r#"{"name": "service_status", "arguments": "{\"service\": \"billing\"}"}"#,
        ] {
            let call = leaked_call(text, &known()).unwrap_or_else(|| panic!("{text}"));
            assert_eq!(call.name, "service_status");
            assert_eq!(call.arguments, json!({"service": "billing"}));
        }
        let call = leaked_call(r#"{"name": "list_services"}"#, &known()).expect("no arguments");
        assert_eq!(call.arguments, json!({}));
    }

    #[test]
    fn prose_and_unknown_tools_stay_answers() {
        for text in [
            "The checkout service is healthy.",
            r#"Call {"name": "service_status"} next."#,
            r#"{"name": "delete_everything", "arguments": {}}"#,
            r#"{"name": "service_status", "arguments": [1, 2]}"#,
            r#"{"service": "checkout", "state": "healthy"}"#,
            r#"[{"name": "service_status", "arguments": {}}]"#,
        ] {
            assert!(leaked_call(text, &known()).is_none(), "{text}");
        }
    }
}
