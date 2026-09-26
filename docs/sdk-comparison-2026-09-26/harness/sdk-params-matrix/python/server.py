# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
import uvicorn
from starlette.applications import Starlette
from a2a.types import AgentCard, AgentCapabilities, AgentInterface, AgentSkill
from a2a.server.request_handlers import DefaultRequestHandler
from a2a.server.tasks import InMemoryTaskStore
from a2a.server.agent_execution import AgentExecutor
from a2a.server.routes import create_jsonrpc_routes, create_agent_card_routes

class Exec(AgentExecutor):
    async def execute(self, context, event_queue): pass
    async def cancel(self, context, event_queue): pass

def card(name):
    return AgentCard(name=name, description="d", version="1.0.0",
        supported_interfaces=[AgentInterface(url="http://127.0.0.1:7621/", protocol_binding="JSONRPC", protocol_version="1.0")],
        capabilities=AgentCapabilities(extended_agent_card=True),
        default_input_modes=["text/plain"], default_output_modes=["text/plain"],
        skills=[AgentSkill(id="s", name="s", description="s", tags=["t"])])

h = DefaultRequestHandler(agent_executor=Exec(), task_store=InMemoryTaskStore(),
    agent_card=card("py-public"), extended_agent_card=card("py-EXTENDED"))
app = Starlette(routes=create_agent_card_routes(card("py-public")) + create_jsonrpc_routes(h, "/"))
uvicorn.run(app, host="127.0.0.1", port=7621, log_level="warning")
