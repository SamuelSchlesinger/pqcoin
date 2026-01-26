#!/bin/bash
# Run this when starting work on pqcoin
# Usage: ./scripts/roadmap-start.sh

set -e
cd "$(dirname "$0")/.."

cat << 'EOF'
╔══════════════════════════════════════════════════════════════════════╗
║                     pqcoin Development Guidelines                      ║
╠══════════════════════════════════════════════════════════════════════╣
║                                                                        ║
║  BEFORE YOU START:                                                     ║
║                                                                        ║
║  1. Read CLAUDE.md for project conventions                             ║
║  2. Run ./scripts/roadmap-status.sh to see available work              ║
║  3. Use ./scripts/roadmap-claim.sh NNN before starting an issue        ║
║  4. Use ./scripts/roadmap-complete.sh NNN when done                    ║
║                                                                        ║
║  KEY RULES:                                                            ║
║                                                                        ║
║  • Never start a blocked issue (🚫) - check dependencies first         ║
║  • No backwards compatibility between network phases                   ║
║  • No unwrap() on network/user input - always handle errors           ║
║  • Run cargo test before committing                                    ║
║  • Update roadmap status as you work                                   ║
║                                                                        ║
╚══════════════════════════════════════════════════════════════════════╝

EOF

echo "=== Current Roadmap Status ==="
echo ""
./scripts/roadmap-status.sh

echo ""
echo "To claim an issue: ./scripts/roadmap-claim.sh <ID>"
echo "Example: ./scripts/roadmap-claim.sh 001"
