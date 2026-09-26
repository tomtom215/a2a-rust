// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

using A2A;
var client = new A2AClient(new Uri("http://127.0.0.1:7695/"));
var card = await client.GetExtendedAgentCardAsync(new GetExtendedAgentCardRequest());
Console.WriteLine("got card: " + card.Name);
