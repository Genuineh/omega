# Claude Code Project Guidance

This file provides the Claude Code entry-point version of the repository rules. Keep it aligned with `AGENTS.md`. If these files diverge, the project guidance is broken and should be reconciled immediately.

## Repository Guidelines

### Project Structure

This repository is documentation-first.

Required documentation layout:

- `docs/TODO.md`: current work and priority order.
- `docs/prds/`: plans, architecture, and design details.
- `docs/guide/`: usage guides and contributor workflows.
- `docs/specs/`: formal specs, contracts, and repository rules.
- `docs/whitepapers/`: long-form rationale and vision.
- `docs/decisions/`: durable architecture decisions when tradeoffs matter.
- `docs/archive/`: retired, superseded, or historical documents kept for traceability.
- `README.md`: top-level index and project entry point.
- `CHANGELOG.md`: notable milestone or release changes after implementation starts shipping.
- `LICENSE`: must stay at the repository root.

Planned code layout:

- `crates/`: Rust workspace crates.
- `skills/`: AI operating guides.

If any document moves, add or update links in the same change, including `README.md`.

All subdirectories under `docs/` should use lowercase names.

### Archive Rules

- Archive documents when they are superseded, completed and no longer active, structurally outdated, or kept only for history.
- Archive does not mean delete. Preserve the file, move it to `docs/archive/`, and keep the filename descriptive.
- Add a short note at the top of the archived file stating why it was archived, what replaced it, and the archive date.
- If a document is replaced, the active replacement must link back to the archived version when historical context matters.

When archiving a document, update related files in the same change:

- `README.md`: remove or relabel active links.
- `docs/TODO.md`: remove closed items or mark them as completed/replaced.
- `docs/prds/`: replace references to archived plans with the active plan.
- `docs/guide/`: remove obsolete usage paths and point to the current workflow.
- `docs/decisions/` or `CHANGELOG.md`: record the change when the archive reflects a meaningful project milestone or decision.

Archive timing:

- Immediately after a document is replaced by a newer canonical version.
- When a plan is finished and no longer drives active work.
- When a guide, draft, or note no longer matches the current repository structure.
- During major restructures, before stale docs can confuse active contributors.

### Development Workflow

Use these checks before and after edits:

- `sed -n '1,120p' docs/TODO.md`: review current priorities.
- `rg --files docs`: inspect tracked documentation.
- `git diff -- README.md docs/`: verify doc moves and link updates.

Once the Rust workspace exists, use:

- `cargo fmt --all`
- `cargo build`
- `cargo test`

Once the Tauri app exists, use:

- `npm run build`
- `npm run test`
- `npm run tauri dev`

Once the dotnet workspace exists, use:

- `dotnet format`
- `dotnet build`
- `dotnet test`

When a task is completed or meaningfully advanced, update `docs/TODO.md` in the same change. Mark progress explicitly so the TODO reflects current status, not just planned work.

### Skills

#### Rule

- At the start of every task, identify the task type and load the matching skill guidance before making substantive changes.
- Use `read_file` to load the relevant skill file from `.claude/skills/` when the task falls into one of the categories below.
- If multiple domains apply, load the small set of directly relevant skills instead of defaulting to one broad skill.
- If no exact skill matches, load the nearest general skill for that area and proceed with the repo rules in this file.

#### Skill Directory

- Coding:
  - `code-rule`: surgical coding principles (MANDATORY for all coding tasks)
- Documentation:
  - `docs-general`: documentation structure, archive rules, and doc hygiene
  - `docs-todo`: `docs/TODO.md` updates and task tracking
  - `docs-prds`: PRDs and feature planning docs
  - `docs-specs`: technical specifications and contracts
  - `docs-guide`: user and contributor guides
  - `docs-adr`: architecture decision records
- Backend:
  - `backend-principles`: backend architecture and service fundamentals
  - `backend-database`: schema, storage, and migration design
  - `backend-microservices`: service boundaries and communication patterns
  - `backend-queue`: async queue and messaging design
  - `backend-api`: API contract and endpoint design
  - `backend-cache`: caching strategy and invalidation patterns
  - `backend-rust`: Rust backend implementation guidance
- Frontend:
  - `frontend-general-principles`: general frontend engineering guidance
  - `frontend-tauri-native`: Tauri desktop behavior and native integration
  - `frontend-framework-nextjs`: Next.js and React framework work
  - `frontend-state-management`: Zustand and client state patterns
  - `frontend-styling-twind`: Twind or Tailwind styling patterns
  - `frontend-api-integration`: frontend data fetching and API integration
  - `frontend-design`: UI quality, interaction, and visual design
- Design:
  - `design-foundations`: aesthetic, mathematical proportion, and color-system guidance for software design
- Testing:
  - `tester`: general testing principles
  - `test-frontend-unit`: frontend unit and component tests
  - `test-e2e`: end-to-end test design and setup
- Review:
  - `review`: code review principles
- Architecture:
  - `architect`: architecture analysis and design patterns
- Development:
  - `developer`: development principles
- Project management:
  - `project-rust`: Rust workspace and project management
- Planning and orchestration:
  - `plan`: task decomposition and execution planning

#### Task Routing

- **ALL coding tasks must load `code-rule` FIRST, before any other skill.**
- Documentation tasks should load the matching `docs-*` skill first.
- Backend implementation or API changes should load the most specific `backend-*` skill plus `backend-principles` when architectural guidance matters.
- Frontend work should load the relevant `frontend-*` skill, and visual or UX-heavy tasks should also load `frontend-design`.
- Design-system, visual-language, or palette/proportion work should load `design-foundations`, optionally alongside `frontend-design` or `product-ux`.
- Frontend testing should load the matching `test-*` skill before writing or changing tests.
- Rust workspace or package-structure work should load `project-rust`.
- Architecture analysis or design tasks should load `architect`.
- General development tasks should load `developer`.
- Review tasks should load `review`.
- Planning and orchestration tasks should load `plan`.
- Testing tasks should load `tester` plus the relevant frontend/backend skill when applicable.
- Coding tasks must load `code-rule` plus the most specific domain skill.

### Style Rules

- Keep docs short, direct, and actionable.
- Use stable filenames and predictable structure.
- For Rust, use `rustfmt`, 4-space indentation, `snake_case` for functions/modules, and `PascalCase` for types.
- For Tauri, use `npm run build`, `npm run test`, and `npm run tauri dev`.
- Prefer explicit names such as `BlockContract`, `ExecutionResult`, and other contract-shaped identifiers.

### Design Principles

- Start with the smallest viable engineering slice, then iterate upward.
- Small scope is not an excuse for weak design. Every iteration must preserve or improve architecture.
- Do architecture analysis before coding: affected modules, shared type ownership, dependency direction, and failure/recovery paths.
- Prefer simpler structure over extra abstraction. Reduce special cases, keep contracts consistent, and keep the runtime thin.
- Make incremental changes that stay safe under existing tests.

### Testing Rules

- Testing is a core part of development, not a final cleanup step.
- Default workflow is red/green TDD:
  1. Write a failing test.
  2. Implement the smallest change to pass it.
  3. Refactor without changing behavior.
- New behavior and bug fixes should normally start with a failing test.
- Use tests as safety rails for iteration.
- When code exists, keep unit tests near source and integration tests in each crate’s `tests/` directory.
- Name tests by behavior, for example `validates_required_inputs`.

### Acceptance Standards

A change is not done unless it meets this baseline:

- Functionality is complete, with no known omissions or obvious errors.
- Tests cover the changed behavior, key failure cases, and regression-prone paths.
- Required documentation is complete, current, and usable by the next contributor.
- Existing behavior is rechecked; no avoidable regressions are introduced.
- Naming, contracts, boundaries, and architecture remain consistent.
- The result is a maintainable solution, not a temporary patch.

If quality is uncertain, raise the bar instead of relaxing acceptance.

### Commits and Pull Requests

- Use short, imperative, capitalized commit subjects, for example `Add blocks language whitepaper`.
- Keep each commit focused on one logical change.
- PRs should include: summary, affected paths, required doc updates, and follow-up work if anything is intentionally deferred.
- Include screenshots for UI changes.
- If priorities change, update `docs/TODO.md` in the same PR.

### Must Follow

- Follow the documentation structure and hygiene rules.
- Use skills for every task.
