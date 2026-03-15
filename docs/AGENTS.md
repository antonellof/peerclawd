# Agent Specification

Agents are defined in TOML files:

```toml
[agent]
name = "research-bot"
version = "0.1.0"
description = "Autonomous research agent"

[model]
provider = "network"          # Use network inference
model = "llama-3.3-70b"
fallback = "llama-3.2-3b"
max_tokens_per_request = 4096
temperature = 0.7

[budget]
max_spend_per_hour = 100      # Max tokens per hour
max_spend_total = 10000       # Lifetime budget

[capabilities]
web_access = true
storage = true
tool_building = true
agent_communication = true

[web_access]
allowed_hosts = ["*.wikipedia.org", "arxiv.org"]
max_requests_per_minute = 30

[tools]
builtin = ["web_fetch", "web_search", "file_store"]
wasm = ["./tools/custom_parser.wasm"]

[channels]
repl = true
webhook = { port = 9090, path = "/hook" }
websocket = true

[routines]
daily_scan = { cron = "0 8 * * *", task = "scan_sources" }
heartbeat = { interval = "5m", task = "check_status" }
```

## Deploy an Agent

```bash
peerclawd agent run agent.toml
```

## Agent Channels

- **REPL**: `peerclawd agent attach <id>`
- **Webhooks**: HTTP triggers
- **WebSocket**: Real-time streaming
- **P2P Direct**: Agent-to-agent over GossipSub
