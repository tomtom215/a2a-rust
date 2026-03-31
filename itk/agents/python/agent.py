#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F.

"""
A2A ITK — Python Echo Agent

A minimal self-contained A2A agent that echoes incoming messages.
Supports both JSON-RPC and REST bindings for cross-language interoperability
testing with the Rust A2A SDK.

Usage:
    python agent.py [--port 9100]
"""

import argparse
import asyncio
import json
import uuid

from starlette.applications import Starlette
from starlette.requests import Request
from starlette.responses import JSONResponse
from starlette.routing import Route

# In-memory stores
tasks: dict[str, dict] = {}
push_configs: dict[str, dict] = {}

AGENT_CARD = {
    "name": "ITK Python Echo Agent",
    "description": "A2A ITK echo agent implemented in Python",
    "url": "http://0.0.0.0:{port}",
    "version": "1.0.0",
    "capabilities": {"streaming": True, "pushNotifications": True},
    "supportedInterfaces": ["a2a"],
    "defaultInputModes": ["text"],
    "defaultOutputModes": ["text"],
    "skills": [
        {
            "id": "echo",
            "name": "Echo",
            "description": "Echoes the input message back",
            "tags": ["echo", "test"],
        }
    ],
}


def extract_text(message: dict) -> str:
    parts = message.get("parts", [])
    return "".join(p.get("text", "") for p in parts if "text" in p)


def process_message(params: dict) -> dict:
    text = extract_text(params.get("message", {}))
    task_id = str(uuid.uuid4())
    context_id = (params.get("message", {}).get("contextId") or params.get("contextId") or str(uuid.uuid4()))

    task = {
        "id": task_id,
        "contextId": context_id,
        "status": {"state": "TASK_STATE_COMPLETED"},
        "history": [params.get("message", {})],
        "artifacts": [
            {
                "artifactId": str(uuid.uuid4()),
                "parts": [{"text": f"[Python Echo] {text}"}],
            }
        ],
    }
    tasks[task_id] = task
    return {"task": task}


def jsonrpc_error(req_id, code: int, message: str) -> dict:
    return {"jsonrpc": "2.0", "id": req_id, "error": {"code": code, "message": message}}


def jsonrpc_result(req_id, result) -> dict:
    return {"jsonrpc": "2.0", "id": req_id, "result": result}


# --- Route handlers ---


async def agent_card(request: Request) -> JSONResponse:
    return JSONResponse(AGENT_CARD)


async def jsonrpc_handler(request: Request) -> JSONResponse:
    body = await request.json()
    req_id = body.get("id")
    method = body.get("method", "")
    params = body.get("params", {})

    if body.get("jsonrpc") != "2.0":
        return JSONResponse(jsonrpc_error(req_id, -32600, "Invalid Request"))

    if method in ("SendMessage", "SendStreamingMessage"):
        if not params.get("message"):
            return JSONResponse(
                jsonrpc_error(req_id, -32602, "Invalid params: missing message field")
            )
        return JSONResponse(jsonrpc_result(req_id, process_message(params)))

    elif method == "GetTask":
        task = tasks.get(params.get("id", ""))
        if task:
            return JSONResponse(jsonrpc_result(req_id, task))
        return JSONResponse(jsonrpc_error(req_id, -32001, "Task not found"))

    elif method == "ListTasks":
        return JSONResponse(
            jsonrpc_result(req_id, {"tasks": list(tasks.values())})
        )

    elif method == "CancelTask":
        task = tasks.get(params.get("id", ""))
        if not task:
            return JSONResponse(jsonrpc_error(req_id, -32001, "Task not found"))
        task["status"] = {"state": "TASK_STATE_CANCELED"}
        return JSONResponse(jsonrpc_result(req_id, task))

    elif method == "CreateTaskPushNotificationConfig":
        cfg_id = params.get("id") or str(uuid.uuid4())
        config = {**params, "id": cfg_id}
        task_id = params.get("taskId", "")
        push_configs[f"{task_id}:{cfg_id}"] = config
        return JSONResponse(jsonrpc_result(req_id, config))

    elif method == "GetTaskPushNotificationConfig":
        key = f"{params.get('taskId', '')}:{params.get('id', '')}"
        cfg = push_configs.get(key)
        if cfg:
            return JSONResponse(jsonrpc_result(req_id, cfg))
        return JSONResponse(jsonrpc_error(req_id, -32001, "Config not found"))

    elif method == "ListTaskPushNotificationConfigs":
        task_id = params.get("taskId", "")
        configs = [c for c in push_configs.values() if c.get("taskId") == task_id]
        return JSONResponse(jsonrpc_result(req_id, configs))

    elif method == "DeleteTaskPushNotificationConfig":
        key = f"{params.get('taskId', '')}:{params.get('id', '')}"
        push_configs.pop(key, None)
        return JSONResponse(jsonrpc_result(req_id, {}))

    else:
        return JSONResponse(
            jsonrpc_error(req_id, -32601, f"Method not found: {method}")
        )


async def rest_send_message(request: Request) -> JSONResponse:
    params = await request.json()
    return JSONResponse(process_message(params))


async def rest_list_tasks(request: Request) -> JSONResponse:
    return JSONResponse({"tasks": list(tasks.values())})


async def rest_get_task(request: Request) -> JSONResponse:
    task_id = request.path_params["task_id"]
    task = tasks.get(task_id)
    if task:
        return JSONResponse(task)
    return JSONResponse({"code": -32001, "message": "Task not found"}, status_code=404)


async def rest_cancel_task(request: Request) -> JSONResponse:
    task_id = request.path_params["task_id"]
    task = tasks.get(task_id)
    if not task:
        return JSONResponse(
            {"code": -32001, "message": "Task not found"}, status_code=404
        )
    task["status"] = {"state": "TASK_STATE_CANCELED"}
    return JSONResponse(task)


async def rest_push_config_collection(request: Request) -> JSONResponse:
    """Handles both POST (create) and GET (list) on /tasks/{task_id}/pushNotificationConfigs."""
    task_id = request.path_params["task_id"]
    if request.method == "POST":
        body = await request.json()
        cfg_id = body.get("id") or str(uuid.uuid4())
        config = {**body, "id": cfg_id, "taskId": task_id}
        push_configs[f"{task_id}:{cfg_id}"] = config
        return JSONResponse(config)
    else:
        configs = [c for c in push_configs.values() if c.get("taskId") == task_id]
        return JSONResponse(configs)


async def rest_get_push_config(request: Request) -> JSONResponse:
    task_id = request.path_params["task_id"]
    config_id = request.path_params["config_id"]
    key = f"{task_id}:{config_id}"
    cfg = push_configs.get(key)
    if cfg:
        return JSONResponse(cfg)
    return JSONResponse(
        {"code": -32001, "message": "Config not found"}, status_code=404
    )


async def rest_delete_push_config(request: Request) -> JSONResponse:
    task_id = request.path_params["task_id"]
    config_id = request.path_params["config_id"]
    push_configs.pop(f"{task_id}:{config_id}", None)
    return JSONResponse({})


async def rest_push_config_item(request: Request) -> JSONResponse:
    """Routes GET and DELETE on /tasks/{task_id}/pushNotificationConfigs/{config_id}."""
    if request.method == "DELETE":
        return await rest_delete_push_config(request)
    return await rest_get_push_config(request)


async def catch_all(request: Request) -> JSONResponse:
    return JSONResponse(
        {"code": -32601, "message": "Not found"}, status_code=404
    )


def create_app(port: int) -> Starlette:
    AGENT_CARD["url"] = f"http://0.0.0.0:{port}"

    routes = [
        Route("/.well-known/agent-card.json", agent_card, methods=["GET"]),
        # JSON-RPC
        Route("/", jsonrpc_handler, methods=["POST"]),
        # REST endpoints
        Route("/message:send", rest_send_message, methods=["POST"]),
        Route("/message:stream", rest_send_message, methods=["POST"]),
        Route("/tasks", rest_list_tasks, methods=["GET"]),
        Route("/tasks/{task_id}", rest_get_task, methods=["GET"]),
        Route("/tasks/{task_id}:cancel", rest_cancel_task, methods=["POST"]),
        Route(
            "/tasks/{task_id}/pushNotificationConfigs/{config_id}",
            rest_push_config_item,
            methods=["GET", "DELETE"],
        ),
        Route(
            "/tasks/{task_id}/pushNotificationConfigs",
            rest_push_config_collection,
            methods=["GET", "POST"],
        ),
        # Catch-all
        Route("/{path:path}", catch_all),
    ]

    return Starlette(routes=routes)


async def main():
    parser = argparse.ArgumentParser(description="A2A ITK Python Echo Agent")
    parser.add_argument("--port", type=int, default=9100, help="Port to listen on")
    args = parser.parse_args()

    import uvicorn

    config = uvicorn.Config(create_app(args.port), host="0.0.0.0", port=args.port)
    server = uvicorn.Server(config)
    print(f"[ITK] Python Echo Agent listening on port {args.port}")
    await server.serve()


if __name__ == "__main__":
    asyncio.run(main())
