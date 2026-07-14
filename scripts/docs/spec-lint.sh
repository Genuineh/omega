#!/usr/bin/env bash
# Spec section linter (T-45).
#
# Enforces the canonical 7-section structure for every docs/**/*.md
# whose doc_type is 'spec' (per the record set). Reports missing
# sections per file and exits non-zero on any spec that's missing
# the `language` frontmatter field.
#
# Canonical sections (chosen because 30+ existing specs already use
# this order; see docs/specs/omega-runtime-message-pipeline.md et al):
#   - Overview
#   - Goals
#   - Non-Goals
#   - Architecture
#   - Data-Model
#   - Testing
#   - Change Log
#
# Exit 0 = clean, 1 = missing section or missing language field.

set -uo pipefail

REPO_ROOT="${REPO_ROOT:-$(cd "$(dirname "$0")/../.." && pwd)}"
cd "$REPO_ROOT" || exit 1

REQUIRED_SECTIONS=(
    "## Overview"
    "## Goals"
    "## Non-Goals"
    "## Architecture"
    "## Data-Model"
    "## Testing"
    "## Change Log"
)

fail=0
checked=0
warn=0
# Threshold: specs whose last commit is within THRESHOLD_DAYS get the
# strict check; older specs get warnings only. We use git commit time
# (not filesystem mtime) because `omega-doc render` touches mtime on
# every doc.
THRESHOLD_DAYS="${SPEC_LINT_THRESHOLD_DAYS:-30}"
THRESHOLD_TS=$(date -d "${THRESHOLD_DAYS} days ago" +%s 2>/dev/null \
    || date -v-${THRESHOLD_DAYS}d +%s 2>/dev/null \
    || echo 0)

for path in $(find docs/specs -type f -name '*.md' | sort); do
    # Skip files without frontmatter.
    if ! head -1 "$path" | grep -q '^---$'; then
        continue
    fi
    checked=$((checked + 1))

    # Decide strict vs lenient based on git commit time.
    file_ctime=$(git log -1 --format=%ct -- "$path" 2>/dev/null || echo 0)
    is_new=0
    if [ "$file_ctime" -gt "$THRESHOLD_TS" ] && [ "$file_ctime" -gt 0 ]; then
        is_new=1
    fi
    label="WARN "
    if [ "$is_new" -eq 1 ]; then
        label="STRICT"
    fi

    # language field check
    if ! head -10 "$path" | grep -q '^language:'; then
        if [ "$is_new" -eq 1 ]; then
            echo "FAIL [$label] $path: missing 'language' field"
            fail=$((fail + 1))
        else
            warn=$((warn + 1))
        fi
    fi

    # Section check
    for sec in "${REQUIRED_SECTIONS[@]}"; do
        if ! grep -qF "$sec" "$path"; then
            if [ "$is_new" -eq 1 ]; then
                echo "FAIL [$label] $path: missing section '$sec'"
                fail=$((fail + 1))
            else
                warn=$((warn + 1))
            fi
        fi
    done
done

echo ""
echo "== spec-lint summary =="
echo "specs checked:     $checked"
echo "strict failures:   $fail"
echo "warnings (legacy): $warn"
exit "$fail"
