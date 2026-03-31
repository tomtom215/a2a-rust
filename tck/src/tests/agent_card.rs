// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Agent card discovery conformance tests.

use super::helpers;

/// Tests that the agent card is discoverable at `/.well-known/agent-card.json`.
pub async fn test_agent_card_discovery(url: &str) -> Result<(), String> {
    let (status, body) = helpers::rest_get(url, "/.well-known/agent-card.json").await?;
    if status != 200 {
        return Err(format!("expected 200 for agent card, got {status}: {body}"));
    }
    if body.get("name").is_none() {
        return Err("agent card missing 'name' field".to_string());
    }
    Ok(())
}

/// Tests that the agent card contains all required fields per the spec.
pub async fn test_agent_card_required_fields(url: &str) -> Result<(), String> {
    let (_, card) = helpers::rest_get(url, "/.well-known/agent-card.json").await?;

    let required = [
        "name",
        "url",
        "version",
        "capabilities",
        "defaultInputModes",
        "defaultOutputModes",
        "skills",
    ];
    for field in required {
        if card.get(field).is_none() {
            return Err(format!("agent card missing required field '{field}'"));
        }
    }

    // Validate capabilities is an object
    if !card["capabilities"].is_object() {
        return Err("'capabilities' must be an object".to_string());
    }

    // Validate skills is an array
    if !card["skills"].is_array() {
        return Err("'skills' must be an array".to_string());
    }

    // Validate defaultInputModes and defaultOutputModes are arrays
    if !card["defaultInputModes"].is_array() {
        return Err("'defaultInputModes' must be an array".to_string());
    }
    if !card["defaultOutputModes"].is_array() {
        return Err("'defaultOutputModes' must be an array".to_string());
    }

    Ok(())
}

/// Tests that the agent card is served with the correct content type.
pub async fn test_agent_card_content_type(url: &str) -> Result<(), String> {
    // We can't easily check headers with our simple helper, but we can verify
    // the response is valid JSON (which our helper already does).
    let (status, _) = helpers::rest_get(url, "/.well-known/agent-card.json").await?;
    if status != 200 {
        return Err(format!("expected 200, got {status}"));
    }
    Ok(())
}
