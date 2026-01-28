#!/bin/bash
#
# pqcoin systemd uninstallation script
#
# Removes pqcoin systemd services, binaries, and optionally data/config.
#
# Usage:
#   sudo ./scripts/uninstall.sh              # Remove services and binaries
#   sudo ./scripts/uninstall.sh --purge      # Also remove data and config
#   sudo ./scripts/uninstall.sh --keep-user  # Don't remove pqcoin user

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

# Stop and disable services
stop_services() {
    log_info "Stopping pqcoin services..."

    # Stop all instances
    for instance in mainnet testnet; do
        if systemctl is-active --quiet "pqcoin@$instance" 2>/dev/null; then
            systemctl stop "pqcoin@$instance"
            log_success "Stopped pqcoin@$instance"
        fi

        if systemctl is-enabled --quiet "pqcoin@$instance" 2>/dev/null; then
            systemctl disable "pqcoin@$instance"
            log_success "Disabled pqcoin@$instance"
        fi
    done

    # Stop target
    if systemctl is-active --quiet pqcoin.target 2>/dev/null; then
        systemctl stop pqcoin.target
        log_success "Stopped pqcoin.target"
    fi
}

# Remove systemd units
remove_systemd_units() {
    log_info "Removing systemd units..."

    rm -f /etc/systemd/system/pqcoin@.service
    rm -f /etc/systemd/system/pqcoin.target

    systemctl daemon-reload
    log_success "Removed systemd units"
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

    if [[ -d /etc/pqcoin ]]; then
        rm -rf /etc/pqcoin
        log_success "Removed /etc/pqcoin"
    fi
}

# Remove data directories
remove_data() {
    log_info "Removing data directories..."

    if [[ -d /var/lib/pqcoin ]]; then
        rm -rf /var/lib/pqcoin
        log_success "Removed /var/lib/pqcoin"
    fi
}

# Remove system user
remove_user() {
    log_info "Removing pqcoin user..."

    if id "pqcoin" &>/dev/null; then
        userdel pqcoin 2>/dev/null || true
        log_success "Removed user 'pqcoin'"
    fi

    if getent group pqcoin &>/dev/null; then
        groupdel pqcoin 2>/dev/null || true
        log_success "Removed group 'pqcoin'"
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
                echo "  --keep-user   Don't remove the pqcoin system user"
                exit 0
                ;;
            *)
                log_error "Unknown argument: $arg"
                exit 1
                ;;
        esac
    done

    echo "=================================="
    echo "  pqcoin systemd uninstaller"
    echo "=================================="
    echo ""

    check_root

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
    remove_systemd_units
    remove_binaries

    if $purge; then
        remove_configs
        remove_data
    else
        log_info "Config and data preserved (use --purge to remove)"
        log_info "  /etc/pqcoin"
        log_info "  /var/lib/pqcoin"
    fi

    if ! $keep_user; then
        remove_user
    else
        log_info "User 'pqcoin' preserved (--keep-user specified)"
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
