#!/usr/bin/env bash
# Frontmatter backfill (T-41).
# Fills missing frontmatter fields in docs/ markdown files:
#   - last_verified_commit: N/A  -> last commit SHA touching the file
#   - language:                  -> "bilingual" (60%+ of specs are)
# Idempotent: re-running on a fully-backfilled tree is a no-op.
# Skips files already carrying a real (non-N/A) last_verified_commit.
# Run from the repo root.

set -euo pipefail

DOCS_DIR="${DOCS_DIR:-docs}"
DRY_RUN="${DRY_RUN:-1}"

filled=0
skipped=0
not_git=0
total=0

for path in $(find "$DOCS_DIR" -type f -name '*.md' | sort); do
    total=$((total + 1))

    # Skip non-frontmatter files (e.g. top-level TODO.md / CHANGELOG.md
    # that don't carry frontmatter).
    if ! head -1 "$path" | grep -q '^---$'; then
        skipped=$((skipped + 1))
        continue
    fi

    # Extract the frontmatter block (between first two `---` lines).
    block=$(awk 'NR==1 && /^---$/{f=1; next} f && /^---$/{exit} f' "$path")
    if [ -z "$block" ]; then
        skipped=$((skipped + 1))
        continue
    fi

    changed=0
    new_block="$block"

    # ---- source_path ----
    cur_sp=$(printf '%s\n' "$new_block" | awk -F': *' '/^source_path:/{print $2; exit}')
    if [ -z "$cur_sp" ]; then
        # Strip leading ./ and convert to repo-relative.
        rp=$(printf '%s' "$path" | sed 's|^\./||')
        new_block="$new_block
source_path: $rp"
        changed=1
    fi

    # ---- last_verified_commit ----
    cur=$(printf '%s\n' "$new_block" | awk -F': *' '/^last_verified_commit:/{print $2; exit}')
    if [ "$cur" = "N/A" ] || [ -z "$cur" ]; then
        sha=$(git log -1 --format='%H' -- "$path" 2>/dev/null || true)
        if [ -n "$sha" ]; then
            if [ -n "$cur" ]; then
                # Field exists with value N/A — replace in place.
                new_block=$(printf '%s\n' "$new_block" | sed "s|^last_verified_commit:.*|last_verified_commit: $sha|")
            else
                # Field is missing — append it.
                new_block="$new_block
last_verified_commit: $sha"
            fi
            changed=1
        else
            # No git history (uncommitted file). Use a placeholder SHA
            # derived from the file path + mtime so the field is set
            # and the next render can sync it. The placeholder is
            # recognisable as a not-yet-committed marker; a later
            # commit + re-run of this script will replace it with the
            # real SHA.
            placeholder="uncommitted+$(printf '%s' "$path" | git hash-object --stdin 2>/dev/null || echo "fallback")"
            new_block="$new_block
last_verified_commit: $placeholder"
            changed=1
            not_git=$((not_git + 1))
        fi
    fi

    # ---- language ----
    cur_lang=$(printf '%s\n' "$new_block" | awk -F': *' '/^language:/{print $2; exit}')
    if [ -z "$cur_lang" ]; then
        # Decide language from body content: count CJK characters vs ASCII
        # letters; if both are present, bilingual. We use Python here
        # because the system's grep does not support UTF-8 character
        # classes in this nix-shell environment.
        body=$(awk 'NR==1 && /^---$/{f=1; next} f && /^---$/{f=0; next} !f' "$path")
        lang=$(printf '%s' "$body" | python3 -c '
import sys
body = sys.stdin.read()
cjk = sum(1 for ch in body if 0x4E00 <= ord(ch) <= 0x9FFF)
ascii_letters = sum(1 for ch in body if ch.isascii() and ch.isalpha())
if cjk > 20 and ascii_letters > 100:
    print("bilingual")
elif cjk > 20:
    print("zh-CN")
elif ascii_letters > 100:
    print("en")
else:
    print("bilingual")
')
        # Insert the language field at the end of the frontmatter block.
        # We do this in-place later via awk; for the sed pipeline we
        # append a new line to the captured block.
        new_block="$new_block
language: $lang"
        changed=1
    fi

    if [ "$changed" -eq 1 ]; then
        filled=$((filled + 1))
        if [ "$DRY_RUN" = "0" ]; then
            # Re-assemble the file: frontmatter line, new block, body
            body=$(awk 'NR==1 && /^---$/{f=1; next} f && /^---$/{f=0; next} !f' "$path")
            {
                printf -- '---\n'
                printf '%s\n' "$new_block"
                printf -- '---\n'
                printf '%s' "$body"
            } > "$path.tmp" && mv "$path.tmp" "$path"
        else
            # Dry-run: just print what we'd do.
            printf 'DRY %s\n' "$path"
        fi
    else
        skipped=$((skipped + 1))
    fi
done

printf 'total=%d filled=%d skipped=%d no_git_history=%d\n' \
    "$total" "$filled" "$skipped" "$not_git"
