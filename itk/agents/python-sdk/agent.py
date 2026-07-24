# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""A2A echo agent built on the OFFICIAL Python SDK (`a2a-sdk`).

Unlike ``itk/agents/python`` (a dependency-light stub that hand-writes the
wire format), this agent is assembled entirely from the official
``a2a-sdk`` server framework — ``DefaultRequestHandlerV2``, the Starlette
route builders, ``TaskUpdater`` — so running our TCK against it validates
this Rust SDK's wire expectations against the reference implementation,
not against code we wrote ourselves.

Behavior contract (same as every ITK echo agent):
  * ``message/send`` returns a completed task whose artifact echoes the
    input text as ``Echo: <text>``.
  * Streaming, push-notification config CRUD, and task lifecycle methods
    are all served by the official SDK's default handler.

Run: ``pip install 'a2a-sdk[http-server]' uvicorn && python agent.py``
Env: ``PORT`` (default 9110).
"""

import os

import httpx
import uvicorn
from starlette.applications import Starlette

from a2a.server.agent_execution import AgentExecutor, RequestContext
from a2a.server.events import EventQueue
from a2a.server.request_handlers import DefaultRequestHandlerV2
from a2a.server.routes import create_jsonrpc_routes
from a2a.server.routes.agent_card_routes import create_agent_card_routes
from a2a.server.routes.rest_routes import create_rest_routes
from a2a.server.tasks import (
    BasePushNotificationSender,
    InMemoryPushNotificationConfigStore,
    InMemoryTaskStore,
    TaskUpdater,
)
from a2a.helpers.proto_helpers import new_task
from a2a.types import (
    AgentCapabilities,
    AgentCard,
    AgentInterface,
    AgentSkill,
    Part,
    TaskState,
)

PORT = int(os.environ.get("PORT", "9110"))
BASE_URL = f"http://127.0.0.1:{PORT}"


class EchoExecutor(AgentExecutor):
    """Echoes the user's text back as a completed task artifact."""

    async def execute(self, context: RequestContext, event_queue: EventQueue) -> None:
        # The SDK's active-task pipeline requires a full Task event to be
        # enqueued before any TaskStatusUpdateEvent for a fresh task.
        if context.current_task is None:
            await event_queue.enqueue_event(
                new_task(
                    task_id=context.task_id,
                    context_id=context.context_id,
                    state=TaskState.TASK_STATE_SUBMITTED,
                    history=[context.message] if context.message else None,
                )
            )
        updater = TaskUpdater(event_queue, context.task_id, context.context_id)
        await updater.start_work()
        text = context.get_user_input()
        await updater.add_artifact([Part(text=f"Echo: {text}")], name="echo")
        await updater.complete()

    async def cancel(self, context: RequestContext, event_queue: EventQueue) -> None:
        updater = TaskUpdater(event_queue, context.task_id, context.context_id)
        await updater.cancel()


def build_card() -> AgentCard:
    return AgentCard(
        name="official-python-echo",
        description="Echo agent built on the official a2a-sdk (Python)",
        version="1.0.0",
        capabilities=AgentCapabilities(streaming=True, push_notifications=True),
        default_input_modes=["text/plain"],
        default_output_modes=["text/plain"],
        skills=[
            AgentSkill(
                id="echo",
                name="Echo",
                description="Echoes back the input text",
                tags=["echo", "test"],
            )
        ],
        supported_interfaces=[
            AgentInterface(
                url=BASE_URL,
                protocol_binding="JSONRPC",
                protocol_version="1.0",
            ),
            AgentInterface(
                url=BASE_URL,
                protocol_binding="HTTP+JSON",
                protocol_version="1.0",
            ),
        ],
    )


def build_app() -> Starlette:
    card = build_card()
    push_store = InMemoryPushNotificationConfigStore()
    handler = DefaultRequestHandlerV2(
        agent_executor=EchoExecutor(),
        task_store=InMemoryTaskStore(),
        agent_card=card,
        push_config_store=push_store,
        push_sender=BasePushNotificationSender(
            httpx_client=httpx.AsyncClient(), config_store=push_store
        ),
    )
    routes = (
        create_agent_card_routes(card)
        + create_jsonrpc_routes(handler, rpc_url="/")
        + create_rest_routes(handler)
    )
    return Starlette(routes=routes)


if __name__ == "__main__":
    uvicorn.run(build_app(), host="127.0.0.1", port=PORT, log_level="warning")
