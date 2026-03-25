---
status: active
owner: omega-team
created: 2026-03-25
updated: 2026-03-25
version: 1.0
supersedes: []
related_prds: []
---

# Omega Session Context And Data Contracts

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
| scene-recognition | None | Required(Json) |
| select-workflow | None | Required(Json) |
| chat | None | None |
| analysis | None | Required(Json) |
| plan | Required(["analysis"]) | Required(Json) |
| execute | Required(["plan"]) | Optional(Json) |
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

## Forward Direction: Hook-Driven Lifecycle Gate

下一阶段应把 `before_step`、`after_step` 与 advance 判定收敛为 step 生命周期事件，并允许 workflow step 绑定 `.omega/hooks/` 下的 Rust hooks。这样“何时允许离开当前 step”可以成为显式 contract，而不是继续散落在 runtime 特例中。

---

### Change Log

- 2026-03-25: 从入口规格中拆出 session context 与 data contract 主线。