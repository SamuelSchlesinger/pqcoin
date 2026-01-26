#!/bin/bash
# Create a new roadmap issue with proper template
# Usage: ./scripts/roadmap-new.sh "Issue Title" priority tag1,tag2

set -e
cd "$(dirname "$0")/.."

if [ -z "$1" ] || [ -z "$2" ] || [ -z "$3" ]; then
    echo "Usage: $0 \"Issue Title\" PRIORITY TAGS [DEPENDENCIES]"
    echo ""
    echo "Arguments:"
    echo "  TITLE        Issue title in quotes"
    echo "  PRIORITY     1-7 (1=highest)"
    echo "  TAGS         Comma-separated: security,network,wallet,mining,infra,docs,testing,performance"
    echo "  DEPENDENCIES Optional comma-separated issue IDs: 001,002"
    echo ""
    echo "Example:"
    echo "  $0 \"Add rate limiting to RPC\" 3 security,network 001,003"
    exit 1
fi

TITLE="$1"
PRIORITY="$2"
TAGS="$3"
DEPS="${4:-}"

# Validate priority
if ! echo "$PRIORITY" | grep -qE '^[1-7]$'; then
    echo "Error: Priority must be 1-7"
    exit 1
fi

# Find next issue number
LAST_NUM=$(ls roadmap/*.md 2>/dev/null | sed 's/.*\///' | cut -c1-3 | sort -n | tail -1)
if [ -z "$LAST_NUM" ]; then
    NEXT_NUM="001"
else
    LAST_NUM_CLEAN=$(echo "$LAST_NUM" | sed 's/^0*//')
    [ -z "$LAST_NUM_CLEAN" ] && LAST_NUM_CLEAN="0"
    NEXT_NUM=$(printf "%03d" $((LAST_NUM_CLEAN + 1)))
fi

# Generate filename
SLUG=$(echo "$TITLE" | tr '[:upper:]' '[:lower:]' | tr ' ' '-' | tr -cd 'a-z0-9-')
FILENAME="roadmap/${NEXT_NUM}-${SLUG}.md"

# Format tags
TAGS_FORMATTED=$(echo "$TAGS" | tr ',' ', ' | sed 's/^/[/' | sed 's/$/]/')

# Format dependencies
if [ -n "$DEPS" ]; then
    DEPS_FORMATTED=$(echo "$DEPS" | tr ',' ', ' | sed 's/^/[/' | sed 's/$/]/')
else
    DEPS_FORMATTED="[]"
fi

# Create file
cat > "$FILENAME" << EOF
---
title: "$TITLE"
priority: $PRIORITY
status: planned
tags: $TAGS_FORMATTED
dependencies: $DEPS_FORMATTED
---

# $TITLE

## Overview

[Describe the goal and context]

## Tasks

- [ ] Task 1
- [ ] Task 2
- [ ] Task 3

## Acceptance Criteria

- [ ] Criterion 1
- [ ] Criterion 2
EOF

echo "Created: $FILENAME"
echo ""
echo "Next steps:"
echo "  1. Edit $FILENAME to fill in details"
echo "  2. Add to ROADMAP.md in the Priority $PRIORITY section"
echo "  3. Run ./scripts/roadmap-validate.sh to verify"
