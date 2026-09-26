// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

import express from 'express';
import { DefaultRequestHandler, InMemoryTaskStore } from '@a2a-js/sdk/server';
import { jsonRpcHandler, agentCardHandler } from '@a2a-js/sdk/server/express';
const card = (name) => ({
  name, description: 'd', version: '1.0.0',
  supportedInterfaces: [{ url: 'http://127.0.0.1:7641/', protocolBinding: 'JSONRPC', protocolVersion: '1.0', tenant: '' }],
  capabilities: { extendedAgentCard: true, streaming: false, pushNotifications: false, extensions: [] },
  defaultInputModes: ['text/plain'], defaultOutputModes: ['text/plain'],
  skills: [{ id: 's', name: 's', description: 's', tags: ['t'], examples: [], inputModes: [], outputModes: [], securityRequirements: [] }],
  securitySchemes: {}, securityRequirements: [], signatures: [],
});
const executor = { execute: async () => {}, cancelTask: async () => {} };
const handler = new DefaultRequestHandler(card('js-public'), new InMemoryTaskStore(), executor, undefined, undefined, undefined, card('js-EXTENDED'));
// Simplest auth: any request carrying "Authorization: Bearer probe" is an authenticated user.
const userBuilder = async (req) => req.header('authorization') === 'Bearer probe'
  ? { isAuthenticated: true, userName: 'probe' }
  : { isAuthenticated: false, userName: '' };
const app = express();
app.use('/.well-known/agent-card.json', agentCardHandler({ agentCardProvider: handler }));
app.use('/', jsonRpcHandler({ requestHandler: handler, userBuilder }));
app.listen(7641, '127.0.0.1', () => console.log('listening 7641'));
