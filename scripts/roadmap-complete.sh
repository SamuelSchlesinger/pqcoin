#!/bin/bash
# Mark an issue as completed
# Usage: ./scripts/roadmap-complete.sh 001

set -e
cd "$(dirname "$0")/.."

if [ -z "$1" ]; then
    echo "Usage: $0 ISSUE_ID"
    echo ""
    echo "Examples:"
    echo "  $0 001    # Complete issue 001"
    echo "  $0 4      # Complete issue 004"
    exit 1
fi

# Pad ID to 3 digits
pad_id() {
    local id="$1"
    id=$(echo "$id" | sed 's/^0*//')
    [ -z "$id" ] && id="0"
    printf "%03d" "$id"
}

ID=$(pad_id "$1")
FILE=""
for f in roadmap/${ID}-*.md; do
    if [ -f "$f" ]; then
        FILE="$f"
        break
    fi
done

if [ -z "$FILE" ]; then
    echo "Error: Issue $ID not found"
    exit 1
fi

TITLE=$(grep "^title:" "$FILE" | sed 's/title: "//' | sed 's/"$//')
STATUS=$(grep "^status:" "$FILE" | sed 's/status: //')

echo "Issue: $ID - $TITLE"
echo "Current status: $STATUS"
echo ""

if [ "$STATUS" = "completed" ]; then
    echo "Issue is already completed"
    exit 0
fi

if [ "$STATUS" = "planned" ]; then
    echo "Warning: Issue was never started (status: planned)"
    echo "Consider using ./scripts/roadmap-claim.sh first"
    read -p "Mark as completed anyway? [y/N] " confirm
    if [ "$confirm" != "y" ] && [ "$confirm" != "Y" ]; then
        exit 1
    fi
fi

# Update status
sed -i.bak 's/^status: planned$/status: completed/' "$FILE"
sed -i.bak 's/^status: in-progress$/status: completed/' "$FILE"
rm -f "${FILE}.bak"

echo "✅ Completed issue $ID"
echo ""

# Show what's now unblocked
echo "Checking for newly unblocked issues..."
./scripts/roadmap-status.sh 2>/dev/null | grep -A100 "AVAILABLE" | head -20
