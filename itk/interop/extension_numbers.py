# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""The numbers of this SDK's extensions, as the official Python SDK sends them.

a2a-python keeps `Message.metadata` in a protobuf `Struct`, whose only number
is a double, so every integer below leaves this client as a float (`30000` is
`30000.0` on the wire; the lineage depth found it, ADR 0049). Each check shows
the Rust server reads the double as the integer and acts on its value:

  * lease (ADR 0020): `ttlMs` 30000 is granted; 500 is refused by the lease's
    own range rule, which only a value read as a number reaches; 30000.5 is
    refused as malformed;
  * budget (ADR 0023): `remainingMs` and a meter are granted, and every status
    update of the turn reports the meter's 50000 back, with time left no more
    than was granted.

Used by python_client_vs_rust.py; written from the wire contract (book: "Swarm
Extensions: the Wire Contract"), as a hop in another language would write it.
"""

from a2a.client.errors import A2AClientError
from a2a.types import Message, Part, SendMessageRequest
from a2a.utils import TransportProtocol
from a2a.utils.errors import A2AError

LEASE_URI = "https://a2a-rust.com/extensions/lease/v1"
LEASE_KEY = "a2a-rust.com/lease"
BUDGET_URI = "https://a2a-rust.com/extensions/budget/v1"
BUDGET_KEY = "a2a-rust.com/budget"


def carrying(message_id: str, key: str, uri: str, value: dict) -> Message:
    message = Message(message_id=message_id, role="ROLE_USER", parts=[Part(text="numbers")])
    message.metadata.update({key: value})
    message.extensions.append(uri)
    return message


def as_sent(message: Message, key: str) -> dict:
    """The value as the SDK will serialize it: the `Struct`'s numbers are floats."""
    return dict(message.metadata[key])


async def refusal(client, message: Message) -> str | None:
    try:
        async for _ in client.send_message(SendMessageRequest(message=message)):
            pass
    except (A2AClientError, A2AError) as e:
        return str(e)
    return None


async def check_lease(client, binding, check) -> None:
    granted = carrying(f"py-{binding}-lease-1", LEASE_KEY, LEASE_URI, {"ttlMs": 30000})
    sent = as_sent(granted, LEASE_KEY)
    check(
        "lease: ttlMs leaves the Python SDK as a double",
        isinstance(sent["ttlMs"], float) and sent["ttlMs"] == 30000.0,
        f"got {sent!r}",
    )
    why = await refusal(client, granted)
    check("lease: ttlMs 30000.0 is granted", why is None, f"refused: {why}")

    # On HTTP+JSON the refusal is a bare 400 to this client (the server adds
    # no a2a-protocol.org ErrorInfo to invalid params); the text is JSON-RPC's.
    short = carrying(f"py-{binding}-lease-2", LEASE_KEY, LEASE_URI, {"ttlMs": 500})
    want = (
        "400 Bad Request"
        if binding == TransportProtocol.HTTP_JSON
        else "this one is 500 ms"
    )
    why = await refusal(client, short)
    check(
        "lease: ttlMs 500.0 is read as 500 and refused by the lease's range",
        why is not None and want in why,
        f"got {why}",
    )

    half = carrying(f"py-{binding}-lease-3", LEASE_KEY, LEASE_URI, {"ttlMs": 30000.5})
    want = "400 Bad Request" if binding == TransportProtocol.HTTP_JSON else "malformed lease"
    why = await refusal(client, half)
    check(
        "lease: ttlMs 30000.5 is refused as malformed",
        why is not None and want in why,
        f"got {why}",
    )


async def check_budget(stream_client, binding, check) -> None:
    grant = carrying(
        f"py-{binding}-budget-1",
        BUDGET_KEY,
        BUDGET_URI,
        {"remainingMs": 30000, "meters": {"tokens": 50000}},
    )
    reports = []
    try:
        async for resp in stream_client.send_message(SendMessageRequest(message=grant)):
            if resp.WhichOneof("payload") == "status_update":
                metadata = resp.status_update.metadata
                if BUDGET_KEY in metadata:
                    reports.append(dict(metadata[BUDGET_KEY]))
        why = "no status update carried the budget"
    except (A2AClientError, A2AError) as e:
        why = f"refused: {e}"
    check(
        "budget: the turn reports the budget it was granted",
        len(reports) > 0,
        why,
    )
    for report in reports:
        meters = dict(report.get("meters", {}))
        left = report.get("remainingMs")
        check(
            "budget: meters.tokens 50000.0 is read and reported as 50000",
            meters.get("tokens") == 50000,
            f"got {report!r}",
        )
        check(
            "budget: remainingMs 30000.0 is read; the time left is within it",
            left is not None and 0 < left <= 30000,
            f"got {report!r}",
        )
