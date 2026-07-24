// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
//
// A2A echo agent built on the OFFICIAL JavaScript SDK (`@a2a-js/sdk`).
//
// Unlike `itk/agents/js-agent` (a dependency-light stub that hand-writes the
// wire format), this agent is assembled from the official SDK's server
// framework — `DefaultRequestHandler`, the Express transport adapters, the
// `ExecutionEventBus` — so running our TCK against it validates this Rust
// SDK's wire expectations against the reference JS implementation.
//
// Behavior contract (same as every ITK echo agent): SendMessage returns a
// completed task whose artifact echoes the input text as `Echo: <text>`.
//
// Run: npm install && node index.js     Env: PORT (default 9111).

const express = require("express");
const { TaskState } = require("@a2a-js/sdk");
const {
  AgentEvent,
  DefaultRequestHandler,
  DefaultPushNotificationSender,
  InMemoryPushNotificationStore,
  InMemoryTaskStore,
} = require("@a2a-js/sdk/server");
const {
  UserBuilder,
  agentCardHandler,
  jsonRpcHandler,
  restHandler,
} = require("@a2a-js/sdk/server/express");

const PORT = parseInt(process.env.PORT || "9111", 10);
const BASE_URL = `http://127.0.0.1:${PORT}`;

const agentCard = {
  name: "official-js-echo",
  description: "Echo agent built on the official @a2a-js/sdk (JavaScript)",
  version: "1.0.0",
  provider: undefined,
  supportedInterfaces: [
    { url: BASE_URL, protocolBinding: "JSONRPC", protocolVersion: "1.0", tenant: "" },
    { url: BASE_URL, protocolBinding: "HTTP+JSON", protocolVersion: "1.0", tenant: "" },
  ],
  capabilities: {
    streaming: true,
    pushNotifications: true,
    extendedAgentCard: false,
    extensions: [],
  },
  securitySchemes: {},
  securityRequirements: [],
  defaultInputModes: ["text/plain"],
  defaultOutputModes: ["text/plain"],
  skills: [
    {
      id: "echo",
      name: "Echo",
      description: "Echoes back the input text",
      tags: ["echo", "test"],
      examples: [],
      inputModes: [],
      outputModes: [],
      securityRequirements: [],
    },
  ],
  signatures: [],
};

class EchoExecutor {
  async execute(ctx, eventBus) {
    const now = () => new Date().toISOString();
    const text = (ctx.userMessage.parts || [])
      .filter((p) => p.content && p.content.$case === "text")
      .map((p) => p.content.value)
      .join("\n");

    // The SDK requires a full `task` (or `message`) event first.
    eventBus.publish(
      AgentEvent.task({
        id: ctx.taskId,
        contextId: ctx.contextId,
        status: { state: TaskState.TASK_STATE_SUBMITTED, message: undefined, timestamp: now() },
        artifacts: [],
        history: [ctx.userMessage],
        metadata: undefined,
      })
    );
    eventBus.publish(
      AgentEvent.statusUpdate({
        taskId: ctx.taskId,
        contextId: ctx.contextId,
        status: { state: TaskState.TASK_STATE_WORKING, message: undefined, timestamp: now() },
        metadata: undefined,
      })
    );
    eventBus.publish(
      AgentEvent.artifactUpdate({
        taskId: ctx.taskId,
        contextId: ctx.contextId,
        artifact: {
          artifactId: "echo-artifact",
          name: "echo",
          description: "",
          parts: [{ content: { $case: "text", value: `Echo: ${text}` }, metadata: undefined }],
          metadata: undefined,
          extensions: [],
        },
        append: false,
        lastChunk: true,
        metadata: undefined,
      })
    );
    eventBus.publish(
      AgentEvent.statusUpdate({
        taskId: ctx.taskId,
        contextId: ctx.contextId,
        status: { state: TaskState.TASK_STATE_COMPLETED, message: undefined, timestamp: now() },
        metadata: undefined,
      })
    );
    eventBus.finished();
  }

  async cancelTask(taskId, eventBus) {
    eventBus.publish(
      AgentEvent.statusUpdate({
        taskId,
        contextId: "",
        status: {
          state: TaskState.TASK_STATE_CANCELED,
          message: undefined,
          timestamp: new Date().toISOString(),
        },
        metadata: undefined,
      })
    );
    eventBus.finished();
  }
}

const pushStore = new InMemoryPushNotificationStore();
const handler = new DefaultRequestHandler(
  agentCard,
  new InMemoryTaskStore(),
  new EchoExecutor(),
  undefined,
  pushStore,
  new DefaultPushNotificationSender(pushStore)
);

const app = express();
app.use("/.well-known/agent-card.json", agentCardHandler({ agentCardProvider: handler }));
app.use("/", restHandler({ requestHandler: handler, userBuilder: UserBuilder.noAuthentication }));
app.use("/", jsonRpcHandler({ requestHandler: handler, userBuilder: UserBuilder.noAuthentication }));

app.listen(PORT, "127.0.0.1", () => {
  console.log(`official-js-echo listening on ${BASE_URL}`);
});
