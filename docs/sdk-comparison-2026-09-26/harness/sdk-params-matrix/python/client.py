# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
import asyncio, httpx
from a2a.client import ClientFactory, ClientConfig
from a2a.client.card_resolver import A2ACardResolver
from a2a.types import GetExtendedAgentCardRequest
async def main():
    async with httpx.AsyncClient() as hc:
        card = await A2ACardResolver(hc, "http://127.0.0.1:7691").get_agent_card()
        client = ClientFactory(ClientConfig(httpx_client=hc)).create(card)
        ext = await client.get_extended_agent_card(GetExtendedAgentCardRequest())
        print("got card:", ext.name)
asyncio.run(main())
