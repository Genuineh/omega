#!/usr/bin/env python3
"""Build docs-data/relations/edges.jsonl from markdown link scan (T-42).

Scans docs/**/*.md for markdown links of the form `[text](path)`, plus
bare doc-id references like `adr:docs-decisions-001-crate-architecture`.
For each link, emits one JSONL row:

  {"from": "<doc-id>", "to": "<doc-id>", "link_text": "...", "link_path": "...",
   "kind": "markdown_link" | "doc_id_reference"}

Doc-id resolution: a markdown path under docs/ maps to a doc-id of the
form `<doc_type>:<source_path_basename>`. For example, the path
`docs/specs/omega-runtime-message-pipeline.md` is published as
`spec:docs-specs-omega-runtime-message-pipeline`. The mapping table
comes from the source_path field of records in
docs-data/records/{specs,decisions,prds,guides,whitepapers,archive}.jsonl.

This is the safe (a) option from the audit's open question: raw scan
only, no inferred edges, no schema changes.
"""
from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent.parent
DOCS_DIR = REPO_ROOT / "docs"
RECORDS_DIR = REPO_ROOT / "docs-data" / "records"
OUTPUT_PATH = REPO_ROOT / "docs-data" / "relations" / "edges.jsonl"

# Map: (source_path, doc_id) for every record we know about.
PATH_TO_DOC_ID: dict[str, str] = {}
# Map: (doc_id, source_path) — reverse.
DOC_ID_TO_PATH: dict[str, str] = {}

# Doc-id reference pattern: `adr:docs-decisions-001-crate-architecture` or
# `spec:docs-specs-omega-runtime-message-pipeline`.
DOC_ID_RE = re.compile(r"\b([a-z]+):(docs-[a-z]+(?:-[a-z0-9]+)*)\b")
# Markdown link pattern: `[text](path)`. We don't try to follow external
# URLs or absolute URLs.
MD_LINK_RE = re.compile(r"\[([^\]]*)\]\(([^)]+)\)")

# Frontmatter extraction: between the first two `---` lines.
FRONTMATTER_RE = re.compile(r"^---\n(.*?)\n---\n", re.DOTALL | re.MULTILINE)
# source_path field in frontmatter.
SOURCE_PATH_RE = re.compile(r"^source_path:\s*(.+)$", re.MULTILINE)


def load_records() -> None:
    for jsonl in sorted(RECORDS_DIR.glob("*.jsonl")):
        with open(jsonl) as f:
            for line in f:
                line = line.strip()
                if not line:
                    continue
                try:
                    rec = json.loads(line)
                except json.JSONDecodeError:
                    continue
                doc_id = rec.get("doc_id")
                sp = rec.get("source_path")
                if doc_id and sp:
                    PATH_TO_DOC_ID[sp] = doc_id
                    DOC_ID_TO_PATH[doc_id] = sp


def source_path_for(path: Path) -> str | None:
    """Resolve a markdown file path to its source_path frontmatter value."""
    if not path.exists() or not path.is_file():
        return None
    text = path.read_text()
    m = FRONTMATTER_RE.search(text)
    if not m:
        return None
    sp = SOURCE_PATH_RE.search(m.group(1))
    return sp.group(1).strip() if sp else None


def doc_id_for_path(path: Path) -> str | None:
    sp = source_path_for(path)
    if not sp:
        return None
    return PATH_TO_DOC_ID.get(sp)


def resolve_link_path(from_path: Path, link_path: str) -> str | None:
    """Resolve a markdown link target to a repo-relative source_path."""
    if link_path.startswith(("http://", "https://", "mailto:", "#")):
        return None
    # Drop any anchor.
    link_path = link_path.split("#", 1)[0]
    if not link_path:
        return None
    # Resolve relative to the source file.
    target = (from_path.parent / link_path).resolve()
    # Normalize to repo-relative.
    try:
        rel = target.relative_to(REPO_ROOT)
    except ValueError:
        return None
    # The link target may or may not have frontmatter; resolve via record
    # table first, then via frontmatter scan.
    rel_str = str(rel)
    if rel_str in PATH_TO_DOC_ID:
        return rel_str
    # Last-resort: re-read the target and pull its source_path.
    sp = source_path_for(target)
    return sp


def main() -> int:
    parser = argparse.ArgumentDefaultsHelpFormatter
    ap = argparse.ArgumentParser(
        description="Scan docs/**/*.md for cross-document links and "
                    "emit JSONL edges to docs-data/relations/edges.jsonl",
        formatter_class=parser,
    )
    ap.add_argument(
        "--dry-run",
        action="store_true",
        help="Read and report but do not write edges.jsonl",
    )
    args = ap.parse_args()

    load_records()
    print(f"loaded {len(PATH_TO_DOC_ID)} (path -> doc-id) mappings")

    edges: list[dict] = []
    seen: set[tuple[str, str, str]] = set()
    md_files = sorted(DOCS_DIR.rglob("*.md"))
    print(f"scanning {len(md_files)} markdown files")

    for md in md_files:
        from_doc_id = doc_id_for_path(md)
        if not from_doc_id:
            continue
        text = md.read_text()

        # Markdown links.
        for m in MD_LINK_RE.finditer(text):
            link_text = m.group(1).strip()
            link_path = m.group(2).strip()
            resolved = resolve_link_path(md, link_path)
            if not resolved:
                continue
            to_doc_id = PATH_TO_DOC_ID.get(resolved)
            if not to_doc_id or to_doc_id == from_doc_id:
                continue
            key = (from_doc_id, to_doc_id, resolved)
            if key in seen:
                continue
            seen.add(key)
            edges.append({
                "from": from_doc_id,
                "to": to_doc_id,
                "link_text": link_text,
                "link_path": resolved,
                "kind": "markdown_link",
            })

        # Doc-id references.
        for m in DOC_ID_RE.finditer(text):
            to_doc_id = m.group(0)
            if to_doc_id == from_doc_id:
                continue
            if to_doc_id not in DOC_ID_TO_PATH:
                continue
            key = (from_doc_id, to_doc_id, "(doc-id-ref)")
            if key in seen:
                continue
            seen.add(key)
            edges.append({
                "from": from_doc_id,
                "to": to_doc_id,
                "link_text": "",
                "link_path": DOC_ID_TO_PATH[to_doc_id],
                "kind": "doc_id_reference",
            })

    OUTPUT_PATH.parent.mkdir(parents=True, exist_ok=True)
    if args.dry_run:
        print(f"DRY {len(edges)} edges would be written to {OUTPUT_PATH}")
    else:
        with open(OUTPUT_PATH, "w") as f:
            for e in edges:
                f.write(json.dumps(e, ensure_ascii=False) + "\n")
        print(f"wrote {len(edges)} edges to {OUTPUT_PATH}")

    # Print a small summary so the operator sees what got connected.
    by_from: dict[str, int] = {}
    for e in edges:
        by_from[e["from"]] = by_from.get(e["from"], 0) + 1
    print("top-5 most-referenced-from docs:")
    for doc_id, count in sorted(by_from.items(), key=lambda kv: -kv[1])[:5]:
        print(f"  {count:>4}  {doc_id}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
