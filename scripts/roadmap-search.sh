#!/bin/bash
# Search roadmap issues by various criteria
# Usage:
#   ./scripts/roadmap-search.sh --tag security
#   ./scripts/roadmap-search.sh --status planned
#   ./scripts/roadmap-search.sh --priority 1
#   ./scripts/roadmap-search.sh --available
#   ./scripts/roadmap-search.sh --blocked

set -e
cd "$(dirname "$0")/.."

usage() {
    echo "Usage: $0 [OPTIONS]"
    echo ""
    echo "Options:"
    echo "  --tag TAG        Filter by tag (security, network, wallet, mining, infra, docs, testing, performance)"
    echo "  --status STATUS  Filter by status (planned, in-progress, completed)"
    echo "  --priority N     Filter by priority (1-7)"
    echo "  --available      Show only issues ready to work on"
    echo "  --blocked        Show only blocked issues"
    echo "  --list-tags      List all unique tags"
    echo "  --help           Show this help"
    exit 0
}

# Convert dep ID to padded format
pad_id() {
    local id="$1"
    id=$(echo "$id" | sed 's/^0*//')
    [ -z "$id" ] && id="0"
    printf "%03d" "$id"
}

# Get status for an issue ID
get_status() {
    local id=$(pad_id "$1")
    for file in roadmap/${id}-*.md; do
        if [ -f "$file" ]; then
            grep "^status:" "$file" | sed 's/status: //'
            return
        fi
    done
}

# Check if all deps are satisfied
deps_satisfied() {
    local file="$1"
    local dep_line=$(grep "^dependencies:" "$file" | sed 's/dependencies: //' | tr -d '[]' | tr -d ' ')

    if [ -z "$dep_line" ]; then
        return 0
    fi

    for dep in $(echo "$dep_line" | tr ',' '\n'); do
        if [ -n "$dep" ]; then
            dep_status=$(get_status "$dep")
            if [ "$dep_status" != "completed" ]; then
                return 1
            fi
        fi
    done
    return 0
}

# List all tags
if [ "$1" = "--list-tags" ]; then
    echo "Available tags:"
    grep "^tags:" roadmap/*.md | sed 's/.*tags: \[//' | sed 's/\]//' | tr ',' '\n' | tr -d ' ' | sort -u | sed 's/^/  /'
    exit 0
fi

[ "$1" = "--help" ] || [ -z "$1" ] && usage

TAG=""
STATUS=""
PRIORITY=""
AVAILABLE_ONLY=false
BLOCKED_ONLY=false

while [ $# -gt 0 ]; do
    case "$1" in
        --tag) TAG="$2"; shift 2 ;;
        --status) STATUS="$2"; shift 2 ;;
        --priority) PRIORITY="$2"; shift 2 ;;
        --available) AVAILABLE_ONLY=true; shift ;;
        --blocked) BLOCKED_ONLY=true; shift ;;
        *) echo "Unknown option: $1"; usage ;;
    esac
done

echo "=== Roadmap Search Results ==="
[ -n "$TAG" ] && echo "Tag: $TAG"
[ -n "$STATUS" ] && echo "Status: $STATUS"
[ -n "$PRIORITY" ] && echo "Priority: $PRIORITY"
[ "$AVAILABLE_ONLY" = "true" ] && echo "Filter: Available only"
[ "$BLOCKED_ONLY" = "true" ] && echo "Filter: Blocked only"
echo ""

count=0
for f in roadmap/*.md; do
    id=$(basename "$f" .md | cut -c1-3)
    title=$(grep "^title:" "$f" | sed 's/title: "//' | sed 's/"$//')
    file_status=$(grep "^status:" "$f" | sed 's/status: //')
    file_priority=$(grep "^priority:" "$f" | sed 's/priority: //')
    file_tags=$(grep "^tags:" "$f" | sed 's/tags: //')

    # Apply filters
    if [ -n "$TAG" ] && ! echo "$file_tags" | grep -q "$TAG"; then
        continue
    fi

    if [ -n "$STATUS" ] && [ "$file_status" != "$STATUS" ]; then
        continue
    fi

    if [ -n "$PRIORITY" ] && [ "$file_priority" != "$PRIORITY" ]; then
        continue
    fi

    if [ "$AVAILABLE_ONLY" = "true" ]; then
        if [ "$file_status" != "planned" ] || ! deps_satisfied "$f"; then
            continue
        fi
    fi

    if [ "$BLOCKED_ONLY" = "true" ]; then
        if [ "$file_status" != "planned" ] || deps_satisfied "$f"; then
            continue
        fi
    fi

    # Show result
    case "$file_status" in
        completed) icon="✅" ;;
        in-progress) icon="🔄" ;;
        planned)
            if deps_satisfied "$f"; then
                icon="⬜"
            else
                icon="🚫"
            fi
            ;;
        *) icon="?" ;;
    esac

    echo "$icon $id: $title"
    echo "   Priority: $file_priority | Status: $file_status | Tags: $file_tags"
    echo ""
    count=$((count + 1))
done

echo "=== Found $count issue(s) ==="
