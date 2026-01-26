#!/usr/bin/env bash
# LAN Testnet Validation Script
# Run this interactively to test each scenario

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

print_header() {
    echo ""
    echo -e "${BLUE}═══════════════════════════════════════════════════════════════${NC}"
    echo -e "${BLUE}  $1${NC}"
    echo -e "${BLUE}═══════════════════════════════════════════════════════════════${NC}"
    echo ""
}

print_step() {
    echo -e "${YELLOW}→ $1${NC}"
}

print_success() {
    echo -e "${GREEN}✓ $1${NC}"
}

print_fail() {
    echo -e "${RED}✗ $1${NC}"
}

prompt_continue() {
    echo ""
    read -p "Press Enter when ready to continue (or 'skip' to skip)... " response
    if [[ "$response" == "skip" ]]; then
        return 1
    fi
    return 0
}

prompt_result() {
    echo ""
    read -p "Did this test pass? (y/n): " result
    if [[ "$result" == "y" || "$result" == "Y" ]]; then
        print_success "$1 - PASSED"
        return 0
    else
        print_fail "$1 - FAILED"
        return 1
    fi
}

# Track results
PASSED=0
FAILED=0
SKIPPED=0

record_result() {
    if [[ $1 -eq 0 ]]; then
        ((PASSED++))
    elif [[ $1 -eq 1 ]]; then
        ((FAILED++))
    else
        ((SKIPPED++))
    fi
}

print_header "LAN Testnet Validation"

echo "This script guides you through testing the pqcoin testnet on your"
echo "Tailscale-connected machines (desktop and MacBook)."
echo ""
echo "Prerequisites:"
echo "  1. Both machines connected via Tailscale"
echo "  2. pqcoin built on both machines: cargo build --release"
echo "  3. Know the Tailscale IPs of both machines"
echo ""

read -p "Enter desktop Tailscale IP: " DESKTOP_IP
read -p "Enter MacBook Tailscale IP: " MACBOOK_IP

echo ""
echo "Configuration:"
echo "  Desktop: $DESKTOP_IP"
echo "  MacBook: $MACBOOK_IP"
echo ""

# Generate config files
print_header "Step 0: Configuration Files"

echo "Create these config files on each machine:"
echo ""
echo -e "${YELLOW}=== Desktop (pqcoin-testnet.toml) ===${NC}"
cat << EOF
network_type = "testnet"

[network]
port = 8333
seed_peers = ["$MACBOOK_IP:8333"]

[mining]
enabled = true

[rpc]
enabled = true
port = 8332
bind = "0.0.0.0"
EOF

echo ""
echo -e "${YELLOW}=== MacBook (pqcoin-testnet.toml) ===${NC}"
cat << EOF
network_type = "testnet"

[network]
port = 8333
seed_peers = ["$DESKTOP_IP:8333"]

[mining]
enabled = false

[rpc]
enabled = true
port = 8332
bind = "0.0.0.0"
EOF

echo ""
echo "Start commands:"
echo "  Desktop: ./target/release/pqcoin --config pqcoin-testnet.toml"
echo "  MacBook: ./target/release/pqcoin --config pqcoin-testnet.toml"
echo ""
echo "Or use the --testnet flag directly:"
echo "  Desktop: ./target/release/pqcoin --testnet --mine -C $MACBOOK_IP:8333"
echo "  MacBook: ./target/release/pqcoin --testnet -C $DESKTOP_IP:8333"

prompt_continue || { record_result 2; }

# ============================================================================
# TEST 1: Block Propagation
# ============================================================================
print_header "Test 1: Block Propagation"

print_step "1a. Mine block on desktop, verify it reaches MacBook"
echo ""
echo "On desktop, you should see: 'mined new block!'"
echo "On MacBook, you should see: 'received new block'"
echo ""
echo "Check both nodes have the same chain height:"
echo "  curl -s http://$DESKTOP_IP:8332 -d '{\"jsonrpc\":\"2.0\",\"method\":\"getblockchaininfo\",\"id\":1}' | jq"
echo "  curl -s http://$MACBOOK_IP:8332 -d '{\"jsonrpc\":\"2.0\",\"method\":\"getblockchaininfo\",\"id\":1}' | jq"

if prompt_continue; then
    prompt_result "Block propagation desktop→MacBook"
    record_result $?
else
    record_result 2
fi

print_step "1b. Mine block on MacBook, verify it reaches desktop"
echo ""
echo "Temporarily enable mining on MacBook (stop and restart with --mine)"
echo "Or just verify blocks continue to propagate from desktop."

if prompt_continue; then
    prompt_result "Block propagation MacBook→desktop"
    record_result $?
else
    record_result 2
fi

print_step "1c. Measure propagation latency"
echo ""
echo "Watch timestamps in logs when blocks are mined vs received."
echo "Latency should be under 1 second on LAN."

if prompt_continue; then
    prompt_result "Propagation latency acceptable"
    record_result $?
else
    record_result 2
fi

# ============================================================================
# TEST 2: Chain Synchronization
# ============================================================================
print_header "Test 2: Chain Synchronization"

print_step "2a. Stop MacBook, let desktop mine blocks"
echo ""
echo "1. Stop pqcoin on MacBook (Ctrl+C)"
echo "2. Let desktop mine 5-10 blocks"
echo "3. Note the desktop's chain height"

if prompt_continue; then
    prompt_result "Desktop mined blocks while MacBook offline"
    record_result $?
else
    record_result 2
fi

print_step "2b. Restart MacBook, verify it syncs"
echo ""
echo "1. Start pqcoin on MacBook"
echo "2. Watch logs for 'sync state changed' messages"
echo "3. Verify MacBook catches up to desktop's height"
echo ""
echo "Check heights match:"
echo "  curl -s http://$DESKTOP_IP:8332 -d '{\"jsonrpc\":\"2.0\",\"method\":\"getblockchaininfo\",\"id\":1}' | jq"
echo "  curl -s http://$MACBOOK_IP:8332 -d '{\"jsonrpc\":\"2.0\",\"method\":\"getblockchaininfo\",\"id\":1}' | jq"

if prompt_continue; then
    prompt_result "MacBook synced after restart"
    record_result $?
else
    record_result 2
fi

print_step "2c. Test sync from scratch"
echo ""
echo "1. Stop MacBook"
echo "2. Delete testnet data: rm -rf ~/.pqcoin-testnet"
echo "3. Start MacBook and verify it syncs the full chain"

if prompt_continue; then
    prompt_result "Fresh sync from empty state"
    record_result $?
else
    record_result 2
fi

# ============================================================================
# TEST 3: Transaction Flow
# ============================================================================
print_header "Test 3: Transaction Flow"

print_step "3a. Create wallet on each node"
echo ""
echo "On each machine:"
echo "  ./target/release/pqwallet create"
echo "  ./target/release/pqwallet address"
echo ""
echo "Note both addresses."

if prompt_continue; then
    prompt_result "Wallets created"
    record_result $?
else
    record_result 2
fi

print_step "3b. Send coins from miner to MacBook wallet"
echo ""
echo "First, the miner needs coins. After mining a block, wait for"
echo "coinbase maturity (100 blocks) or use the balance check."
echo ""
echo "Check miner balance:"
echo "  ./target/release/pqwallet balance"
echo ""
echo "Send coins (replace ADDRESS with MacBook's address):"
echo "  ./target/release/pqwallet send ADDRESS 1000000"

if prompt_continue; then
    prompt_result "Transaction created and sent"
    record_result $?
else
    record_result 2
fi

print_step "3c. Verify UTXO updates on both nodes"
echo ""
echo "After transaction is mined, check balances on both nodes."
echo "MacBook should show received coins."

if prompt_continue; then
    prompt_result "UTXO updates verified"
    record_result $?
else
    record_result 2
fi

print_step "3d. Test transaction propagation before mining"
echo ""
echo "1. Send another transaction"
echo "2. Before it's mined, check mempool on both nodes via RPC"
echo "   curl -s http://$DESKTOP_IP:8332 -d '{\"jsonrpc\":\"2.0\",\"method\":\"getmempoolinfo\",\"id\":1}' | jq"
echo "   curl -s http://$MACBOOK_IP:8332 -d '{\"jsonrpc\":\"2.0\",\"method\":\"getmempoolinfo\",\"id\":1}' | jq"
echo "3. Both should show the pending transaction in 'txids'"

if prompt_continue; then
    prompt_result "Transaction propagates to mempool"
    record_result $?
else
    record_result 2
fi

# ============================================================================
# TEST 4: Reorg Handling
# ============================================================================
print_header "Test 4: Reorg Handling"

print_step "4a. Disconnect nodes, mine on both"
echo ""
echo "This tests chain reorganization:"
echo "1. Stop the MacBook node"
echo "2. Mine 3 blocks on desktop"
echo "3. Start MacBook with mining enabled but NO seed peers:"
echo "   ./target/release/pqcoin --testnet --mine"
echo "4. Let MacBook mine 5 blocks (longer chain)"
echo "5. Note: MacBook will have different blocks than desktop"

if prompt_continue; then
    prompt_result "Created divergent chains"
    record_result $?
else
    record_result 2
fi

print_step "4b. Reconnect and verify reorg"
echo ""
echo "1. Stop MacBook"
echo "2. Restart with seed peer:"
echo "   ./target/release/pqcoin --testnet -C $DESKTOP_IP:8333"
echo "3. The shorter chain (desktop) should reorg to MacBook's longer chain"
echo "4. Both should end up at the same height/tip"

if prompt_continue; then
    prompt_result "Reorg to longest chain"
    record_result $?
else
    record_result 2
fi

print_step "4c. Verify UTXO consistency after reorg"
echo ""
echo "Check that balances are consistent on both nodes after reorg."
echo "Any transactions in the orphaned blocks should return to mempool."

if prompt_continue; then
    prompt_result "UTXO consistency after reorg"
    record_result $?
else
    record_result 2
fi

# ============================================================================
# TEST 5: Stress Testing
# ============================================================================
print_header "Test 5: Stress Testing"

print_step "5a. Sustained mining"
echo ""
echo "Let the testnet run for at least 30 minutes with continuous mining."
echo "Watch for:"
echo "  - Memory growth"
echo "  - Disk usage growth"
echo "  - Any error messages"
echo ""
echo "Use 'top' or Activity Monitor to watch resource usage."

if prompt_continue; then
    prompt_result "Sustained mining stable"
    record_result $?
else
    record_result 2
fi

print_step "5b. Many transactions in mempool"
echo ""
echo "If you have enough mature coins, send multiple transactions quickly"
echo "before they get mined. Verify mempool handles them correctly."

if prompt_continue; then
    prompt_result "Mempool handles multiple transactions"
    record_result $?
else
    record_result 2
fi

print_step "5c. Node restart recovery"
echo ""
echo "1. While mining is active, force-kill the desktop node (kill -9)"
echo "2. Restart and verify it recovers without corruption"
echo "3. Verify it reconnects and continues syncing"

if prompt_continue; then
    prompt_result "Node restart recovery"
    record_result $?
else
    record_result 2
fi

# ============================================================================
# Summary
# ============================================================================
print_header "Test Summary"

TOTAL=$((PASSED + FAILED + SKIPPED))
echo "Results:"
echo -e "  ${GREEN}Passed:  $PASSED${NC}"
echo -e "  ${RED}Failed:  $FAILED${NC}"
echo -e "  ${YELLOW}Skipped: $SKIPPED${NC}"
echo "  Total:   $TOTAL"
echo ""

if [[ $FAILED -eq 0 && $SKIPPED -eq 0 ]]; then
    print_success "All tests passed! LAN testnet validation complete."
    echo ""
    echo "You can mark the issue complete:"
    echo "  ./scripts/roadmap-complete.sh 006"
elif [[ $FAILED -eq 0 ]]; then
    echo -e "${YELLOW}Some tests were skipped. Re-run to complete all tests.${NC}"
else
    echo -e "${RED}Some tests failed. Investigate and fix issues before proceeding.${NC}"
fi
