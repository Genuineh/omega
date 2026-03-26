---
status: active
owner: omega-team
created: 2026-03-25
updated: 2026-03-26
version: 1.1
supersedes: []
related_prds: []
---

# Omega Agent Plan: Task 15 Runtime And Visibility

本文覆盖 `Task 15` 中与 runtime contract、step context、diagnostics 和 TUI 可见性直接相关的后续主线。

## Covered Tasks

| Task | Status | Focus |
|------|--------|-------|
| Task 15B-19 | Completed | scene / workflow routing 可见性 |
| Task 15F-6 | Completed | 流式 response / thinking runtime contract |
| Task 15B-20 | Completed | 结构化 response timeline |
| Task 15B-21 | Completed | provider-exposed thinking 实时展示 |
| Task 15F-7 | Completed | tool-run runtime contract 与 provider markup 清洗 |
| Task 15F-8 | Completed | 全 step 有界最小 agent loop |
| Task 15F-9 | Completed | session-owned step context |
| Task 15F-10 | Completed | Step Data Contract 框架 |
| Task 15F-11 | Completed | Feature workflow schema 绑定与 todo 集成 |
| Task 15F-12 | Completed | 上下文观测与诊断 |
| Task 15B-22 | Completed | step 内工具使用可见性 |
| Task 15B-23 | Completed | thinking 可读性强化 |
| Task 15B-18 | Completed | 统一 runtime UI sink / reducer |
| Task 15F-23 | Completed | runtime message pipeline 规划收口 |
| Task 15F-24 | Completed | frontend-neutral RuntimeMessage contract |
| Task 15B-27 | Completed | TuiEngine surface API + app policy |
| Task 15F-25 | Completed | runtime message pipeline matrix tests |

## Runtime Contract Track

### Task 15F-6

- `omega-client`、`omega-core` 与 `omega-session` 已建立 typed streaming event 到 runtime UI contract 的上游链路。
- Response、thinking、tool use 与 completion 不再需要由 TUI 通过纯文本猜测。

### Task 15F-7

- session-owned `ToolRun` lifecycle 已建立稳定 typed contract。
- provider 自带的 tool markup 会在 session 层清洗，避免污染 step/thinking sections。

### Task 15B-18 / 15B-20 / 15B-21 / 15B-22 / 15B-23

- `omega-tui` 已把 runtime envelope 收敛到 reducer 驱动的 response timeline。
- thinking、tool lane、routing blocks 与 final answer 均拥有稳定 section identity。

## Message Pipeline Follow-up

runtime message pipeline follow-up 已于 2026-03-26 落地，当前 ownership 已收敛为更小的消息管道模型：

- `omega-session` 主路径现已产出 frontend-neutral `RuntimeMessageEnvelope { turn_id, message }`，其中 `message` 拆分为 `ConversationMessage` 与 `StateMessage`。
- `omega-app` 现已装配 `DefaultRuntimeMessagePolicy`，真正持有 runtime message → rendering policy 的组织权。
- `omega-tui` 继续保留 runtime shell、terminal lifecycle、current-turn filter 和 render loop，并通过 `TuiEngine` 执行 surface-oriented 写入。
- `RuntimeUiEnvelope` 保留为 compat adapter 与 legacy tests surface，避免一次性切断既有回归矩阵。

当前实现与 `docs/specs/omega-runtime-message-pipeline.md` 已对齐；后续 `subagent/background/message/team/worktree` 应直接接到这个 seam，而不是继续扩大 reducer 特例。

### Task 15F-23

- 已固化 v0.3 的三个硬约束：保留 turn envelope、保留 `omega-tui` runtime shell ownership、把“消息到渲染策略”的 ownership 交给 `omega-app`。
- 相关 spec、TODO 和实现计划入口页已同步改为已实现状态，不再只描述未来方向。

### Task 15F-24

- `omega-session` 已从 session-owned UI envelope 主路径迁移到 frontend-neutral `RuntimeMessageEnvelope { turn_id, message }` contract。
- 其中 `message` 已拆分为 `ConversationMessage`（section/tool/fallback text）和 `StateMessage`（workflow/session/todo/diagnostics/activity/turn-finish）。
- `LegacyRuntimeUiBridge` 与 `spawn_turn_ui_compat()` 保留 compat adapter，避免一次性切断现有 TUI 体验与回归测试。

### Task 15B-27

- `omega-tui` 已收口为 UI runtime + 渲染引擎，新增 `TuiSurface` / `TuiEngine` 和 `apply_runtime_message_with_policy()` helper。
- `omega-app` 已装配 `RuntimeMessagePolicy`，并由 `omega-tui` runtime 在 current-turn 过滤之后执行，而不是把整个 event loop 迁到 app。
- reducer 和 render 继续负责 TUI 状态、样式、交互和 runtime shell，但主要的 runtime semantic routing 已迁到 app policy。

### Task 15F-25

- 已建立 `RuntimeMessageEnvelope -> current-turn filter -> app policy -> TuiEngine` matrix tests，锁住 routing / tool / todo / diagnostics / stale turn drop 的规则。
- `omega-session` 也补充了 producer-path tests，保证 frontend-neutral envelope 的 section/tool/activity 发射行为稳定。
## Workflow Context Track

### Task 15F-8

- root/chat/feature steps 已统一进入 bounded minimal agent loop。
- step 间差异改由 tool subset、iteration budget 与 prompt policy 表达，而不是执行器分叉。

### Task 15F-9

- session 已显式维护 `SessionContext`、`RoutingContext` 与 `StepSummary`。
- root-child handoff 改为 typed context，而不是长期依赖自由文本 token matching。

### Task 15F-10 / 15F-11

- Step Data Contract 已提供结构化输入输出、校验、重试与 feature workflow schema binding。
- todo 不再只是旁路工具；它已成为 `plan -> execute -> report` 链路中的正式上下文投射。

### Task 15F-12

- context diagnostics 已能展示 step 输入、step 输出、contract 状态与 session context diff。
- 后续主线应继续在 observability/compression 上演进，而不是回退到非结构化日志猜测。

## Routing Visibility

### Task 15B-19

- scene-aware routing 已成为用户可见的运行态信息。
- UI 可区分 root workflow 与 child workflow，不再把 routing 藏在后台。

## Recommended Reading Order

1. `../omega-runtime-message-pipeline.md`
2. `../omega-runtime-ui-message-contract.md`
3. `../omega-step-session-asset-model.md`
4. `../omega-tui-runtime-experience.md`
5. `../omega-tui-response-thinking-experience.md`

---

### Change Log

- 2026-03-26: 标记 Task 15F-23 / 15F-24 / 15B-27 / 15F-25 已实现；当前主路径已切到 `RuntimeMessageEnvelope` + app-owned policy + `TuiEngine`，`RuntimeUiEnvelope` 降为 compat surface。
- 2026-03-26: 增补 runtime message pipeline follow-up，并在同日复审后收敛到更小的 v0.3：保留消息模型，但恢复 turn envelope，保留 `omega-tui` runtime shell，把策略 ownership 交给 `omega-app`。
- 2026-03-25: 从 `omega-agent-impl-plan.md` 中拆出 Task 15 runtime/visibility 主线内容，避免继续与 Task 15 foundation 共写在单文件中。