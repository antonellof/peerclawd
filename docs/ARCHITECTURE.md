# Architecture

## Single Binary Design

PeerClaw'd ships as a single statically-linked binary that operates in multiple modes. Every peer runs the same binary — roles (resource provider, agent host, gateway) are determined at runtime.

```
peerclawd
├── serve          # Start a peer node
│   ├── --gpu              # Advertise GPU resources
│   ├── --web <addr>       # Enable web UI
│   ├── --bootstrap <peer> # Join network via known peer
│   └── --provider         # Accept jobs from network
├── run <model>    # Run model (Ollama-style)
├── chat           # Interactive AI chat
├── models         # Model management
├── peers          # Peer management
├── wallet         # Token wallet
├── job            # Job submission
└── test           # Testing utilities
```

## Internal Architecture

```
┌──────────────────────────────────────────────────────────┐
│                    peerclawd binary                       │
│                                                          │
│  ┌─────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐ │
│  │ P2P     │  │ Task     │  │ Inference│  │ Job      │ │
│  │ Network │◄►│ Executor │◄►│ Engine   │◄►│ Manager  │ │
│  │ Layer   │  │          │  │          │  │          │ │
│  └────┬────┘  └────┬─────┘  └────┬─────┘  └────┬─────┘ │
│       │            │             │              │        │
│  ┌────┴────────────┴─────────────┴──────────────┴─────┐  │
│  │              Async Runtime (Tokio)                  │  │
│  └────────────────────────┬───────────────────────────┘  │
│                           │                              │
│  ┌────────────────────────┴───────────────────────────┐  │
│  │         Embedded Web UI (Axum)                     │  │
│  └────────────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────────────┘
```

## Core Components

### Runtime (`src/runtime.rs`)
Coordinates all subsystems: TaskExecutor, JobManager, InferenceEngine, P2P Network, and BatchAggregator.

### Task Executor (`src/executor/`)
Smart routing of tasks between local and remote execution:
- Local inference with GPU offloading
- Web fetch with rate limiting
- WASM tool execution

### Inference Engine (`src/inference/`)
GGUF model loading and inference:
- `llama-cpp-2` for real inference
- Model caching with LRU eviction
- Batch aggregation for multi-agent scenarios

### Job Manager (`src/job/`)
P2P job marketplace:
- Job request broadcasting
- Bid collection and selection
- Escrow and settlement

### P2P Network (`src/p2p/`)
libp2p-based networking:
- Kademlia DHT for routing
- GossipSub for pub/sub
- mDNS for local discovery
- Noise encryption

## Technology Stack

| Subsystem | Crate |
|-----------|-------|
| Async Runtime | `tokio` |
| P2P Networking | `libp2p` |
| WASM Sandbox | `wasmtime` |
| HTTP/Web | `axum` |
| Database | `redb` |
| Serialization | `serde` + `rmp-serde` |
| Crypto | `ed25519-dalek` |
| AI Inference | `llama-cpp-2` |
| CLI | `clap` |
| Logging | `tracing` |
