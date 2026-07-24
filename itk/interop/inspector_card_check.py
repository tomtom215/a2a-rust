# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Headless a2a-inspector agent-card validation against a running agent.

The official [a2a-inspector](https://github.com/a2aproject/a2a-inspector) is
the tool a reviewer reaches for first, but it ships only as a web UI — there
is no CLI or API mode to script. Its agent-card validation, however, is a
small self-contained ruleset in ``backend/validators.py``. This script
vendors that exact ruleset (``validate_agent_card``) and runs it against a
live agent's ``/.well-known/agent-card.json``, so the inspector's headline
check is reproducible in CI without the browser.

Keep ``_INSPECTOR_REQUIRED_FIELDS`` and the checks below in sync with the
upstream ``validate_agent_card`` if the inspector changes them.

Usage: python inspector_card_check.py [http://127.0.0.1:9090]
Exit codes: 0 card passes the inspector's checks, 1 otherwise.
"""
from __future__ import annotations

import sys
from typing import Any

import httpx

# ── Vendored verbatim from a2a-inspector backend/validators.py ───────────────
_INSPECTOR_REQUIRED_FIELDS = frozenset(
    [
        "name",
        "description",
        "url",
        "version",
        "capabilities",
        "defaultInputModes",
        "defaultOutputModes",
        "skills",
    ]
)


def validate_agent_card(card_data: dict[str, Any]) -> list[str]:
    """The inspector's agent-card validation, reproduced exactly."""
    errors: list[str] = []

    for field in _INSPECTOR_REQUIRED_FIELDS:
        if field not in card_data:
            errors.append(f"Required field is missing: '{field}'.")

    if "url" in card_data and not (
        card_data["url"].startswith("http://")
        or card_data["url"].startswith("https://")
    ):
        errors.append(
            "Field 'url' must be an absolute URL starting with http:// or https://."
        )

    if "capabilities" in card_data and not isinstance(card_data["capabilities"], dict):
        errors.append("Field 'capabilities' must be an object.")

    for field in ["defaultInputModes", "defaultOutputModes"]:
        if field in card_data:
            if not isinstance(card_data[field], list):
                errors.append(f"Field '{field}' must be an array of strings.")
            elif not all(isinstance(item, str) for item in card_data[field]):
                errors.append(f"All items in '{field}' must be strings.")

    if "skills" in card_data:
        if not isinstance(card_data["skills"], list):
            errors.append("Field 'skills' must be an array of AgentSkill objects.")
        elif not card_data["skills"]:
            errors.append(
                "Field 'skills' array is empty. Agent must have at least one "
                "skill if it performs actions."
            )

    return errors


def main() -> int:
    base = sys.argv[1] if len(sys.argv) > 1 else "http://127.0.0.1:9090"
    card = httpx.get(f"{base}/.well-known/agent-card.json", timeout=10).json()

    errors = validate_agent_card(card)
    if errors:
        print(f"a2a-inspector card validation FAILED for {base}:")
        for e in errors:
            print(f"  - {e}")
        return 1
    print(f"a2a-inspector card validation passed for {base} ({card.get('name')}).")
    return 0


if __name__ == "__main__":
    sys.exit(main())
