---
content_revision: 117
generation_id: gen_000033_r000117
last_verified_commit: N/A
owner: omega-team
projection_version: 33
source_doc_id: "prd:docs-prds-readme"
status: active
version: v1.0
---

# Product Requirements Documents

## Overview

This directory is reserved for formal product requirement documents (PRDs) that define features, user needs, and product specifications.

## Current Status

This directory now contains the first formal PRD for a cross-cutting user-facing capability:

- `docs/prds/omega-project-plan-management.md` — project-scoped long-term planning distinct from runtime `todo`

Most day-to-day implementation tracking still lives in `docs/TODO.md`, but features that define durable product behavior should prefer a dedicated PRD + spec pair.

## When to Create a PRD

Consider creating a formal PRD when:

- A feature requires cross-team alignment on scope and acceptance criteria
- A significant user-facing capability needs documented requirements
- A complex feature has multiple implementation options that need product-level decision making
- You need to capture user research, problem statements, or design rationale

## PRD Template

When creating a new PRD, use this structure:

```markdown
---
status: draft
last_verified_commit: N/A
owner: [team or owner]
version: v1.0
related_issue: #[issue-number]
related_pr: #[pr-number]
---

# [Feature Name]

## Summary

Brief description of the feature.

## Problem

What problem does this solve?

## Users

Who are the target users?

## Requirements

### Must Have (P0)
- ...

### Should Have (P1)
- ...

### Nice to Have (P2)
- ...

## Design

[Link to relevant specs]

## Implementation Tasks

[Link to TODO.md items]

## Open Questions

- ...
```

## Related Documents

- [docs/TODO.md](../TODO.md) — Current task tracking and priorities
- [docs/prds/omega-project-plan-management.md](./omega-project-plan-management.md) — Built-in project plan management requirements
- [docs/specs/](../specs/) — Technical specifications
- [docs/guide/](../guide/) — Usage and contributor guides

---

### Change Log

- 2026-04-13: Added the first formal PRD, `omega-project-plan-management.md`, and updated this index to stop claiming the directory is empty.
- 2026-04-09: Created with guidance for future PRD creation
