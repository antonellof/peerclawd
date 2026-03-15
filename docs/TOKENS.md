# Token Economy

## Token Utility

| Use Case | Description |
|----------|-------------|
| **Inference** | Pay peers for LLM inference |
| **Storage** | Rent distributed storage |
| **Web Access** | Token-gated web scraping (HTTP 402) |
| **Tool Execution** | Pay for WASM tool runs |
| **Staking** | Stake to become verified provider |

## Pricing Model

Each peer sets its own pricing:

```rust
struct PricingTable {
    inference_per_token: u64,    // μPCLAW per output token
    web_fetch_per_request: u64,  // μPCLAW per request
    storage_per_gb_month: u64,   // μPCLAW per GB/month
}
```

## Payment Flow

1. **Job Request** - Requester specifies max budget
2. **Bidding** - Providers submit price bids
3. **Escrow** - Winner's price locked in escrow
4. **Execution** - Provider executes job
5. **Settlement** - On success, escrowed funds released to provider

## Local Accounting

Tokens are tracked locally with eventual on-chain settlement:
- Payment channels between frequent peers
- HTLC for atomic job payment
- Reputation affects trust thresholds
