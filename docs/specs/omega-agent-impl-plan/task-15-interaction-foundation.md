---
status: active
owner: omega-team
created: 2026-03-25
updated: 2026-03-25
version: 1.0
supersedes: []
related_prds: []
---

# Omega Agent Plan: Task 15 Interaction Foundation

本文覆盖 `Task 15` 的交互层基础边界，以及 `Task 15F-1` 到 `Task 15F-5` 的 workflow/session/routing 主线。

## Task 15 Boundary

- Stable boundary: `omega-app -> omega-tui + omega-session + omega-observability`
- UI rule: `omega-tui` 只负责 UI runtime、reducer、event/render 与 terminal 生命周期
- Orchestration rule: `omega-session` 拥有会话 turn 编排、workflow run、session assets 与 runtime UI bridge
- App rule: `omega-app` 负责 provider/config/runtime/tracing/session wiring

## Task 15 Overview

- `Task 15A`: 最小终端交互里程碑（历史完成）
- `Task 15B`: Ratatui TUI 能力与可见性增强
- `Task 15C` / `15C-2`: 交互层重构与 `omega-app` 单入口装配
- `Task 15D`: 非 UI 职责剥离到 `omega-session` / `omega-observability`
- `Task 15F`: 可见执行工作流与运行态摘要基础

## Task 15F-1: omega-workflow - 可配置四阶段工作流系统

- Status: Completed
- Scope: 新增 `omega-workflow` crate；将 `explore -> plan -> execute -> report` 收敛为可配置 workflow 定义与 prompt 加载模型。
- Runtime effect: `omega-session` 能发出 typed step change，`omega-tui` 能展示当前阶段。
- Related spec: `../omega-workflow-package.md`

## Task 15F-2A: omega-session - 会话资产管理基础

- Status: Completed
- Scope: `SessionToolCatalog` 与 `SessionSkillCatalog` 落地，`AgentSession` 改为组合持有 catalogs。
- Core contract: tools 与 skills 作为 session-owned assets 统一解析，而不是散落在各 step runner 内。
- Related spec: `../omega-step-session-asset-model.md`

## Task 15F-2B: omega-workflow - 通用 Step 编排接入会话资产层

- Status: Completed
- Scope: `WorkflowStep` 从 enum-centric 阶段模型泛化为 string-keyed step definition，并正式引入 `StepToolRequest`、`StepSkillRequest` 与 `step_id` 事件字段。
- Runtime effect: `omega-session` 改为通用 step runner，按 step 解析工具/技能而不是对 `execute` 做硬编码特判。

## Task 15F-3: omega-session - 统一 runtime UI 消息与效果协议

- Status: Completed
- Scope: `SessionUpdate` 被统一的 `RuntimeUiEnvelope`/`RuntimeUiMessage`/`RuntimeUiEffect` 取代。
- Runtime effect: `omega-tui` 不再消费 workflow 专属 update 枚举，而是消费统一 runtime sink。
- Related spec: `../omega-runtime-ui-message-contract.md`

## Task 15F-4: omega-workflow - scene catalog 与 workflow routing 模型

- Status: Completed
- Scope: 引入 scene/workflow catalogs、root/chat/feature presets，以及 `scene-recognition` / `select-workflow` steps。
- Runtime effect: child workflow 选择不再只是隐式 prompt 约定，而是显式 workflow model。
- Related spec: `../omega-scene-routing.md`

## Task 15F-5: omega-session - scene recognition 与 child workflow delegation

- Status: Completed
- Scope: turn 先运行 root workflow，再切换到 child workflow；session 维护 scene/workflow routing state。
- Runtime effect: session 产出 root/child workflow 可见状态，为后续 TUI 可视化与 step context handoff 提供稳定输入。

## Implementation Notes

- 此阶段的重点是先把运行时边界收口，再做更丰富的 runtime 可见性与 step context。
- 若后续继续推进 scene-aware delegation 或 step lifecycle，优先在 session/workflow 合同上扩展，不把特殊流程重新塞回 TUI 或 app shell。

---

### Change Log

- 2026-03-25: 从 `omega-agent-impl-plan.md` 中拆出 Task 15 基础边界与 15F-1..15F-5 主线内容。