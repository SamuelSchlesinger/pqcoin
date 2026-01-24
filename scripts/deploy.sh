#!/bin/bash
# pqcoin Cluster Deployment Script
# Usage: ./scripts/deploy.sh [seed|worker] [SEED_HOST]

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
BINARY="$PROJECT_DIR/target/release/pqcoin"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

print_step() {
    echo -e "${GREEN}[*]${NC} $1"
}

print_warn() {
    echo -e "${YELLOW}[!]${NC} $1"
}

print_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

usage() {
    echo "pqcoin Cluster Deployment"
    echo ""
    echo "Usage:"
    echo "  $0 seed                     - Start as seed node (first node in cluster)"
    echo "  $0 worker <SEED_HOST>       - Start as worker node connecting to seed"
    echo "  $0 build                    - Build release binary only"
    echo "  $0 copy <USER@HOST>         - Copy binary to remote host"
    echo ""
    echo "Examples:"
    echo "  $0 seed"
    echo "  $0 worker Samuels-MacBook-Pro.local:8333"
    echo "  $0 copy sam@Sams-Desktop.local:~/pqcoin/"
    exit 1
}

build() {
    print_step "Building release binary..."
    cd "$PROJECT_DIR"
    cargo build --release
    print_step "Binary built: $BINARY"
}

start_seed() {
    if [[ ! -f "$BINARY" ]]; then
        print_error "Binary not found. Run '$0 build' first."
        exit 1
    fi

    print_step "Starting seed node..."
    print_step "Hostname: $(hostname)"
    print_step "IP addresses:"
    ifconfig | grep "inet " | grep -v "127.0.0.1" | awk '{print "  " $2}'
    echo ""
    print_warn "Other nodes should connect to one of the above addresses on port 8333"
    echo ""

    exec "$BINARY" \
        --port 8333 \
        --mine \
        --rpc \
        --rpc-bind 0.0.0.0 \
        --rpc-port 8332 \
        --log-level info
}

start_worker() {
    local seed_host="$1"

    if [[ -z "$seed_host" ]]; then
        print_error "Seed host required for worker mode"
        usage
    fi

    if [[ ! -f "$BINARY" ]]; then
        print_error "Binary not found. Run '$0 build' first."
        exit 1
    fi

    # Add port if not specified
    if [[ ! "$seed_host" == *":"* ]]; then
        seed_host="${seed_host}:8333"
    fi

    print_step "Starting worker node connecting to $seed_host..."

    exec "$BINARY" \
        --port 8333 \
        --connect "$seed_host" \
        --mine \
        --rpc \
        --rpc-bind 0.0.0.0 \
        --rpc-port 8332 \
        --log-level info
}

copy_binary() {
    local remote="$1"

    if [[ -z "$remote" ]]; then
        print_error "Remote destination required"
        usage
    fi

    if [[ ! -f "$BINARY" ]]; then
        print_error "Binary not found. Run '$0 build' first."
        exit 1
    fi

    print_step "Copying binary to $remote..."
    scp "$BINARY" "$remote"
    print_step "Done!"
}

# Main
case "${1:-}" in
    seed)
        start_seed
        ;;
    worker)
        start_worker "$2"
        ;;
    build)
        build
        ;;
    copy)
        copy_binary "$2"
        ;;
    *)
        usage
        ;;
esac
