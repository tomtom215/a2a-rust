// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

using A2A;
using A2A.AspNetCore;
using Microsoft.Extensions.Logging.Abstractions;

static AgentCard Card(string name) => new()
{
    Name = name, Description = "d", Version = "1.0.0",
    SupportedInterfaces = [ new AgentInterface { Url = "http://127.0.0.1:7661/", ProtocolBinding = "JSONRPC", ProtocolVersion = "1.0" } ],
    DefaultInputModes = ["text/plain"], DefaultOutputModes = ["text/plain"],
    Capabilities = new AgentCapabilities { ExtendedAgentCard = true },
    Skills = [ new AgentSkill { Id = "s", Name = "s", Description = "s", Tags = ["t"] } ],
};

var builder = WebApplication.CreateBuilder(args);
builder.Logging.SetMinimumLevel(LogLevel.Warning);
var app = builder.Build();
var server = new ExtServer(new Noop(), new InMemoryTaskStore(), new ChannelEventNotifier(), NullLogger<A2AServer>.Instance, Card("dotnet-EXTENDED"));
app.MapA2A(server, "/");
app.MapWellKnownAgentCard(Card("dotnet-public"));
app.Run("http://127.0.0.1:7661");

sealed class Noop : IAgentHandler
{
    public Task ExecuteAsync(RequestContext context, AgentEventQueue eventQueue, CancellationToken cancellationToken) => Task.CompletedTask;
}
sealed class ExtServer(IAgentHandler h, ITaskStore s, ChannelEventNotifier n, Microsoft.Extensions.Logging.ILogger<A2AServer> l, AgentCard ext) : A2AServer(h, s, n, l)
{
    public override Task<AgentCard> GetExtendedAgentCardAsync(GetExtendedAgentCardRequest request, CancellationToken cancellationToken = default) => Task.FromResult(ext);
}
