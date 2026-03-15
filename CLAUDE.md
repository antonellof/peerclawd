# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

PeerClaw is a fully decentralized peer-to-peer AI agent network written in Rust. It ships as a single static binary where autonomous AI agents collaborate, share resources, and transact using a native token economy.

**Current Status:** Pre-implementation (design specification only). The repository contains a comprehensive README.md with architectural specifications but no source code yet.

## Build Commands (When Development Begins)

```bash
# Build for current platform
cargo build --release

# Build static Linux binary
cargo build --release --target x86_64-unknown-linux-musl

# Run tests
cargo test

# Lint
cargo clippy

# Format
cargo fmt

# Run single test
cargo test test_name

# Run tests in specific module
cargo test module_name::
```

## Architecture

### Single Binary Design

One statically-linked binary operates in multiple modes based on flags/subcommands. Every peer runs the same binary - roles (resource provider, agent host, gateway) are determined at runtime.

**CLI Structure:**
- `peerclaw serve` - Start peer node (with `--gpu`, `--storage`, `--web` flags)
- `peerclaw agent run|list|logs|stop` - Agent management
- `peerclaw network status|peers|discover` - Network operations
- `peerclaw wallet create|balance|send|history` - Token wallet
- `peerclaw tool build|install|list` - WASM tool management

### Core Subsystems

Internal architecture uses concurrent subsystems communicating via lock-free channels:

1. **P2P Network Layer** (libp2p) - Kademlia DHT, mDNS, GossipSub, Noise encryption, QUIC/TCP transports
2. **Agent Runtime** - TOML-based agent specs with multi-channel presence (REPL, webhooks, WebSocket, P2P)
3. **Resource Manager** - CPU, GPU, storage tracking and pricing
4. **Token Ledger** - Local accounting + payment channels (Lightning-style)
5. **WASM Sandbox** (Wasmtime) - Capability-based isolation for untrusted tools
6. **MicroVM Isolation** (Firecracker) - Heavy workload isolation with <125ms boot
7. **Embedded Web UI** (Axum + htmx) - Dashboard on configurable port

### Key Dependencies

| Subsystem | Crate |
|-----------|-------|
| Async Runtime | `tokio` |
| P2P Networking | `libp2p` |
| WASM Sandbox | `wasmtime` |
| HTTP/Web | `axum` |
| Database | `redb` |
| Serialization | `serde` + `rmp-serde` (MessagePack) |
| Crypto | `ed25519-dalek`, `x25519-dalek` |
| Hashing | `blake3` |
| AI Inference | `candle`, `llama-cpp-rs` |
| CLI | `clap` |
| Logging | `tracing` |
| Config | `figment` |

### Security Model

- WASM sandbox for untrusted tools with explicit capability grants
- Firecracker microVMs for heavy workloads
- Secrets injected at host boundary, never exposed to agent code
- All P2P communication encrypted via Noise protocol
- Ed25519 signatures on all messages

### Agent Specification

Agents are defined in TOML files (`agent.toml`) specifying:
- Identity and model configuration
- Budget limits (per-hour and total)
- Capabilities (web_access, storage, tool_building)
- Allowed hosts for web access
- Tools (builtin + custom WASM)
- Channels (REPL, webhook, websocket)
- Routines (cron schedules, heartbeats)

### Distributed Storage

- Content-addressed chunks using BLAKE3 hashing
- 256KB chunks with erasure coding (r=3)
- Vector indexes (HNSW) sharded across peers
- CRDT-based offline-first sync

## Development Phases

- **v0.1 (Foundation):** Binary scaffold, P2P discovery, WASM sandbox, redb state store, Ed25519 identity
- **v0.2 (Economy):** Token wallet, payment channels, job bidding/escrow, HTTP 402 proxy
- **v0.3 (Scale):** Distributed inference, vector memory, dynamic tool building, multi-agent collaboration
- **v1.0 (Ecosystem):** On-chain settlement, governance, public tool registry, Leptos dashboard
