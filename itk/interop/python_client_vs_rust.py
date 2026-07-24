# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Bidirectional interop: the OFFICIAL Python SDK client vs our Rust server.

The forward direction (our TCK against official-SDK servers) proves our
*client-side* wire expectations; this script proves the reverse — that the
reference `a2a-sdk` client can drive a server built on `a2a-protocol-server`
end to end over both HTTP bindings:

  * agent-card resolution
  * SendMessage (non-streaming and streaming) with echo verification
  * GetTask, ListTasks
  * CancelTask on a completed task -> TaskNotCancelableError
  * push-notification config create/get/list/delete
  * SubscribeToTask on a completed task -> snapshot then clean end (§3.5.2)

Usage: python python_client_vs_rust.py http://127.0.0.1:9090
Exit codes: 0 all passed, 1 failures.
"""

import asyncio
import sys

from a2a.client import ClientConfig, create_client
from a2a.client.errors import A2AClientError
from a2a.utils.errors import A2AError
from a2a.types import (
    CancelTaskRequest,
    GetTaskRequest,
    ListTasksRequest,
    Message,
    Part,
    SendMessageRequest,
    SubscribeToTaskRequest,
    TaskPushNotificationConfig,
    TaskState,
)
from a2a.utils import TransportProtocol

FAILURES: list[str] = []


def check(name: str, cond: bool, detail: str = "") -> None:
    if cond:
        print(f"  [PASS] {name}")
    else:
        print(f"  [FAIL] {name} {detail}")
        FAILURES.append(name)


def user_message(text: str, message_id: str) -> Message:
    return Message(
        message_id=message_id,
        role="ROLE_USER",
        parts=[Part(text=text)],
    )


async def run_binding(base_url: str, binding: str) -> None:
    print(f"binding: {binding}")
    config = ClientConfig(
        streaming=False,
        supported_protocol_bindings=[binding],
        use_client_preference=True,
    )
    client = await asyncio.wait_for(create_client(base_url, config), timeout=10)

    # 1. SendMessage (non-streaming): completed echo task.
    responses = []
    async for resp in client.send_message(
        SendMessageRequest(message=user_message("interop", f"py-{binding}-1"))
    ):
        responses.append(resp)
    check("send_message returns one response", len(responses) == 1)
    task = responses[-1].task
    check(
        "task completed",
        task.status.state == TaskState.TASK_STATE_COMPLETED,
        f"got {task.status.state}",
    )
    echo_texts = [p.text for a in task.artifacts for p in a.parts if p.text]
    check(
        "artifact echoes input",
        any("interop" in t for t in echo_texts),
        f"got {echo_texts}",
    )

    # 2. GetTask round-trips.
    got = await client.get_task(GetTaskRequest(id=task.id))
    check("get_task id round-trip", got.id == task.id)

    # 3. ListTasks contains the task.
    listed = await client.list_tasks(ListTasksRequest())
    check(
        "list_tasks contains task",
        any(t.id == task.id for t in listed.tasks),
        f"got {len(listed.tasks)} tasks",
    )

    # 4. CancelTask on the completed task must surface TaskNotCancelable.
    try:
        await client.cancel_task(CancelTaskRequest(id=task.id))
        check("cancel completed task rejected", False, "no error raised")
    except (A2AClientError, A2AError) as e:
        check(
            "cancel completed task rejected",
            type(e).__name__ == "TaskNotCancelableError",
            f"got {type(e).__name__}",
        )

    # 5. Push-notification config CRUD.
    created = await client.create_task_push_notification_config(
        TaskPushNotificationConfig(
            task_id=task.id, url="https://example.com/interop-hook"
        )
    )
    check("push config created with id", bool(created.id))
    from a2a.types import (
        DeleteTaskPushNotificationConfigRequest,
        GetTaskPushNotificationConfigRequest,
        ListTaskPushNotificationConfigsRequest,
    )

    got_cfg = await client.get_task_push_notification_config(
        GetTaskPushNotificationConfigRequest(task_id=task.id, id=created.id)
    )
    check("push config get round-trip", got_cfg.url.endswith("interop-hook"))
    cfgs = await client.list_task_push_notification_configs(
        ListTaskPushNotificationConfigsRequest(task_id=task.id)
    )
    check("push config list non-empty", len(cfgs.configs) >= 1)
    await client.delete_task_push_notification_config(
        DeleteTaskPushNotificationConfigRequest(task_id=task.id, id=created.id)
    )
    cfgs_after = await client.list_task_push_notification_configs(
        ListTaskPushNotificationConfigsRequest(task_id=task.id)
    )
    check(
        "push config deleted",
        all(c.id != created.id for c in cfgs_after.configs),
    )

    await client.close()

    # 7. Streaming SendMessage over a fresh client.
    stream_config = ClientConfig(
        streaming=True,
        supported_protocol_bindings=[binding],
        use_client_preference=True,
    )
    stream_client = await create_client(base_url, stream_config)
    stream_events = []
    async for resp in stream_client.send_message(
        SendMessageRequest(message=user_message("stream-interop", f"py-{binding}-s"))
    ):
        stream_events.append(resp)
    kinds = [e.WhichOneof("payload") for e in stream_events]
    check(
        "streaming yields task + terminal status",
        "task" in kinds and "status_update" in kinds,
        f"got {kinds}",
    )
    final_states = [
        e.status_update.status.state
        for e in stream_events
        if e.WhichOneof("payload") == "status_update"
    ]
    check(
        "stream reaches completed",
        TaskState.TASK_STATE_COMPLETED in final_states,
        f"got {final_states}",
    )

    # 8. Subscribing to a terminal task is rejected — reference parity: the
    # Python SDK's ActiveTask.subscribe raises for already-completed tasks
    # (snapshot-then-EOF reconnection per §3.5.2 applies to live tasks that
    # lost their queue, e.g. after a process restart — not terminal ones).
    try:
        async for _ev in stream_client.subscribe(
            SubscribeToTaskRequest(id=task.id)
        ):
            pass
        check("subscribe to terminal task rejected", False, "stream opened")
    except (A2AClientError, A2AError) as e:
        check(
            "subscribe to terminal task rejected",
            True,
            f"({type(e).__name__})",
        )
    await stream_client.close()


async def main() -> int:
    base_url = sys.argv[1] if len(sys.argv) > 1 else "http://127.0.0.1:9090"
    for binding in (TransportProtocol.JSONRPC, TransportProtocol.HTTP_JSON):
        await run_binding(base_url, binding)
    print()
    if FAILURES:
        print(f"FAILED: {len(FAILURES)} checks: {', '.join(FAILURES)}")
        return 1
    print("All official-Python-client interop checks passed.")
    return 0


if __name__ == "__main__":
    sys.exit(asyncio.run(main()))
