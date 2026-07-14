#!/usr/bin/env python3
"""Force-fill N/A last_verified_commit in records (T-41 follow-up).

For every record whose frontmatter.last_verified_commit is 'N/A',
look up the file's last commit SHA and write it to the record's
BTreeMap. This is the inverse of `sync-records-from-fm.py`: that
script copies file -> record when the file has a real SHA; this one
copies git -> record when the file has N/A but git has history.

This breaks the cascading-write cycle: previously T-41 updated the
file (N/A -> SHA), but the next render reverted it to N/A because
the record's BTreeMap still had N/A. With this script, the record is
fixed directly, and render preserves the SHA on subsequent passes.

Run from the repo root.
"""
from __future__ import annotations

import json
import re
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent.parent
RECORDS = REPO / "docs-data" / "records"

GENERATED_KEYS = {
    "status", "owner", "created", "updated", "version",
    "content_revision", "generation_id", "projection_version",
    "source_doc_id", "source_path",
}


def main() -> int:
    updated = 0
    for jsonl in sorted(RECORDS.glob("*.jsonl")):
        with open(jsonl) as f:
            records = [json.loads(line) for line in f if line.strip()]
        for rec in records:
            fm = rec.setdefault("frontmatter", {})
            cur = fm.get("last_verified_commit")
            if cur != "N/A":
                continue
            sp = rec.get("source_path", "")
            if not sp:
                continue
            full = REPO / sp
            if not full.is_file():
                continue
            sha = subprocess.run(
                ["git", "log", "-1", "--format=%H", "--", sp],
                cwd=REPO, capture_output=True, text=True,
            ).stdout.strip()
            if not sha:
                continue
            fm["last_verified_commit"] = sha
            updated += 1
        with open(jsonl, "w") as f:
            for r in records:
                f.write(json.dumps(r, ensure_ascii=False) + "\n")
    print(f"forced {updated} records from N/A to last-commit SHA")
    return 0


if __name__ == "__main__":
    sys.exit(main())
