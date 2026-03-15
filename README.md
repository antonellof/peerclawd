# PeerClaw'd

**Decentralized P2P AI Agent Network**

> One binary. Distributed intelligence. Token-powered autonomy.

PeerClaw'd is a peer-to-peer network where AI agents collaborate, share compute resources, and transact using a native token economy. Think **BitTorrent meets AI inference** — every peer contributes compute and earns tokens, while agents spend tokens to execute tasks across the network.

**Ships as a single static binary.** No containers, no orchestrators, no cloud dependencies.

---

## Features

### AI Inference
- **Local GGUF models** - Run Llama, Phi, Qwen, Gemma locally
- **GPU acceleration** - Metal (macOS) and CUDA support via llama-cpp-2
- **Streaming output** - Real-time token generation in CLI and API
- **Batch aggregation** - Efficient multi-agent request handling
- **Model caching** - LRU eviction, automatic memory management

### P2P Network
- **Decentralized** - No central server, peers discover each other
- **libp2p stack** - Kademlia DHT, GossipSub, mDNS, Noise encryption
- **Job marketplace** - Request → Bid → Execute → Settle workflow
- **Multi-peer clusters** - Test distributed execution locally

### Token Economy
- **Native wallet** - Ed25519-based identity and transactions
- **Escrow system** - Funds locked until job completion
- **Dynamic pricing** - Each peer sets their own rates
- **Payment channels** - Efficient micro-payments between peers

### CLI Experience
- **Ollama-style commands** - `peerclawd run llama-3.2-3b`
- **Claude-Code slash commands** - `/help`, `/model`, `/settings`, `/status`
- **Interactive chat** - Conversation history, settings persistence
- **Model management** - Download, list, remove models

### OpenAI-Compatible API
- **Drop-in replacement** - Use any OpenAI SDK
- **SSE streaming** - Real-time token output via Server-Sent Events
- **`/v1/chat/completions`** - Full chat completions endpoint
- **`/v1/models`** - List available models

### Web Dashboard
- **Network topology** - Visual graph of connected peers
- **Resource monitoring** - Real-time CPU, RAM, GPU stats
- **Job tracking** - Active and completed jobs with peer IDs
- **AI Chat interface** - Send prompts via browser

### Security
- **WASM sandbox** - Wasmtime for isolated tool execution
- **End-to-end encryption** - Noise protocol for all P2P traffic
- **Ed25519 signatures** - Cryptographic identity verification
- **Capability-based access** - Explicit permission grants

---

## Quick Start

### Install

```bash
git clone https://github.com/yourusername/peerclawd.git
cd peerclawd
cargo build --release
```

### Download a Model

```bash
mkdir -p ~/.peerclawd/models

# Llama 3.2 1B (~770MB) - fast, good for testing
curl -L -o ~/.peerclawd/models/llama-3.2-1b-instruct-q4_k_m.gguf \
  "https://huggingface.co/bartowski/Llama-3.2-1B-Instruct-GGUF/resolve/main/Llama-3.2-1B-Instruct-Q4_K_M.gguf"
```

### Run

```bash
# Interactive chat (Ollama-style)
./target/release/peerclawd run llama-3.2-1b

# Full-featured chat with slash commands
./target/release/peerclawd chat

# Start peer node with web dashboard
./target/release/peerclawd serve --web 127.0.0.1:8080
```

---

## Commands

### Chat & Inference

```bash
peerclawd run <model>              # Interactive chat
peerclawd run <model> "prompt"     # Single query
peerclawd chat                     # Chat with slash commands

# Slash commands
/help                              # Show all commands
/model <name>                      # Switch model
/temperature <n>                   # Set temperature
/settings                          # Settings menu
/status                            # Show runtime status
/peers                             # List connected peers
/balance                           # Show token balance
```

### Models

```bash
peerclawd models list              # List downloaded models
peerclawd models download <model>  # Download from HuggingFace
peerclawd pull <model>             # Alias for download
```

### Network

```bash
peerclawd serve                    # Start peer node
peerclawd serve --web 0.0.0.0:8080 # With web dashboard
peerclawd serve --provider         # Accept jobs from network
peerclawd peers list               # Show connected peers
```

### Testing

```bash
peerclawd test inference           # Test local inference
peerclawd test cluster --nodes 3   # Spawn test cluster
```

---

## OpenAI API

```bash
peerclawd serve --web 127.0.0.1:8080
```

```python
from openai import OpenAI

client = OpenAI(base_url="http://localhost:8080/v1", api_key="unused")
response = client.chat.completions.create(
    model="llama-3.2-3b",
    messages=[{"role": "user", "content": "Hello!"}],
    stream=True
)
for chunk in response:
    print(chunk.choices[0].delta.content, end="")
```

---

## Roadmap

### Current (v0.2)
- [x] P2P networking with libp2p
- [x] GGUF inference with GPU acceleration
- [x] Job marketplace protocol
- [x] Token wallet with escrow
- [x] OpenAI-compatible API
- [x] Claude-Code-style CLI
- [x] Web dashboard
- [x] Batch aggregation

### Next (v0.3)
- [ ] Distributed inference (pipeline parallelism)
- [ ] Vector memory (HNSW)
- [ ] Dynamic WASM tool building
- [ ] Multi-agent collaboration

### Future (v1.0)
- [ ] On-chain settlement
- [ ] Public tool registry
- [ ] Governance

---

## Documentation

- [Architecture](docs/ARCHITECTURE.md)
- [P2P Protocol](docs/P2P_PROTOCOL.md)
- [Token Economy](docs/TOKENS.md)
- [Security](docs/SECURITY.md)
- [Agent Spec](docs/AGENTS.md)

---

*v0.2 — March 2026*
