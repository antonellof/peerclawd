# Security Model

## Sandboxed Execution

### WASM Sandbox (Wasmtime)
- Capability-based isolation
- No filesystem/network by default
- Fuel metering prevents infinite loops
- Component Model for typed interfaces

### MicroVM Isolation (Firecracker)
- Heavy workloads in microVMs
- <125ms boot time
- Strict memory limits
- Read-only rootfs

## Credential Protection

- Secrets never exposed to agent code
- Injected at host boundary only
- Leak detection on outbound data
- Pattern matching + entropy analysis

## Encryption

- **P2P**: Noise protocol (end-to-end)
- **Identity**: Ed25519 signatures
- **Content**: BLAKE3 hashing

## Endpoint Allowlisting

HTTP requests restricted to approved hosts:

```toml
[web_access]
allowed_hosts = ["*.wikipedia.org", "api.example.com"]
max_requests_per_minute = 30
```
