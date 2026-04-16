---
content_revision: 96
generation_id: gen_000013_r000096
last_verified_commit: N/A
owner: omega-team
projection_version: 13
source_doc_id: "todo:docs-todo"
status: active
updated: 2026-04-15
---

# TODO

## Scope

本文件只保留当前未完成任务、仍然有效的依赖顺序，以及需要作为现行基线记住的事实。

已完成里程碑的实现细节、历史方案和长摘要不再继续堆在这里；请改看对应 spec、doc changelog 和 git history。

## Current Priorities

### High

- **Task 10**: `omega-subagent` 仍是当前主线。已有 fresh-context child loop 与定向测试，剩余工作是把父 Agent 的 `task` tool 真正接到 child execution，并补齐 runtime 可见性与交付闭环。
- **Task 15F-36 ~ 15F-38**: 当前 `Delivery` 面板与 `Task Delivery Summary` 已可用，但仍主要依赖 TUI 侧聚合。下一步要把 task-delivery summary 收敛为 frontend-neutral contract，并让 changed files / knowledge search 统计来自可信 evidence，而不是 UI 侧推断。

### Medium

- **Task 15B-52**: 继续收紧 `Sidebar` 的默认密度、摘要卡片和首屏信息优先级，让 quiet dashboard 方向真正稳定下来。
- **Task 12 / 3 / 13**: 后台执行、消息总线与 team orchestration 仍保留，但都排在 `Task 10` 和 delivery-summary contract 之后。

### Low

- **Task 15B-54**: 把现有视觉方向沉淀为可维护的 theme preset / density mode 系统。
- **Task 15B-9 ~ 15B-12**: 语法高亮、输入历史、面板搜索和可调布局保留为后续 TUI 增量能力。
- **Task 6**: `omega-worktree` 对后期自治执行重要，但当前不是主瓶颈。
- **Task 16**: 最终整合测试只作为收尾门槛，不前置抢占主线。

## Current Baseline

- **Project ownership and layout split are complete**: `Task 18 ~ 18N` 已完成；repo-local config/source 继续留在 `.omega/`，runtime-generated state 已收口到 `.omega-state/`。相关规格：`docs/specs/omega-project-system.md`、`docs/specs/omega-project-path-layout.md`。
- **Session control plane baseline is complete**: `Task 17C ~ 17N` 已完成；startup 维持 `Unbound`，session restore 以 per-session `session.context.jsonl` canonical ledger 为主。相关规格：`docs/specs/omega-session-resume.md`。
- **Context, document, memory, observation recall baseline is complete**: `Task 11A ~ 11G-10` 已完成；当前只剩 retrieval precision 和交付统计层面的 follow-up，不再是能力建基阶段。相关规格：`docs/specs/omega-context-management.md`、`docs/specs/omega-knowledge-evolution.md`、`docs/specs/omega-tui-document-memory-supervision.md`。
- **Tool capability system is complete**: `Task 8J ~ 8U` 已完成；tool manifest、permission/storage/UI effects 与 capability metrics 已成为基线，不再作为当前主线跟踪。
- **Root skill routing is complete**: `Task 5A ~ 5E` 已完成；root 已具备 `select-workflow -> select-skills -> load-skills` 路径与 loaded/ignored routed skill state。
- **Deterministic test seam baseline is complete**: `Task 15F-30 ~ 15F-35` 已完成；shared scripted LLM harness、runtime recorder、temp-root helper 与 replay harness 已就位。
- **Delivery UI baseline is complete**: `Task 15B-49 ~ 15B-50` 已完成；当前剩余工作是把 summary contract 和 evidence source 从 TUI 侧 provisional aggregation 提升为 session/app-owned contract。
- **Project plan management baseline is complete**: `Task 4A ~ 4K` 已完成；`omega-plan` store、`/plan` command family、`docs/TODO.md` projection + 首轮迁移、`/plan load` file/dir preview+apply 导入、selected-task restore warning、`ProjectDetailSnapshot.plan` 驱动的 TUI `Project` surface、task-bound `delivery_attached` / `partial_delivery` log 回写，以及 docs-data-backed `ProjectPlanStore` / `/plan` runtime / TODO projection convergence 都已成为基线。旧的 `.omega/plans/` compatibility/import layer 已移除，project plan 现在只通过 `docs-data/tasks/*` canonical persistence 工作。 当 docs-data cutover 导致 `project-tasks.jsonl` 缺失时，project planning 现在会从 `docs-data/tasks/doc-tasks.jsonl` 回填自愈，且 `/plan list` 会通过 picker overlay 展示当前任务。相关规格：`docs/specs/omega-project-plan-system.md`、`docs/specs/omega-project-plan-docs-data-convergence.md`。
- **Structured docs system is complete**: `Task 19A ~ 19K` 已完成；`docs-data/` canonical layout、structured doc/task/relation schema、文档技能迁移、`manage_document` mandatory structured actions、`/document render|validate|extract`、deterministic renderer、projection validator、`docs-data/tasks/doc-tasks.jsonl`、`omega-doc-cli` foundation/query/mutation surface、CLI-first guidance enforcement、docs/docs-data version contract，以及最终 CLI-only workflow cutover 都已成为基线。正常文档 mutation 现在默认走 `omega-doc`，直接 markdown/docs-data edits 仅保留给 emergency projection repair。
- **TUI visual refresh baseline is complete**: `Task 15B-51` 与 `Task 15B-61 ~ 15B-64` 已完成；当前仅保留 `Task 15B-52` 与 `Task 15B-54` 两个 open follow-up。

## Active Tasks

### Task 10: omega-subagent — SubAgent

- **Status**: Pending
- **Priority**: High
- **Description**: 完成 `task` tool 到真实 child execution 的接线，让父 Agent 能以独立 message list / run loop 委托子任务，并把 child run 变成稳定的 runtime-visible surface。
- **Planning Note**: fresh-context `SubAgent` loop、tool loop、tool error 回写和定向测试已落地；当前缺口是 parent orchestration、tool contract 接线和交付可见性，而不是再重写 child runtime。
- **Related**: `docs/specs/omega-agent-impl-plan.md`, `docs/specs/omega-tui-runtime-experience.md`

### Task 15F-36: omega-session / omega-client / omega-app — Task Delivery Summary Contract And Turn-Scoped Accumulator

- **Status**: Pending
- **Priority**: Medium
- **Description**: 为单次任务交付建立 frontend-neutral `TaskDeliverySummary` contract，并在 `omega-session` 中增加 turn-scoped accumulator，统一累计 token 消耗、LLM 调用次数、model 使用、tool/skill 使用、document/memory search 次数与 changed files 摘要；失败或中断任务也必须产出 partial summary。
- **Planning Note**: 当前 UI 已能展示 delivery summary，但统计仍主要由 TUI 侧基于 runtime-visible signal 近似聚合；本任务负责把合同与所有权收口到 session/app 路径。
- **Blocks**: `Task 15F-37`, `Task 15F-38`
- **Related**: `docs/specs/omega-task-delivery-observability.md`, `docs/specs/omega-runtime-message-pipeline.md`, `docs/specs/omega-runtime-ui-message-contract.md`

### Task 15F-37: omega-context / omega-tools / omega-session — Knowledge Search And Workspace Mutation Accounting

- **Status**: Pending
- **Priority**: Medium
- **Description**: 为 task delivery summary 提供可信 evidence source：document / memory search 次数与 query evidence 要从 recall 路径稳定上报，workspace changed files 要从 structured tool result / mutation evidence 收口，而不是在 turn 结束后靠文本或临时 `git diff` 猜测。
- **Blocked by**: `Task 15F-36`
- **Blocks**: `Task 15F-38`
- **Related**: `docs/specs/omega-task-delivery-observability.md`, `docs/specs/omega-tool-system-upgrade.md`, `docs/specs/omega-tui-document-memory-supervision.md`

### Task 15F-38: omega-session / omega-app — Delivery Completion Message And Runtime State Wiring

- **Status**: Pending
- **Priority**: Medium
- **Description**: 在 `RuntimeMessageEnvelope -> policy -> TuiEngine` 主路径上新增 task-level delivery summary wiring：turn 运行中持续 upsert summary，turn 完成时冻结为最终快照，并发出统一的 `Task Delivery Summary` completion message，同时保留可供 Sidebar / Overlay 复用的 detail identity。
- **Blocked by**: `Task 15F-36`, `Task 15F-37`
- **Related**: `docs/specs/omega-task-delivery-observability.md`, `docs/specs/omega-runtime-message-pipeline.md`, `docs/specs/omega-runtime-ui-message-contract.md`

### Task 15B-52: omega-tui — Sidebar Dashboard Density And Summary Card Cleanup

- **Status**: Pending
- **Priority**: Medium
- **Description**: 继续把 `Sidebar` 从“信息很多的功能区”收敛为 quiet dashboard：减少内部边框感、稳定 section padding、压缩重复标签与摘要噪音，并明确默认首屏该优先展示什么。
- **Blocked by**: `Task 15B-51`
- **Blocks**: `Task 15B-54`
- **Related**: `docs/specs/omega-tui-visual-refresh.md`, `docs/specs/omega-tui-runtime-experience.md`

### Task 15B-54: omega-theme / omega-tui — Theme Presets And Density Modes

- **Status**: Pending
- **Priority**: Low
- **Description**: 把当前视觉方向沉淀为可维护的主题/密度系统，明确默认主题的 monochrome foundation、single-accent discipline，以及 compact / comfortable 等密度档位是否进入 `omega-theme` 默认能力。
- **Blocked by**: `Task 15B-52`
- **Related**: `docs/specs/omega-tui-visual-refresh.md`, `docs/specs/omega-theme-package.md`

### Task 15B-9: omega-tui — 代码语法高亮

- **Status**: Pending
- **Priority**: Low
- **Description**: 为代码块增加按语言的语法高亮，首轮至少覆盖 Rust / Python / Shell。

### Task 15B-10: omega-tui — 输入历史

- **Status**: Pending
- **Priority**: Low
- **Description**: 支持历史输入浏览与持久化历史记录。

### Task 15B-11: omega-tui — 面板内搜索

- **Status**: Pending
- **Priority**: Low
- **Description**: 在当前聚焦面板内提供浮动搜索框、高亮匹配与跳转能力。

### Task 15B-12: omega-tui — 可调面板布局

- **Status**: Pending
- **Priority**: Low
- **Description**: 保留可调 panel ratio / layout 能力；原“会话统计”范围已被 `Task 15F-36 ~ 15F-38` 和 `Task 15B-49 ~ 15B-50` 取代，不再在这里重复跟踪。

## Notes

- `docs/README.md` 是阅读入口索引；`docs/TODO.md` 只负责 open work 与当前基线，不再充当历史总账。
- 若需要某个已完成任务的完整背景，请进入对应 spec 或使用 git history，而不是继续把完成记录回填到本文件。
