#!/usr/bin/env python3
"""Augment docs/TODO.md's Current Baseline section (T-44).

Reads decisions.jsonl + specs.jsonl + CHANGELOG.md, generates
"X baseline is complete" lines for each ADR / major spec /
changelog milestone, and APPENDS them below the existing
hand-curated entries (preserves the existing prose intro).

Run from the repo root.
"""
from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent.parent
DECISIONS_RECORDS = REPO_ROOT / "docs-data" / "records" / "decisions.jsonl"
SPECS_RECORDS = REPO_ROOT / "docs-data" / "records" / "specs.jsonl"
TODO_PATH = REPO_ROOT / "docs" / "TODO.md"
CHANGELOG_PATH = REPO_ROOT / "CHANGELOG.md"

MARKER_START = "<!-- BEGIN AUTO-GENERATED BASELINE: T-44 -->"
MARKER_END = "<!-- END AUTO-GENERATED BASELINE: T-44 -->"


def collect_adr_baselines() -> list[str]:
    """For each ADR, produce a one-line baseline entry summarising the
    extraction milestone."""
    rows: list[str] = []
    if not DECISIONS_RECORDS.exists():
        return rows
    with open(DECISIONS_RECORDS) as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            rec = json.loads(line)
            if rec.get("doc_type") not in ("decision", "adr"):
                continue
            doc_id = rec.get("doc_id", "")
            if doc_id.endswith("readme"):
                continue
            short = doc_id.removeprefix("adr:docs-decisions-")
            status = rec.get("status", "?")
            if status != "accepted":
                continue
            title = rec.get("title", "?")
            related = rec.get("relations", [])
            related_links = ", ".join(
                f"`{r.get('target', r)}`" for r in related
            ) if related else ""
            rows.append(
                f"- **{short}: {title}** ({status}). 详见 `{doc_id}`"
                + (f"; 相关: {related_links}" if related_links else "")
            )
    return rows


def collect_changelog_baselines() -> list[str]:
    """Walk CHANGELOG.md and emit one baseline per Added/Changed entry."""
    if not CHANGELOG_PATH.exists():
        return []
    rows: list[str] = []
    text = CHANGELOG_PATH.read_text()
    # Each bullet is `- <description>`; pick out the meaningful ones.
    in_added = False
    for line in text.splitlines():
        s = line.strip()
        if s.startswith("### "):
            in_added = s in ("### Added", "### Changed", "### Removed")
            continue
        if in_added and s.startswith("- "):
            rows.append(f"- {s[2:]}")
    return rows


def main() -> int:
    ap = argparse.ArgumentDefaultsHelpFormatter
    ap = argparse.ArgumentParser(
        description="Augment docs/TODO.md's Current Baseline section",
        formatter_class=ap,
    )
    ap.add_argument("--dry-run", action="store_true",
                    help="Print to stdout instead of writing")
    args = ap.parse_args()

    adr_rows = collect_adr_baselines()
    cl_rows = collect_changelog_baselines()

    new_block = "\n".join([MARKER_START] + adr_rows + cl_rows + [MARKER_END])

    todo = TODO_PATH.read_text()
    if MARKER_START in todo:
        # Replace existing auto-generated block.
        todo = re.sub(
            re.escape(MARKER_START) + r".*?" + re.escape(MARKER_END),
            new_block,
            todo,
            flags=re.DOTALL,
        )
    else:
        # Append after the last baseline entry, before "## Active Tasks".
        todo = todo.replace("\n## Active Tasks\n", "\n" + new_block + "\n\n## Active Tasks\n", 1)

    if args.dry_run:
        sys.stdout.write(todo)
        return 0

    TODO_PATH.write_text(todo)
    print(f"wrote {TODO_PATH} with {len(adr_rows)} ADR rows + {len(cl_rows)} changelog rows")
    return 0


if __name__ == "__main__":
    sys.exit(main())
