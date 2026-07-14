#!/usr/bin/env python3
"""Render docs/decisions/README.md from decisions.jsonl (T-43).

Replaces the hand-maintained ADR index table with a generated version
that derives every row from the canonical record set. The output is
sorted by ID and includes the status and date that records carry.

Run from the repo root. The output is a complete README.md, including
the frontmatter and the prose intro that readers expect before the
table.
"""
from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent.parent
RECORDS_PATH = REPO_ROOT / "docs-data" / "records" / "decisions.jsonl"
OUTPUT_PATH = REPO_ROOT / "docs" / "decisions" / "README.md"

INTRO = (
    "Auto-generated from `docs-data/records/decisions.jsonl` by "
    "`scripts/docs/render-decisions-index.py`. Hand-edits to this "
    "file are forbidden by AGENTS.md; modify the record set instead.\n"
)

BODY_TEMPLATE = """# Architecture Decision Records Index

## Body

| ID | Title | Status | Date |
|----|-------|--------|------|
{rows}
"""

FRONTMATTER = """---
content_revision: {rev}
generation_id: {gen}
last_verified_commit: {sha}
owner: omega-team
projection_version: {proj}
source_doc_id: "adr:docs-decisions-readme"
status: active
updated: {date}
---

"""


def main() -> int:
    ap = argparse.ArgumentDefaultsHelpFormatter
    ap = argparse.ArgumentParser(
        description="Render docs/decisions/README.md from decisions.jsonl",
        formatter_class=ap,
    )
    ap.add_argument("--dry-run", action="store_true",
                    help="Print the rendered content to stdout instead of writing")
    args = ap.parse_args()

    records = []
    with open(RECORDS_PATH) as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            try:
                rec = json.loads(line)
            except json.JSONDecodeError:
                continue
            if rec.get("doc_type") not in ("decision", "adr"):
                continue
            # Skip the self-reference README record so the table does not
            # list itself.
            if rec.get("doc_id", "").endswith("readme"):
                continue
            records.append(rec)
    records.sort(key=lambda r: r.get("doc_id", ""))

    rows = []
    for r in records:
        doc_id = r.get("doc_id", "?")
        # Strip the "adr:docs-decisions-NNN-" prefix to get the numeric ID.
        # The doc_id format is fixed by extract: adr:docs-decisions-NNN-<slug>.
        # We want the NNN-slug part for the table.
        short = doc_id.removeprefix("adr:docs-decisions-")
        rows.append(
            f"| {short} | {r.get('title', '?')} | {r.get('status', '?')} | "
            f"{r.get('updated', '?') or r.get('created', '?')} |"
        )
    body = BODY_TEMPLATE.format(rows="\n".join(rows))

    sha = ""
    try:
        import subprocess
        sha = subprocess.run(
            ["git", "log", "-1", "--format=%H", "--", str(OUTPUT_PATH.relative_to(REPO_ROOT))],
            cwd=REPO_ROOT,
            capture_output=True,
            text=True,
            check=False,
        ).stdout.strip()
    except Exception:
        pass

    import datetime
    today = datetime.date.today().isoformat()
    fm = FRONTMATTER.format(
        rev=0,
        gen=f"gen_000000_r000000",
        sha=sha or "N/A",
        proj=0,
        date=today,
    )

    out = fm + INTRO + "\n" + body

    if args.dry_run:
        sys.stdout.write(out)
        return 0

    OUTPUT_PATH.write_text(out)
    print(f"wrote {OUTPUT_PATH} with {len(rows)} rows")
    return 0


if __name__ == "__main__":
    sys.exit(main())
