#!/bin/bash
#
# pqcoin systemd installation script
#
# Installs pqcoin as a systemd service with multi-instance support.
# Supports mainnet and testnet running simultaneously.
#
# Usage:
#   sudo ./scripts/install.sh           # Full install (build + install)
#   sudo ./scripts/install.sh --no-build # Install only (assumes binaries exist)
#
# After installation:
#   sudo systemctl start pqcoin@testnet   # Start testnet
#   sudo systemctl start pqcoin@mainnet   # Start mainnet
#   sudo systemctl enable pqcoin@testnet  # Enable on boot
#   journalctl -u pqcoin@testnet -f       # View logs

set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

log_info() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

log_success() {
    echo -e "${GREEN}[OK]${NC} $1"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# Check if running as root
check_root() {
    if [[ $EUID -ne 0 ]]; then
        log_error "This script must be run as root (use sudo)"
        exit 1
    fi
}

# Detect the source directory (where this script is located)
detect_source_dir() {
    SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
    SOURCE_DIR="$(dirname "$SCRIPT_DIR")"

    if [[ ! -f "$SOURCE_DIR/Cargo.toml" ]]; then
        log_error "Cannot find pqcoin source directory"
        log_error "Expected Cargo.toml at: $SOURCE_DIR/Cargo.toml"
        exit 1
    fi

    log_info "Source directory: $SOURCE_DIR"
}

# Build pqcoin from source
build_pqcoin() {
    log_info "Building pqcoin..."

    if ! command -v cargo &> /dev/null; then
        log_error "cargo not found. Please install Rust first."
        log_error "Visit: https://rustup.rs/"
        exit 1
    fi

    cd "$SOURCE_DIR"

    # Build release binaries
    if cargo build --release; then
        log_success "Build completed"
    else
        log_error "Build failed"
        exit 1
    fi

    # Check binaries exist
    if [[ ! -f "$SOURCE_DIR/target/release/pqcoin" ]]; then
        log_error "pqcoin binary not found after build"
        exit 1
    fi

    if [[ ! -f "$SOURCE_DIR/target/release/pqwallet" ]]; then
        log_warn "pqwallet binary not found (wallet features may be missing)"
    fi
}

# Create system user and group
create_user() {
    log_info "Creating pqcoin system user..."

    if id "pqcoin" &>/dev/null; then
        log_info "User 'pqcoin' already exists"
    else
        useradd --system --user-group --home-dir /var/lib/pqcoin --shell /usr/sbin/nologin pqcoin
        log_success "Created user 'pqcoin'"
    fi
}

# Install binaries
install_binaries() {
    log_info "Installing binaries to /usr/local/bin/..."

    # Install pqcoin
    install -m 755 "$SOURCE_DIR/target/release/pqcoin" /usr/local/bin/pqcoin
    log_success "Installed pqcoin"

    # Install pqwallet if it exists
    if [[ -f "$SOURCE_DIR/target/release/pqwallet" ]]; then
        install -m 755 "$SOURCE_DIR/target/release/pqwallet" /usr/local/bin/pqwallet
        log_success "Installed pqwallet"
    fi
}

# Create directory structure
create_directories() {
    log_info "Creating directory structure..."

    # Config directory
    mkdir -p /etc/pqcoin
    chmod 755 /etc/pqcoin

    # Data directories
    mkdir -p /var/lib/pqcoin/mainnet/data
    mkdir -p /var/lib/pqcoin/testnet/data

    # Set ownership
    chown -R pqcoin:pqcoin /var/lib/pqcoin
    chmod -R 750 /var/lib/pqcoin

    log_success "Created directories"
}

# Install configuration files
install_configs() {
    log_info "Installing configuration files..."

    # Install config files (don't overwrite existing)
    for conf in mainnet.toml testnet.toml mainnet.env testnet.env; do
        src="$SOURCE_DIR/etc/pqcoin/$conf"
        dst="/etc/pqcoin/$conf"

        if [[ -f "$src" ]]; then
            if [[ -f "$dst" ]]; then
                log_info "Config $dst already exists, skipping (backup at $dst.new)"
                cp "$src" "$dst.new"
            else
                cp "$src" "$dst"
                log_success "Installed $dst"
            fi
        else
            log_warn "Source config not found: $src"
        fi
    done

    # Set config file permissions
    chmod 644 /etc/pqcoin/*.toml 2>/dev/null || true
    chmod 644 /etc/pqcoin/*.env 2>/dev/null || true
}

# Install systemd units
install_systemd_units() {
    log_info "Installing systemd units..."

    # Install service template
    if [[ -f "$SOURCE_DIR/systemd/pqcoin@.service" ]]; then
        cp "$SOURCE_DIR/systemd/pqcoin@.service" /etc/systemd/system/
        chmod 644 /etc/systemd/system/pqcoin@.service
        log_success "Installed pqcoin@.service"
    else
        log_error "systemd/pqcoin@.service not found"
        exit 1
    fi

    # Install target
    if [[ -f "$SOURCE_DIR/systemd/pqcoin.target" ]]; then
        cp "$SOURCE_DIR/systemd/pqcoin.target" /etc/systemd/system/
        chmod 644 /etc/systemd/system/pqcoin.target
        log_success "Installed pqcoin.target"
    else
        log_error "systemd/pqcoin.target not found"
        exit 1
    fi

    # Reload systemd
    systemctl daemon-reload
    log_success "Reloaded systemd daemon"
}

# Print usage instructions
print_usage() {
    echo ""
    echo -e "${GREEN}Installation complete!${NC}"
    echo ""
    echo "Quick start:"
    echo "  sudo systemctl start pqcoin@testnet     # Start testnet node"
    echo "  sudo systemctl start pqcoin@mainnet     # Start mainnet node"
    echo ""
    echo "Enable on boot:"
    echo "  sudo systemctl enable pqcoin@testnet"
    echo "  sudo systemctl enable pqcoin@mainnet"
    echo ""
    echo "View logs:"
    echo "  journalctl -u pqcoin@testnet -f"
    echo "  journalctl -u pqcoin@mainnet -f"
    echo ""
    echo "Configuration files:"
    echo "  /etc/pqcoin/mainnet.toml    - Mainnet config"
    echo "  /etc/pqcoin/testnet.toml    - Testnet config"
    echo "  /etc/pqcoin/mainnet.env     - Mainnet environment overrides"
    echo "  /etc/pqcoin/testnet.env     - Testnet environment overrides"
    echo ""
    echo "Data directories:"
    echo "  /var/lib/pqcoin/mainnet/data"
    echo "  /var/lib/pqcoin/testnet/data"
    echo ""
    echo "Port allocation:"
    echo "  Mainnet: P2P=8333, RPC=8332, Metrics=9091"
    echo "  Testnet: P2P=18333, RPC=18332, Metrics=19091"
    echo ""
    echo "To enable mining, edit the .env file:"
    echo "  sudo vim /etc/pqcoin/testnet.env"
    echo "  # Add: PQCOIN_EXTRA_ARGS=--mine"
    echo "  sudo systemctl restart pqcoin@testnet"
    echo ""
}

# Main installation function
main() {
    local do_build=true

    # Parse arguments
    for arg in "$@"; do
        case $arg in
            --no-build)
                do_build=false
                ;;
            --help|-h)
                echo "Usage: $0 [--no-build]"
                echo ""
                echo "Options:"
                echo "  --no-build    Skip building, assume binaries exist in target/release/"
                exit 0
                ;;
            *)
                log_error "Unknown argument: $arg"
                exit 1
                ;;
        esac
    done

    echo "=================================="
    echo "  pqcoin systemd installer"
    echo "=================================="
    echo ""

    check_root
    detect_source_dir

    if $do_build; then
        build_pqcoin
    else
        log_info "Skipping build (--no-build specified)"
        if [[ ! -f "$SOURCE_DIR/target/release/pqcoin" ]]; then
            log_error "Binary not found: $SOURCE_DIR/target/release/pqcoin"
            log_error "Run 'cargo build --release' first, or remove --no-build"
            exit 1
        fi
    fi

    create_user
    install_binaries
    create_directories
    install_configs
    install_systemd_units
    print_usage
}

main "$@"
