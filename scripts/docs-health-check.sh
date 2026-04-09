#!/bin/bash
# Documentation Health Check Script
# Validates docs/ structure, frontmatter, and consistency

set -e

DOCS_DIR="docs"
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo "=== Omega Documentation Health Check ==="
echo ""

# Check required directories exist
echo "Checking directory structure..."
REQUIRED_DIRS=("prds" "guide" "specs" "whitepapers" "decisions" "archive")
for dir in "${REQUIRED_DIRS[@]}"; do
    if [ -d "$DOCS_DIR/$dir" ]; then
        echo -e "${GREEN}✓${NC} $DOCS_DIR/$dir exists"
    else
        echo -e "${RED}✗${NC} $DOCS_DIR/$dir is MISSING"
    fi
done
echo ""

# Check frontmatter in markdown files
echo "Checking frontmatter compliance..."
ERRORS=0
for f in $(find "$DOCS_DIR" -name "*.md" -type f); do
    if grep -q "^---" "$f"; then
        # Has frontmatter, check required fields
        if ! grep -q "^status:" "$f"; then
            echo -e "${RED}✗${NC} Missing status in: $f"
            ERRORS=$((ERRORS + 1))
        fi
        if ! grep -q "^owner:" "$f"; then
            echo -e "${YELLOW}⚠${NC} Missing owner in: $f"
        fi
        if ! grep -q "^last_verified_commit:" "$f"; then
            echo -e "${YELLOW}⚠${NC} Missing last_verified_commit in: $f"
        fi
    fi
done

if [ $ERRORS -eq 0 ]; then
    echo -e "${GREEN}✓${NC} All markdown files have required frontmatter"
else
    echo -e "${RED}✗${NC} $ERRORS files missing required frontmatter"
fi
echo ""

# Check for invalid status values
echo "Checking status values..."
for f in $(find "$DOCS_DIR" -name "*.md" -type f); do
    if grep -q "^status:" "$f"; then
        STATUS=$(grep "^status:" "$f" | head -1 | sed 's/status: *//')
        VALID_STATUSES=("draft" "active" "implemented" "deprecated" "superseded" "archived" "accepted")
        VALID=false
        for valid in "${VALID_STATUSES[@]}"; do
            if [ "$STATUS" = "$valid" ]; then
                VALID=true
                break
            fi
        done
        if [ "$VALID" = false ]; then
            echo -e "${RED}✗${NC} Invalid status '$STATUS' in: $f"
        fi
    fi
done
echo ""

# Check archive documents have proper archive frontmatter
echo "Checking archive documents..."
for f in $(find "$DOCS_DIR/archive" -name "*.md" -type f 2>/dev/null); do
    if ! grep -q "^archived:" "$f"; then
        echo -e "${YELLOW}⚠${NC} Archive doc missing 'archived:' field: $f"
    fi
done
echo ""

# Check decisions README has ADR index
echo "Checking ADR index..."
if [ -f "$DOCS_DIR/decisions/README.md" ]; then
    if grep -q "ID.*Title.*Status" "$DOCS_DIR/decisions/README.md"; then
        echo -e "${GREEN}✓${NC} ADR index table found"
    else
        echo -e "${YELLOW}⚠${NC} ADR index table not found in decisions/README.md"
    fi
fi
echo ""

# Count documents by type
echo "Document counts:"
echo "  Specs: $(find "$DOCS_DIR/specs" -name "*.md" | wc -l)"
echo "  Guides: $(find "$DOCS_DIR/guide" -name "*.md" | wc -l)"
echo "  Decisions: $(find "$DOCS_DIR/decisions" -name "*.md" | wc -l)"
echo "  Archive: $(find "$DOCS_DIR/archive" -name "*.md" | wc -l)"
echo "  PRDs: $(find "$DOCS_DIR/prds" -name "*.md" | wc -l)"
echo "  Whitepapers: $(find "$DOCS_DIR/whitepapers" -name "*.md" | wc -l)"
echo ""

echo "=== Health Check Complete ==="
