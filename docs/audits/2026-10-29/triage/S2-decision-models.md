<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Decision models ("JEV models"): what the primary pages say

Read on 2026-10-31 by two web-fetch agents (a model's reading; quotations are theirs, not re-fetched by the lead). Pages: <https://docs.vllm.ai/en/latest/models/structured_decisions_models/> (footer date October 29, 2026; the maintainer supplied this link), and the OpenAI guide it links, <https://developers.openai.com/api/docs/guides/decisions>. Not read: TypeSafe's own documentation (`docs.typesafe.ai` was not found by search), OpenAI's field-level reference, the Hugging Face model cards. Earlier web-search results (secondary: Telnyx, Eden AI, Lava, OpenRouter, Pydantic AI, ElectronHub) describe the same System One wire shape on hosted gateways; their details conflict (calibrated probabilities against relative preferences).

## What it is

vLLM: "Structured Decisions Models (aka JEV Models) map inputs directly to predefined options or categories and return probabilities and confidence scores, commonly used for classification and routing." Two HTTP surfaces, both served by one vLLM server:

- `/v1/systemone`, section "TypeSafe System One Endpoint": "retains its existing request format and is available by default". Method not stated on the page (the example is a `curl -d`, so POST).
- `POST /v1/decisions`: "vLLM provides a text-only implementation of OpenAI's `POST /v1/decisions` API." OpenAI's guide: "The Decisions API is now generally available." A reader of the vLLM page needs "OpenAI Python SDK 3.26.0 or newer for `client.decisions.create`".

## `/v1/systemone` (TypeSafe System One)

Request: `model`, `state` ("a string, or JSON that is sent as its JSON text"), `questions` (id to `{type, instructions, criteria}`, asked in order), optional `instructions`, `chat_template_kwargs`, `cache_salt`, `joint` (DiffusionGemma only), and media URLs `images`, `audio`, `videos`. Question types: `choice` (criteria: option name to description or `null`), `noul` (a yes/no question; criteria `null` or `{"true": "...", "false": "..."}`), `score` (ordered list of level names). Limits: 64 questions per request, 26 options per choice, 10 levels per score; a choice has at least one option, a score at least two levels. "Unknown top-level fields, and question fields other than `type`, `instructions` and `criteria`, are rejected with a 400."

Response: `id`, `object: "structured_decision"`, `created`, `model`, `answers` (per question id: `{type, choice|noul|score, probabilities, confidence}`; a score also has `legend`), `usage` (`input_tokens`, `output_tokens`), `diagnostics` (per question `label_mass`, `argmax_is_label`), and `joint` (null unless a joint read). "`confidence` is the chosen option's probability over the whole vocabulary: its share of the labels times `label_mass`." The page's own example does not agree with its definitions (probabilities `{0:0.7,1:0.2,2:0.1}` give an expected level of 0.4, the example says `score: 0.6`): a client should parse the numbers and compute none of them.

## `/v1/decisions` (OpenAI)

Request: `model`, `input` (a string or user messages), `questions` (types `predicate`, `choice` with `choices: [{"value": ...}]`, `score` with `levels: [{"label": ...}]`; `name`, `instructions`), optional `safety_identifier`. "This MVP supports 1-64 questions, 2-26 choices, and 2-10 score levels." OpenAI's guide allows 1 to 400 questions. Response (OpenAI's guide): an `answers` array in question order, each with `type`, `name`; predicate `probability`; choice `choice`, `probabilities` (array of `{value, probability}`), `confidence`; score `score`, `probabilities` (array with `label`), `confidence`; a refusal is `type: "refusal"`. Not supported by vLLM's MVP: images, other roles, tools, streaming.

## Semantics, serving, errors

- Probabilities: "Softmax over the labels"; `confidence` "is not calibrated to OpenAI's models"; neither page claims calibration. They "can differ between identical requests" with batching and the prefix cache (`VLLM_BATCH_INVARIANT=1` for repeatability).
- Served with `vllm serve <model>` (the first example is `Qwen/Qwen3-0.6B`), with `--logprobs-mode raw_logprobs` or `processed_logprobs` required for `label_mass`. Supported architectures include `Qwen3ForCausalLM`, `Gemma4ForCausalLM`, `Qwen3_5ForConditionalGeneration`, and purpose-built ones (`Cloudflare/clef-flash`, `vllm-sr/d3`, `LiquidAI/d1-3B`). A GPU is implied; hardware requirements were not on the page.
- Errors seen: 400 (unknown fields, schema over a bound, unsupported options for a model), 501 ("Models without a supported read strategy"). Not seen: 401, 404, 413, 429, 5xx, or the error body.
- No authentication is described for the server.

## What this means for the build

The SDK's llama.cpp and Ollama setups do not serve these endpoints, so a live run here needs vLLM, which this sandbox lacks. A client written from these pages would be tested against a mock server built from the documented shapes and labelled "not run against a real engine". Two wire shapes exist for one idea (System One and OpenAI), plus hosted gateways with their own paths; the first design question is one `DecisionModel` trait with two wire adapters, and where in the tool loop a decision belongs (a gate before a side-effecting tool; routing a delegation).
