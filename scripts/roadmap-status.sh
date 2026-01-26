#!/bin/bash
# Show roadmap status and available issues
# Usage: ./scripts/roadmap-status.sh [--tag TAG]

set -e
cd "$(dirname "$0")/.."

TAG_FILTER=""
if [ "$1" = "--tag" ] && [ -n "$2" ]; then
    TAG_FILTER="$2"
    echo "=== pqcoin Roadmap Status (tag: $TAG_FILTER) ==="
else
    echo "=== pqcoin Roadmap Status ==="
fi
echo ""

# Convert dep ID to padded format (handles octal issue with leading zeros)
pad_id() {
    local id="$1"
    # Remove leading zeros to avoid octal interpretation, then repad
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

# Check if file matches tag filter
matches_tag() {
    local file="$1"
    if [ -z "$TAG_FILTER" ]; then
        return 0
    fi
    grep "^tags:" "$file" | grep -q "$TAG_FILTER"
}

# Show completed
echo "COMPLETED:"
completed=0
for f in roadmap/*.md; do
    if ! matches_tag "$f"; then continue; fi
    s=$(grep "^status:" "$f" | sed 's/status: //')
    if [ "$s" = "completed" ]; then
        id=$(basename "$f" .md | cut -c1-3)
        title=$(grep "^title:" "$f" | sed 's/title: "//' | sed 's/"$//')
        echo "  ✅ $id: $title"
        completed=$((completed + 1))
    fi
done
[ $completed -eq 0 ] && echo "  (none)"
echo ""

# Show in-progress
echo "IN PROGRESS:"
in_progress=0
for f in roadmap/*.md; do
    if ! matches_tag "$f"; then continue; fi
    s=$(grep "^status:" "$f" | sed 's/status: //')
    if [ "$s" = "in-progress" ]; then
        id=$(basename "$f" .md | cut -c1-3)
        title=$(grep "^title:" "$f" | sed 's/title: "//' | sed 's/"$//')
        echo "  🔄 $id: $title"
        in_progress=$((in_progress + 1))
    fi
done
[ $in_progress -eq 0 ] && echo "  (none)"
echo ""

# Show available (planned + deps satisfied)
echo "AVAILABLE (deps satisfied):"
available=0
for f in roadmap/*.md; do
    if ! matches_tag "$f"; then continue; fi
    s=$(grep "^status:" "$f" | sed 's/status: //')
    if [ "$s" = "planned" ]; then
        id=$(basename "$f" .md | cut -c1-3)
        title=$(grep "^title:" "$f" | sed 's/title: "//' | sed 's/"$//')
        dep_line=$(grep "^dependencies:" "$f" | sed 's/dependencies: //' | tr -d '[]' | tr -d ' ')

        blocked=false
        if [ -n "$dep_line" ]; then
            for dep in $(echo "$dep_line" | tr ',' '\n'); do
                if [ -n "$dep" ]; then
                    dep_status=$(get_status "$dep")
                    if [ "$dep_status" != "completed" ]; then
                        blocked=true
                        break
                    fi
                fi
            done
        fi

        if [ "$blocked" = "false" ]; then
            echo "  ⬜ $id: $title"
            available=$((available + 1))
        fi
    fi
done
[ $available -eq 0 ] && echo "  (none)"
echo ""

# Show blocked
echo "BLOCKED (waiting on dependencies):"
blocked_count=0
for f in roadmap/*.md; do
    if ! matches_tag "$f"; then continue; fi
    s=$(grep "^status:" "$f" | sed 's/status: //')
    if [ "$s" = "planned" ]; then
        id=$(basename "$f" .md | cut -c1-3)
        title=$(grep "^title:" "$f" | sed 's/title: "//' | sed 's/"$//')
        dep_line=$(grep "^dependencies:" "$f" | sed 's/dependencies: //' | tr -d '[]' | tr -d ' ')

        blocked=false
        missing=""
        if [ -n "$dep_line" ]; then
            for dep in $(echo "$dep_line" | tr ',' '\n'); do
                if [ -n "$dep" ]; then
                    dep_padded=$(pad_id "$dep")
                    dep_status=$(get_status "$dep")
                    if [ "$dep_status" != "completed" ]; then
                        blocked=true
                        missing="$missing $dep_padded"
                    fi
                fi
            done
        fi

        if [ "$blocked" = "true" ]; then
            echo "  🚫 $id: $title [needs:$missing]"
            blocked_count=$((blocked_count + 1))
        fi
    fi
done
[ $blocked_count -eq 0 ] && echo "  (none)"
echo ""

# Summary
if [ -z "$TAG_FILTER" ]; then
    total=$(ls roadmap/*.md | wc -l | tr -d ' ')
else
    total=$(grep -l "tags:.*$TAG_FILTER" roadmap/*.md 2>/dev/null | wc -l | tr -d ' ')
fi
echo "=== Summary ==="
echo "Completed:   $completed / $total"
echo "In Progress: $in_progress"
echo "Available:   $available"
echo "Blocked:     $blocked_count"
