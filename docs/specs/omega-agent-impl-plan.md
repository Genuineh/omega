---
content_revision: 101
created: 2026-03-18
generation_id: gen_000017_r000101
last_verified_commit: N/A
owner: omega-team
projection_version: 17
related_prds: []
source_doc_id: "spec:docs-specs-omega-agent-impl-plan"
status: active
updated: 2026-04-13
---

# Omega Agent 实现计划

## Overview

本文件现在作为 Omega 工作空间实现计划的稳定入口页，保留原始文件路径，负责说明总体目标、当前进度和子文档导航。原先混合在单文件中的 crate 初始化、交互层演进、runtime contract 与可见性计划已按主题拆到子文档，避免后续继续把“已完成历史”“当前有效边界”和“下一阶段计划”写在同一长文中。

## Goal

逐步实现并重构当前工作空间；保持 Cargo workspace 多 crate 架构稳定演进，在已完成基础 crate、交互层边界收敛和 workflow/runtime 主链路之后，继续以可验证的小步任务推进 Omega Agent。

## Architecture

每个 crate 独立实现，通过 Cargo workspace 组合。底层 crate 无依赖，上层依赖下层；`omega-tui` 只负责 UI，`omega-app` 负责装配，`omega-session` 负责会话运行态编排，`omega-core` 负责底层 agent loop 与工具调用。

## Tech Stack

Rust, tokio, reqwest, ratatui, serde, uuid

## Document Map

| Document | Scope | Contents |
|----------|-------|----------|
| `docs/specs/omega-agent-impl-plan/foundation-crates.md` | Tasks 1-7 | Workspace init, client/message/tasks/skills/worktree/tools foundations |
| `docs/specs/omega-agent-impl-plan/execution-runtime-crates.md` | Tasks 8-14 | Builtin tools, todo, subagent, compression, background, team, core agent |
| `docs/specs/omega-agent-impl-plan/task-15-interaction-foundation.md` | Task 15 overview, 15F-1..15F-5 | `omega-app` / `omega-tui` / `omega-session` boundary, workflow package, session asset baseline, scene routing |
| `docs/specs/omega-agent-impl-plan/task-15-runtime-visibility.md` | 15B-19, 15F-6..15B-23 | Streaming runtime contract, step context, data contracts, diagnostics, tool/thinking visibility |
| `docs/specs/omega-runtime-message-pipeline.md` | Task 15 follow-up design | Runtime message pipeline v0.3: session produces frontend-neutral `RuntimeMessageEnvelope`, app owns message policy, TUI keeps runtime shell and engine |
| `docs/specs/omega-command-system.md` | Task 17 planning | Command registry, slash invocation model, `/document` and `/project` command families |
| `docs/specs/omega-session-resume.md` | Task 17B-17H planning | Project-scoped session resume, replay log hydration, context reload, `/session` command family, and session operator UX |
| `docs/specs/omega-operator-picker-overlay.md` | Task 15B-70, 17G-17H planning | Reusable operator picker overlay, action hotkeys, and session picker flow |

## Progress Snapshot

| Bucket | Status | Notes |
|--------|--------|-------|
| Tasks 1-14 | Implemented baseline | 基础 crate 已落地；后续若做重构，优先以各 crate 自身 spec 或 TODO 子任务为准 |
| Task 15 foundation | Implemented baseline | `omega-app`、`omega-session`、`omega-observability`、`omega-workflow` 与 scene-aware routing 主路径已建立 |
| Task 15 runtime visibility | Implemented baseline | 流式 response/thinking、step tool lane、context diagnostics 与 runtime reducer 已落地 |
| Task 15 message pipeline | Implemented follow-up | 主路径已收敛到 `RuntimeMessageEnvelope` + app-owned `RuntimeMessagePolicy` + `TuiEngine`；`RuntimeUiEnvelope` 降为 compat surface |
| Task 17 command system | Implemented baseline | command registry、`/document`、`/project`、`/session` picker/control plane、canonical ledger 与 lazy binding 已落地 |
| Task 18 project ownership | Implemented baseline | project-owned context/document/memory/session 边界与 `.omega/` / `.omega-state/` layout split 已落地 |
| Current open follow-ups | In progress | 仍然打开的主线是 `Task 10`、`Task 15F-36 ~ 15F-38`、`Task 15B-52`、`Task 15B-54` 与最终 `Task 16` |
| Task 16 | Pending | 仍保留为最终整合验证里程碑 |

## Current Focus

- `Task 10`: 补完父 Agent 到 child execution 的 `task` tool 接线，并把 subagent run 变成正式 runtime-visible surface。
- `Task 15F-36 ~ 15F-38`: 把当前 delivery UI baseline 收敛为 session/app-owned contract 与 evidence-backed accounting。
- `Task 15B-52` 与 `Task 15B-54`: 只保留必要的 TUI 默认密度与主题系统化工作，不再把已完成的视觉刷新历史继续堆回 TODO。

<a id="task-15"></a>

## Task 15: omega-tui - TUI 界面

`Task 15` 现已不再适合作为单一长章节维护。当前应按两类子文档理解：

- 交互层与 workflow/session 边界，见 `docs/specs/omega-agent-impl-plan/task-15-interaction-foundation.md`
- runtime contract、上下文演进与 TUI 可见性，见 `docs/specs/omega-agent-impl-plan/task-15-runtime-visibility.md`

其中稳定边界为：`omega-app -> omega-tui + omega-session + omega-observability`；`omega-tui` 不再承担非 UI 编排职责。

## Task 16: 最终整合测试

该里程碑仍保留为计划尾部的总体验证门槛：

1. `cargo build`
2. `cargo test`
3. 在需要提交时再执行最终提交步骤

## Usage Notes

- 原始文件路径保持不变，方便 `docs/TODO.md`、ADR 与历史文档继续引用。
- 若需要查看某个任务的详细实现步骤、文件范围和验证命令，优先进入对应子文档，而不是继续把细节回填到入口页。
- 后续新增 Task 15 follow-up 时，优先追加到最贴近主题的子文档；runtime message pipeline 收敛方向以 `docs/specs/omega-runtime-message-pipeline.md` 为主，且默认保留 `omega-tui` runtime shell ownership。

新增 command system 相关工作时，优先以 `docs/specs/omega-command-system.md` 为入口，而不是把命令注册、slash UX 与 document command 细节回填到本索引页。

新增 session restore / session control-plane 相关工作时，优先以 `docs/specs/omega-session-resume.md` 为入口，而不是把 replay/hydration/context reload 细节回填到本索引页。

---

### Change Log

- 2026-04-13: Refreshed the progress snapshot after Task 17 and Task 18 completion, and aligned the current-focus section with the trimmed active TODO.
- 2026-04-10: 新增 `docs/specs/omega-operator-picker-overlay.md` 作为 session/operator picker UX 规划入口，避免把 picker/action hotkey 细节堆回 session spec 或本索引页。
- 2026-04-10: 新增 `docs/specs/omega-session-resume.md` 作为 Task 17 下一阶段规划入口，收口 `/session` control plane、resume snapshot 与 replay hydration 设计。
- 2026-04-02: 新增 Task 17 command system 规划入口，明确 `/document` 是首个命令族，并将细节下沉到 `docs/specs/omega-command-system.md`。
- 2026-03-26: runtime message pipeline follow-up 已落地实现；主路径现已切到 `RuntimeMessageEnvelope`、app-owned `RuntimeMessagePolicy` 与 `TuiEngine`，并保留 `RuntimeUiEnvelope` compat surface。
- 2026-03-26: 补充 runtime message pipeline follow-up 导航与进度摘要，并在同日复审后收敛到更小的 v0.3：保留消息模型，但恢复 turn envelope，明确 `omega-app` 只拥有 message policy，`omega-tui` 继续拥有 runtime shell。
- 2026-03-25: 将原单体实现计划拆为“入口页 + 主题子文档”，保留原路径作为稳定索引，并把 Task 15 相关内容按交互基础与 runtime/visibility 两个主题分离。
- 2026-03-19: 初版实现计划创建。
