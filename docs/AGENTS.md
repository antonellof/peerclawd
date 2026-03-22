# Agent Specification

Agents are autonomous AI entities that run within PeerClaw. They have their own identity, budget, tools, skills, and communication channels.

## Agent Configuration

Agents are defined in TOML files:

```toml
[agent]
name = "research-bot"
version = "0.1.0"
description = "Autonomous research agent with web access and memory"

[model]
provider = "network"          # "local" or "network"
model = "llama-3.3-70b"
fallback = "llama-3.2-3b"     # Fallback if primary unavailable
max_tokens_per_request = 4096
temperature = 0.7
top_p = 0.9

[budget]
funded_by = "pclaw1master..."  # Operator wallet
max_spend_per_request = 10.0   # Max per single job
max_spend_per_hour = 100       # Hourly cap
max_spend_per_day = 500        # Daily cap
max_spend_total = 10000        # Lifetime budget
auto_refill = true
refill_trigger = 50.0
refill_amount = 200.0

[capabilities]
web_access = true
storage = true
tool_building = true
agent_communication = true
vector_memory = true

[web_access]
allowed_hosts = ["*.wikipedia.org", "arxiv.org", "api.github.com"]
blocked_hosts = ["*.onion"]
max_requests_per_minute = 30
timeout_seconds = 30

[tools]
builtin = ["web_fetch", "web_search", "file_read", "file_write", "memory_search", "memory_write"]
wasm = ["./tools/custom_parser.wasm"]
mcp = ["filesystem", "github"]  # MCP server connections

[skills]
local = ["./skills/"]           # Local skill directory
installed = true                # Allow installed network skills
network = false                 # Auto-activate network skills (risky)

[channels]
repl = true
webhook = { port = 9090, path = "/hook" }
websocket = { port = 9091 }
telegram = { token_env = "TELEGRAM_BOT_TOKEN" }
discord = { token_env = "DISCORD_BOT_TOKEN" }

[routines]
daily_scan = { cron = "0 8 * * *", task = "scan_sources" }
heartbeat = { interval = "5m", task = "check_status" }
startup = { on = "start", task = "initialize" }

[memory]
collection = "research-bot-memory"
embedding_model = "nomic-embed-text"
auto_persist = true
max_results = 10
```

## Deploy an Agent

```bash
# Run agent from spec file
peerclaw agent run agent.toml

# Run with custom home directory
PEERCLAW_HOME=/data/agents/research peerclaw agent run agent.toml

# List running agents
peerclaw agent list

# View agent logs
peerclaw agent logs <agent-id>

# Stop agent
peerclaw agent stop <agent-id>

# Attach to agent REPL
peerclaw agent attach <agent-id>
```

## Agent Channels

Agents can communicate through multiple channels simultaneously:

### REPL
Interactive command-line interface:
```bash
peerclaw agent attach research-bot
> What papers were published on transformers this week?
```

### Webhooks
HTTP triggers for external integrations:
```bash
curl -X POST http://localhost:9090/hook \
  -H "Content-Type: application/json" \
  -d '{"message": "Summarize the latest news"}'
```

### WebSocket
Real-time bidirectional streaming:
```javascript
const ws = new WebSocket('ws://localhost:9091');
ws.send(JSON.stringify({ message: 'Hello agent' }));
ws.onmessage = (event) => console.log(event.data);
```

### Telegram
```toml
[channels.telegram]
token_env = "TELEGRAM_BOT_TOKEN"
allowed_users = [123456789]  # Optional user allowlist
```

### Discord
```toml
[channels.discord]
token_env = "DISCORD_BOT_TOKEN"
guild_ids = [123456789]      # Server IDs
channel_ids = [987654321]    # Specific channels
```

### Slack
```toml
[channels.slack]
token_env = "SLACK_BOT_TOKEN"
app_token_env = "SLACK_APP_TOKEN"
channels = ["#ai-assistant"]
```

### P2P Direct
Agent-to-agent communication over GossipSub:
```toml
[channels.p2p]
enabled = true
topics = ["research-agents", "data-sharing"]
```

## Skills

Skills extend agent capabilities with reusable prompt templates.

### Skill File Format (SKILL.md)

```markdown
---
name: code-review
version: 1.0.0
description: Reviews code for bugs, style, and security issues
author: peerclaw
tags: [code, review, security]

activation:
  keywords: [review, code, bug, security, lint]
  exclude_keywords: [write, create, generate]
  patterns:
    - "review (this|the|my) code"
    - "check for (bugs|issues|vulnerabilities)"

requirements:
  tools: [file_read]

trust: local
---

# Code Review Skill

You are a code review expert. When reviewing code:

1. Check for bugs and logic errors
2. Identify security vulnerabilities (OWASP top 10)
3. Suggest style improvements
4. Look for performance issues

## Response Format

Provide findings in this structure:
- **Critical**: Must fix before merge
- **Warning**: Should fix
- **Info**: Nice to have

Always explain *why* something is an issue, not just *what*.
```

### Skill Management

```bash
# List installed skills
peerclaw skill list

# Install from file
peerclaw skill install ./skills/code-review.md

# Install from network peer
peerclaw skill install pclaw1abc.../code-review

# Search network for skills
peerclaw skill search "data analysis"

# Show skill details
peerclaw skill info code-review

# Remove skill
peerclaw skill remove code-review
```

### Skill Activation

Skills are automatically selected based on user input:

1. **Keyword matching** — Input contains skill keywords
2. **Pattern matching** — Input matches regex patterns
3. **Tag matching** — Context tags align with skill tags
4. **Exclusion** — Skip if exclude_keywords present

Activation score determines which skill (if any) is applied.

### Trust Levels

| Level | Source | Tool Access |
|-------|--------|-------------|
| `local` | User's `~/.peerclaw/skills/` | All tools |
| `installed` | Explicitly installed from network | Read-only tools |
| `network` | Discovered, not installed | Minimal (echo, time) |

## Tools

### Builtin Tools

| Tool | Description |
|------|-------------|
| `echo` | Echo input back |
| `time` | Current timestamp |
| `json` | Parse/format JSON |
| `web_fetch` | HTTP GET request |
| `web_search` | Web search query |
| `file_read` | Read local file |
| `file_write` | Write local file |
| `file_list` | List directory |
| `shell` | Execute shell command |
| `memory_search` | Vector similarity search |
| `memory_write` | Store in vector memory |
| `job_submit` | Submit P2P job |
| `job_status` | Check job status |
| `peer_list` | List connected peers |
| `wallet_balance` | Check token balance |

### WASM Tools

Custom tools packaged as WASM modules:

```bash
# Build from Rust source
peerclaw tool build ./my-tool --output ./tools/my-tool.wasm

# Install WASM tool
peerclaw tool install ./tools/my-tool.wasm

# List installed tools
peerclaw tool list
```

### MCP Tools

Connect to external MCP servers:

```toml
[mcp.servers]
filesystem = { command = "npx", args = ["-y", "@anthropic/mcp-server-filesystem"] }
github = { url = "http://localhost:3000/mcp" }
```

## Vector Memory

Agents can store and retrieve information using vector memory:

```toml
[memory]
collection = "agent-memory"
embedding_model = "nomic-embed-text"  # Or API-based
embedding_dim = 384
auto_persist = true
persistence_path = "~/.peerclaw/vector/agent-memory"
```

### Memory Tools

```
# Store information
memory_write("The user prefers dark mode", metadata={"type": "preference"})

# Search memory
memory_search("user preferences", k=5)
```

### Memory Commands

```bash
# Create collection for agent
peerclaw vector create agent-memory

# Insert memory
peerclaw vector insert agent-memory "Important fact to remember"

# Search memories
peerclaw vector search agent-memory "what does user prefer" -k 5
```

## Routines

Automated background tasks:

### Cron Schedule
```toml
[routines.daily_report]
cron = "0 9 * * *"           # 9 AM daily
task = "generate_report"
prompt = "Generate daily summary of research findings"
```

### Interval
```toml
[routines.health_check]
interval = "5m"              # Every 5 minutes
task = "check_health"
tool = "web_fetch"
args = { url = "https://api.example.com/health" }
```

### Event-Triggered
```toml
[routines.on_message]
event = "channel_message"
channel = "telegram"
task = "process_message"
```

### Startup
```toml
[routines.init]
on = "start"
task = "initialize"
prompt = "Load previous context and prepare for operations"
```

## Agent Identity

Each agent has a unique Ed25519 identity:

```
~/.peerclaw/agents/research-bot/
├── identity.key              # Agent keypair
├── config.toml               # Runtime config
├── state.db                  # Agent state (redb)
├── memory/                   # Vector collections
└── logs/                     # Agent logs
```

The agent's peer ID is derived from its public key and used for:
- P2P messaging
- Job attribution
- Reputation tracking
- Token transactions

## Multi-Agent Collaboration

Agents can communicate and delegate tasks:

```toml
[agent_communication]
enabled = true
allowed_agents = ["data-collector", "report-writer"]
broadcast_topics = ["research-updates"]
```

### Direct Messaging
```
# In agent prompt
Send a message to data-collector: "Fetch latest arxiv papers on LLMs"
```

### Job Delegation
```
# Agent can submit jobs to network
Submit job to network: inference task with llama-3.3-70b, budget 5 PCLAW
```

## Example Agents

Ready-to-use agent configurations in `examples/agents/`:

| Agent | File | Purpose |
|-------|------|---------|
| **ResearchBot** | `researcher.toml` | Web search, document reading, summarization with vector memory |
| **CodeBot** | `coder.toml` | Code reading, patching, shell testing with restricted commands |
| **NetWatch** | `monitor.toml` | Automated network health checks on cron schedule, daily reports |
| **PeerClawBot** | `telegram-bot.toml` | Telegram bot with web search, powered by local inference |
| **DataBot** | `data-analyst.toml` | Data processing with Python/SQLite, generates insights and reports |

### Quick Start

```bash
# Deploy a research assistant
peerclaw agent run examples/agents/researcher.toml

# Deploy a Telegram bot (set token first)
echo 'TELEGRAM_BOT_TOKEN=your_token_here' >> ~/.peerclaw/.env
peerclaw agent run examples/agents/telegram-bot.toml

# Deploy a network monitor with scheduled checks
peerclaw agent run examples/agents/monitor.toml

# Check running agents
peerclaw agent list

# View agent details
peerclaw agent info <agent-id>
```

### Creating Your Own Agent

1. Copy an example: `cp examples/agents/researcher.toml my-agent.toml`
2. Customize the `[model]` section (model name, temperature, system prompt)
3. Set `[capabilities]` based on what the agent needs
4. Configure `[tools]` with builtin tools and optional WASM tools
5. Add `[channels]` for how users interact (REPL, webhook, Telegram, etc.)
6. Optionally add `[routines]` for scheduled background tasks
7. Deploy: `peerclaw agent run my-agent.toml`

---

*v0.3 — March 2026*
