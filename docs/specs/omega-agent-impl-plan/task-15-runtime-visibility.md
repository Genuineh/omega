---
status: active
owner: omega-team
created: 2026-03-25
updated: 2026-03-25
version: 1.0
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

1. `../omega-runtime-ui-message-contract.md`
2. `../omega-step-session-asset-model.md`
3. `../omega-tui-runtime-experience.md`
4. `../omega-tui-response-thinking-experience.md`

---

### Change Log

- 2026-03-25: 从 `omega-agent-impl-plan.md` 中拆出 Task 15 runtime/visibility 主线内容，避免继续与 Task 15 foundation 共写在单文件中。