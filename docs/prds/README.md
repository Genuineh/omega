---
status: active
last_verified_commit: N/A
owner: omega-team
version: v1.0
---

# Product Requirements Documents

This directory is reserved for formal product requirement documents (PRDs) that define features, user needs, and product specifications.

## Current Status

**This directory is currently empty.** Omega's product requirements are currently tracked in `docs/TODO.md`, which serves as a hybrid:
- Task prioritization and tracking
- Implementation history
- Follow-up items and maintenance notes

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
- [docs/specs/](../specs/) — Technical specifications
- [docs/guide/](../guide/) — Usage and contributor guides

---

### Change Log

- 2026-04-09: Created with guidance for future PRD creation
