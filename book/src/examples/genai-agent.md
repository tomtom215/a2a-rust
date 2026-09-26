# Genai Agent

Demonstrates how to wrap a [genai](https://crates.io/crates/genai) multi-provider LLM client behind the A2A protocol. Incoming A2A messages are forwarded to the LLM, and the response is returned as an A2A artifact.

**Source:** [`examples/genai-agent/`](https://github.com/tomtom215/a2a-rust/tree/main/examples/genai-agent)

## Supported providers

| Provider | Model example | Env var |
|----------|--------------|---------|
| OpenAI | `gpt-4o`, `gpt-4o-mini` | `OPENAI_API_KEY` |
| Anthropic | `claude-sonnet-4-20250514` | `ANTHROPIC_API_KEY` |
| Google Gemini | `gemini-1.5-flash` | `GEMINI_API_KEY` |
| Ollama | `llama3` | (local, no key) |
| Groq | `llama3-70b-8192` | `GROQ_API_KEY` |
| Cohere | `command-r-plus` | `COHERE_API_KEY` |

## Running

```bash
# GENAI_MODEL defaults to qwen3.5:0.8b, which runs locally (see below), so
# name a hosted model to use a hosted key:
export OPENAI_API_KEY=sk-...
GENAI_MODEL=gpt-4o-mini cargo run -p genai-a2a-agent

# Use a different model:
GENAI_MODEL=claude-sonnet-4-20250514 cargo run -p genai-a2a-agent
```

Without `A2A_BIND_ADDR`, `cargo run` is a self-driving demo, not a server: it
starts the agent on all four bindings, drives every A2A method over each,
prints `LLM leg: EXERCISED` or `LLM leg: NOT EXERCISED`, and exits. In that
demo a provider error is answered with a *labelled* mechanical fallback
(`[no model reachable — mechanical fallback, not an LLM answer]`), so the
protocol mechanics stay visible with no model at all. Set `A2A_BIND_ADDR` to
serve instead — JSON-RPC on that address, where a provider error fails the
task unless `A2A_ALLOW_FALLBACK=1` turns the fallback back on.

## How it works

```text
A2A Client ──→ A2A Server (JSON-RPC)
                    │
                    ▼
              GenaiAgentExecutor
                    │
              1. Extract user text (no text part → TASK_STATE_FAILED)
              2. Transition to Working
              3. Call genai::Client for LLM completion
              4. Success → artifact + Completed
                 LLM error → TASK_STATE_FAILED when serving
                             (labelled fallback artifact in the demo)
```

## Fully local, no API key

Model names that don't match a hosted provider route to the Ollama adapter
on `:11434` — which is also what llama.cpp's `llama-server` speaks:

```bash
GENAI_MODEL=qwen3.5:0.8b cargo run -p genai-a2a-agent
```

Served with `A2A_BIND_ADDR`, the agent publishes a discovery card, supports
push-config CRUD, and passes the in-repo TCK: 21/21 graded
checks, 1 not applicable, on the JSON-RPC binding. `tck.yml`'s
`tck-example-agents` job re-runs that grade on every push and pull request,
with no model configured — the TCK grades protocol conformance, and none of it
reaches the executor's brain.

## Key integration point

The `GenaiAgentExecutor` implements `AgentExecutor` — the single trait that bridges any LLM framework with A2A. The integration is ~60 lines of code: extract text from the A2A message, call the genai client, and emit the result as an artifact.

This pattern is the same for any LLM integration — see also the [Rig Agent](./rig-agent.md) for an alternative framework.
