// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

import { ClientFactory } from '@a2a-js/sdk/client';
const client = await new ClientFactory().createFromUrl('http://127.0.0.1:7693');
const card = await client.getAgentCard();
console.log('got card:', card.name);
