---
content_revision: 101
created: 2026-03-26
generation_id: gen_000015_r000101
last_verified_commit: N/A
owner: omega-team
projection_version: 15
related_prds: []
source_doc_id: "spec:docs-specs-omega-runtime-message-pipeline"
status: implemented
supersedes:
  - docs/specs/omega-runtime-event-presentation-boundary.md (v0.1)
updated: 2026-03-26
---

# Omega Runtime Message Pipeline Specification

## Overview

当前主路径已按本规格落地为 `RuntimeMessageEnvelope { turn_id, message } -> current-turn filter -> RuntimeMessagePolicy -> TuiEngine`：`omega-session` 产出 frontend-neutral `ConversationMessage` / `StateMessage`，`omega-app` 装配消息策略，`omega-tui` 保留 runtime shell、turn 过滤和渲染执行。旧 `RuntimeUiEnvelope` 未被删除，而是降为 compat adapter 与 legacy test surface，用于平滑迁移和回归保护。

本规格保留“消息模型”这个正确方向，但把上一版过度外移的职责收回来。v0.3 的目标不是让 `omega-app` 接管整个 TUI runtime，而是让它拥有**消息到渲染策略的组织权**；`omega-tui` 继续拥有 terminal lifecycle、event loop、turn 过滤和渲染执行；`omega-session` 则只产出 frontend-neutral 的消息。

## Goals

- 让 `omega-session` 与未来 runtime 模块不再依赖 `UiTarget`、`StatusSlot`、`OverlayTarget` 等 TUI surface 概念。
- 保留当前系统已经验证过的 turn 隔离语义，避免 stale update 串进新 turn。
- 让 `omega-app` 拥有“消息如何映射到当前前端渲染策略”的组织权，但不吞掉 terminal lifecycle 和 UI runtime。
- 让 `omega-tui` 收口为 UI runtime + 渲染引擎，保留 event loop、input handling、rendering、view state。
- 为 `omega-subagent`、`omega-background`、`omega-message`、`omega-team`、`omega-worktree` 建立稳定的消息接入 seam。
- 在不退化 routing / response / thinking / tool / todo / diagnostics 体验的前提下，逐步替换旧 contract。

## Non-Goals

- 本轮不引入第二套 GUI 或 Web frontend。
- 本轮不重写 workflow、step contract、hook lifecycle 或 tool runtime。
- 本轮不把所有 cross-crate communication 都改造成统一总线。
- 本轮不把 terminal lifecycle、crossterm event loop 和 render loop 从 `omega-tui` 挪到 `omega-app`。
- 本轮不强行把 todo 从文本快照一次性升级成全新的结构化 view model；先保 compat。

## Design Constraints

优化后的方案必须同时满足这几个硬约束：

1. **保留 turn envelope**：当前系统通过 turn id 丢弃 stale runtime update，这个约束不能丢。
2. **保留 ADR 006 边界**：`omega-tui` 继续拥有 terminal UI runtime；`omega-app` 继续是装配与策略入口，而不是新的 God runtime。
3. **减少而不是平移复杂度**：不能只是把 `omega-tui` reducer 的 match 逻辑原封不动挪到 `omega-app`。
4. **迁移可分阶段**：必须允许 `RuntimeUiEnvelope` 和新消息 contract 并行一段时间。

## Problem Statement

当前 `RuntimeUiEnvelope` 的问题是真实存在的：producer 侧替 consumer 做了 surface 选择，`omega-app` 几乎没有介入点，而 `omega-tui` reducer 同时做 semantic routing 和 state mutation。

但上一版 v0.2 还有两个额外问题：

- 它去掉了 transport 层的 turn envelope，容易打破现有 stale update 过滤。
- 它把整个 event loop 迁到 `omega-app`，与已接受的 UI-only 边界相冲突。

因此，优化目标不是“更激进地外移”，而是**只把真正该移出的语义 ownership 移出**。

## Architecture

### Design Principle

按“谁拥有哪类决定”划分，而不是按“谁拿着 channel”划分：

- `omega-session` 决定“发生了什么”。
- `omega-app` 决定“当前前端如何解释这些消息”。
- `omega-tui` 决定“如何在 terminal 中执行这些渲染动作”。

### Target Boundaries

```text
┌─────────────────────────────────────────────────────────────┐
│ omega-session / future modules                              │
│   产出 RuntimeMessageEnvelope { turn_id, message }          │
│   不知道 panel / slot / widget / overlay                    │
└────────────────────┬────────────────────────────────────────┘
                     │ mpsc channel: RuntimeMessageEnvelope
┌────────────────────▼────────────────────────────────────────┐
│ omega-app                                                    │
│   装配 RuntimeMessagePolicy                                  │
│   持有“消息类型 → 当前前端渲染策略”的 policy                  │
│   不拥有 terminal loop                                       │
└────────────────────┬────────────────────────────────────────┘
                     │ policy injected into TUI runtime
┌────────────────────▼────────────────────────────────────────┐
│ omega-tui                                                    │
│   拥有 event loop / terminal lifecycle / current-turn filter │
│   drain 消息后执行 app 注入的 policy                         │
│   通过 TuiEngine 写入 view state 并 render                   │
└─────────────────────────────────────────────────────────────┘
```

### Role Definitions

**omega-session（消息生产者）**：

- 产出 `RuntimeMessageEnvelope { turn_id, message }`。
- 只表达 runtime 事实，不表达 TUI surface。
- 保留当前已有的 section id、tool run id、workflow metadata 等稳定 identity。

**omega-app（策略装配者）**：

- 装配 `RuntimeMessagePolicy` 并把它传给 `omega-tui` runtime。
- 决定“某类消息如何映射到当前前端的 response / activity / status / overlay 策略”。
- 不接管 crossterm event loop、terminal lifecycle 或 render loop。

**omega-tui（UI runtime + 渲染引擎）**：

- 保留 event loop、terminal lifecycle、input handling、render。
- 保留 current-turn 过滤和 stale update 丢弃。
- 提供 `TuiEngine` surface-oriented API，供 policy 执行写入 view state。

### Ownership Matrix

| Role | Owner | Must Own | Must Not Own |
|------|-------|----------|--------------|
| 消息生产 | `omega-session` + future modules | 消息内容、时序、turn lifecycle identity | panel / surface / slot / widget 概念 |
| 渲染策略 | `omega-app` | message → rendering policy、装配 wiring、policy selection | terminal lifecycle、input loop、widget state |
| UI runtime | `omega-tui` | terminal lifecycle、event loop、turn filtering、view state、render | workflow orchestration、provider/bootstrap policy |

## Contract Design

### RuntimeMessageEnvelope

```rust
/// Transport envelope for runtime-visible messages.
/// Keeps turn scoping explicit so stale messages can be discarded.
pub struct RuntimeMessageEnvelope {
    pub turn_id: u64,
    pub message: RuntimeMessage,
}
```

### RuntimeMessage

```rust
/// Frontend-neutral runtime message.
pub enum RuntimeMessage {
    Conversation(ConversationMessage),
    State(StateMessage),
}
```

### ConversationMessage

```rust
pub enum ConversationMessage {
    BeginSection {
        id: String,
        kind: SectionKind,
        title: String,
        metadata: SectionMetadata,
    },
    TextDelta {
        section_id: String,
        text: String,
    },
    ThinkingDelta {
        section_id: String,
        text: String,
    },
    CompleteSection {
        id: String,
        state: SectionState,
    },
    BeginTool {
        id: String,
        section_id: String,
        name: String,
        invocation_preview: String,
        detail: ToolDetail,
    },
    CompleteTool {
        id: String,
        status: ToolStatus,
        result_preview: Option<String>,
        detail: ToolDetail,
    },
}
```

`SectionKind`、`SectionState` 和 `SectionMetadata` 继续沿用当前 response section / tool lifecycle 已验证过的 identity 模型。

### StateMessage

```rust
pub enum StateMessage {
    WorkflowStep {
        workflow_id: String,
        workflow_role: WorkflowRole,
        step_id: String,
        step_label: String,
        index: usize,
        total: usize,
    },
    AgentStatus(String),
    SessionRouting {
        root_workflow_id: String,
        active_workflow_id: String,
        active_workflow_role: WorkflowRole,
        recognized_scene_id: Option<String>,
        selected_workflow_id: Option<String>,
    },
    TodoSnapshot(String),
    Diagnostics(Box<StepDiagnostics>),
    Log(String),
    Warning(String),
    Error(String),
    TurnFinished,
}
```

`TodoSnapshot(String)` 在 v0.3 先保留文本快照，以降低迁移面；结构化 todo view model 留待下一轮。

### Key Constraints

- transport 层必须保留 `turn_id`。
- `RuntimeMessage` 不得出现 `UiTarget`、`StatusSlot`、`OverlayTarget` 等 TUI surface 命名。
- `ConversationMessage` 继续与现有 response section / tool lane 自然对齐，避免重新发明数据形状。
- `StateMessage` 的 variant 必须足够完整，使 consumer 不需要额外拼装多条消息才能理解含义。

## TUI Engine And App Policy

v0.3 不再要求一开始就把 `omega-tui` 暴露成超细粒度的一对一 public API。先引入两个更小的 seam：

1. `RuntimeMessagePolicy`：app-owned 的消息解释策略。
2. `TuiEngine`：tui-owned 的 surface-oriented 写接口。

```rust
/// Assembled by omega-app, executed inside omega-tui runtime.
pub struct RuntimeMessagePolicy;

impl RuntimeMessagePolicy {
    pub fn apply(&self, engine: &mut TuiEngine, envelope: RuntimeMessageEnvelope);
}

/// Owned by omega-tui. Surface-oriented, not widget-by-widget.
impl TuiEngine {
    // Response timeline
    pub fn begin_section(&mut self, section: SectionSpec);
    pub fn append_section_text(&mut self, section_id: &str, text: &str);
    pub fn complete_section(&mut self, section_id: &str, state: SectionState);
    pub fn upsert_tool_run(&mut self, tool: ToolSpec);

    // Status / sidebar / diagnostics / activity
    pub fn set_workflow_step(&mut self, step: WorkflowStepSpec);
    pub fn set_agent_status(&mut self, label: &str);
    pub fn set_session_routing(&mut self, routing: RoutingSpec);
    pub fn set_todo_snapshot(&mut self, turn_id: u64, text: &str);
    pub fn upsert_diagnostics(&mut self, diagnostics: DiagnosticsSpec);
    pub fn add_activity_line(&mut self, text: String);
    pub fn mark_turn_finished(&mut self);
}
```

这里的关键不是把 API 变多，而是把 ownership 变清楚：

- engine API 只表达 view-facing 操作；
- policy 决定某条消息要调用哪些 engine 操作；
- terminal loop 仍由 `omega-tui` 执行。

## Data Flow: Current vs Target

### Current

```text
omega-session  ──RuntimeUiEnvelope(target=Response, effect=BeginSection)──▶  omega-tui reducer
               ──RuntimeUiEnvelope(target=StatusBar, effect=SetSlot)──────▶  omega-tui reducer
               ──RuntimeUiEnvelope(target=Activity, msg=tool_log)─────────▶  omega-tui reducer
                         │
                   omega-app 基本只做装配透传
```

### Target v0.3

```text
omega-session  ──RuntimeMessageEnvelope{turn_id,message}──▶  omega-tui runtime
                                                          │
                                                current-turn filter
                                                          │
                                             app-owned RuntimeMessagePolicy
                                                          │
                                                   TuiEngine surface API
                                                          │
                                                       view state / render
```

这比 v0.2 更小：

- session 不再知道 TUI surface；
- app 拿到策略 ownership；
- tui 不丢 runtime shell ownership。

## Runtime Shell Ownership

当前 `omega-tui/src/runtime.rs` 已拥有这些职责：channel drain、trace drain、leader timeout、spinner tick、render、event poll、interrupt turn 后 stale update 过滤。v0.3 明确保留这一层在 `omega-tui`。

`omega-app` 的职责是把 policy 和 runtime 依赖装配好，再传给 `omega-tui::run(...)`，而不是重新实现外层 loop。

目标形态更接近：

```rust
// omega-app
run_tui(TuiLaunchConfig {
    session,
    runtime_message_policy: RuntimeMessagePolicy::default(),
    ...
})

// omega-tui runtime
loop {
    while let Ok(envelope) = rx.try_recv() {
        if app.is_current_turn(envelope.turn_id) {
            runtime_message_policy.apply(&mut engine, envelope);
        }
    }

    draw(...)?;

    if poll_input_and_handle(...)? {
        break;
    }
}
```

## Migration Strategy

### Phase 1: 引入新 envelope，不动 outer runtime shell

- 新增 `RuntimeMessageEnvelope { turn_id, message }`。
- 新增 `RuntimeMessage` / `ConversationMessage` / `StateMessage`。
- 允许 `RuntimeUiEnvelope` 与新 envelope 并存。

### Phase 2: 引入 app-owned policy + tui-owned engine seam

- 在 `omega-app` 中定义并装配 `RuntimeMessagePolicy`。
- 在 `omega-tui` 中新增 `TuiEngine` surface-oriented API，先包装现有 `App` 方法。
- 在 `omega-tui` runtime 中执行 current-turn 过滤后，调用 policy，而不是直接把 surface-heavy envelope 喂给 reducer。

### Phase 3: 迁移 session emitters + 收缩旧 reducer

- `omega-session::ui_emit.rs` 从构造 `RuntimeUiEnvelope` 迁移为构造 `RuntimeMessageEnvelope`。
- 旧 `RuntimeUiEnvelope` 仅保留为 compat adapter，直到所有 producer 迁完。
- 逐步删除 `UiTarget` / `StatusSlot` / `OverlayTarget` 对 session contract 的暴露。

### Phase 4: 清理 compat layer + 锁测试矩阵

- 删除 compat adapter 和不再需要的旧 surface-heavy contract。
- 建立 `RuntimeMessageEnvelope -> current-turn filter -> policy -> TuiEngine -> view state` 的测试矩阵。

## Planned Task Set

### Task 15F-23

- 固化 v0.3 设计约束：保留 turn envelope、保留 `omega-tui` runtime shell ownership、引入 app-owned policy seam。
- 交付物：本规格、TODO 更新、实现计划更新。

### Task 15F-24

- 新增 `RuntimeMessageEnvelope`、`RuntimeMessage`、`ConversationMessage`、`StateMessage`。
- 保留 compat adapter，使旧 `RuntimeUiEnvelope` 可桥接到新消息模型。
- `omega-session` 的 producer 逐步迁移到新 envelope。
- 交付物：session 侧消息 contract、compat adapter、producer 迁移起点。

### Task 15B-27

- 新增 `TuiEngine` surface-oriented API，先包装现有 `App` 方法。
- 在 `omega-app` 中装配 `RuntimeMessagePolicy`，由 `omega-tui` runtime 执行该 policy。
- 保持 event loop、terminal lifecycle 和 current-turn filter 继续在 `omega-tui`。
- 交付物：app-owned policy seam、tui-owned engine seam、reducer shrink path。

### Task 15F-25

- 建立 `RuntimeMessageEnvelope -> current-turn filter -> RuntimeMessagePolicy -> TuiEngine` 的 deterministic 测试矩阵。
- 覆盖 routing / step response / thinking / tool / todo / diagnostics / log / stale turn drop。
- 交付物：基线测试矩阵 + future module onboarding seam。

## Acceptance Criteria

- `omega-session` 新消息 contract 不含 `UiTarget` / `StatusSlot` / `OverlayTarget`。
- transport 层保留 `turn_id`，stale update 过滤能力不退化。
- `omega-app` 获得 runtime message policy ownership，不再只是 channel 透传。
- `omega-tui` 保留 terminal lifecycle、event loop、render 和 current-turn filter ownership。
- 当前 routing / response / thinking / tool / todo / diagnostics 体验不退化。
- 新模块接入只需：(1) 发送新 `RuntimeMessageEnvelope`，(2) 在 policy 中补充对应渲染规则。

## Related Documents

- `docs/specs/omega-runtime-ui-message-contract.md`
- `docs/specs/omega-tui-runtime-experience.md`
- `docs/specs/omega-app-package.md`
- `docs/specs/omega-agent-impl-plan/task-15-runtime-visibility.md`
- `docs/decisions/006-omega-tui-ui-boundary.md`

---

### Change Log

- 2026-03-26 v0.4: 规格已实现；主路径切到 `RuntimeMessageEnvelope`、app-owned `RuntimeMessagePolicy` 与 `TuiEngine`，`RuntimeUiEnvelope` 降为 compat adapter。
- 2026-03-26 v0.3: 基于架构复审收敛为更小方案。保留消息模型，但恢复 `turn_id` transport envelope，明确 `omega-tui` 继续拥有 runtime shell，`omega-app` 只拥有 message policy / assembly ownership，不再接管整个 event loop。
- 2026-03-26 v0.2: 重新设计为消息模型。消息管道 `RuntimeMessage` 按渲染归属分为 `Conversation` + `State` 二分法；`omega-app` 消费管道并组织渲染；`omega-tui` 收口为 typed engine API。任务从 5 个压缩到 4 个，删除独立 presenter 任务。替换旧文件 `omega-runtime-event-presentation-boundary.md`。
- 2026-03-26 v0.1: 初版规格（已替换），定义 runtime event、app-owned presentation routing 与 TUI-only rendering 的三层收敛方向。
