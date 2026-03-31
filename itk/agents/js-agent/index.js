#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

/**
 * A2A ITK — JavaScript/Node.js Echo Agent
 *
 * A minimal A2A agent using Express that echoes incoming messages.
 * Used for cross-language interoperability testing with the Rust A2A SDK.
 *
 * Usage:
 *   node index.js [--port 9101]
 */

import express from 'express';
import { v4 as uuidv4 } from 'uuid';

const PORT = parseInt(process.argv.find((_, i, a) => a[i - 1] === '--port') || '9101', 10);

const app = express();
app.use(express.json());

// In-memory task store
const tasks = new Map();
const pushConfigs = new Map();

// Agent card
const agentCard = {
  name: 'ITK JavaScript Echo Agent',
  description: 'A2A ITK echo agent implemented in JavaScript/Node.js',
  url: `http://0.0.0.0:${PORT}`,
  version: '1.0.0',
  capabilities: { streaming: true, pushNotifications: true },
  supportedInterfaces: ['a2a'],
  defaultInputModes: ['text'],
  defaultOutputModes: ['text'],
  skills: [{
    id: 'echo',
    name: 'Echo',
    description: 'Echoes the input message back',
    tags: ['echo', 'test'],
  }],
};

// Well-known agent card endpoint
app.get('/.well-known/agent-card.json', (_, res) => {
  res.json(agentCard);
});

// Helper: extract text from message parts
function extractText(message) {
  if (!message?.parts) return '';
  return message.parts
    .filter(p => 'text' in p)
    .map(p => p.text)
    .join('');
}

// Helper: create a task from SendMessage params
function processMessage(params) {
  const text = extractText(params.message);
  const taskId = uuidv4();
  const contextId = (params.message && params.message.contextId) || params.contextId || uuidv4();

  const task = {
    id: taskId,
    contextId,
    status: { state: 'TASK_STATE_COMPLETED' },
    history: [params.message],
    artifacts: [{
      artifactId: uuidv4(),
      parts: [{ text: `[JS Echo] ${text}` }],
    }],
  };

  tasks.set(taskId, task);
  return { task };
}

// JSON-RPC handler
app.post('/', (req, res) => {
  const { jsonrpc, id, method, params } = req.body;

  if (jsonrpc !== '2.0') {
    return res.json({ jsonrpc: '2.0', id, error: { code: -32600, message: 'Invalid Request' } });
  }

  try {
    let result;

    switch (method) {
      case 'SendMessage':
        if (!params || !params.message) {
          return res.json({ jsonrpc: '2.0', id, error: { code: -32602, message: 'Invalid params: missing message field' } });
        }
        result = processMessage(params);
        break;

      case 'SendStreamingMessage': {
        if (!params || !params.message) {
          return res.json({ jsonrpc: '2.0', id, error: { code: -32602, message: 'Invalid params: missing message field' } });
        }
        result = processMessage(params);
        break;
      }

      case 'GetTask': {
        const task = tasks.get(params.id);
        if (!task) {
          return res.json({ jsonrpc: '2.0', id, error: { code: -32001, message: 'Task not found' } });
        }
        result = task;
        break;
      }

      case 'ListTasks': {
        const allTasks = Array.from(tasks.values());
        result = { tasks: allTasks };
        break;
      }

      case 'CancelTask': {
        const task = tasks.get(params.id);
        if (!task) {
          return res.json({ jsonrpc: '2.0', id, error: { code: -32001, message: 'Task not found' } });
        }
        task.status = { state: 'TASK_STATE_CANCELED' };
        result = task;
        break;
      }

      case 'CreateTaskPushNotificationConfig': {
        const configId = params.id || uuidv4();
        const config = { ...params, id: configId };
        pushConfigs.set(`${params.taskId}:${configId}`, config);
        result = config;
        break;
      }

      case 'GetTaskPushNotificationConfig': {
        const key = `${params.taskId}:${params.id}`;
        const cfg = pushConfigs.get(key);
        if (!cfg) {
          return res.json({ jsonrpc: '2.0', id, error: { code: -32001, message: 'Config not found' } });
        }
        result = cfg;
        break;
      }

      case 'ListTaskPushNotificationConfigs': {
        const configs = Array.from(pushConfigs.values())
          .filter(c => c.taskId === params.taskId);
        result = configs;
        break;
      }

      case 'DeleteTaskPushNotificationConfig': {
        const delKey = `${params.taskId}:${params.id}`;
        pushConfigs.delete(delKey);
        result = {};
        break;
      }

      default:
        return res.json({ jsonrpc: '2.0', id, error: { code: -32601, message: `Method not found: ${method}` } });
    }

    res.json({ jsonrpc: '2.0', id, result });
  } catch (err) {
    res.json({ jsonrpc: '2.0', id, error: { code: -32603, message: err.message } });
  }
});

// REST endpoints
app.post('/message\\:send', (req, res) => {
  res.json(processMessage(req.body));
});

app.post('/message\\:stream', (req, res) => {
  res.json(processMessage(req.body));
});

app.get('/tasks/:id', (req, res) => {
  const task = tasks.get(req.params.id);
  if (!task) return res.status(404).json({ error: 'Task not found' });
  res.json(task);
});

app.get('/tasks', (_, res) => {
  res.json({ tasks: Array.from(tasks.values()) });
});

// Cancel task
app.post('/tasks/:id\\:cancel', (req, res) => {
  const task = tasks.get(req.params.id);
  if (!task) return res.status(404).json({ code: -32001, message: 'Task not found' });
  task.status = { state: 'TASK_STATE_CANCELED' };
  res.json(task);
});

// Push notification config — create
app.post('/tasks/:taskId/pushNotificationConfigs', (req, res) => {
  const taskId = req.params.taskId;
  const configId = req.body.id || uuidv4();
  const config = { ...req.body, id: configId, taskId };
  pushConfigs.set(`${taskId}:${configId}`, config);
  res.json(config);
});

// Push notification config — get by id
app.get('/tasks/:taskId/pushNotificationConfigs/:configId', (req, res) => {
  const key = `${req.params.taskId}:${req.params.configId}`;
  const cfg = pushConfigs.get(key);
  if (!cfg) return res.status(404).json({ code: -32001, message: 'Config not found' });
  res.json(cfg);
});

// Push notification config — list
app.get('/tasks/:taskId/pushNotificationConfigs', (req, res) => {
  const configs = Array.from(pushConfigs.values())
    .filter(c => c.taskId === req.params.taskId);
  res.json(configs);
});

// Push notification config — delete (DELETE method)
app.delete('/tasks/:taskId/pushNotificationConfigs/:configId', (req, res) => {
  const key = `${req.params.taskId}:${req.params.configId}`;
  pushConfigs.delete(key);
  res.json({});
});

// Catch-all for unknown routes — return JSON 404
app.use((req, res) => {
  res.status(404).json({ code: -32601, message: 'Not found' });
});

app.listen(PORT, '0.0.0.0', () => {
  console.log(`[ITK] JavaScript Echo Agent listening on port ${PORT}`);
});
