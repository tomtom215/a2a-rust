#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F.

"""
A2A ITK — Python Echo Agent

A minimal A2A agent using the official Python SDK that echoes incoming messages.
Used for cross-language interoperability testing with the Rust A2A SDK.

Usage:
    python agent.py [--port 9100]
"""

import argparse
import asyncio
import uuid

from a2a.server.apps.jsonrpc.starlette_app import A2AStarletteApplication
from a2a.server.request_handlers import DefaultRequestHandler
from a2a.server.agent_execution import AgentExecutor, RequestContext
from a2a.server.events import EventQueue
from a2a.server.tasks import InMemoryTaskStore
from a2a.types import (
    AgentCard,
    AgentCapabilities,
    AgentSkill,
    Artifact,
    Message,
    Part,
    TaskState,
    TaskStatus,
    TaskStatusUpdateEvent,
    TaskArtifactUpdateEvent,
)


class EchoExecutor(AgentExecutor):
    """Echoes the incoming message back as a completed task."""

    async def execute(self, context: RequestContext, event_queue: EventQueue):
        # Extract text from the incoming message
        incoming_text = ""
        if context.message and context.message.parts:
            for part in context.message.parts:
                if hasattr(part, "text") and part.text:
                    incoming_text += part.text

        echo_text = f"[Python Echo] {incoming_text}"

        # Emit working status
        await event_queue.enqueue_event(
            TaskStatusUpdateEvent(
                taskId=context.task_id,
                contextId=context.context_id,
                status=TaskStatus(state=TaskState.working),
                final=False,
            )
        )

        # Emit artifact
        await event_queue.enqueue_event(
            TaskArtifactUpdateEvent(
                taskId=context.task_id,
                contextId=context.context_id,
                artifact=Artifact(
                    artifactId=str(uuid.uuid4()),
                    parts=[Part(type="text", text=echo_text)],
                ),
            )
        )

        # Emit completed status
        await event_queue.enqueue_event(
            TaskStatusUpdateEvent(
                taskId=context.task_id,
                contextId=context.context_id,
                status=TaskStatus(state=TaskState.completed),
                final=True,
            )
        )

    async def cancel(self, context: RequestContext, event_queue: EventQueue):
        await event_queue.enqueue_event(
            TaskStatusUpdateEvent(
                taskId=context.task_id,
                contextId=context.context_id,
                status=TaskStatus(state=TaskState.canceled),
                final=True,
            )
        )


def make_agent_card(port: int) -> AgentCard:
    return AgentCard(
        name="ITK Python Echo Agent",
        description="A2A ITK echo agent implemented in Python",
        url=f"http://0.0.0.0:{port}",
        version="1.0.0",
        capabilities=AgentCapabilities(streaming=True, pushNotifications=True),
        defaultInputModes=["text"],
        defaultOutputModes=["text"],
        skills=[
            AgentSkill(
                id="echo",
                name="Echo",
                description="Echoes the input message back",
                tags=["echo", "test"],
            )
        ],
    )


async def main():
    parser = argparse.ArgumentParser(description="A2A ITK Python Echo Agent")
    parser.add_argument("--port", type=int, default=9100, help="Port to listen on")
    args = parser.parse_args()

    agent_card = make_agent_card(args.port)
    executor = EchoExecutor()
    task_store = InMemoryTaskStore()

    handler = DefaultRequestHandler(
        agent_executor=executor,
        task_store=task_store,
    )

    app = A2AStarletteApplication(
        agent_card=agent_card,
        http_handler=handler,
    )

    import uvicorn
    config = uvicorn.Config(app.build(), host="0.0.0.0", port=args.port)
    server = uvicorn.Server(config)
    print(f"[ITK] Python Echo Agent listening on port {args.port}")
    await server.serve()


if __name__ == "__main__":
    asyncio.run(main())
