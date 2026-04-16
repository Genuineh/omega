---
content_revision: 101
created: 2026-04-14
generation_id: gen_000016_r000101
last_verified_commit: N/A
owner: omega-team
projection_version: 16
related_prds:
  - docs/specs/omega-project-plan-system.md
  - docs/specs/omega-structured-document-system.md
  - docs/specs/omega-command-system.md
source_doc_id: "spec:docs-specs-omega-project-plan-docs-data-convergence"
status: active
supersedes: []
updated: 2026-04-15
---

# Omega Project Plan Docs-Data Convergence Specification

## Overview

当前仓库已完成 project planning canonical persistence 收口：

- `/plan` 与 `omega-plan` 现在以 `docs-data/tasks/` 为 canonical store
- structured docs v2 继续以 `docs-data/` 为 canonical knowledge base，并在同一 base directory 下维护文档任务 ledger

这意味着 project plan、doc task 和 `docs/TODO.md` projection 现在共享同一个 canonical base directory。此前 `.omega/plans/` 与 `docs-data/` 的 repo-local dual storage 已被收敛，`omega-plan` 继续拥有 project planning 的领域逻辑和 API，但其 repo-local source of truth 不再停留在 `.omega/plans/`。

本规格记录这次 convergence 的最终状态：`/plan` 的 canonical persistence 已迁入 `docs-data/`，剩余的 `.omega/plans/` compatibility/import layer 也已从 runtime、layout 与文档合同中移除。

## Goals

- 把 project plan、doc task 和 `docs/TODO.md` projection 收口到同一个 repo knowledge base，也就是 `docs-data/`。
- 让 `/plan` 完全以 `docs-data/` 为 canonical persistence，而不是继续跨 `.omega/plans/` 与 `docs-data/` 双写或双读。
- 保留 `omega-plan` 作为 task graph / query / mutation 的领域 owner，不把 planning 逻辑塞回 `omega-document`。
- 为 repo-external automation 和 docs/project unified validation 提供单一 base directory。

## Non-Goals

- 不把 runtime `todo` 并入 docs-data。
- 不要求 Phase 1 就把所有 planning 和 docs records 合并到单个 JSONL 文件。
- 不要求删除 `omega-plan` crate；变化的是 canonical persistence，而不是 domain ownership。
- 不要求在本轮改造里重新设计 `/plan` command family 的用户交互。

## Desired End State

```text
<project-root>/
  docs-data/
    manifest.json
    records/
      ...
    tasks/
      project-plan.toml
      project-tasks.jsonl
      doc-tasks.jsonl        # docs-facing compatibility/projection surface
      logs/
        TASK-0001.jsonl
        DOC-0019G.jsonl
    relations/
      links.jsonl
      task-links.jsonl
    render/
      render-state.json
  .omega-state/
    plans/                   # optional cache only
```

规则：

- `/plan` 的 canonical mutation target 必须落在 `docs-data/`。
- `.omega/plans/` 已不再出现在 runtime 或 layout 合同中；repo-local project plan persistence 只保留 `docs-data/tasks/*`。
- `docs/TODO.md` 继续作为 presentation projection，但它的 open-work source 必须来自 docs-data-backed plan graph。
- 文档任务与一般 project task 至少要共享同一个 base directory 和兼容的 task graph contract。

## Architecture

### Ownership

| Layer | Responsibility |
|------|----------------|
| `omega-plan` | task graph、query/mutation API、dependency validation、selected task context、task logs |
| `omega-document` | docs-data layout helpers、TODO/doc projection、render/validate/extract |
| `omega-project-layout` | `docs-data/` 下的 planning paths 与 manifest constants |
| `omega-session` / `omega-app` | `/plan` handlers、selected task wiring、delivery log write-back |

### Core Decision

`omega-plan` 继续拥有 planning 的业务逻辑，但 canonical persistence 改成 docs-data-backed store。

也就是说，变化是：

- **不再**：`omega-plan` = `.omega/plans/` store
- **改为**：`omega-plan` = planning domain API over `docs-data/`

这可以保持领域边界清晰，同时消除 repo-local dual storage。

## Storage Direction

### Implemented Convergence Baseline

本轮已完成以下收口：

1. `/plan` 的 canonical task records 已迁入 `docs-data/tasks/project-tasks.jsonl`
2. task logs 已迁入 `docs-data/tasks/logs/`
3. `docs-data/tasks/project-plan.toml` 与 `docs-data/manifest.json` 现在共同描述 planning store contract
4. 剩余的 `.omega/plans/` compatibility/import path 已移除，runtime 与 layout 不再读取该路径

### `doc-tasks.jsonl` Compatibility Boundary

当前 `docs-data/tasks/doc-tasks.jsonl` 继续作为 docs-facing ledger 存在，而 `docs-data/tasks/project-tasks.jsonl` 拥有一般 project plan graph。两者已经共享同一个 `docs-data/tasks/` base directory，不再跨 `.omega/plans/` 与 `docs-data/` 双存储。

## Rollout Tasks

| Task | Scope | Outcome |
|------|-------|---------|
| `Task 4H` | 定义 docs-data-backed plan store contract | Completed; `docs-data/tasks/` 下的 project task、task log、manifest path 与 compatibility boundary 已冻结 |
| `Task 4I` | 迁移 `ProjectPlanStore` 与 `/plan` runtime 到 docs-data | Completed; `/plan` 不再直接读写 `.omega/plans/` |
| `Task 4J` | 统一 TODO projection 与 doc task convergence | Completed; `docs/TODO.md`、doc task 视图和 docs-facing export 由同一 plan graph 派生 |
| `Task 4K` | `.omega/plans` migration and cutover | Completed; canonical write target cutover 已落地，且剩余 `.omega/plans/` compatibility/import layer 已移除 |

## Acceptance Standards

- `/plan list/show/create/update/select/send/sync-todo/load` 现已全部以 docs-data-backed store 为 canonical source。
- `docs/TODO.md` 的 open-work projection 不再需要跨 `.omega/plans/` 与 `docs-data/` 双解释。
- runtime 与 layout 已不再读取 `.omega/plans/`；project plan persistence 只通过 `docs-data/tasks/*` 暴露。
- `.omega/plans/` 不再被文档、命令或 runtime 规格描述为长期 source of truth。

## Related Specs

- `docs/specs/omega-project-plan-system.md`
- `docs/specs/omega-structured-document-system.md`
- `docs/specs/omega-command-system.md`

---

### Change Log

- 2026-04-15: v0.2 — 移除了剩余 `.omega/plans/` compatibility/import layer；当前 runtime、layout 与 generated docs 统一把 `docs-data/tasks/*` 视为唯一 project plan persistence 路径。
- 2026-04-14: v0.1 — 初版 follow-up 规格，定义把 `/plan` canonical persistence 从 `.omega/plans/` 收口到 `docs-data/` 的目标、边界和 Task 4H ~ 4K rollout。
