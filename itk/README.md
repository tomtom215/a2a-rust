# A2A Integration Test Kit (ITK)

Cross-language interoperability testing for the A2A protocol.

## Overview

The ITK verifies that the Rust A2A SDK can communicate with agents written in
all official SDK languages: Python, JavaScript/TypeScript, Go, and Java.

Each language implements a simple "echo" agent that:
1. Accepts a `message/send` request
2. Returns a completed task with the echoed message as an artifact
3. Supports all 9 A2A v1.0 JSON-RPC methods

## Architecture

```
itk/
├── agents/
│   ├── python/          # Python agent (a2a-python SDK)
│   ├── js-agent/        # Node.js agent (a2a-js SDK)
│   ├── go-agent/        # Go agent (a2a-go SDK)
│   └── java-agent/      # Java agent (a2a-java SDK)
├── tests/               # (placeholder for future integration tests)
├── docker-compose.yml   # Runs all agents + tests
└── README.md
```

## Running

### Docker Compose (recommended)

```bash
docker compose -f itk/docker-compose.yml up --build --abort-on-container-exit
```

### Manual

1. Start each language agent on its designated port:
   - Python: `cd itk/agents/python && pip install -r requirements.txt && python agent.py` (port 9100)
   - Node.js: `cd itk/agents/js-agent && npm install && node index.js` (port 9101)
   - Go: `cd itk/agents/go-agent && go run .` (port 9102)
   - Java: `cd itk/agents/java-agent && mvn compile exec:java` (port 9103)

2. Run the Rust TCK against each:
   ```bash
   cargo run -p a2a-tck -- --url http://localhost:9100
   cargo run -p a2a-tck -- --url http://localhost:9101
   cargo run -p a2a-tck -- --url http://localhost:9102
   cargo run -p a2a-tck -- --url http://localhost:9103
   ```

