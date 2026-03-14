#!/bin/bash
# Run multiple PeerClaw'd agents for distributed testing

set -e

NUM_AGENTS=${1:-3}
DURATION=${2:-30}
BASE_PORT=9000

echo "=== PeerClaw'd Multi-Agent Test ==="
echo "Agents: $NUM_AGENTS"
echo "Duration: ${DURATION}s"
echo ""

# Build release if needed
if [ ! -f target/release/peerclawd ]; then
    echo "Building release binary..."
    cargo build --release
fi

# Create temp directories for each agent
TEMP_DIR=$(mktemp -d)
echo "Temp directory: $TEMP_DIR"

# Function to cleanup on exit
cleanup() {
    echo ""
    echo "Cleaning up..."
    kill $(jobs -p) 2>/dev/null || true
    rm -rf "$TEMP_DIR"
    echo "Done."
}
trap cleanup EXIT

# Start agents
PIDS=()
for i in $(seq 0 $((NUM_AGENTS - 1))); do
    AGENT_DIR="$TEMP_DIR/agent_$i"
    mkdir -p "$AGENT_DIR"

    PORT=$((BASE_PORT + i))
    LISTEN="/ip4/127.0.0.1/tcp/$PORT"

    # First agent is bootstrap, others connect to it
    if [ $i -eq 0 ]; then
        BOOTSTRAP=""
    else
        BOOTSTRAP="--bootstrap /ip4/127.0.0.1/tcp/$BASE_PORT"
    fi

    echo "Starting agent $i on port $PORT..."

    PEERCLAWD_HOME="$AGENT_DIR" \
    RUST_LOG=peerclawd=info \
    ./target/release/peerclawd serve \
        --listen "$LISTEN" \
        $BOOTSTRAP \
        > "$AGENT_DIR/output.log" 2>&1 &

    PIDS+=($!)

    # Small delay between agent starts
    sleep 1
done

echo ""
echo "All $NUM_AGENTS agents started."
echo "Peer IDs:"
for i in $(seq 0 $((NUM_AGENTS - 1))); do
    AGENT_DIR="$TEMP_DIR/agent_$i"
    # Wait for peer ID to appear in log
    sleep 2
    PEER_ID=$(grep "Peer ID:" "$AGENT_DIR/output.log" 2>/dev/null | head -1 | sed 's/.*Peer ID: //' || echo "Unknown")
    echo "  Agent $i: $PEER_ID"
done

echo ""
echo "Running for ${DURATION} seconds..."
echo "Press Ctrl+C to stop early."
echo ""

# Monitor connections
for ((t=0; t<DURATION; t+=5)); do
    echo "[$t s] Checking connections..."
    for i in $(seq 0 $((NUM_AGENTS - 1))); do
        AGENT_DIR="$TEMP_DIR/agent_$i"
        CONNECTIONS=$(grep "Connected to peer" "$AGENT_DIR/output.log" 2>/dev/null | wc -l | tr -d ' ')
        echo "  Agent $i: $CONNECTIONS connections"
    done
    echo ""
    sleep 5
done

echo ""
echo "=== Final Status ==="
for i in $(seq 0 $((NUM_AGENTS - 1))); do
    AGENT_DIR="$TEMP_DIR/agent_$i"
    echo ""
    echo "Agent $i log summary:"
    tail -20 "$AGENT_DIR/output.log" 2>/dev/null || echo "  (no log)"
done

echo ""
echo "Test complete!"
