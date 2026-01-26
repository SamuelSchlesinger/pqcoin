#!/bin/bash
# Validate roadmap files for consistency and correctness
# Usage: ./scripts/roadmap-validate.sh
# Exit code: 0 if valid, 1 if errors found

set -e
cd "$(dirname "$0")/.."

errors=0
warnings=0

error() {
    echo "ERROR: $1"
    errors=$((errors + 1))
}

warn() {
    echo "WARNING: $1"
    warnings=$((warnings + 1))
}

echo "=== Validating Roadmap ==="
echo ""

# Check ROADMAP.md exists
if [ ! -f "ROADMAP.md" ]; then
    error "ROADMAP.md not found"
fi

# Check roadmap directory exists
if [ ! -d "roadmap" ]; then
    error "roadmap/ directory not found"
    echo ""
    echo "=== Validation Failed ==="
    exit 1
fi

# Validate each issue file
for f in roadmap/*.md; do
    filename=$(basename "$f")
    id=$(echo "$filename" | cut -c1-3)

    # Check filename format
    if ! echo "$filename" | grep -qE '^[0-9]{3}-[a-z0-9-]+\.md$'; then
        error "$filename: Invalid filename format (expected NNN-name.md)"
    fi

    # Check required YAML frontmatter fields
    if ! grep -q "^title:" "$f"; then
        error "$filename: Missing 'title' field"
    fi

    if ! grep -q "^priority:" "$f"; then
        error "$filename: Missing 'priority' field"
    fi

    if ! grep -q "^status:" "$f"; then
        error "$filename: Missing 'status' field"
    fi

    if ! grep -q "^tags:" "$f"; then
        error "$filename: Missing 'tags' field"
    fi

    if ! grep -q "^dependencies:" "$f"; then
        error "$filename: Missing 'dependencies' field"
    fi

    # Validate status value
    status=$(grep "^status:" "$f" | sed 's/status: //')
    case "$status" in
        planned|in-progress|completed) ;;
        *) error "$filename: Invalid status '$status' (must be planned, in-progress, or completed)" ;;
    esac

    # Validate priority value
    priority=$(grep "^priority:" "$f" | sed 's/priority: //')
    if ! echo "$priority" | grep -qE '^[1-7]$'; then
        error "$filename: Invalid priority '$priority' (must be 1-7)"
    fi

    # Validate dependencies reference existing issues
    dep_line=$(grep "^dependencies:" "$f" | sed 's/dependencies: //' | tr -d '[]' | tr -d ' ')
    if [ -n "$dep_line" ]; then
        for dep in $(echo "$dep_line" | tr ',' '\n'); do
            if [ -n "$dep" ]; then
                # Remove leading zeros for comparison
                dep_num=$(echo "$dep" | sed 's/^0*//')
                [ -z "$dep_num" ] && dep_num="0"
                dep_padded=$(printf "%03d" "$dep_num")

                found=false
                for dep_file in roadmap/${dep_padded}-*.md; do
                    if [ -f "$dep_file" ]; then
                        found=true
                        break
                    fi
                done

                if [ "$found" = "false" ]; then
                    error "$filename: Dependency '$dep_padded' does not exist"
                fi
            fi
        done
    fi
done

# Check for gaps in issue numbering
echo "Checking issue numbering..."
expected=1
for f in roadmap/*.md; do
    id=$(basename "$f" .md | cut -c1-3)
    id_num=$(echo "$id" | sed 's/^0*//')
    [ -z "$id_num" ] && id_num="0"

    if [ "$id_num" -ne "$expected" ]; then
        warn "Gap in issue numbering: expected $(printf "%03d" "$expected"), found $id"
    fi
    expected=$((id_num + 1))
done

# Check ROADMAP.md links
echo "Checking ROADMAP.md links..."
if [ -f "ROADMAP.md" ]; then
    for link in $(grep -oE 'roadmap/[0-9]{3}-[a-z0-9-]+\.md' ROADMAP.md | sort -u); do
        if [ ! -f "$link" ]; then
            error "ROADMAP.md: Broken link to $link"
        fi
    done

    # Check all roadmap files are linked
    for f in roadmap/*.md; do
        filename=$(basename "$f")
        if ! grep -q "roadmap/$filename" ROADMAP.md; then
            warn "$filename: Not linked in ROADMAP.md"
        fi
    done
fi

# Check for circular dependencies
echo "Checking for circular dependencies..."
# Simple check: no issue can depend on itself or higher-numbered issues
for f in roadmap/*.md; do
    id=$(basename "$f" .md | cut -c1-3)
    id_num=$(echo "$id" | sed 's/^0*//')
    [ -z "$id_num" ] && id_num="0"

    dep_line=$(grep "^dependencies:" "$f" | sed 's/dependencies: //' | tr -d '[]' | tr -d ' ')
    if [ -n "$dep_line" ]; then
        for dep in $(echo "$dep_line" | tr ',' '\n'); do
            if [ -n "$dep" ]; then
                dep_num=$(echo "$dep" | sed 's/^0*//')
                [ -z "$dep_num" ] && dep_num="0"

                if [ "$dep_num" -ge "$id_num" ]; then
                    error "$id: Depends on $dep (must depend on lower-numbered issues only)"
                fi
            fi
        done
    fi
done

echo ""
echo "=== Validation Summary ==="
echo "Errors:   $errors"
echo "Warnings: $warnings"

if [ $errors -gt 0 ]; then
    echo ""
    echo "Validation FAILED"
    exit 1
else
    echo ""
    echo "Validation PASSED"
    exit 0
fi
