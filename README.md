# PeerClaw'd — A Decentralized Peer-to-Peer AI Agent Network

> **Tagline:** *One binary. Distributed intelligence. Token-powered autonomy.*

---

## Vision

PeerClaw'd is a fully decentralized, peer-to-peer network where autonomous AI agents collaborate, share resources, and transact using a native crypto-token economy. Think of it as **BitTorrent meets AI inference** — every peer contributes a slice of compute, storage, or GPU capacity and earns tokens in return, while AI agents spend those tokens to execute tasks, access the web, and scale inference across the network.

PeerClaw'd ships as a **single static binary** with both CLI and embedded web UI. One command to join the network, one command to run an agent, one command to contribute resources. No containers, no orchestrators, no cloud dependencies — just a self-organizing mesh of intelligent agents and resource providers, connected by cryptographic trust and economic incentives.

```
$ peerclawd serve --gpu --storage 50GB --web :8080
```

---

## Quickstart

### Build from source

```bash
# Clone the repository
git clone https://github.com/yourusername/peerclawd.git
cd peerclawd

# Build in release mode
cargo build --release

# The binary is at ./target/release/peerclawd
```

### Start a peer node

```bash
# Start a basic node
./target/release/peerclawd serve

# Start with web dashboard on port 8080
./target/release/peerclawd serve --web 127.0.0.1:8080

# Start as a job provider (accept jobs from network)
./target/release/peerclawd serve --provider --price-per-token 100

# Start with a bootstrap peer
./target/release/peerclawd serve --bootstrap /ip4/192.168.1.10/tcp/9000/p2p/12D3KooW...
```

### Interactive AI Chat

```bash
# Start chat with default model
./target/release/peerclawd chat

# Specify model and settings
./target/release/peerclawd chat --model llama-3.2-3b --max-tokens 500 --temperature 0.7

# Use distributed inference (offload to network peers)
./target/release/peerclawd chat --distributed

# Disable streaming (wait for complete response)
./target/release/peerclawd chat --no-stream
```

**Streaming:** By default, tokens are printed as they're generated for a more interactive experience. Use `--no-stream` to wait for the complete response.

Chat commands (Claude-Code style):
- `/help` - Show all available commands
- `/status` - Show runtime status (peer ID, balance, resources)
- `/model <name>` - Switch to a different model
- `/temperature <n>` - Set temperature (0.0-2.0)
- `/max-tokens <n>` - Set max tokens per response
- `/settings` - Open interactive settings menu
- `/history` - Show conversation summary
- `/export <path>` - Export conversation to file
- `/distributed on|off` - Toggle distributed mode
- `/stream on|off` - Toggle streaming (real-time token output)
- `/clear` - Clear conversation history
- `/quit` or `exit` - Exit chat

Settings are persisted to `~/.peerclawd/chat_settings.json`.

### Model Setup

PeerClaw'd uses GGUF format models for local inference. Download a model to get started:

```bash
# Create models directory
mkdir -p ~/.peerclawd/models

# Download a quantized model (Llama 3.2 1B, ~770MB)
curl -L -o ~/.peerclawd/models/llama-3.2-1b-instruct-q4_k_m.gguf \
  "https://huggingface.co/bartowski/Llama-3.2-1B-Instruct-GGUF/resolve/main/Llama-3.2-1B-Instruct-Q4_K_M.gguf"

# Or download a larger model for better quality (Llama 3.2 3B, ~2GB)
curl -L -o ~/.peerclawd/models/llama-3.2-3b-instruct-q4_k_m.gguf \
  "https://huggingface.co/bartowski/Llama-3.2-3B-Instruct-GGUF/resolve/main/Llama-3.2-3B-Instruct-Q4_K_M.gguf"
```

### Test Commands

```bash
# Test local inference
./target/release/peerclawd test inference --model llama-3.2-1b --prompt "Hello!" --max-tokens 50

# Test web fetch
./target/release/peerclawd test fetch --url https://example.com

# Run all local tests (inference, web fetch)
./target/release/peerclawd test all

# Test distributed execution with multiple nodes
./target/release/peerclawd test distributed --agents 3 --duration 30
```

### Multi-Peer Cluster Testing

Spawn a test cluster with multiple peer nodes to test distributed job execution:

```bash
# Start a 3-node cluster
./target/release/peerclawd test cluster --nodes 3

# Start cluster and run a test inference job
./target/release/peerclawd test cluster --nodes 3 --run-test-job

# Keep cluster running for manual testing (Ctrl+C to stop)
./target/release/peerclawd test cluster --nodes 3 --keep-alive

# Custom ports
./target/release/peerclawd test cluster --nodes 5 --base-web-port 9000 --base-p2p-port 10000
```

The cluster command:
- Spawns N separate peer node processes
- Auto-connects nodes via bootstrap
- Displays status table with peer IDs
- Shows which peer executed each job

### OpenAI-Compatible API

PeerClaw'd provides OpenAI-compatible endpoints for easy integration with existing tools and SDKs:

```bash
# Start a node with web API
./target/release/peerclawd serve --web 127.0.0.1:8080
```

**Endpoints:**
- `POST /v1/chat/completions` - Chat completions (with SSE streaming support)
- `GET /v1/models` - List available models
- `POST /v1/embeddings` - Embeddings (coming soon)

**Python Example:**
```python
from openai import OpenAI

client = OpenAI(
    base_url="http://localhost:8080/v1",
    api_key="unused"  # No auth required
)

# Non-streaming
response = client.chat.completions.create(
    model="llama-3.2-3b",
    messages=[{"role": "user", "content": "Hello!"}]
)
print(response.choices[0].message.content)

# Streaming
stream = client.chat.completions.create(
    model="llama-3.2-3b",
    messages=[{"role": "user", "content": "Tell me a story"}],
    stream=True
)
for chunk in stream:
    if chunk.choices[0].delta.content:
        print(chunk.choices[0].delta.content, end="")
```

**curl Example:**
```bash
# Non-streaming
curl http://localhost:8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{"model": "llama-3.2-3b", "messages": [{"role": "user", "content": "Hello!"}]}'

# Streaming
curl http://localhost:8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{"model": "llama-3.2-3b", "messages": [{"role": "user", "content": "Hello!"}], "stream": true}'

# List models
curl http://localhost:8080/v1/models
```

### Multi-Node Testing

Run a distributed test with multiple peers:

```bash
# Terminal 1: Start main node
./target/release/peerclawd serve --web 127.0.0.1:8080

# Terminal 2: Start provider node (connects to main)
./target/release/peerclawd serve --bootstrap /ip4/127.0.0.1/tcp/9000 --provider

# Terminal 3: Submit a job from main node's web UI at http://127.0.0.1:8080
# Or use the chat command to trigger distributed inference
./target/release/peerclawd chat --distributed
```

### Web Dashboard

Start the node with `--web` flag, then open http://localhost:8080 in your browser:

```bash
./target/release/peerclawd serve --web 127.0.0.1:8080
```

The dashboard shows:
- **Network topology** - Visual graph of connected peers
- **System resources** - Real-time CPU, RAM, GPU monitoring
- **Job status** - Active and completed jobs
- **Wallet balance** - Token balance and transactions

---

## Core Principles

1. **Single Binary, Zero Dependencies** — One static binary for Linux, macOS, and Windows. CLI-first with an embedded web dashboard. Inspired by Consul, Nomad, k3s.
2. **Fully Decentralized** — No orchestrator, no cloud dependency. Peers discover each other, negotiate resources, and execute workloads through P2P protocols.
3. **Token Economy** — Every resource has a price. Computation, storage, bandwidth, GPU cycles, and web access are all traded via a native utility token.
4. **Security First** — Untrusted code runs in WASM sandboxes and microVM isolation. Secrets never leak. Prompt injection is actively defended against.
5. **Always Available** — Agents operate online and offline, syncing state when connectivity returns. The network is resilient by design.
6. **Self-Expanding** — Agents can build their own tools, discover new capabilities, and evolve their skill set dynamically.

---

## Architecture Overview

### Single Binary Architecture

PeerClaw'd follows the HashiCorp model: a single statically-linked binary that operates in multiple modes depending on flags and subcommands. Every peer runs the same binary — the role (resource provider, agent host, gateway, or all-in-one) is determined at runtime.

```
peerclawd
├── serve          # Start a peer node (resource provider + agent host)
│   ├── --gpu              # Advertise GPU resources
│   ├── --cpu <cores>      # Limit CPU contribution
│   ├── --storage <size>   # Allocate distributed storage
│   ├── --web <addr>       # Enable embedded web UI
│   ├── --bootstrap <peer> # Join existing network via known peer
│   └── --wallet <path>    # Path to wallet keyfile
├── agent
│   ├── run <spec>         # Deploy and run an agent from spec
│   ├── list               # List running agents
│   ├── logs <id>          # Stream agent logs
│   └── stop <id>          # Stop an agent
├── network
│   ├── status             # Show network topology and connected peers
│   ├── peers              # List known peers and their resources
│   └── discover           # Force peer discovery round
├── wallet
│   ├── create             # Generate new keypair and wallet
│   ├── balance            # Check token balance
│   ├── send <to> <amt>    # Transfer tokens
│   └── history            # Transaction history
├── tool
│   ├── build <desc>       # Build a WASM tool from description
│   ├── install <url>      # Install a WASM tool from registry
│   └── list               # List installed tools
└── version                # Print version and build info
```

### Internal Process Architecture

Inside the single binary, PeerClaw'd runs as a set of concurrent subsystems communicating via lock-free channels:

```
┌──────────────────────────────────────────────────────────┐
│                    peerclawd binary                       │
│                                                          │
│  ┌─────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐ │
│  │ P2P     │  │ Agent    │  │ Resource │  │ Token    │ │
│  │ Network │◄►│ Runtime  │◄►│ Manager  │◄►│ Ledger   │ │
│  │ Layer   │  │          │  │          │  │          │ │
│  └────┬────┘  └────┬─────┘  └────┬─────┘  └────┬─────┘ │
│       │            │             │              │        │
│  ┌────┴────────────┴─────────────┴──────────────┴─────┐  │
│  │              Async Runtime (Tokio)                  │  │
│  └────────────────────────┬───────────────────────────┘  │
│                           │                              │
│  ┌────────────────────────┴───────────────────────────┐  │
│  │         Embedded Web UI (Axum + htmx/Leptos)       │  │
│  └────────────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────────────┘
```

---

## Technology Stack

### Language: Rust

PeerClaw'd is written entirely in **Rust**. The choice is non-negotiable for this class of system:

- **Single static binary** — `musl` target produces a fully static, zero-dependency binary. No libc, no runtime, nothing to install.
- **Memory safety without GC** — Critical for a long-running daemon handling untrusted workloads. No use-after-free, no data races, no GC pauses.
- **Async at scale** — Tokio provides the async runtime. Thousands of concurrent peer connections, agent tasks, and HTTP requests on a single thread pool.
- **WASM native** — Rust has first-class WASM support both as a compilation target and as a host runtime (Wasmtime is written in Rust).
- **Performance** — Near-C throughput for crypto operations, network I/O, and inference dispatching. Zero-cost abstractions for protocol encoding/decoding.
- **Cross-compilation** — Single codebase produces binaries for `x86_64-linux-musl`, `aarch64-linux-musl`, `x86_64-apple-darwin`, `aarch64-apple-darwin`, `x86_64-pc-windows-msvc`.

### Core Dependencies

| Subsystem | Crate / Library | Role |
|---|---|---|
| **Async Runtime** | `tokio` | Async I/O, task spawning, timers, signal handling |
| **P2P Networking** | `libp2p` | Peer discovery (mDNS + Kademlia DHT), NAT traversal, encrypted transport (Noise), pubsub (GossipSub), relay circuits |
| **Alternative P2P** | `iroh` (n0 stack) | QUIC-based P2P with content-addressed data transfer. Lighter alternative to libp2p for data replication layer |
| **WASM Sandbox** | `wasmtime` | Execute untrusted agent tools in capability-based WASM sandbox. Component Model for typed interfaces |
| **MicroVM Isolation** | `rust-vmm` / Firecracker SDK | Heavy workload isolation (full model inference) in Firecracker microVMs with <125ms boot |
| **HTTP/Web** | `axum` | Embedded web server for dashboard UI, REST API, WebSocket streaming, webhook endpoints |
| **Web UI** | `leptos` or `htmx` + server-side templates | Reactive embedded dashboard. Leptos for full Rust WASM SPA, or htmx for lightweight progressive enhancement |
| **Serialization** | `serde` + `rmp-serde` (MessagePack) | Wire protocol encoding. MessagePack for compact binary messages between peers |
| **Crypto** | `ed25519-dalek` + `x25519-dalek` | Ed25519 signatures for identity/transactions, X25519 for key exchange |
| **Hashing** | `blake3` | Content-addressed storage, Merkle tree construction, integrity verification |
| **Database / State** | `redb` | Embedded key-value store (pure Rust, ACID, zero-config). Local state, wallet, peer cache, agent metadata |
| **Alternative DB** | `sled` or `rocksdb` | If higher write throughput needed for transaction logs |
| **AI Inference** | `llama-cpp-2` / `candle` | Local inference engine. `llama-cpp-2` for GGUF model support with Metal/CUDA GPU acceleration, `candle` for pure-Rust GPU inference |
| **GPU Compute** | `wgpu` | Cross-platform GPU abstraction (Vulkan/Metal/DX12) for inference acceleration |
| **CLI** | `clap` | Command-line argument parsing with subcommands, shell completions, man page generation |
| **Logging** | `tracing` + `tracing-subscriber` | Structured logging with span-based context, async-aware. JSON output for machine consumption |
| **Config** | `figment` | Layered configuration: defaults → config file (TOML) → env vars → CLI flags |
| **Metrics** | `metrics` + `metrics-exporter-prometheus` | Prometheus-compatible metrics endpoint for monitoring resource usage, peer count, token flow |
| **TLS** | `rustls` | TLS for HTTPS dashboard and external API connections. No OpenSSL dependency |
| **Content Addressing** | `cid` + `multihash` | IPFS-compatible content identifiers for distributed storage chunks |
| **CRDT** | `automerge` or custom | Conflict-free replicated data types for offline-first state synchronization |
| **MCP Protocol** | Custom implementation | Model Context Protocol client/server for external tool connectivity |

### Build & Distribution

| Concern | Approach |
|---|---|
| **Build system** | `cargo` with `cross` for cross-compilation |
| **Static linking** | `x86_64-unknown-linux-musl` target. Single binary, no shared libraries |
| **Binary size** | Target <50MB with `strip`, `lto = true`, `opt-level = "z"`, `codegen-units = 1` |
| **Release** | GitHub Releases + `cargo-binstall` support. One-liner install: `curl -sSL peerclawd.dev/install.sh \| sh` |
| **Reproducible builds** | Nix flake or Docker-based build environment for deterministic outputs |
| **Auto-update** | Built-in `peerclawd update` with signature verification (ed25519-signed releases) |

---

## P2P Network Layer

### Transport & Discovery

PeerClaw'd uses **libp2p** as the primary networking stack, with **iroh** as an optional data-transfer acceleration layer:

```
┌─────────────────────────────────────────────────┐
│                   Application                    │
├─────────────────────────────────────────────────┤
│  GossipSub (pub/sub)  │  Kademlia DHT (routing) │
├─────────────────────────────────────────────────┤
│  Request/Response     │  Relay Circuit v2        │
├─────────────────────────────────────────────────┤
│           Noise Protocol (encryption)            │
├─────────────────────────────────────────────────┤
│   QUIC Transport   │   TCP Transport (fallback)  │
├─────────────────────────────────────────────────┤
│        mDNS (local)  │  Bootstrap peers (WAN)    │
└─────────────────────────────────────────────────┘
```

**Peer Discovery:**
- **Local network** — mDNS for zero-config LAN discovery
- **Wide area** — Kademlia DHT with hardcoded bootstrap peers. Peers announce their PeerId, available resources, and listening addresses
- **NAT traversal** — QUIC with hole-punching, libp2p relay circuit v2 as fallback
- **Peer exchange** — Connected peers gossip known peer addresses (PEX) to accelerate mesh formation

**Wire Protocol:**
- All inter-peer messages are MessagePack-encoded, signed with the sender's Ed25519 key, and transported over Noise-encrypted channels
- Protocol versioning via semver negotiation during libp2p multistream-select

### Resource Advertisement

Every peer periodically publishes a **ResourceManifest** to the DHT:

```rust
struct ResourceManifest {
    peer_id: PeerId,
    timestamp: u64,
    signature: Ed25519Signature,
    resources: Resources {
        cpu_cores: u16,
        cpu_available_mhz: u32,
        gpu: Option<GpuInfo {
            vendor: GpuVendor,          // Nvidia, AMD, Apple, Intel
            vram_mb: u32,
            compute_capability: String,  // e.g. "8.9" for RTX 4090
            model_name: String,
        }>,
        storage_available_bytes: u64,
        bandwidth_mbps: u32,
        ram_available_mb: u32,
    },
    capabilities: Vec<Capability>,       // WASM, MicroVM, Inference, Storage, Relay
    supported_models: Vec<ModelId>,      // Pre-loaded model identifiers
    pricing: PricingTable {
        cpu_per_hour: TokenAmount,
        gpu_per_hour: TokenAmount,
        storage_per_gb_month: TokenAmount,
        inference_per_token: TokenAmount,
        web_fetch_per_request: TokenAmount,
    },
    uptime_hours: u64,
    reputation_score: f64,
}
```

---

## Security Model

Inspired by IronClaw's security-first approach ([nearai/ironclaw](https://github.com/nearai/ironclaw)):

### Sandboxed Execution

- **WASM Sandbox (Wasmtime)** — Untrusted tools and agent code run in isolated WebAssembly containers using the WASI Component Model. Capabilities are explicitly granted: no filesystem, no network, no clock unless the host policy allows it. Resource limits (fuel metering) prevent infinite loops and CPU abuse.
- **MicroVM Isolation (Firecracker)** — Heavier workloads (full model inference, container-based tools) execute in Firecracker microVMs via `rust-vmm` crates. Boot in <125ms, strict memory limits, virtio-net for network isolation, read-only rootfs.
- **Capability-based permissions** — Every tool invocation requires an explicit capability grant. Tools request capabilities in their WASM manifest; the host policy decides which to allow.

### Credential & Secret Protection

- Secrets are **never exposed to agent code**. They are injected at the host boundary via Wasmtime host functions, with active leak detection scanning all outbound data streams using pattern matching (regex + entropy analysis).
- Peer-to-peer communication is encrypted end-to-end via Noise protocol. Agent identities are verified via Ed25519 signatures on every message.

### Prompt Injection Defense

- Multi-layer defense: input pattern detection, content sanitization (strip known injection patterns), and system-level policy enforcement that cannot be overridden by user or inter-agent prompts.
- Agent system prompts are signed and immutable. Any attempt to modify system behavior via injected content is detected and logged.

### Endpoint Allowlisting

- HTTP requests from agents are restricted to explicitly approved hosts and paths via a declarative allowlist in the agent spec.
- Web scraping agents must negotiate access through the token-gated proxy layer, preventing abuse and ensuring cost accountability.

---

## Agent Runtime

### Agent Specification

Agents are defined declaratively in TOML and deployed via the CLI:

```toml
# agent.toml
[agent]
name = "research-bot"
version = "0.1.0"
description = "Autonomous research agent with web access"

[identity]
keypair = "~/.peerclawd/agents/research-bot.key"

[model]
provider = "network"          # Use network inference (distributed)
model = "llama-3.3-70b"
fallback = "llama-3.2-8b"    # Fallback to smaller model if budget low
max_tokens_per_request = 4096
temperature = 0.7

[budget]
max_spend_per_hour = 100      # Max tokens to spend per hour
max_spend_total = 10000       # Lifetime budget cap
auto_refill = false

[capabilities]
web_access = true
storage = true
tool_building = true
agent_communication = true

[web_access]
allowed_hosts = ["*.wikipedia.org", "arxiv.org", "api.semanticscholar.org"]
max_requests_per_minute = 30

[tools]
builtin = ["web_fetch", "web_search", "file_store", "vector_search"]
wasm = ["./tools/custom_parser.wasm"]

[channels]
repl = true
webhook = { port = 9090, path = "/hook" }
websocket = true

[routines]
daily_research = { cron = "0 8 * * *", task = "scan_new_papers" }
heartbeat = { interval = "5m", task = "check_sources" }
```

```
$ peerclawd agent run agent.toml
[2026-03-14T10:00:00Z] INFO  agent=research-bot status=started peer_id=12D3KooW...
[2026-03-14T10:00:01Z] INFO  agent=research-bot status=connected peers=47 budget=10000
```

### Agent Capabilities

**Multi-Channel Presence:**
- **REPL** — Direct command-line interaction via `peerclawd agent attach <id>`
- **HTTP Webhooks** — Event-driven triggers from external systems
- **WASM Channels** — Lightweight integrations (Telegram, Slack, Discord) compiled as WASM plugins
- **Web Gateway** — Browser UI with real-time SSE/WebSocket streaming via the embedded Axum server
- **P2P Direct** — Agent-to-agent communication over GossipSub topics

**Autonomous Operations:**
- **Routines** — Cron schedules, event triggers, and webhook handlers for background automation
- **Heartbeat System** — Proactive background execution for monitoring, maintenance, and network participation
- **Parallel Jobs** — Handle multiple requests concurrently with isolated async task contexts
- **Self-Repair** — Automatic detection and recovery of stuck operations, stale peers, and failed transactions

**Dynamic Tool Building:**
- Agents can **describe a capability they need** and request tool compilation from network peers. The tool is compiled to WASM, verified (content hash + signature), cached, and made available — all without human intervention.
- **MCP Protocol** support for connecting to external Model Context Protocol servers.
- **Plugin Architecture** — Drop in new WASM tools and channels without restarting the agent or the peer.

---

## Distributed Memory & State

Memory is not centralized — it is distributed across the network using content-addressed chunking with **BLAKE3** hashing and **CID** identifiers:

### Storage Architecture

```
┌────────────────────────────────────┐
│          Agent Memory API          │
├──────────┬──────────┬──────────────┤
│  Vector  │  K/V     │  Append-Only │
│  Index   │  Store   │  Log         │
├──────────┴──────────┴──────────────┤
│      Content-Addressed Chunks      │
│      (BLAKE3 hash, CID, 256KB)     │
├────────────────────────────────────┤
│       Replication Manager          │
│  (erasure coding, r=3, locality)   │
├────────────────────────────────────┤
│  Local: redb    │  Remote: DHT     │
└─────────────────┴──────────────────┘
```

- **Embedding Stores** — Vector indexes (HNSW via a lightweight Rust implementation or VittoriaDB integration) are sharded across peers, with redundancy. Agents query the DHT to locate relevant shards and run approximate nearest-neighbor searches in parallel.
- **Context Caches** — Frequently accessed conversation contexts and knowledge bases are replicated to nearby peers (locality-aware caching based on network latency measurements).
- **Append-Only Logs** — Agent history and transaction records are stored as Merkle-linked append-only logs (using BLAKE3 for tree construction), providing tamper-evident auditability.
- **Offline Sync** — Peers that go offline retain their local state in `redb` and reconcile with the network upon reconnection using CRDTs (via `automerge` or a custom CRDT implementation over the append-only log).

---

## The P2P Compute Model

Traditional blockchain networks waste computation on proof-of-work hashing. PeerClaw'd repurposes that model: **instead of solving arbitrary hashes, peers solve AI inference tasks.**

### Job Lifecycle

```
Agent                    Network                   Peer(s)
  │                         │                         │
  ├─── JobRequest ─────────►│                         │
  │    (model, prompt,      │                         │
  │     budget, SLA)        │                         │
  │                         ├─── Broadcast ──────────►│
  │                         │    (GossipSub topic)    │
  │                         │                         │
  │                         │◄─── Bid ────────────────┤
  │                         │    (price, latency,     │
  │                         │     reputation)         │
  │◄── BidSet ─────────────┤                         │
  │                         │                         │
  ├─── Accept(peer_id) ────►│                         │
  │                         ├─── Escrow Lock ────────►│
  │                         │                         │
  │                         │◄─── Result ─────────────┤
  │                         │    (output, proof)      │
  │◄── Result ─────────────┤                         │
  │                         │                         │
  ├─── Verify + Release ───►│                         │
  │                         ├─── Token Transfer ─────►│
  │                         │                         │
```

### Verification Strategies

| Strategy | Use Case | Overhead |
|---|---|---|
| **Redundant Execution** | Critical tasks: same job sent to N peers, majority result wins | N× compute cost |
| **Optimistic Execution** | Default: trust peer, verify post-hoc via sampling | Low, ~5% verification tax |
| **Reputation-Weighted Trust** | High-reputation peers skip verification for routine tasks | Minimal |
| **Cryptographic Attestation** | Peer signs result + includes execution trace hash | Signature verification only |

### Distributed Inference

For large models that exceed a single peer's capacity:

- **Pipeline Parallelism** — Model layers are split across multiple peers, with activations streamed peer-to-peer over QUIC.
- **Tensor Parallelism** — Weight matrices are sharded across peers with synchronized forward passes (requires low-latency interconnect, best for LAN clusters).
- **Ensemble Routing** — Multiple smaller models on different peers contribute partial answers, aggregated by the requesting agent.
- **Speculative Decoding** — A fast small model on a nearby peer generates draft tokens; a larger model on a GPU peer verifies and corrects.

---

## Token Economy

### Web Economy: The HTTP 402 Model

AI agents interact with the broader web through a **token-gated access layer**, inspired by the HTTP 402 ("Payment Required") standard and emerging implementations by Cloudflare and Stripe:

```
Agent ──► Proxy Peer ──► Target Website
  │           │               │
  │  request  │    request    │
  │──────────►│──────────────►│
  │           │    402 or     │
  │  402 +    │◄──────────────│
  │◄──price───│    content    │
  │           │               │
  │  payment  │               │
  │──────────►│               │
  │           │    request    │
  │  content  │──────────────►│
  │◄──────────│◄──────────────│
  │           │    content    │
```

### Token Utility

| Use Case | Description |
|---|---|
| **Inference** | Pay peers for LLM, vision, embedding, and other AI model inference |
| **Storage** | Rent distributed storage for embeddings, context, datasets |
| **Web Access** | Pay for token-gated web scraping and API calls (HTTP 402) |
| **Tool Execution** | Pay for sandboxed WASM/microVM tool runs |
| **Bandwidth** | Pay for relay, proxy, and streaming capacity |
| **Staking** | Stake tokens to become a verified resource provider with higher trust score |
| **Governance** | Vote on protocol upgrades, fee structures, and network policies |

### Token Implementation

| Concern | Decision |
|---|---|
| **Settlement Layer** | Lightweight L2 rollup or appchain (Substrate / Cosmos SDK compatible). Not a mainnet L1 — settlement is a utility, not the product |
| **Local Accounting** | Off-chain payment channels between frequent peers (Lightning-style). On-chain settlement only for channel open/close |
| **Pricing** | Dynamic, supply/demand driven via a local order book per resource type. No global price — each peer sets its own rates |
| **Escrow** | Hash Time-Locked Contracts (HTLCs) for atomic job payment: tokens locked until result delivered or timeout |

---

## Embedded Web Dashboard

The embedded web UI is served by Axum on a configurable port and provides a real-time operational view:

```
$ peerclawd serve --web :8080
[INFO] Web dashboard available at http://localhost:8080
```

**Dashboard pages:**
- **Overview** — Peer status, network topology, resource utilization gauges, token balance
- **Agents** — Running agents, their status, logs, budget consumption, active jobs
- **Network** — Connected peers map, resource availability heatmap, latency matrix
- **Marketplace** — Active job offers, resource bids, pricing trends
- **Wallet** — Balance, transaction history, payment channel status, staking info
- **Tools** — Installed WASM tools, build queue, tool registry browser
- **Settings** — Resource limits, pricing configuration, allowlists, agent policies

**Tech choice for the UI:**
- **Option A: Leptos** — Full Rust stack (server-side rendering + WASM client hydration). Type-safe, no JavaScript toolchain, produces compact WASM bundles. Ideal for PeerClaw'd's zero-dependency ethos.
- **Option B: htmx + Askama** — Server-rendered HTML with htmx for interactivity. Simpler, lighter, no client-side WASM. Better for constrained peers.
- **Recommendation:** Start with **htmx + Askama** for v0.x (faster iteration, smaller binary impact), migrate to **Leptos** for v1.0 when the UI needs richer interactivity.

---

## Comparison with Existing Approaches

| Feature | Centralized AI (OpenAI, etc.) | Blockchain AI (e.g., Bittensor) | IronClaw | **PeerClaw'd** |
|---|---|---|---|---|
| Deployment | Cloud SaaS | Validator nodes | Docker + REPL | **Single static binary** |
| Infrastructure | Cloud-owned | Mining-centric | Self-hosted | **P2P resource sharing** |
| Cost Model | Per-token API pricing | Staking + mining | Self-funded | **Dynamic token marketplace** |
| Privacy | Data sent to provider | On-chain transparency | Local-first | **E2E encrypted P2P** |
| Offline Support | None | Limited | Partial (local) | **Full offline-first + CRDT sync** |
| Agent Autonomy | API consumer only | Smart contracts | Autonomous agent | **Autonomous + P2P economy** |
| Security | Provider-managed | Consensus-based | WASM + Docker | **WASM + Firecracker microVM** |
| Web Integration | Separate concern | Not addressed | Endpoint allowlist | **Native HTTP 402 token economy** |
| Self-Expansion | Not possible | Not possible | Dynamic WASM tools | **Dynamic WASM + network compilation** |
| Distribution | N/A | N/A | Docker image | **`curl \| sh` one-liner** |

---

## Roadmap

### Phase 1 — Foundation (`v0.1`) ✅ IMPLEMENTED
- [x] Single binary scaffold: CLI (clap), config (figment), logging (tracing), embedded web (axum)
- [x] P2P peer discovery and resource advertisement (libp2p Kademlia + mDNS + GossipSub)
- [x] WASM sandbox runtime for tool execution (wasmtime)
- [x] Local `redb` state store for wallet, peer cache, agent metadata
- [x] Ed25519 identity generation and message signing
- [x] Inference module with GGUF model support (llama-cpp-2 with Metal/CUDA)
- [x] Smart task routing (local vs network execution)
- [x] Job marketplace protocol (request → bid → accept → execute → settle)
- [x] Interactive AI chat CLI (`peerclawd chat`)
- [x] Web dashboard with network topology visualization
- [x] Distributed job execution with auto-accept and escrow
- [x] Real-time resource monitoring and GPU offloading

### Phase 2 — Economy (`v0.2`) ✅ IMPLEMENTED
- [x] Token wallet with local accounting and peer-to-peer payment channels
- [x] Job broadcast, bidding, and escrow (HTLC) protocol
- [x] Claude-Code-style CLI with slash commands and persistent settings
- [x] Multi-peer cluster testing (`peerclawd test cluster`)
- [x] OpenAI-compatible API (`/v1/chat/completions`, `/v1/models`)
- [x] Peer ID tracking across jobs and chat responses
- [x] Real-time token streaming in CLI chat
- [ ] HTTP 402 web access proxy layer
- [ ] Distributed storage with BLAKE3 content-addressed chunking
- [ ] Reputation system (uptime, delivery rate, verification pass rate)

### Phase 3 — Scale (`v0.3`)
- [ ] Distributed inference: pipeline parallelism over QUIC
- [ ] Distributed vector memory (sharded HNSW indexes)
- [ ] Dynamic WASM tool building and peer-assisted compilation
- [ ] Multi-agent collaboration protocols (GossipSub topics per agent swarm)
- [ ] Offline-first CRDT state sync

### Phase 4 — Ecosystem (`v1.0`)
- [ ] On-chain settlement layer (Substrate appchain or L2 rollup)
- [ ] Governance token and DAO structure
- [ ] Public tool/skill registry
- [ ] Leptos web dashboard migration
- [ ] SDK, API docs, and developer documentation
- [ ] Mainnet launch

---

## Inspirations & Prior Art

- **BitTorrent** — Distributed content delivery with tit-for-tat incentives
- **IronClaw** ([nearai/ironclaw](https://github.com/nearai/ironclaw)) — Security-first AI agent framework with WASM sandboxing, credential protection, prompt injection defense, MCP integration, and dynamic tool building
- **Holepunch / Pear** — P2P primitives (Hypercore, Hyperbee, HyperDHT, Hyperswarm)
- **HTTP 402 / Cloudflare AI Gateway / Stripe** — Token-gated web access and micropayment models
- **Bittensor** — Decentralized AI inference network (different consensus model)
- **IPFS / Filecoin** — Content-addressed distributed storage with economic incentives
- **HashiCorp Consul / Nomad** — Single-binary, multi-mode daemon architecture pattern
- **iroh (n0)** — Rust-native QUIC-based P2P content transfer

---

## Summary

PeerClaw'd reimagines AI infrastructure as a **commons** — a decentralized network where compute, storage, and intelligence are shared resources, traded through a transparent token economy, and secured by WASM and microVM isolation. Ship one binary, join the network, contribute resources, run agents. No cloud accounts, no Docker, no Kubernetes.

**One binary. One network. Distributed intelligence.**

---

*Version 0.2 — March 2026*
