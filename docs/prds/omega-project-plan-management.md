---
content_revision: 101
generation_id: gen_000017_r000101
last_verified_commit: N/A
owner: omega-team
projection_version: 17
source_doc_id: "prd:docs-prds-omega-project-plan-management"
status: draft
version: v0.1
---

# Omega Project Plan Management

## Summary

为 Omega 引入一个 project-scoped 的长期计划管理能力，作为 distinct surface 区分于 runtime `todo`。该能力需要成为内置规范的一部分：每个 project 都有稳定的 plan home，能够沉淀长期任务、历史记录、优先级、依赖链、任务日志，以及与 design / implementation 的可追踪关联。

## Problem

当前仓库里已经有两类相邻但不等价的东西：

- `docs/TODO.md`：只负责 open work、当前优先级和当前 baseline，不再承担历史任务总账。
- `omega-todo`：只负责当前任务、当前 turn / session 的 runtime working set。

这意味着 Omega 仍缺少一个正式的 project-level planning surface：

- 没有 durable source of truth 来承载长期任务与历史任务。
- 没有稳定结构来表达 priority、dependency chain 和 task logs。
- 没有把任务和 `spec` / `prd` / code path / delivery evidence 关联起来的正式 contract。
- 没有一套显式 command 让用户选择任务、管理任务、调整优先级，并把某个任务直接作为 requirement 发给 AI。
- TUI 只能看当前 runtime `todo`，不能看项目层面的长期计划。

## Users

- 在当前 project 内长期推进工作的主操作者
- 需要把“当前要做哪一个项目任务”显式交给 AI 的使用者
- 未来需要基于同一任务图协调 subagent / background / team 能力的运行时

## Requirements

### Must Have (P0)

- 提供 project-scoped 的长期计划系统，与 runtime `todo` 明确分离。
- 该系统必须作为 Omega 的内置规范存在，并在 project 打开后有稳定的 repo-local 存储位置。
- 长期任务至少包含：历史任务、当前代办、优先级、依赖链、任务日志、design/implementation 关联。
- 支持 TUI 查看长期计划摘要、当前选中任务、相关任务与历史任务。
- 支持显式 command：列出、查看、创建、更新、调整优先级、维护依赖、记录日志、选择任务。
- 支持把某个任务直接作为 requirement 输入给 AI，效果等价于“用户以该任务为背景发起一条普通请求”。
- 默认不依赖时间字段做管理，不把 due date、工时估算或 completed-at 作为一等输入。
- `docs/TODO.md` 继续保留为 open work 摘要，不再被迫承载长期历史账本。

### Should Have (P1)

- 支持 parent / child task 或 epic -> task 关系。
- 支持 task 与 `spec` / `prd` / `guide` / code path / delivery summary 建立结构化 link。
- 支持把 session 的 selected task 持久化到 session snapshot，并在后续 turn 中复用。
- 支持从 project plan 显式同步一份精简后的 open-work 投影到 `docs/TODO.md`。
- 支持将 task-bound delivery 产出的 changed files / summary 回写到 task log。

### Nice to Have (P2)

- 支持 task 过滤、搜索、标签和分组视图。
- 支持导入现有 open TODO tracks 作为首轮 plan seed。
- 支持为不同类型任务提供模板，例如 `feature`、`research`、`refactor`、`cleanup`。

## Design

- 技术规格：`docs/specs/omega-project-plan-system.md`
- 相关边界：`docs/specs/omega-project-system.md`
- 命令系统：`docs/specs/omega-command-system.md`
- TUI 体验：`docs/specs/omega-tui-runtime-experience.md`

## Implementation Tasks

- `Task 4`: project-scoped plan management 总任务（`omega-plan` crate，独立于 `omega-tasks`，见 spec 架构说明）
- `Task 4A`: `omega-plan` crate、plan store、task graph、repo-local persistence
- `Task 4B`: `/plan` command family 与 mutation handlers
- `Task 4C`: selected task context 与 task-seeded turns（注入机制见 spec / Implementation Contracts）
- `Task 4D`: TUI 的 plan summary、activity view、detail overlay（前置：`Activity::Tasks` enum variant）
- `Task 4E`: `docs/TODO.md` 投影、artifact link 与 open-work migration
- `Task 4F`: delivery-backed task logs 与验证闭环（Phase 1 占位，待 Task 15F-36 升级）

## Open Questions

- 是否需要在 Phase 1 就支持“多任务批量操作”，还是先把单任务 command + overlay 走通即可。
- 是否需要在 `/plan send` 之外，再提供“选择任务后所有普通输入都默认绑定该任务”的显式模式切换开关。

---

### Change Log

- 2026-04-13: v0.2 — 同步 spec v0.2 架构修订：Task 实施任务说明中标注 `omega-plan` 独立 crate，注入机制选型，Phase 1 delivery log 占位策略，TUI 前置扩展要求。
- 2026-04-13: 初版 PRD，定义 project-scoped 长期计划管理需求，并与 runtime todo、`docs/TODO.md`、command/TUI 集成边界做切分。
