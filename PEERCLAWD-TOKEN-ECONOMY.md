# PeerClaw'd — Token Economy Specification

> **Document:** Token Economy & Wallet Architecture  
> **Version:** 0.1-draft  
> **Date:** March 2026

---

## Overview

The PeerClaw'd token (**PCLAW**) is the native utility token that powers every transaction in the network. It is not a speculative asset — it is fuel. Agents spend PCLAW to consume resources, peers earn PCLAW by providing them, and the entire economy is designed to reach a sustainable equilibrium where resource prices reflect real supply and demand.

Every entity in the network — whether a human-operated peer node or an autonomous AI agent — has a wallet. No wallet, no participation.

---

## 1. Wallet Architecture

### Every Agent Has a Wallet

A wallet is created automatically when a peer starts or an agent is deployed. Each wallet is an Ed25519 keypair stored locally, with the public key serving as the wallet address.

```
$ peerclawd wallet create
✓ Wallet created
  Address:  pclaw1q7x9k3m2f8v... (Bech32-encoded Ed25519 pubkey)
  Keyfile:  ~/.peerclawd/wallet/default.key
  Balance:  0.000000 PCLAW

$ peerclawd wallet balance
  Available:   1,250.00 PCLAW
  In escrow:     180.00 PCLAW  (3 active jobs)
  Staked:      5,000.00 PCLAW  (resource provider bond)
  Total:       6,430.00 PCLAW
```

### Wallet Types

| Wallet Type | Owner | Purpose |
|---|---|---|
| **Peer Wallet** | Human operator running a node | Receives resource rewards, pays for services consumed, holds staking bond |
| **Agent Wallet** | Autonomous AI agent | Spends tokens on inference, storage, web access, tools. Funded by its operator or by earning from other agents |
| **Operator Wallet** | Human who deploys agents | Master wallet that funds agent wallets. Can set spending limits and auto-refill policies |
| **Escrow Wallet** | System-managed | Temporary hold during job execution. Released on completion or refunded on timeout |

### Wallet Hierarchy

An operator can manage multiple agents, each with their own wallet and independent budget:

```
Operator Wallet (pclaw1master...)
├── Agent: research-bot     (pclaw1agen01...)  budget: 500/day
├── Agent: trading-monitor  (pclaw1agen02...)  budget: 200/day
├── Agent: content-writer   (pclaw1agen03...)  budget: 100/day
└── Reserve: unallocated funds
```

### Wallet Configuration

Wallets are configured in the peer/agent TOML config:

```toml
# ~/.peerclawd/config.toml — Peer-level wallet config

[wallet]
keyfile = "~/.peerclawd/wallet/default.key"
auto_backup = true
backup_path = "~/.peerclawd/wallet/backups/"

[wallet.spending]
# Maximum tokens this peer can spend per day (across all agents)
max_daily_spend = 2000.0

# Reserve balance — never spend below this threshold
# Ensures the peer always has enough to maintain staking bond
reserve_balance = 1000.0

# Auto-purchase: if balance drops below threshold, buy from exchange
auto_purchase = true
auto_purchase_threshold = 500.0          # Trigger when available < 500
auto_purchase_amount = 1000.0            # Buy 1000 PCLAW
auto_purchase_payment_method = "stripe"  # or "crypto", "wire"
auto_purchase_max_price_usd = 0.12      # Don't buy if price > $0.12/PCLAW

[wallet.staking]
# Bond staked to participate as a resource provider
# Higher stake = higher trust score = more job assignments
staked_amount = 5000.0
auto_restake_rewards = true              # Compound rewards into stake
```

```toml
# agent.toml — Agent-level budget config

[budget]
# Funded by operator wallet
funded_by = "pclaw1master..."

# Spending limits
max_spend_per_request = 10.0     # No single job can cost more than this
max_spend_per_hour = 100.0       # Hourly cap
max_spend_per_day = 500.0        # Daily cap
max_spend_total = 50000.0        # Lifetime cap (agent shuts down at zero)

# Auto-refill from operator wallet
auto_refill = true
refill_trigger = 50.0            # When balance drops below 50
refill_amount = 200.0            # Top up 200 PCLAW from operator wallet

# Spending priorities (agent allocates budget across resource types)
[budget.priorities]
inference = 0.60                 # 60% of budget for LLM inference
web_access = 0.15                # 15% for web fetching
storage = 0.10                   # 10% for distributed storage
tools = 0.10                     # 10% for WASM tool execution
communication = 0.05             # 5% for agent-to-agent messaging
```

### Wallet Security

| Concern | Implementation |
|---|---|
| **Key storage** | Ed25519 private key encrypted at rest with Argon2id-derived key from user passphrase |
| **Backup** | Mnemonic seed phrase (BIP39-compatible) for recovery. Encrypted backup files with timestamp versioning |
| **Transaction signing** | Every spend requires a valid Ed25519 signature from the wallet owner. No unsigned transactions are relayed |
| **Multi-sig (future)** | Threshold signatures for high-value operator wallets (e.g., 2-of-3 keys required) |
| **Rate limiting** | Wallet enforces local rate limits before broadcasting. Prevents accidental drain from misconfigured agents |

---

## 2. Earning Tokens: Resource Contribution

### How Peers Earn

Every peer that contributes resources to the network earns PCLAW. Earnings are proportional to the **quantity, quality, and reliability** of resources provided.

### Resource Types & Reward Rates

Reward rates are not fixed — they float based on network supply and demand. The following are illustrative baseline rates at network launch:

| Resource | Unit | Indicative Rate | Measurement |
|---|---|---|---|
| **CPU** | core-hour | 2.0 PCLAW | Metered by verified CPU-seconds consumed by jobs |
| **GPU (consumer)** | GPU-hour | 15.0 PCLAW | RTX 3060–4070 class. Measured by allocated GPU-minutes |
| **GPU (datacenter)** | GPU-hour | 40.0 PCLAW | A100/H100 class. Premium rate for high-VRAM inference |
| **Storage** | GB-month | 0.5 PCLAW | Content-addressed chunks stored and served. Verified by random challenge-response |
| **Bandwidth / Relay** | GB transferred | 0.3 PCLAW | Metered relay traffic for NAT-challenged peers |
| **Web Proxy** | per request | 0.1 PCLAW | Proxied web fetches on behalf of agents |
| **Uptime Bonus** | per day | 1.0 PCLAW | Flat bonus for maintaining >95% uptime in a 24h window |

### Reward Calculation

Rewards are calculated per-job and settled via the escrow system:

```
Reward = base_rate × resource_units × quality_multiplier × reputation_multiplier

Where:
  base_rate           = Current market rate for the resource type
  resource_units      = Measured units consumed (CPU-seconds, GPU-minutes, bytes, etc.)
  quality_multiplier  = 0.8–1.2 based on latency, throughput, result correctness
  reputation_multiplier = 0.5–1.5 based on peer's historical reputation score
```

### Reputation Score

Reputation directly affects earning potential. It is computed as a weighted rolling average:

| Factor | Weight | Measurement |
|---|---|---|
| **Job Completion Rate** | 30% | % of accepted jobs successfully completed |
| **Result Accuracy** | 25% | % of results that pass verification (redundant execution checks) |
| **Latency Performance** | 15% | Actual latency vs. promised SLA |
| **Uptime** | 15% | % of time online and reachable in the last 30 days |
| **Stake Weight** | 10% | Higher stake = more skin in the game |
| **Age** | 5% | Longer network participation = slight trust bonus |

**Reputation thresholds:**

| Score | Tier | Effect |
|---|---|---|
| 0.0 – 0.3 | Untrusted | Jobs require full redundant verification. Lower job assignment priority |
| 0.3 – 0.6 | Standard | Normal operation. Sampled verification (~10% of jobs) |
| 0.6 – 0.8 | Trusted | Optimistic execution. Higher job assignment priority. 1.2× reward multiplier |
| 0.8 – 1.0 | Elite | Skip verification for routine jobs. 1.5× reward multiplier. Eligible for governance votes |

### Slashing

Peers that misbehave lose staked tokens:

| Offense | Penalty |
|---|---|
| **Failed job delivery** (accepted but no result within timeout) | 1% of stake |
| **Incorrect result** (failed verification) | 2% of stake |
| **Repeated failures** (>5 in 24h) | 10% of stake + temporary suspension (24h) |
| **Malicious behavior** (tampered results, replay attacks) | 100% of stake + permanent ban (governance vote to lift) |

---

## 3. Spending Tokens: What Agents Pay For

### Pricing Model

All resource pricing in PeerClaw'd follows a **local order book** model. There is no global fixed price — each peer sets its own rates, and agents choose based on price, latency, and reputation.

```
Agent Request:
  "I need 4096 tokens of Llama-3.3-70B inference, max latency 2s, budget 8.0 PCLAW"

Peer Bids:
  Peer A: 6.5 PCLAW, est. 1.2s latency, reputation 0.85
  Peer B: 7.0 PCLAW, est. 0.8s latency, reputation 0.92
  Peer C: 5.0 PCLAW, est. 3.1s latency, reputation 0.61  ← exceeds latency SLA, filtered

Agent selects: Peer B (best latency within budget, highest reputation)
```

### Cost Table (Indicative)

These are network-average costs at launch. Actual prices vary by peer:

| Service | Unit | Indicative Cost | Notes |
|---|---|---|---|
| **LLM Inference (small)** | 1K tokens | 0.5 PCLAW | 7B–13B parameter models |
| **LLM Inference (medium)** | 1K tokens | 2.0 PCLAW | 30B–70B parameter models |
| **LLM Inference (large)** | 1K tokens | 5.0 PCLAW | 70B+ or MoE models |
| **Embedding Generation** | 1K tokens | 0.2 PCLAW | Text embedding models |
| **Image Generation** | per image | 3.0 PCLAW | Stable Diffusion class |
| **Web Fetch** | per request | 0.1 PCLAW | HTML content retrieval |
| **Web Search** | per query | 0.5 PCLAW | Search + top-N result fetch |
| **Vector Search** | per query | 0.05 PCLAW | Distributed HNSW lookup |
| **Storage Write** | per MB | 0.01 PCLAW | Content-addressed chunk storage |
| **Storage Read** | per MB | 0.005 PCLAW | Chunk retrieval |
| **WASM Tool Execution** | per invocation | 0.02 PCLAW | Sandboxed tool run |
| **Agent-to-Agent Message** | per message | 0.001 PCLAW | GossipSub relay |

### Payment Flow

```
1. Agent signs a JobRequest with budget and SLA requirements
2. Matching peer accepts → tokens moved to Escrow Wallet (HTLC)
3. Peer executes the job
4. Result delivered → Agent verifies (or verification is sampled)
5a. Success → Escrow releases tokens to peer
5b. Failure → Escrow refunds tokens to agent
5c. Timeout → Escrow refunds tokens to agent, peer reputation decremented
```

---

## 4. Token Supply & Emission Plan

### Token Overview

| Property | Value |
|---|---|
| **Name** | PeerClaw'd Token |
| **Symbol** | PCLAW |
| **Total Maximum Supply** | 1,000,000,000 (1 billion) |
| **Decimals** | 6 (smallest unit: 0.000001 PCLAW = 1 μPCLAW) |
| **Initial Circulating Supply** | 0 (all tokens are minted through defined mechanisms) |
| **Consensus** | No mining. Tokens are minted via resource contribution and scheduled emission |

### Emission Schedule

PCLAW follows a **declining emission curve** over 10 years, inspired by Bitcoin's halving model but smoother (continuous decay rather than step function):

```
Annual Emission = Base_Emission × decay_factor^year

Where:
  Base_Emission = 200,000,000 PCLAW (Year 1)
  decay_factor  = 0.75 (25% reduction per year)
```

| Year | Annual Emission | Cumulative Supply | % of Max Supply |
|---|---|---|---|
| 1 | 200,000,000 | 200,000,000 | 20.0% |
| 2 | 150,000,000 | 350,000,000 | 35.0% |
| 3 | 112,500,000 | 462,500,000 | 46.3% |
| 4 | 84,375,000 | 546,875,000 | 54.7% |
| 5 | 63,281,250 | 610,156,250 | 61.0% |
| 6 | 47,460,938 | 657,617,188 | 65.8% |
| 7 | 35,595,703 | 693,212,891 | 69.3% |
| 8 | 26,696,777 | 719,909,668 | 72.0% |
| 9 | 20,022,583 | 739,932,251 | 74.0% |
| 10 | 15,016,937 | 754,949,188 | 75.5% |
| 11+ | Tail emission | → 1,000,000,000 | 100% (asymptotic) |

After year 10, a **tail emission** of 1% annual inflation continues indefinitely to fund ongoing resource rewards and prevent deflationary stagnation. The tail emission is drawn from the remaining unallocated supply until the 1B cap is reached, then governance decides whether to continue inflation.

### Emission Allocation

Each year's minted tokens are distributed across four pools:

```
┌──────────────────────────────────────────────────────────────┐
│                    Annual Token Emission                      │
├──────────────┬──────────────┬───────────────┬────────────────┤
│  Resource    │  Network     │  Treasury &   │  Founding      │
│  Rewards     │  Growth      │  Development  │  Team &        │
│              │  Fund        │  Fund         │  Investors     │
│  60%         │  15%         │  15%          │  10%           │
└──────────────┴──────────────┴───────────────┴────────────────┘
```

| Pool | Allocation | Purpose | Vesting |
|---|---|---|---|
| **Resource Rewards** | 60% | Distributed to peers for CPU, GPU, storage, bandwidth contribution. This is the primary emission mechanism — tokens are minted and paid when verified work is completed | Immediate (earned on delivery) |
| **Network Growth Fund** | 15% | Grants for ecosystem development: tool builders, integration developers, content creators, educational material. Governed by DAO vote after Phase 4 | 6-month cliff, 24-month linear vest |
| **Treasury & Development** | 15% | Core protocol development, security audits, infrastructure (bootstrap nodes, relay servers), legal, operational costs | 12-month cliff, 36-month linear vest |
| **Founding Team & Early Investors** | 10% | Compensation for founding contributors and seed capital providers | 12-month cliff, 48-month linear vest |

### Token Generation Event (TGE)

At network launch, **no tokens exist**. The first PCLAW tokens are minted when the first peer completes the first verified job. There is no pre-mine, no ICO, and no airdrop of Resource Reward tokens.

However, to bootstrap liquidity and fund development:

| Event | Tokens | Timing |
|---|---|---|
| **Seed Round** | 30,000,000 PCLAW (3% of max) | Pre-launch. Minted at TGE, subject to 12+48mo vest |
| **Strategic Round** | 20,000,000 PCLAW (2% of max) | Pre-launch. Minted at TGE, subject to 12+36mo vest |
| **Public Sale** | 50,000,000 PCLAW (5% of max) | At TGE. Available immediately on exchange |
| **Liquidity Pool Seeding** | 20,000,000 PCLAW (2% of max) | At TGE. Paired with USDC on DEX for initial price discovery |
| **Total Pre-Allocated** | 120,000,000 PCLAW (12% of max) | Remaining 88% minted via emission schedule |

---

## 5. Acquiring Tokens: The Central Exchange

### Exchange Architecture

While PeerClaw'd is decentralized, token acquisition needs an accessible on-ramp. The project operates a **first-party exchange portal** alongside third-party DEX listings:

```
┌───────────────────────────────────────────────────────────────┐
│                    Token Acquisition Channels                  │
├─────────────────┬──────────────────┬──────────────────────────┤
│  PeerClaw'd     │  Decentralized   │  Peer-to-Peer            │
│  Exchange       │  Exchanges       │  (OTC)                   │
│  (First-Party)  │  (Third-Party)   │                          │
│                 │                  │                          │
│  Fiat → PCLAW   │  Crypto → PCLAW  │  Direct peer trades      │
│  Stripe, Wire   │  Uniswap, etc.   │  Escrow-protected        │
└─────────────────┴──────────────────┴──────────────────────────┘
```

### First-Party Exchange Portal

The PeerClaw'd Exchange is a web application (hosted at `exchange.peerclawd.dev`) that enables fiat-to-PCLAW purchases:

**Purchase flow:**

```
User                    Exchange Portal              Settlement Layer
 │                          │                              │
 │  1. "Buy 1000 PCLAW"    │                              │
 │─────────────────────────►│                              │
 │                          │  2. Price quote              │
 │  3. Pay $100 USD         │     (oracle feed)            │
 │  (Stripe / Wire / Crypto)│                              │
 │─────────────────────────►│                              │
 │                          │  4. Payment confirmed        │
 │                          │─────────────────────────────►│
 │                          │  5. Mint PCLAW from          │
 │                          │     Public Sale pool         │
 │                          │     (or buy from LP)         │
 │                          │◄─────────────────────────────│
 │  6. PCLAW in wallet      │                              │
 │◄─────────────────────────│                              │
 │                          │                              │
```

**Payment methods:**

| Method | Provider | Speed | Fees | Min/Max |
|---|---|---|---|---|
| **Credit/Debit Card** | Stripe | Instant | 2.9% + $0.30 | $5 – $10,000 |
| **Bank Transfer (SEPA)** | Stripe / Direct | 1–2 business days | 0.5% | $50 – $100,000 |
| **Wire Transfer** | Direct | 2–5 business days | Flat $25 | $1,000 – $1,000,000 |
| **Crypto (USDC/USDT)** | On-chain | ~2 minutes | Gas fees only | No limit |
| **Crypto (ETH/BTC)** | Swap via DEX | ~5 minutes | Swap fee + gas | No limit |

**Integrated CLI purchase:**

```
$ peerclawd wallet buy 1000
  Current price:   $0.10 / PCLAW
  Total cost:      $100.00 USD + $3.20 fees
  Payment method:  Stripe (card ending 4242)
  
  Confirm purchase? [y/N] y
  
  ✓ Payment processed
  ✓ 1,000.000000 PCLAW credited to pclaw1q7x9k3m2f8v...
  New balance: 2,250.000000 PCLAW
```

### Price Discovery & Stability

| Mechanism | Description |
|---|---|
| **Initial Price** | Set at TGE via public sale: target $0.05–$0.10 per PCLAW |
| **AMM Liquidity Pool** | 20M PCLAW + equivalent USDC seeded on Uniswap v3 (or Substrate DEX). Provides continuous price discovery |
| **Oracle Feed** | Chainlink (or equivalent) price oracle aggregates prices across DEXs. Used by the first-party exchange for fiat quotes |
| **Buyback & Burn** | 5% of Treasury revenue is used to buy PCLAW from the open market and burn it. Creates deflationary pressure as network usage grows |
| **Resource Peg Anchor** | Long-term price is anchored to real resource costs. If 1 GPU-hour costs ~$0.50 on cloud providers, and earns ~15 PCLAW, then 1 PCLAW ≈ $0.033. Market premium/discount applies based on demand |

### Third-Party Exchange Listings

**Phase 1 (Launch):**
- Uniswap v3 (Ethereum L2 — Arbitrum or Base)
- PeerClaw'd Substrate DEX (native chain)

**Phase 2 (6 months post-launch):**
- CEX listings: target Tier-2 exchanges (Gate.io, MEXC, KuCoin)
- Additional DEX pairs: PCLAW/ETH, PCLAW/SOL

**Phase 3 (12+ months):**
- Tier-1 CEX applications (Binance, Coinbase) contingent on volume and regulatory readiness

---

## 6. Wallet Configuration Reference

### Full Configuration Example

```toml
# ═══════════════════════════════════════════════════════════════
# PeerClaw'd Node Configuration — Wallet & Economy Section
# File: ~/.peerclawd/config.toml
# ═══════════════════════════════════════════════════════════════

# ── Wallet Identity ────────────────────────────────────────────
[wallet]
keyfile = "~/.peerclawd/wallet/default.key"
# Display name (optional, shown in peer listings)
alias = "antonello-bari-node"

# Backup settings
auto_backup = true
backup_path = "~/.peerclawd/wallet/backups/"
max_backups = 10

# ── Spending Controls ──────────────────────────────────────────
[wallet.spending]
# Global daily spending limit (across all agents and peer operations)
max_daily_spend = 5000.0

# Never let available balance drop below this
# Critical: ensures staking bond is always covered
reserve_balance = 2000.0

# Per-transaction ceiling (safety net against bugs)
max_single_transaction = 100.0

# ── Auto-Purchase ──────────────────────────────────────────────
[wallet.auto_purchase]
enabled = true

# Trigger auto-buy when available balance drops below this
trigger_balance = 500.0

# Amount to purchase each time
amount = 2000.0

# Payment source
method = "stripe"                        # "stripe", "crypto_usdc", "crypto_eth"
stripe_payment_method_id = "pm_1234..."  # Stored Stripe payment method

# Price protection: refuse to buy if market price exceeds this
max_price_usd = 0.15                     # Don't buy above $0.15 / PCLAW

# Cooldown between auto-purchases (prevents rapid-fire buys)
cooldown_hours = 24

# Monthly auto-purchase cap
max_monthly_spend_usd = 500.0

# ── Staking ────────────────────────────────────────────────────
[wallet.staking]
# Amount staked as resource provider bond
amount = 5000.0

# Auto-compound: restake a percentage of earned rewards
auto_restake = true
restake_percentage = 20                  # Restake 20% of earnings

# Minimum stake to maintain (auto-unstake warning if below)
minimum_stake = 1000.0

# ── Resource Contribution (Earning) ───────────────────────────
[resources]
# What this peer offers to the network

[resources.cpu]
enabled = true
cores = 4                                # Dedicate 4 cores to network jobs
max_utilization = 0.80                   # Never exceed 80% CPU for network tasks

[resources.gpu]
enabled = true
device = "auto"                          # Auto-detect GPU, or specify "cuda:0"
vram_limit_mb = 8192                     # Max VRAM to allocate for network inference
models = [                               # Pre-loaded models available for jobs
    "llama-3.2-8b-q4",
    "nomic-embed-text-v1.5",
]

[resources.storage]
enabled = true
path = "/data/peerclawd/storage"
capacity_gb = 100                        # Dedicate 100 GB for distributed storage
min_free_gb = 20                         # Stop accepting chunks if disk free < 20GB

[resources.bandwidth]
enabled = true
relay = true                             # Act as relay for NAT-challenged peers
max_relay_bandwidth_mbps = 50

[resources.web_proxy]
enabled = true
max_concurrent_requests = 10
rate_limit_per_minute = 60
# Domains this peer is willing to proxy (empty = any non-blocked)
allowed_domains = []
blocked_domains = ["*.onion", "*.i2p"]

# ── Pricing (What this peer charges) ──────────────────────────
[pricing]
# Strategy: "fixed", "market", or "auto"
# - fixed:  Use the rates below as-is
# - market: Track network average and match it (±margin)
# - auto:   Dynamic pricing based on local utilization
strategy = "auto"

# Base rates (used when strategy = "fixed", or as floor for "auto")
[pricing.rates]
cpu_per_core_hour = 2.0
gpu_per_hour = 18.0
storage_per_gb_month = 0.5
bandwidth_per_gb = 0.3
web_fetch_per_request = 0.1
inference_per_1k_tokens = 1.5

# Auto-pricing parameters
[pricing.auto]
# When utilization is high, increase prices up to this multiplier
max_price_multiplier = 3.0
# When utilization is low, decrease prices down to this multiplier
min_price_multiplier = 0.5
# Target utilization percentage (prices adjust to reach this)
target_utilization = 0.70

# ── Agent Budget Defaults ─────────────────────────────────────
[agent_defaults]
# Default budget for newly created agents (can be overridden per agent)
max_spend_per_request = 10.0
max_spend_per_hour = 100.0
max_spend_per_day = 500.0
auto_refill = true
refill_trigger = 50.0
refill_amount = 200.0
```

---

## 7. Economic Flywheel

The token economy is designed as a self-reinforcing cycle:

```
                    ┌──────────────────┐
                    │  More Resource    │
              ┌────►│  Providers Join   │────┐
              │     └──────────────────┘    │
              │                             ▼
     ┌────────┴────────┐          ┌─────────────────┐
     │  Token Price     │          │  More Resources  │
     │  Appreciates     │          │  Available       │
     │  (demand > supply)│          │  (cheaper, faster)│
     └────────┬────────┘          └────────┬────────┘
              ▲                             │
              │                             ▼
     ┌────────┴────────┐          ┌─────────────────┐
     │  More Token      │          │  More Agents     │
     │  Purchases       │◄─────────│  Deploy          │
     │  (exchange vol)  │          │  (better service) │
     └─────────────────┘          └─────────────────┘
```

### Key Economic Invariants

1. **Tokens have intrinsic utility** — You cannot use the network without spending PCLAW. Demand is tied to actual compute consumption, not speculation.
2. **Supply is capped and declining** — Emission decreases 25% annually. As demand grows and supply tightens, price pressure is upward.
3. **Earning requires real work** — No staking-only rewards. Tokens are minted only when verified resource contribution occurs.
4. **Slashing prevents freeloading** — Bad actors lose their stake. The network is self-policing.
5. **Price is anchored to real costs** — Long-term PCLAW value is bounded by the cost of equivalent cloud resources, providing a fundamental floor.

### Anti-Gaming Measures

| Attack Vector | Mitigation |
|---|---|
| **Sybil attack** (fake peers to farm rewards) | Staking bond required. Minimum stake creates economic cost for each identity |
| **Self-dealing** (agent pays own peer) | Jobs are randomly assigned from the bid pool. Self-bids are deprioritized by reputation algorithm |
| **Wash trading** (inflating volume on exchange) | On-chain analytics + minimum hold periods for resource reward tokens |
| **Resource inflation** (reporting fake resources) | Random challenge-response proofs: storage peers must serve random chunks; GPU peers must complete benchmark inference within time limit |
| **Price manipulation** (peers colluding on pricing) | Minimum N bids required for job assignment. Price outliers >3σ from network median are flagged and deprioritized |

---

## 8. Token Lifecycle Summary

```
MINTING                    CIRCULATION                    SINKING
───────                    ───────────                    ───────

Resource Rewards ──►┐                                ┌──► Slashing (burned)
                    │                                │
Public Sale ───────►├──► Peer Wallets ◄──► Exchange  ├──► Buyback & Burn
                    │         │                      │
Seed/Strategic ────►┘         ▼                      ├──► Transaction Fees
                         Agent Wallets               │    (partially burned)
                              │                      │
                              ▼                      └──► Expired Escrow
                         Job Payments ──────────────►     (small % burned)
                              │
                              ▼
                         Peer Earnings
                              │
                         ┌────┴────┐
                         │         │
                      Restake   Withdraw
                      (bond)    (exchange)
```

### Deflationary Mechanisms

| Mechanism | Rate | Source |
|---|---|---|
| **Transaction fee burn** | 0.1% of every transaction is burned | All on-chain transfers |
| **Slashing burn** | 100% of slashed tokens | Malicious or failed peers |
| **Buyback & burn** | 5% of Treasury revenue | Quarterly burns published on-chain |
| **Expired escrow burn** | 1% of timed-out escrow | Unclaimed after 30-day grace period |

Over time, the declining emission + deflationary burns creates a **net-deflationary** token supply once network usage reaches critical mass (estimated at ~50,000 active peers).

---

*PeerClaw'd Token Economy — Draft v0.1 — March 2026*
