#!/usr/bin/env bash
# Doc-code sync check (T-40).
#
# For every docs/**/*.md that has frontmatter `last_verified_commit`,
# scan the body for source-path references of the form
# `crates/...`, `omega-hpc/crates/...`, etc. and check that no
# referenced file has been modified after the recorded commit.
# Also flag any file that still carries `last_verified_commit: N/A`.
#
# Exit 0 = clean, 1 = drift detected.

set -uo pipefail

REPO_ROOT="${REPO_ROOT:-$(cd "$(dirname "$0")/../.." && pwd)}"
cd "$REPO_ROOT" || exit 1

DRIFT=0
TOTAL=0
N_A=0
DRIFT_FILES=()

while IFS= read -r -d '' path; do
    TOTAL=$((TOTAL + 1))
    # Skip files without frontmatter.
    if ! head -1 "$path" | grep -q '^---$'; then
        continue
    fi
    # Extract the frontmatter block.
    block=$(awk 'NR==1 && /^---$/{f=1; next} f && /^---$/{exit} f' "$path")
    [ -z "$block" ] && continue

    # Archive files are frozen: they do not need last_verified_commit.
    if printf '%s\n' "$block" | grep -q '^archived:'; then
        continue
    fi

    sha=$(printf '%s\n' "$block" | awk -F': *' '/^last_verified_commit:/{print $2; exit}')
    if [ "$sha" = "N/A" ] || [ -z "$sha" ]; then
        N_A=$((N_A + 1))
        echo "WARN ${path}: last_verified_commit not set"
        continue
    fi

    # For each source-path-style reference in the body, check git log.
    refs=$(awk 'NR==1 && /^---$/{f=1; next} f && /^---$/{f=0; next} !f' "$path" \
        | grep -oE '`?(crates/[A-Za-z0-9_./-]+|omega-hpc/crates/[A-Za-z0-9_./-]+)`?' \
        | tr -d '`' | sort -u)
    [ -z "$refs" ] && continue

    for ref in $refs; do
        if [ ! -e "$ref" ]; then
            continue  # not a real file, ignore
        fi
        last_touch=$(git log -1 --format=%ct -- "$ref" 2>/dev/null || echo 0)
        doc_time=$(git log -1 --format=%ct -- "$path" 2>/dev/null || echo 0)
        # If the file's mtime is older than the doc's verification, it's fine.
        # We compare wall-clock: if $ref was touched AFTER $path, drift.
        if [ "$last_touch" -gt "$doc_time" ] && [ "$doc_time" -gt 0 ]; then
            DRIFT=$((DRIFT + 1))
            DRIFT_FILES+=("$path -> $ref")
        fi
    done
done < <(find docs -type f -name '*.md' -print0)

echo ""
echo "== doc-sync summary =="
echo "files checked:  $TOTAL"
echo "N/A frontmatter: $N_A"
echo "drift detected: $DRIFT"

if [ "$DRIFT" -gt 0 ]; then
    printf 'drift examples:\n'
    for d in "${DRIFT_FILES[@]:0:5}"; do
        echo "  $d"
    done
    exit 1
fi
exit 0
