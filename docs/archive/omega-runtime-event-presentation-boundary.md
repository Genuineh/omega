---
archived: 2026-03-26
content_revision: 118
created: 2026-03-26
generation_id: gen_000037_r000118
owner: omega-team
projection_version: 37
related_prds: []
source_doc_id: "archive:docs-archive-omega-runtime-event-presentation-boundary"
status: archived
superseded_by: docs/specs/omega-runtime-message-pipeline.md
supersedes: []
updated: 2026-03-26
---

# Omega Runtime Event And Presentation Boundary Specification

## Overview

> **Archived 2026-03-26**: Superseded by `docs/specs/omega-runtime-message-pipeline.md` (v0.2 message pipeline model). The 3-layer event/presenter/update model proposed here was replaced by a simpler message pipeline: session produces `RuntimeMessage`, app consumes and organizes rendering, TUI provides engine.

## Overview

当前主路径已经通过 `RuntimeUiEnvelope` 打通了 `omega-session -> omega-tui` 的运行态可见性，短期内解决了 routing、step timeline、thinking、tool run、todo snapshot 与 diagnostics 的展示问题。但这条链路仍然把三种职责揉在一起：`omega-session` 一边产出运行态语义，一边直接知道 `UiTarget / StatusSlot / OverlayTarget` 这类前端 surface；`omega-app` 作为装配层基本只是透传 channel；`omega-tui` reducer 则同时承担“当前消息应该落到哪个 surface”和“这个 surface 如何更新 view state”两类决策。

本规格定义下一轮收敛目标：把“运行态事件”“当前前端的呈现策略”“具体 TUI 渲染状态”拆成明确三层。目标不是否定当前 `RuntimeUiEnvelope` 的价值，而是把它从临时主路径 contract 提升为更清晰的长期边界：后端产出 frontend-neutral runtime event，`omega-app` 拥有当前前端的 presentation/router 策略，`omega-tui` 只消费面向视图的更新并渲染。

## Goals

- 让 `omega-session`、`omega-core`、`omega-workflow` 与未来 runtime-visible 模块不再直接依赖 TUI surface 概念。
- 让 `omega-app` 真正拥有“不同消息在当前前端上如何显示”的 presentation policy，而不是仅做 channel 装配。
- 让 `omega-tui` 回到 view/update/render 边界，不再承担过多运行态语义路由职责。
- 为 `omega-subagent`、`omega-background`、`omega-message`、`omega-team`、`omega-worktree` 接入前，先固定可扩展的消息分类与 surface 映射模型。
- 保持当前已实现的 routing / response / thinking / tool / todo / diagnostics 可见性能力，在迁移期间允许 compatibility adapter 存在。

## Non-Goals

- 本轮不引入第二套 GUI 或 Web frontend。
- 本轮不重写 workflow、step contract 或 tool runtime 本身。
- 本轮不把所有 cross-crate communication 都改造成总线；只处理“需要被前端消费的运行态输出”。
- 本轮不要求一次性删掉当前 `RuntimeUiEnvelope`；允许经过 compatibility bridge 平滑迁移。

## Architectural Assessment

当前边界可评为 `6/10`。

优点：

- `omega-app -> omega-session -> omega-tui` 的主路径已经稳定。
- `RuntimeUiEnvelope` 已经避免了 runtime 模块直接调用 widget。
- reducer / timeline / diagnostics / tool lane 已经把大量临时特例收敛到统一消费入口。

主要问题：

- `omega-session` 仍直接暴露 `UiTarget / StatusSlot / OverlayTarget`，属于后端知道太多前端 surface 的 layer violation。
- `omega-app` 还没有真正持有 presentation policy，装配层职责偏弱。
- `omega-tui` reducer 目前同时做 semantic routing 和 view-state mutation，耦合偏高。
- 未来每接一个 runtime-visible 模块，都有继续膨胀 `UiSource`、`RuntimeUiEffect` 与 `TuiUpdateReducer` 的趋势。

## Problem Statement

当前模型里，“什么事情发生了”和“这件事在 TUI 的哪个区域如何显示”是同一个 contract：

- `BeginResponseSection`、`SetStatusSlot`、`ReplacePanel` 这类 effect，已经直接带上了当前前端的布局语义。
- `UiTarget::Response / Activity / Todo / StatusBar / Overlay` 说明 producer 侧已经在替 frontend 做 surface 选择。
- `TuiUpdateReducer` 仍要根据 `UiSource` 和 `UiMessageKind` 再次推断样式和信息层级，说明 presentation policy 没有在单一边界收口。

这会带来三个后果：

1. `omega-session` 对 TUI surface 过度知情，未来若引入其他 frontend 或仅想调整 surface mapping，需要回头修改 backend contract。
2. `omega-app` 无法成为真正的 presentation orchestrator，难以承载“按消息类型切换不同渲染策略”的责任。
3. `omega-tui` 继续承担语义分流，后续 `subagent/background/message/team/worktree` 接入时很容易再次长成混合状态机。

## Proposed Layered Model

### Layer 1: Runtime Event

frontend-neutral、session-owned 的运行态事件层。该层只表达“发生了什么”，不表达“应该显示在哪个 panel”。

推荐 ownership：新增一个窄的 contract crate，例如 `omega-interaction`，承载 `RuntimeEvent`、`RuntimeEventEnvelope` 和相关 typed payload；如果首轮不单独起 crate，至少也要把它从 `omega-session` 的 TUI-specific contract 中拆出为独立模块，并保证不依赖 `ratatui` 或 `omega-tui` surface 枚举。

示例事件族：

- `TurnEvent`: started, finished, interrupted
- `WorkflowEvent`: workflow entered, step changed, route resolved
- `ResponseEvent`: section started, delta appended, section completed
- `ToolEvent`: tool run started, updated, completed
- `TodoEvent`: snapshot updated, stale, cleared
- `DiagnosticsEvent`: step diagnostics upserted
- `RuntimeNotice`: warning, error, compacted, policy notice
- `ModuleActivity`: future `subagent/background/message/team/worktree` 活动摘要

### Layer 2: Presentation Routing

当前 frontend 的 presentation policy 层，由 `omega-app` 持有。该层负责把 `RuntimeEvent` 映射为当前前端能理解的 surface update 或 render intent。

这一层必须回答：

- routing 事件是进底部 badge、Activity，还是同时进两处？
- tool run 是在 Response 的 tool lane 呈现、Activity 追加日志，还是只做其中一种？
- diagnostics、warning、overlay request 的优先级和 surface 选择是什么？
- 当未来 `subagent` 或 `message bus` 事件接入时，当前 frontend 如何呈现？

推荐形态：`omega-app` 新增 `RuntimePresenter` 或 `PresentationRouter`，消费 `RuntimeEvent` 并产出当前 frontend-specific update，例如 `TuiUpdate`。

### Layer 3: TUI Update And Rendering

`omega-tui` 只消费面向视图的 update，并将其折叠到 `App` view state，再由 render pipeline 画出来。

这一层应只关心：

- 某条更新属于哪类 surface / variant
- 当前 panel / sidebar / overlay / status slot 如何改写 view state
- 具体样式、折叠、焦点、列表与 overlay 如何渲染

它不应再关心：

- runtime producer 是 `workflow` 还是 `session` 才应该进入某个 panel
- 哪些 domain event 需要双写到 Response 和 Activity
- 哪些 message family 在当前 frontend 上应该采用什么 surface policy

## Ownership Matrix

| Layer | Owner | Must Own | Must Not Own |
|------|-------|----------|--------------|
| Runtime event | `omega-session` + future runtime modules | semantic event production, event payload normalization | panel names, overlay kinds, badge slots, widget update verbs |
| Presentation routing | `omega-app` | current frontend surface mapping, render policy, compatibility adapters | terminal widget state, workflow execution, session context internals |
| TUI rendering | `omega-tui` | view model state, reducer, styling, layout, focus, overlay widgets | backend semantic routing rules, module-specific policy branching |

## Contract Direction

### Current Direction

```text
omega-session -> RuntimeUiEnvelope(target/effect/source) -> omega-tui reducer -> render
```

### Target Direction

```text
omega-session/future modules -> RuntimeEvent -> omega-app RuntimePresenter -> TuiUpdate -> omega-tui reducer -> render
```

## Suggested Contract Shapes

### RuntimeEvent

```rust
pub enum RuntimeEvent {
    Turn(TurnEvent),
    Workflow(WorkflowEvent),
    Response(ResponseEvent),
    Tool(ToolEvent),
    Todo(TodoEvent),
    Diagnostics(DiagnosticsEvent),
    Notice(RuntimeNotice),
    ModuleActivity(ModuleActivityEvent),
}
```

关键约束：

- 不出现 `UiTarget`、`StatusSlot`、`OverlayTarget` 这类当前前端专用命名。
- `ResponseEvent` 允许继续保留 section identity / streaming delta / completion state，因为这仍是 frontend-neutral 的内容时序语义。
- `ModuleActivityEvent` 应为后续模块预留 family，而不是现在就让 `omega-session` 为每个未来 crate 发明一个 panel target。

### RuntimePresenter

```rust
pub trait RuntimePresenter {
    fn present(&mut self, event: RuntimeEvent) -> Vec<TuiUpdate>;
}
```

关键约束：

- `omega-app` 决定一个 runtime event 在当前 frontend 上投射为多少条 update。
- 允许一个 event 同时映射到多个 surface，例如 routing 更新可同时投射为 `StatusBadge` 和 `ActivityEntry`。
- 允许未来不同 frontend 提供不同 presenter，而不改 backend event shape。

### TuiUpdate

```rust
pub enum TuiUpdate {
    Response(ResponseUpdate),
    Activity(ActivityUpdate),
    Todo(TodoUpdate),
    Status(StatusUpdate),
    Overlay(OverlayUpdate),
}
```

关键约束：

- `TuiUpdate` 是 view-facing contract，不再要求 `omega-session` 直接构造。
- `omega-tui` reducer 只做状态更新，不重新决定 event taxonomy。

## Migration Strategy

### Phase 1: 固化 taxonomy 与 ownership

- 写清 `RuntimeEvent` family、`RuntimePresenter` ownership、`TuiUpdate` 边界。
- 以 compatibility-first 方式保留现有 `RuntimeUiEnvelope` 直到 migration 完成。

### Phase 2: 把 frontend-neutral event 从 session 中抽出来

- `omega-session`、`omega-core`、`omega-workflow` 改为产出 `RuntimeEvent`。
- 当前 `RuntimeUiEnvelope` 退化为 app presenter 后的 frontend adapter output，而不是 backend source-of-truth。

### Phase 3: app 收口 presentation policy

- `omega-app` 增加 `RuntimePresenter`。
- 当前“某条消息写到 Response / Activity / Status / Todo 的规则”从 `omega-session` 与 `omega-tui` 中回收。

### Phase 4: tui 收口为 presentation-first consumer

- `omega-tui` reducer 改为只消费 `TuiUpdate`。
- `App` / render pipeline 继续负责 view state 与 visual semantics，不再承担 domain routing。

### Phase 5: 测试矩阵与 future module onboarding

- 建立 runtime event -> presentation -> TUI snapshot 的 deterministic test matrix。
- 新模块接入时先补 event family 与 presenter mapping，再决定是否需要新增 surface variant。

## Planned Task Set

### Task 15F-23

- 固化 RuntimeEvent / Presentation / TUI update 三层模型与 ownership。
- 交付物：本规格、实现计划更新、TODO 拆分。

### Task 15F-24

- 将 `omega-session` 当前 session-owned UI contract 改为 frontend-neutral runtime event contract。
- 交付物：新的 contract module/crate、compat adapter、`omega-session` producer 迁移。

### Task 15C-4

- 在 `omega-app` 中新增 `RuntimePresenter`，按消息 family 决定当前 frontend 的 surface routing 与 render variant。
- 交付物：app-owned presenter、wiring、当前 TUI 的 presentation policy。

### Task 15B-27

- 将 `omega-tui` reducer/render 改为 presentation-first consumer，只消费 app 产出的 `TuiUpdate`。
- 交付物：精简后的 reducer、面向视图的 update model、保持现有 Response/Activity/Todo/Status/Overlay 体验。

### Task 15F-25

- 建立 runtime event -> presentation -> TUI matrix tests。
- 交付物：routing/tool/todo/thinking/diagnostics 基线矩阵，以及 future module onboarding seam。

## Acceptance Criteria

- `omega-session` 的长期 source-of-truth contract 不再直接暴露 `UiTarget / StatusSlot / OverlayTarget`。
- `omega-app` 拥有当前 frontend 的 presentation routing 规则，而不是只负责 channel 透传。
- `omega-tui` reducer 只消费面向视图的 update，不再承担主要的 domain routing 责任。
- 当前已落地的 routing / response / thinking / tool / todo / diagnostics 体验保持不退化。
- `omega-subagent`、`omega-background`、`omega-message`、`omega-team` 接入前，已有可复用的 event family 和 presenter seam。

## Related Documents

- `docs/specs/omega-runtime-ui-message-contract.md`
- `docs/specs/omega-tui-runtime-experience.md`
- `docs/specs/omega-app-package.md`
- `docs/specs/omega-agent-impl-plan/task-15-runtime-visibility.md`

---

### Change Log

- 2026-03-26: 初版规格，定义 runtime event、app-owned presentation routing 与 TUI-only rendering 的三层收敛方向。
