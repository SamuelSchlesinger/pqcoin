#!/bin/bash
#
# pqcoin launchd uninstallation script for macOS
#
# Removes pqcoin launchd services, binaries, and optionally data/config.
#
# Usage:
#   sudo ./scripts/uninstall-macos.sh              # Remove services and binaries
#   sudo ./scripts/uninstall-macos.sh --purge      # Also remove data and config
#   sudo ./scripts/uninstall-macos.sh --keep-user  # Don't remove _pqcoin user

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
        log_error "This script is for macOS only. Use uninstall.sh for Linux."
        exit 1
    fi
}

# Stop and unload services
stop_services() {
    log_info "Stopping pqcoin services..."

    for network in mainnet testnet; do
        local plist="/Library/LaunchDaemons/com.pqcoin.${network}.plist"
        local label="com.pqcoin.${network}"

        if [[ -f "$plist" ]]; then
            # Try to stop the service
            if launchctl list "$label" &>/dev/null; then
                launchctl stop "$label" 2>/dev/null || true
                log_success "Stopped $label"
            fi

            # Unload the plist
            launchctl unload "$plist" 2>/dev/null || true
            log_success "Unloaded $plist"
        fi
    done
}

# Remove launchd plists
remove_launchd_plists() {
    log_info "Removing launchd plists..."

    for network in mainnet testnet; do
        local plist="/Library/LaunchDaemons/com.pqcoin.${network}.plist"
        if [[ -f "$plist" ]]; then
            rm -f "$plist"
            log_success "Removed $plist"
        fi
    done
}

# Remove binaries
remove_binaries() {
    log_info "Removing binaries..."

    if [[ -f /usr/local/bin/pqcoin ]]; then
        rm -f /usr/local/bin/pqcoin
        log_success "Removed /usr/local/bin/pqcoin"
    fi

    if [[ -f /usr/local/bin/pqwallet ]]; then
        rm -f /usr/local/bin/pqwallet
        log_success "Removed /usr/local/bin/pqwallet"
    fi
}

# Remove configuration files
remove_configs() {
    log_info "Removing configuration files..."

    if [[ -d /usr/local/etc/pqcoin ]]; then
        rm -rf /usr/local/etc/pqcoin
        log_success "Removed /usr/local/etc/pqcoin"
    fi
}

# Remove data directories
remove_data() {
    log_info "Removing data directories..."

    if [[ -d /usr/local/var/pqcoin ]]; then
        rm -rf /usr/local/var/pqcoin
        log_success "Removed /usr/local/var/pqcoin"
    fi

    if [[ -d /usr/local/var/log/pqcoin ]]; then
        rm -rf /usr/local/var/log/pqcoin
        log_success "Removed /usr/local/var/log/pqcoin"
    fi
}

# Remove system user
remove_user() {
    log_info "Removing _pqcoin user..."

    if dscl . -read /Users/_pqcoin &>/dev/null; then
        dscl . -delete /Users/_pqcoin
        log_success "Removed user '_pqcoin'"
    fi

    if dscl . -read /Groups/_pqcoin &>/dev/null; then
        dscl . -delete /Groups/_pqcoin
        log_success "Removed group '_pqcoin'"
    fi
}

# Main uninstallation function
main() {
    local purge=false
    local keep_user=false

    # Parse arguments
    for arg in "$@"; do
        case $arg in
            --purge)
                purge=true
                ;;
            --keep-user)
                keep_user=true
                ;;
            --help|-h)
                echo "Usage: $0 [--purge] [--keep-user]"
                echo ""
                echo "Options:"
                echo "  --purge       Also remove data and configuration"
                echo "  --keep-user   Don't remove the _pqcoin system user"
                exit 0
                ;;
            *)
                log_error "Unknown argument: $arg"
                exit 1
                ;;
        esac
    done

    echo "=================================="
    echo "  pqcoin macOS uninstaller"
    echo "=================================="
    echo ""

    check_root
    check_macos

    # Confirm if purging
    if $purge; then
        log_warn "This will PERMANENTLY DELETE all pqcoin data!"
        echo ""
        read -p "Are you sure you want to continue? (yes/no): " confirm
        if [[ "$confirm" != "yes" ]]; then
            log_info "Aborted"
            exit 0
        fi
    fi

    stop_services
    remove_launchd_plists
    remove_binaries

    if $purge; then
        remove_configs
        remove_data
    else
        log_info "Config and data preserved (use --purge to remove)"
        log_info "  /usr/local/etc/pqcoin"
        log_info "  /usr/local/var/pqcoin"
        log_info "  /usr/local/var/log/pqcoin"
    fi

    if ! $keep_user; then
        remove_user
    else
        log_info "User '_pqcoin' preserved (--keep-user specified)"
    fi

    echo ""
    log_success "Uninstallation complete"

    if ! $purge; then
        echo ""
        echo "To completely remove all data and config:"
        echo "  sudo $0 --purge"
    fi
}

main "$@"
