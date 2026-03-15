# P2P Network Protocol

## Transport Stack

```
┌─────────────────────────────────────────────────┐
│                   Application                    │
├─────────────────────────────────────────────────┤
│  GossipSub (pub/sub)  │  Kademlia DHT (routing) │
├─────────────────────────────────────────────────┤
│  Request/Response     │  Identify                │
├─────────────────────────────────────────────────┤
│           Noise Protocol (encryption)            │
├─────────────────────────────────────────────────┤
│   QUIC Transport   │   TCP Transport (fallback)  │
├─────────────────────────────────────────────────┤
│        mDNS (local)  │  Bootstrap peers (WAN)    │
└─────────────────────────────────────────────────┘
```

## Peer Discovery

- **Local network**: mDNS for zero-config LAN discovery
- **Wide area**: Kademlia DHT with bootstrap peers
- **NAT traversal**: QUIC with hole-punching

## GossipSub Topics

| Topic | Purpose |
|-------|---------|
| `peerclawd/jobs/requests` | Job request broadcasts |
| `peerclawd/jobs/bids` | Bid announcements |
| `peerclawd/jobs/status` | Job status updates, results |
| `peerclawd/models/announce` | Model availability |

## Job Protocol

```
Requester                Network                  Provider
    │                       │                         │
    ├─── JobRequest ───────►│                         │
    │    (model, prompt,    │                         │
    │     budget)           ├─── Broadcast ──────────►│
    │                       │                         │
    │                       │◄─── JobBid ─────────────┤
    │                       │    (price, latency)     │
    │◄── Collect Bids ─────┤                         │
    │                       │                         │
    ├─── BidAccepted ──────►│                         │
    │    (winner_peer_id)   ├─── Notify ─────────────►│
    │                       │                         │
    │                       │◄─── JobResult ──────────┤
    │◄── Result ───────────┤    (output)             │
    │                       │                         │
```

## Message Format

All messages are MessagePack-encoded with this wrapper:

```rust
struct NetworkMessage {
    message_type: String,
    payload: Vec<u8>,
    timestamp: u64,
    signature: Vec<u8>,
}
```
