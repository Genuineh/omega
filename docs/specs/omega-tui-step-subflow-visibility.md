---
content_revision: 101
created: 2026-03-26
generation_id: gen_000015_r000101
last_verified_commit: N/A
owner: omega-team
projection_version: 15
related_prds: []
source_doc_id: "spec:docs-specs-omega-tui-step-subflow-visibility"
status: draft
supersedes: []
updated: 2026-03-26
---

# Omega TUI Step Subflow Visibility Specification

## Overview

`Task 15F-27` 与 `Task 15F-28` 已把 `feature/research.execute` 从 whole-step repeat 收敛为 itemized execute loop：workflow step 显式声明 `loop_contract`，runtime 能稳定识别 `current_item_id`、`item_index/item_total` 与 `max_item_repeats`，并通过 `execute_progress` 暴露 todo/item 进展。

当前缺口在 TUI 呈现层。现有 `Response` 已经具备 `route / step / final / thinking` 的结构化 timeline，但它仍把 `execute` 视为单一 step block，缺少“step 内子流程”这一层级。结果是用户能在 diagnostics 里看见 item 进度，却无法在主阅读区直观看到：当前 `execute` 正在推进哪个 item、已经完成了哪些 item、每个 item 的正文/工具/thinking 应归属于父 step 的哪个子流程。

本规格定义一套 step-owned subflow 可见性方案。核心约束是：itemized execute 不是新的顶层 workflow step，而是父级 `execute` step block 内的 nested subflow。TUI 必须把它显示成“step 的子流程”，而不是伪装成新的 workflow phase。

## Goals

- 让 `Response` 在不打破现有 `route / step / final / thinking` 主结构的前提下，新增稳定的 step 内子流程层级。
- 让用户能直接看见父级 `execute` 的 item 进度、当前 item 身份、已完成 item 摘要，以及 item 级正文/工具/thinking 归属。
- 保持 `Todo` 面板继续作为 checklist 真值来源，`Response` 只承载执行轨迹与子流程进度，不重复渲染整份 todo 文本。
- 让 `Bottom status bar`、`Activity` 与 `Diagnostics` 对同一个 step subflow 使用一致 identity，而不是各自拼接字符串。
- 继续保持 `omega-session` / `omega-app` 拥有 runtime 语义归一，`omega-tui` 只消费 typed contract。

## Non-Goals

- 不把 itemized execute 提升为新的顶层 workflow step，也不改变现有 `WorkflowStep` / `route` 状态栏语义。
- 不让 `Response` 重新承担完整日志、完整 todo 列表或 hook tracing 明细；完整详情仍留在 `Activity` / `Diagnostics` / `Overlay`。
- 不要求首轮就支持任意深度的多层 subflow 嵌套；v1 只覆盖“step 内单层子流程”，首个使用者是 itemized execute。
- 不要求首轮就实现跨 turn 的 subflow 历史树、全局检索或批量折叠管理。

## Problem Statement

当前 itemized execute 的 runtime contract 已经足够稳定，但 TUI 仍存在三类认知断裂：

- `Response` 只看得见父级 `execute`，看不见 `execute-1`、`execute-2` 这类稳定 item run identity。
- `Diagnostics` 能解释 `todo_total/completed/open/current_item_id`，但主阅读区无法把这些字段映射回正在阅读的 step 正文。
- `Todo` 面板知道“哪项还没完成”，`Activity` 知道“发生了哪些 runtime 事件”，但两者之间缺少一个直接承载“当前 step 正在推进哪个 item”的中间层。

如果继续只靠 `[route]`、`[tool]` 或 diagnostics 文本提示，itemized execute 会重复走回 whole-step repeat 时代的老问题：用户知道系统在“继续跑 execute”，但不知道当前具体在跑什么。

## UX Direction

### Response Hierarchy

`Response` 维持当前 turn timeline 主结构，但在 `Step block` 内新增 `Subflow lane`：

- `Turn`
- `Step block` (`execute`)
- `Subflow lane`
- `Item run card` (`execute-1`, `execute-2`, ...)
- `Item-owned body / thinking / tool lane`

其中：

- 父级 `execute` 仍然是 response timeline 中的主 block。
- item run 作为父级 step block 内的二级结构出现，不单独参与顶层 block 排序。
- final answer 仍然位于 turn 末尾，不被 item run 抢占层级。

### Parent Step Header

父级 `execute` header 应在现有 step title 基础上补充一组紧凑摘要：

- `items 2/5`
- `current execute-2`
- `todo #risk-2`
- `repeat 1/8`

这些信息是父级 step 的摘要，不是新的底部状态栏替代品。header 只提供“当前 step 正在推进哪个子流程”的一眼可见入口。

### Subflow Lane Model

`Subflow lane` 放在父级 step header 与 step 正文之间，展示 step-owned item run 列表：

- 当前 item run 默认展开。
- 已完成 item run 默认折叠为单行摘要。
- 失败 item run 在失败当轮默认保持展开，便于直接阅读失败上下文。
- 未开始 item run 只显示占位 token，不渲染正文。

推荐最小摘要格式：

- `execute-1  #risk-1  done`
- `execute-2  #risk-2  running  repeat 1/3`
- `execute-3  #risk-3  queued`

### Item Run Card

每个展开中的 item run card 最少包含：

- `child_step_id`，例如 `execute-2`
- todo/item label，例如 `#risk-2`
- item status：`queued / running / done / failed`
- item repeat/no-progress 摘要
- item-owned body / thinking / tool lane

v1 的核心规则：

- 当前 item 的正文、thinking 与 tool lane 都归属于当前 item card，而不是继续混在父级 `execute` 的单一正文区。
- 已完成 item 默认只保留摘要；若用户展开，再显示正文/工具/diagnostics 摘要。
- 父级 step 本身仍可保留总体说明文本，但 item 级文本应优先挂到 item card 下。

### Sidebar, Todo, And Status Coordination

三块面向用户的 surface 应共享同一 identity：

- `Todo` 面板：继续展示完整 checklist，但当前 item 对应的 todo 行应被高亮。
- `Bottom status bar`：只显示一条紧凑摘要，例如 `item execute-2 2/5 r1/3`。
- `Activity`：继续记录 item 切换、deny/repeat、完成来源与失败事件，但不承担主阅读职责。

这样用户可以回答三个不同层级的问题：

- `Response`: 当前 step 的哪个子流程在产出正文。
- `Todo`: 这项任务在整体 checklist 中的位置。
- `Activity`: 为什么切换、重试、失败或完成。

### Overlay And Navigation

首轮不强制要求 overlay，但应为后续 drill-down 预留稳定入口：

- 在 `Response` 里选中 item run card 后用 `Enter` 打开 detail overlay。
- overlay 展示该 item 的 compact diagnostics、最近工具摘要与 completion source。
- `n/N` 或后续自定义快捷键可在 item run 之间跳转，而不是只能依赖整块滚动。

## Runtime Contract Direction

### Architectural Rule

step-owned subflow 必须通过 frontend-neutral runtime contract 表达，而不是让 `omega-tui` 根据 `step_id`、`current_item_id` 和日志文本自行拼模型。

v1 推荐两层 additive-only 扩展：

1. 为 response section metadata 增加 optional `subflow_ref`，把 item-owned body/thinking/tool lane 绑定到父级 step。
2. 为 state channel 增加 generic `StepSubflowStatus`，让 status bar / Activity / diagnostics 能共享相同 identity。

### Section Metadata Extension

推荐新增：

```rust
pub struct StepSubflowRef {
    pub parent_workflow_id: String,
    pub parent_step_id: String,
    pub parent_step_label: String,
    pub subflow_id: String,
    pub item_id: Option<String>,
    pub item_label: Option<String>,
    pub item_index: usize,
    pub item_total: usize,
}
```

规则：

- 当 response section 属于普通 step 正文时，`subflow_ref = None`。
- 当 response section 属于 itemized execute 当前 item 时，`subflow_ref = Some(...)`。
- tool run 与 thinking section 继续沿用现有 `parent_section_id` 归属，但可通过 section metadata 间接落到正确的 item card。

### State Message Extension

推荐在 `StateMessage` 上新增通用 variant，而不是 execute-only 命名：

```rust
pub enum StateMessage {
    // ...existing variants...
    StepSubflowStatus {
        workflow_id: String,
        step_id: String,
        step_label: String,
        subflow_id: String,
        item_id: Option<String>,
        item_label: Option<String>,
        item_index: usize,
        item_total: usize,
        status: StepSubflowState,
        repeat_count_for_item: u32,
        no_progress_streak_for_item: u32,
    },
}
```

这样 app policy 可以把同一条消息同时映射到：

- step header progress summary
- subflow lane 当前项状态
- 底部状态栏 compact badge
- Activity 简短事件摘要

### TuiEngine Surface

`TuiEngine` 不需要暴露 widget 级 API，但应具备 step-owned subflow 写接口，例如：

```rust
impl TuiEngine {
    pub fn upsert_step_subflow(&mut self, subflow: StepSubflowSpec);
    pub fn set_active_step_subflow(&mut self, step_id: &str, subflow_id: &str);
}
```

关键点不在于具体命名，而在于 ownership：

- `omega-session` 生产 subflow identity 与状态。
- `omega-app` policy 决定这些消息怎样映射到 TUI surface。
- `omega-tui` 只维护 view state，不重新推导 orchestration 语义。

## Rendering Rules

### Parent And Child Separation

- 父级 `execute` block 永远存在。
- 子 item run 永远依附于父级 block 显示。
- 子 item 不参与顶层 `Tab` 面板焦点循环；它属于 `Response` 内部选择态。

### Default Collapse Policy

- `running`: 展开
- `failed`: 展开
- `done`: 折叠为单行摘要
- `queued`: 折叠为短 token

### Text Placement Rule

- item-specific正文优先渲染在 item card 下。
- 父级 step 只保留总体说明、阶段摘要或 fallback 内容。
- 若当前 runtime 只能产生父级 step 文本而不能精确切 item，TUI 不应伪造 item 正文，只显示 subflow header + diagnostics 摘要，直到 contract 补齐。

### Narrow Terminal Degradation

- 宽度不足时，subflow lane 退化为单列列表。
- 只保留当前 item 展开；已完成 item 强制折叠。
- status bar 保留 `execute-2 2/5` 这类最短摘要。

## Task Breakdown

### Task 15F-29: omega-session / omega-app / omega-tui — Step-Owned Subflow Presentation Contract

- 扩展 `RuntimeMessageEnvelope` 主路径，新增 step-owned subflow identity 与状态消息。
- 为 response section metadata 增加 optional `subflow_ref`，让 item-owned正文 / thinking / tool lane 能挂到父级 step block。
- 在 `omega-app` policy 与 `TuiEngine` 间定义最小 subflow surface API，并补 deterministic policy tests。

### Task 15B-28: omega-tui / omega-app — Execute Step Nested Subflow Timeline

- 在 `Response` 的 `execute` step block 内新增 `Subflow lane` 与 item run card。
- 让当前 item 展开、已完成 item 折叠、失败 item 保持展开。
- 同步接入底部状态带与 `Todo` 当前项高亮，让三个 surface 对齐同一 item identity。

### Task 15B-29: omega-tui — Subflow Detail Overlay And Navigation

- 为 item run 提供 detail overlay drill-down。
- 补齐 item run 间导航、折叠切换与选择态恢复。
- 让 overlay、Activity 与 Response 之间能稳定跳转到同一 item run。

## Testing Strategy

- `omega-session` / `omega-app`: `StepSubflowStatus` 与 response section metadata 的 producer/policy tests。
- `omega-tui`: parent step + subflow lane + item card 的 view-state/render tests。
- `omega-tui`: 窄终端退化、折叠策略、当前项切换与失败项展开 tests。
- integration: `RuntimeMessageEnvelope -> current-turn filter -> policy -> TuiEngine` matrix 补充 itemized execute coverage，覆盖 `queued -> running -> done`、`running -> failed`、`repeat` 与 `stale turn drop`。

## Risks

| Risk | Level | Mitigation |
|------|-------|------------|
| 把 item run 当成新的顶层 workflow step | High | 明确 parent step 永远存在，subflow 只在 step block 内显示 |
| 让 TUI 直接从 diagnostics/log 拼 subflow model | High | 必须先做 `Task 15F-29` contract，再做渲染 |
| `Todo` 与 `Response` 双重渲染导致重复噪音 | Medium | `Todo` 保持 checklist 真值，`Response` 只展示执行轨迹 |
| 已完成 item 全展开导致主阅读区过长 | Medium | 默认折叠已完成 item，只展开当前项与失败项 |

## Related Specs

- `docs/specs/omega-step-lifecycle-hooks.md`
- `docs/specs/omega-step-session-asset-model/session-context-and-data-contracts.md`
- `docs/specs/omega-runtime-message-pipeline.md`
- `docs/specs/omega-tui-runtime-experience.md`
- `docs/specs/omega-tui-response-thinking-experience.md`

---

### Change Log

- 2026-03-26: 初版规格，定义 itemized execute 在 TUI 中应作为 step-owned nested subflow 呈现，并拆出 contract / render / overlay 三段任务。
