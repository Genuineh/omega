---
status: active
owner: omega-team
created: 2026-03-18
updated: 2026-03-26
version: 1.2
related_prds: []
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

## Progress Snapshot

| Bucket | Status | Notes |
|--------|--------|-------|
| Tasks 1-14 | Mostly implemented | 基础 crate 已落地；后续若做重构，优先以各 crate 自身 spec 或 TODO 子任务为准 |
| Task 15 foundation | Implemented baseline | `omega-app`、`omega-session`、`omega-observability`、`omega-workflow` 与 scene-aware routing 主路径已建立 |
| Task 15 runtime visibility | Implemented baseline | 流式 response/thinking、step tool lane、context diagnostics 与 runtime reducer 已落地 |
| Task 15 message pipeline | Implemented follow-up | 主路径已收敛到 `RuntimeMessageEnvelope` + app-owned `RuntimeMessagePolicy` + `TuiEngine`；`RuntimeUiEnvelope` 降为 compat surface |
| Task 16 | Pending | 仍保留为最终整合验证里程碑 |

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

---

### Change Log

- 2026-03-26: runtime message pipeline follow-up 已落地实现；主路径现已切到 `RuntimeMessageEnvelope`、app-owned `RuntimeMessagePolicy` 与 `TuiEngine`，并保留 `RuntimeUiEnvelope` compat surface。
- 2026-03-26: 补充 runtime message pipeline follow-up 导航与进度摘要，并在同日复审后收敛到更小的 v0.3：保留消息模型，但恢复 turn envelope，明确 `omega-app` 只拥有 message policy，`omega-tui` 继续拥有 runtime shell。
- 2026-03-25: 将原单体实现计划拆为“入口页 + 主题子文档”，保留原路径作为稳定索引，并把 Task 15 相关内容按交互基础与 runtime/visibility 两个主题分离。
- 2026-03-19: 初版实现计划创建。