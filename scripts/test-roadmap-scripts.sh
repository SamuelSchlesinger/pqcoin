#!/bin/bash
# Test suite for roadmap scripts
# Usage: ./scripts/test-roadmap-scripts.sh

set -e
cd "$(dirname "$0")/.."

TESTS_PASSED=0
TESTS_FAILED=0
TEMP_DIR=""

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
NC='\033[0m' # No Color

pass() {
    echo -e "${GREEN}PASS${NC}: $1"
    TESTS_PASSED=$((TESTS_PASSED + 1))
}

fail() {
    echo -e "${RED}FAIL${NC}: $1"
    TESTS_FAILED=$((TESTS_FAILED + 1))
}

setup() {
    TEMP_DIR=$(mktemp -d)
    # Backup current roadmap state
    cp -r roadmap "$TEMP_DIR/roadmap_backup"
    cp ROADMAP.md "$TEMP_DIR/ROADMAP_backup.md"
}

teardown() {
    # Restore roadmap state
    if [ -n "$TEMP_DIR" ] && [ -d "$TEMP_DIR" ]; then
        rm -rf roadmap
        mv "$TEMP_DIR/roadmap_backup" roadmap
        mv "$TEMP_DIR/ROADMAP_backup.md" ROADMAP.md
        rm -rf "$TEMP_DIR"
    fi
}

# Ensure cleanup on exit
trap teardown EXIT

echo "=== Testing Roadmap Scripts ==="
echo ""

# Setup
setup

# Test 1: roadmap-validate.sh passes on valid roadmap
echo "Test 1: roadmap-validate.sh on valid roadmap"
if ./scripts/roadmap-validate.sh > /dev/null 2>&1; then
    pass "Validation passes on valid roadmap"
else
    fail "Validation failed unexpectedly"
fi

# Test 2: roadmap-status.sh runs without error
echo "Test 2: roadmap-status.sh runs"
if ./scripts/roadmap-status.sh > /dev/null 2>&1; then
    pass "Status script runs"
else
    fail "Status script failed"
fi

# Test 3: roadmap-search.sh --list-tags works
echo "Test 3: roadmap-search.sh --list-tags"
if ./scripts/roadmap-search.sh --list-tags | grep -q "security"; then
    pass "Search lists tags including 'security'"
else
    fail "Search --list-tags failed"
fi

# Test 4: roadmap-search.sh --tag filters correctly
echo "Test 4: roadmap-search.sh --tag security"
SECURITY_COUNT=$(./scripts/roadmap-search.sh --tag security 2>/dev/null | grep -c "^[⬜🚫✅🔄]" || echo "0")
if [ "$SECURITY_COUNT" -gt 0 ]; then
    pass "Search --tag finds security issues ($SECURITY_COUNT found)"
else
    fail "Search --tag found no security issues"
fi

# Test 5: roadmap-search.sh --available shows only unblocked
echo "Test 5: roadmap-search.sh --available shows unblocked issues"
if ./scripts/roadmap-search.sh --available 2>/dev/null | grep -q "🚫"; then
    fail "Available search shows blocked issues"
else
    pass "Available search excludes blocked issues"
fi

# Test 6: roadmap-claim.sh blocks claiming blocked issue
echo "Test 6: roadmap-claim.sh refuses blocked issue"
# Issue 005 depends on 004 which is planned, so 005 should be blocked
if ./scripts/roadmap-claim.sh 005 2>&1 | grep -q "blocked\|Cannot claim"; then
    pass "Claim refuses blocked issue 005"
else
    fail "Claim allowed blocked issue 005"
fi

# Test 7: roadmap-claim.sh allows claiming available issue
echo "Test 7: roadmap-claim.sh claims available issue"
if ./scripts/roadmap-claim.sh 001 > /dev/null 2>&1; then
    # Check status was updated
    if grep -q "status: in-progress" roadmap/001-*.md; then
        pass "Claim updates status to in-progress"
    else
        fail "Claim did not update status"
    fi
else
    fail "Claim failed on available issue"
fi

# Test 8: roadmap-claim.sh refuses already claimed issue
echo "Test 8: roadmap-claim.sh refuses already claimed issue"
if ./scripts/roadmap-claim.sh 001 2>&1 | grep -q "already\|Warning"; then
    pass "Claim refuses already in-progress issue"
else
    fail "Claim allowed claiming in-progress issue again"
fi

# Test 9: roadmap-complete.sh marks issue completed
echo "Test 9: roadmap-complete.sh completes issue"
if ./scripts/roadmap-complete.sh 001 > /dev/null 2>&1; then
    if grep -q "status: completed" roadmap/001-*.md; then
        pass "Complete updates status to completed"
    else
        fail "Complete did not update status"
    fi
else
    fail "Complete script failed"
fi

# Test 10: roadmap-new.sh creates valid issue file
echo "Test 10: roadmap-new.sh creates issue"
./scripts/roadmap-new.sh "Test Issue" 3 "testing" > /dev/null 2>&1
NEW_FILE=$(ls roadmap/022-*.md 2>/dev/null | head -1)
if [ -f "$NEW_FILE" ]; then
    if grep -q "title: \"Test Issue\"" "$NEW_FILE" && grep -q "priority: 3" "$NEW_FILE"; then
        pass "New issue created with correct metadata"
    else
        fail "New issue has incorrect metadata"
    fi
    rm -f "$NEW_FILE"  # Clean up
else
    fail "New issue file not created"
fi

# Test 11: roadmap-validate.sh catches invalid status
echo "Test 11: roadmap-validate.sh catches invalid status"
# Temporarily break a file
sed -i.bak 's/status: planned/status: invalid/' roadmap/002-*.md
if ./scripts/roadmap-validate.sh 2>&1 | grep -q "Invalid status"; then
    pass "Validation catches invalid status"
else
    fail "Validation missed invalid status"
fi
mv roadmap/002-*.md.bak roadmap/002-fuzzing-tests.md

# Test 12: roadmap-validate.sh catches missing dependency
echo "Test 12: roadmap-validate.sh catches invalid dependency"
# Temporarily add invalid dependency
sed -i.bak 's/dependencies: \[\]/dependencies: [999]/' roadmap/001-*.md
if ./scripts/roadmap-validate.sh 2>&1 | grep -q "does not exist"; then
    pass "Validation catches invalid dependency"
else
    fail "Validation missed invalid dependency"
fi
mv roadmap/001-*.md.bak roadmap/001-security-audit-prep.md

# Test 13: roadmap-status.sh --tag filters output
echo "Test 13: roadmap-status.sh --tag filters"
WALLET_OUTPUT=$(./scripts/roadmap-status.sh --tag wallet 2>&1)
if echo "$WALLET_OUTPUT" | grep -q "Multisig\|HD Key\|Watch-Only"; then
    if echo "$WALLET_OUTPUT" | grep -q "Fuzzing"; then
        fail "Tag filter includes non-wallet issues"
    else
        pass "Status --tag filters correctly"
    fi
else
    fail "Status --tag missing wallet issues"
fi

# Test 14: roadmap-start.sh displays guidelines
echo "Test 14: roadmap-start.sh displays guidelines"
if ./scripts/roadmap-start.sh 2>&1 | grep -q "BEFORE YOU START"; then
    pass "Start script shows guidelines"
else
    fail "Start script missing guidelines"
fi

echo ""
echo "=== Test Summary ==="
echo -e "Passed: ${GREEN}$TESTS_PASSED${NC}"
echo -e "Failed: ${RED}$TESTS_FAILED${NC}"

if [ $TESTS_FAILED -gt 0 ]; then
    exit 1
else
    exit 0
fi
