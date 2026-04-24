---
content_revision: 120
created: 2026-03-25
generation_id: gen_000046_r000120
last_verified_commit: N/A
owner: omega-team
projection_version: 46
related_prds: []
source_doc_id: "spec:docs-specs-omega-step-session-asset-model-session-context-and-data-contracts"
status: active
supersedes: []
updated: 2026-03-25
---

# Omega Session Context And Data Contracts

## Overview

本文覆盖 `SessionContext`、step summaries、budget-aware context assembly 与 Step Data Contract 主线。

## Session Context Model

```rust
pub struct SessionContext {
    pub latest_user_turn: String,
    pub routing: RoutingContext,
    pub step_summaries: Vec<StepSummary>,
}

pub struct RoutingContext {
    pub recognized_scene_id: Option<String>,
    pub selected_workflow_id: Option<String>,
    pub active_workflow_id: String,
    pub active_workflow_role: WorkflowRunRole,
}

pub struct StepSummary {
    pub workflow_id: String,
    pub step_id: String,
    pub title: String,
    pub summary: String,
    pub estimated_tokens: u32,
}
```

约束：`StepSummary` 是 session-owned typed context，不等于直接复用上一步 assistant 原文。

## Context Budget Model

`.omega/model.toml` 通过 `[context].context_window` 提供完整上下文窗口，单次输出预算仍由 `[request].max_tokens` 控制：

```text
available_input_budget = context_window - max_output_tokens - safety_margin
```

当 summaries 超出预算时：

1. 保留最近一个 summary
2. 其余 summary 按从旧到新依次丢弃
3. routing context 不参与裁剪

## Summary Generation Strategy

- Phase 1: 截断路径，固定上限字符数并用 `len / 4` 估算 tokens
- Phase 2: 可引入独立 LLM 摘要路径

## Current Gap After Task 15F-9

当前实现已经能把文本摘要传给下一 step，但还不足以形成完整结构化闭环：

- `explore`、`plan`、`execute` 之间仍需更强结构化输入输出
- `report` 还不应只依赖 raw transcript 与文本 summary
- todo 需要成为 execute 阶段的正式完成语义，而不是旁路提醒

## Step Data Contract

结构化输入输出应作为 step 级通用能力，而不是硬编码到固定四阶段 artifact 槽位。

### Step Input Contract

```rust
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum StepInputContract {
    #[default]
    None,
    Required { sources: Vec<String> },
    Optional { sources: Vec<String> },
}
```

### Step Output Contract

```rust
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum StepOutputContract {
    #[default]
    None,
    Required {
        format: DataFormat,
        schema_path: Option<PathBuf>,
        max_retries: u32,
    },
    Optional {
        format: DataFormat,
        schema_path: Option<PathBuf>,
    },
}
```

### SessionContext Extension

```rust
pub struct SessionContext {
    pub latest_user_turn: String,
    pub routing: RoutingContext,
    pub step_summaries: Vec<StepSummary>,
    pub step_outputs: BTreeMap<String, serde_json::Value>,
}
```

### StepExecutionInput / Result Extension

```rust
pub struct StepExecutionInput {
    pub structured_input: Option<serde_json::Value>,
}

pub struct StepExecutionResult {
    pub structured_output: Option<serde_json::Value>,
}
```

## TOML Configuration Shape

```toml
[[steps]]
id = "plan"
input_contract = { mode = "required", sources = ["analysis"] }
output_contract = { mode = "required", format = "json", max_retries = 2 }
```

## Builtin Step Contract Defaults

| Step | Input | Output |
|------|-------|--------|
| select-workflow | None | Required(Json) |
| chat | None | None |
| analysis | None | Required(Json) |
| plan | Required(["analysis"]) | Required(Json) |
| execute | Required(["plan"]) for `feature` / `deep-research`; Required(["analysis"]) for lightweight `research` | Optional(Json) |
| report | Required(["analysis","plan","execute"]) | None |

## Validation And Prompt Injection

输出校验流程：

1. 从 step 最终文本提取 JSON
2. 按需做 schema/required-key 校验
3. `Required` 失败时进入恢复流程，成功后才允许写入 `step_outputs`
4. `Optional` 失败时可继续，但 `structured_output = None`

输入校验流程：

1. 从 `step_outputs` 读取所需 sources
2. `Required` source 缺失则阻止启动
3. `Optional` source 缺失则注入已有部分

当 step 拥有 data contract 时，system prompt 自动追加：

- `<structured_input>` 段
- `<output_contract>` 段

## Todo-Driven Execute Contract

feature workflow 的目标链路是：

1. `explore` 产出 objective / findings / constraints / risks
2. `plan` 消费 explore 结果并产出 ordered tasks / validation targets
3. `plan` 输出映射到 `TodoManager`
4. `execute` 围绕 todo items 推进
5. `report` 消费 explore/plan/execute 的结构化结果组织结论

当前默认只读分析 workflow 已拆成两条：

- `research`: `explore -> execute -> report`，适合聚焦型、较轻量的只读分析；`execute` 直接消费 `explore` 产物，不建立 plan/todo loop。
- `deep-research`: `explore -> plan -> execute -> report`，适合系统性、全局性、深入式的只读分析；保留只读 plan/todo loop，并要求 `changed_paths` 始终为空。

## Execute Loop Diagnostics Follow-up

当前 `Task 15F-11 / 15F-12 / 15F-22` 已让 `execute` 能围绕 todo state 重试、同步与展示，但 diagnostics 仍主要停留在“step 级是否通过/是否重试”的粒度。下一轮应把 execute 的 todo progress 提升为 session-owned typed diagnostics，而不是继续依赖用户从 raw todo 面板和 warning 文本自行推断。

### Desired Execute Diagnostics Fields

建议在现有 step diagnostics 上扩展 execute-specific 字段：

- `todo_total: usize`
- `todo_completed: usize`
- `todo_open: usize`
- `current_todo_ids: Vec<String>`
- `repeat_count: u32`
- `no_progress_streak: u32`
- `max_step_repeats: u32`
- `completion_source: Option<String>`

这些字段既应进入 tracing，也应进入前端可见的 Contract Diagnostics / detail overlay，使用户可以回答：

- 当前 execute 一共有多少代办
- 已完成几个
- 目前卡在哪个 item
- 这一轮为什么继续重试或为什么允许前进

### Diagnostics Structure Design

为避免 15F-26（step 级观测）与 15F-28（item 级 loop）之间的结构断裂，诊断字段从一开始就应设计为支持 optional item 粒度。推荐在 `StepDiagnostics` 上新增 optional 嵌套结构，而不是在 step 级平铺所有 execute 字段：

```rust
pub struct StepDiagnostics {
    // ...existing fields...
    pub execute_progress: Option<ExecuteProgressDiagnostics>,
}

pub struct ExecuteProgressDiagnostics {
    pub todo_total: usize,
    pub todo_completed: usize,
    pub todo_open: usize,
    pub current_item_id: Option<String>,   // None in 15F-26, Some in 15F-28
    pub current_item_index: Option<usize>,  // None in 15F-26, Some in 15F-28
    pub current_item_total: Option<usize>,  // None in 15F-26, Some in 15F-28
    pub repeat_count: u32,
    pub no_progress_streak: u32,
    pub max_step_repeats: u32,
    pub max_item_repeats: Option<u32>,      // None in 15F-26, Some in 15F-28
    pub completion_source: Option<String>,
}
```

这样 15F-26 填充 step 级 todo 汇总与 repeat 指标（`current_item_id = None`），15F-28 只需补充 item 字段，不需要重写诊断输出路径。

### Itemized Execute Loop Context

当 `execute` 进入基于 todo/list 的外层循环后，runner 需要持有当前 item run 的瞬态 identity。注意这是 runner runtime 的瞬态视图，不应写入持久化 `SessionContext`（`SessionContext` 是跨 step 持久上下文，item run 是当前 step 的运行态）：

```rust
/// Runner-owned transient state, not persisted in SessionContext.
pub struct ExecuteItemRun {
    pub parent_step_id: String,
    pub child_step_id: String,
    pub todo_id: String,
    pub index: usize,
    pub total: usize,
}
```

这类结构可作为 diagnostics、activity、summary 和后续 hook context 的共同来源。对用户可见的语义应是：父级 step 仍然是 `execute`，但当前 item run 是 `execute-1`、`execute-2` 这类稳定子 id。

Loop source 解析由 `runner.rs` 拥有，封装为 `resolve_loop_items()` 纯函数，从 `step_outputs["plan"]` 与 `TodoManager` 推导 item 列表（详见 `omega-step-lifecycle-hooks.md` Runtime Behavior 节）。

### Forward Direction

下一轮不应继续把 todo-driven repeat 理解为“整个 `execute` step 再来一次”。更稳定的方向是：

- `plan.tasks` / todo state 决定外层 item loop
- 单个 item run 继续使用现有 `agent_loop + tool_request + output_contract + hook gate`
- 当前 item 完成后再切换到下一个 item
- 只有所有 required items 完成后，父级 `execute` 才允许进入 `report`

这会把 `execute` 从当前的 whole-step repeat 收敛为 itemized execute loop，同时保留共享的 `SessionContext`、todo state 与 step-scoped storage。

## Forward Direction: Hook-Driven Lifecycle Gate

下一阶段应把 `before_step`、`after_step` 与 advance 判定收敛为 step 生命周期事件，并允许 workflow step 绑定 `.omega/hooks/` 下的 Rust hooks。这样“何时允许离开当前 step”可以成为显式 contract，而不是继续散落在 runtime 特例中。

---

### Change Log

- 2026-03-26: 架构评审后补充：`ExecuteProgressDiagnostics` 嵌套结构预留 optional item 粒度，避免 15F-26→28 断裂重写；`ExecuteItemRun` 明确为 runner-owned 瞬态视图而非 `SessionContext` 持久字段；loop source resolution 指向 `runner.rs` 与 lifecycle hooks spec。
- 2026-03-26: 补充 execute loop diagnostics follow-up，定义 execute-specific todo progress 字段与 itemized execute loop 的 session-owned identity 方向。
- 2026-03-25: 从入口规格中拆出 session context 与 data contract 主线。
