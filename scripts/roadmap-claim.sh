#!/bin/bash
# Claim an issue and mark it as in-progress
# Usage: ./scripts/roadmap-claim.sh 001

set -e
cd "$(dirname "$0")/.."

if [ -z "$1" ]; then
    echo "Usage: $0 ISSUE_ID"
    echo ""
    echo "Examples:"
    echo "  $0 001    # Claim issue 001"
    echo "  $0 4      # Claim issue 004"
    exit 1
fi

# Pad ID to 3 digits
pad_id() {
    local id="$1"
    id=$(echo "$id" | sed 's/^0*//')
    [ -z "$id" ] && id="0"
    printf "%03d" "$id"
}

get_status() {
    local id=$(pad_id "$1")
    for file in roadmap/${id}-*.md; do
        if [ -f "$file" ]; then
            grep "^status:" "$file" | sed 's/status: //'
            return
        fi
    done
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

# Check if already claimed
if [ "$STATUS" = "in-progress" ]; then
    echo "Warning: This issue is already in-progress"
    exit 1
fi

if [ "$STATUS" = "completed" ]; then
    echo "Error: This issue is already completed"
    exit 1
fi

# Check dependencies
dep_line=$(grep "^dependencies:" "$FILE" | sed 's/dependencies: //' | tr -d '[]' | tr -d ' ')
if [ -n "$dep_line" ]; then
    missing=""
    for dep in $(echo "$dep_line" | tr ',' '\n'); do
        if [ -n "$dep" ]; then
            dep_padded=$(pad_id "$dep")
            dep_status=$(get_status "$dep")
            if [ "$dep_status" != "completed" ]; then
                missing="$missing $dep_padded"
            fi
        fi
    done

    if [ -n "$missing" ]; then
        echo "Error: Cannot claim - blocked by incomplete dependencies:$missing"
        exit 1
    fi
fi

# Update status
sed -i.bak 's/^status: planned$/status: in-progress/' "$FILE"
rm -f "${FILE}.bak"

echo "✅ Claimed issue $ID"
echo "Status updated to: in-progress"
echo ""
echo "Next steps:"
echo "  1. Read the full issue: cat $FILE"
echo "  2. Implement the changes"
echo "  3. When done: ./scripts/roadmap-complete.sh $ID"
