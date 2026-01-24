#!/bin/bash
# pqcoin Cluster Verification Script
# Checks node status and cluster health

set -e

RPC_PORT="${RPC_PORT:-8332}"
RPC_HOST="${RPC_HOST:-127.0.0.1}"

# Colors
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m'

rpc_call() {
    local method="$1"
    curl -s "http://${RPC_HOST}:${RPC_PORT}" \
        -H "Content-Type: application/json" \
        -d "{\"jsonrpc\":\"2.0\",\"method\":\"$method\",\"params\":[],\"id\":1}" 2>/dev/null
}

check_result() {
    local name="$1"
    local result="$2"

    if [[ -z "$result" ]] || echo "$result" | grep -q '"error"'; then
        echo -e "${RED}[FAIL]${NC} $name"
        if [[ -n "$result" ]]; then
            echo "       Error: $(echo "$result" | jq -r '.error.message // .error // "Unknown error"' 2>/dev/null)"
        else
            echo "       No response from RPC"
        fi
        return 1
    else
        echo -e "${GREEN}[OK]${NC}   $name"
        return 0
    fi
}

echo "==================================="
echo "  pqcoin Cluster Verification"
echo "==================================="
echo ""
echo "Checking RPC at http://${RPC_HOST}:${RPC_PORT}"
echo ""

# Check blockchain info
echo "--- Blockchain Status ---"
blockchain_info=$(rpc_call "getblockchaininfo")
if check_result "Blockchain info" "$blockchain_info"; then
    height=$(echo "$blockchain_info" | jq -r '.result.height // "N/A"' 2>/dev/null)
    tip=$(echo "$blockchain_info" | jq -r '.result.best_block_hash // "N/A"' 2>/dev/null | head -c 20)
    difficulty=$(echo "$blockchain_info" | jq -r '.result.difficulty // "N/A"' 2>/dev/null)
    mempool_size=$(echo "$blockchain_info" | jq -r '.result.mempool_size // 0' 2>/dev/null)
    echo "       Height: $height"
    echo "       Tip: ${tip}..."
    echo "       Difficulty: $difficulty"
    echo "       Mempool: $mempool_size txs"
fi
echo ""

# Check mempool details
echo "--- Mempool Status ---"
mempool_info=$(rpc_call "getmempoolinfo")
if check_result "Mempool info" "$mempool_info"; then
    tx_count=$(echo "$mempool_info" | jq -r '.result.size // 0' 2>/dev/null)
    echo "       Transactions: $tx_count"
    if [[ "$tx_count" != "0" ]]; then
        echo "       TXIDs:"
        echo "$mempool_info" | jq -r '.result.txids[]' 2>/dev/null | head -5 | while read txid; do
            echo "         - ${txid:0:20}..."
        done
    fi
fi
echo ""

# Get best block hash and block info
echo "--- Latest Block ---"
best_hash=$(rpc_call "getbestblockhash")
if check_result "Best block hash" "$best_hash"; then
    hash=$(echo "$best_hash" | jq -r '.result' 2>/dev/null)
    echo "       Hash: ${hash:0:20}..."

    # Get block details
    block_info=$(curl -s "http://${RPC_HOST}:${RPC_PORT}" \
        -H "Content-Type: application/json" \
        -d "{\"jsonrpc\":\"2.0\",\"method\":\"getblock\",\"params\":[\"$hash\"],\"id\":1}" 2>/dev/null)
    if [[ -n "$block_info" ]] && ! echo "$block_info" | grep -q '"error"'; then
        tx_count=$(echo "$block_info" | jq -r '.result.tx_count // "N/A"' 2>/dev/null)
        timestamp=$(echo "$block_info" | jq -r '.result.timestamp // "N/A"' 2>/dev/null)
        echo "       Transactions: $tx_count"
        echo "       Timestamp: $timestamp"
    fi
fi
echo ""

# Note about peer info
echo "--- Network Status ---"
echo -e "${YELLOW}[NOTE]${NC} Peer info not available via RPC"
echo "       Check node logs for connection status"
echo ""

echo "==================================="
echo "Verification complete"
echo "==================================="
