#!/bin/bash
#
# pqcoin launchd installation script for macOS
#
# Installs pqcoin as a launchd service with multi-instance support.
# Supports mainnet and testnet running simultaneously.
#
# Usage:
#   sudo ./scripts/install-macos.sh              # Full install (build + install)
#   sudo ./scripts/install-macos.sh --no-build   # Install only (assumes binaries exist)
#
# After installation:
#   sudo launchctl load /Library/LaunchDaemons/com.pqcoin.testnet.plist
#   sudo launchctl start com.pqcoin.testnet
#   tail -f /usr/local/var/log/pqcoin/testnet.log

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

# Check we're on macOS
check_macos() {
    if [[ "$(uname)" != "Darwin" ]]; then
        log_error "This script is for macOS only. Use install.sh for Linux."
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

    # Build release binaries (as the original user, not root)
    ORIGINAL_USER="${SUDO_USER:-$USER}"
    if sudo -u "$ORIGINAL_USER" cargo build --release; then
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
    log_info "Creating _pqcoin system user..."

    # Check if user already exists
    if dscl . -read /Users/_pqcoin &>/dev/null; then
        log_info "User '_pqcoin' already exists"
        return
    fi

    # Check if group already exists
    if dscl . -read /Groups/_pqcoin &>/dev/null; then
        log_info "Group '_pqcoin' already exists"
        # Get existing group's GID for the user
        local uid
        uid=$(dscl . -read /Groups/_pqcoin PrimaryGroupID | awk '{print $2}')
    else
        # Find an available UID/GID in the system range (< 500)
        local uid=400
        while dscl . -list /Users UniqueID 2>/dev/null | grep -q "\\b${uid}\\b" || \
              dscl . -list /Groups PrimaryGroupID 2>/dev/null | grep -q "\\b${uid}\\b"; do
            uid=$((uid + 1))
            if [[ $uid -ge 500 ]]; then
                log_error "Cannot find available system UID"
                exit 1
            fi
        done

        # Create the group
        dscl . -create /Groups/_pqcoin
        dscl . -create /Groups/_pqcoin PrimaryGroupID "$uid"
        dscl . -create /Groups/_pqcoin RealName "pqcoin service"
        log_success "Created group '_pqcoin' (gid: $uid)"
    fi

    # Create the user (group exists at this point)
    local gid
    gid=$(dscl . -read /Groups/_pqcoin PrimaryGroupID | awk '{print $2}')

    # Find available UID if not set
    if [[ -z "${uid:-}" ]]; then
        uid=400
        while dscl . -list /Users UniqueID 2>/dev/null | grep -q "\\b${uid}\\b"; do
            uid=$((uid + 1))
            if [[ $uid -ge 500 ]]; then
                log_error "Cannot find available system UID"
                exit 1
            fi
        done
    fi

    dscl . -create /Users/_pqcoin
    dscl . -create /Users/_pqcoin UniqueID "$uid"
    dscl . -create /Users/_pqcoin PrimaryGroupID "$gid"
    dscl . -create /Users/_pqcoin UserShell /usr/bin/false
    dscl . -create /Users/_pqcoin RealName "pqcoin service"
    dscl . -create /Users/_pqcoin NFSHomeDirectory /usr/local/var/pqcoin

    # Hide the user from login window
    dscl . -create /Users/_pqcoin IsHidden 1

    log_success "Created user '_pqcoin' (uid: $uid)"
}

# Install binaries
install_binaries() {
    log_info "Installing binaries to /usr/local/bin/..."

    # Create /usr/local/bin if it doesn't exist
    mkdir -p /usr/local/bin

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
    mkdir -p /usr/local/etc/pqcoin
    chmod 755 /usr/local/etc/pqcoin

    # Data directories
    mkdir -p /usr/local/var/pqcoin/mainnet/data
    mkdir -p /usr/local/var/pqcoin/testnet/data

    # Log directory
    mkdir -p /usr/local/var/log/pqcoin

    # Set ownership
    chown -R _pqcoin:_pqcoin /usr/local/var/pqcoin
    chown -R _pqcoin:_pqcoin /usr/local/var/log/pqcoin
    chmod -R 750 /usr/local/var/pqcoin
    chmod -R 750 /usr/local/var/log/pqcoin

    log_success "Created directories"
}

# Install configuration files
install_configs() {
    log_info "Installing configuration files..."

    # Install config files (don't overwrite existing)
    for conf in mainnet.toml testnet.toml; do
        src="$SOURCE_DIR/etc/pqcoin/$conf"
        dst="/usr/local/etc/pqcoin/$conf"

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
    chmod 644 /usr/local/etc/pqcoin/*.toml 2>/dev/null || true
}

# Install launchd plists
install_launchd_plists() {
    log_info "Installing launchd plists..."

    for network in mainnet testnet; do
        src="$SOURCE_DIR/launchd/com.pqcoin.${network}.plist"
        dst="/Library/LaunchDaemons/com.pqcoin.${network}.plist"

        if [[ -f "$src" ]]; then
            cp "$src" "$dst"
            chmod 644 "$dst"
            chown root:wheel "$dst"
            log_success "Installed $dst"
        else
            log_error "Plist not found: $src"
            exit 1
        fi
    done
}

# Print usage instructions
print_usage() {
    echo ""
    echo -e "${GREEN}Installation complete!${NC}"
    echo ""
    echo "Quick start:"
    echo "  # Load and start testnet"
    echo "  sudo launchctl load /Library/LaunchDaemons/com.pqcoin.testnet.plist"
    echo "  sudo launchctl start com.pqcoin.testnet"
    echo ""
    echo "  # Load and start mainnet"
    echo "  sudo launchctl load /Library/LaunchDaemons/com.pqcoin.mainnet.plist"
    echo "  sudo launchctl start com.pqcoin.mainnet"
    echo ""
    echo "Enable on boot (load includes enable):"
    echo "  sudo launchctl load -w /Library/LaunchDaemons/com.pqcoin.testnet.plist"
    echo ""
    echo "View logs:"
    echo "  tail -f /usr/local/var/log/pqcoin/testnet.log"
    echo "  tail -f /usr/local/var/log/pqcoin/mainnet.log"
    echo ""
    echo "Stop services:"
    echo "  sudo launchctl stop com.pqcoin.testnet"
    echo "  sudo launchctl unload /Library/LaunchDaemons/com.pqcoin.testnet.plist"
    echo ""
    echo "Configuration files:"
    echo "  /usr/local/etc/pqcoin/mainnet.toml"
    echo "  /usr/local/etc/pqcoin/testnet.toml"
    echo ""
    echo "Data directories:"
    echo "  /usr/local/var/pqcoin/mainnet/data"
    echo "  /usr/local/var/pqcoin/testnet/data"
    echo ""
    echo "Port allocation:"
    echo "  Mainnet: P2P=8333, RPC=8332, Metrics=9091"
    echo "  Testnet: P2P=18333, RPC=18332, Metrics=19091"
    echo ""
    echo "To enable mining, edit the config file:"
    echo "  sudo vim /usr/local/etc/pqcoin/testnet.toml"
    echo "  # Set: mining.enabled = true"
    echo "  sudo launchctl stop com.pqcoin.testnet"
    echo "  sudo launchctl start com.pqcoin.testnet"
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
    echo "  pqcoin macOS installer"
    echo "=================================="
    echo ""

    check_root
    check_macos
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
    install_launchd_plists
    print_usage
}

main "$@"
