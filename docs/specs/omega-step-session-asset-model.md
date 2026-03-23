---
status: draft
owner: omega-team
created: 2026-03-20
updated: 2026-03-23
version: 0.1
supersedes: []
related_prds: []
---

# Omega Step And Session Asset Model Specification

## Overview

当前 `omega-workflow` 已经能表达四阶段工作流，并把每个阶段的 prompt 外置到 `.omega/prompt/step/*.md`。下一阶段不再把“flow”理解成一套独立于现有 `step` 的新概念，而是明确把现在的 `step` 视为工作流中的最小执行单元，只是在讨论时曾临时把它叫作 flow。

本规格的下一阶段目标，是在保持 `step` 术语不变的前提下，把当前固定四阶段 step 提升为通用 step definition，并把 session 级资产与 step context 一起收敛到 `omega-session`。`omega-session` 不仅统一管理 tools 与 skills 这类会话内资产，也负责把 routing state、历史 step summary、本轮用户输入与当前 step 的功能提示拼装成下一个 step 的执行上下文。

下一阶段还会在此基础上加入 scene-aware routing：`scene-recognition` 与 `select-workflow` 也将被建模为 step，而不是 session 外围的特殊预处理逻辑；未来个别 step 也可能触发 child workflow delegation。相关规划见 [docs/specs/omega-scene-routing.md](omega-scene-routing.md)。

截至 2026-03-20，Task 15F-2B 与 Task 15F-8 已完成首轮落地：`omega-workflow` 内部模型已从 enum-centric 四阶段切换为 string-keyed `WorkflowStep`，`WorkflowPrompts` 已改为按 `step_id` 查找的映射结构，`omega-session` 已基于统一的 bounded `AgentLoop` 驱动通用 step 编排，`SessionUpdate::WorkflowStepChanged` 也已补齐稳定的 `step_id` 字段；当前 root / chat / feature step 已全部进入同一种最小循环，只通过工具子集、loop budget 和 prompt 约束区别行为。下一轮主线将继续在此基础上引入结构化 step context。

## Goals

- 保持 `step` 作为 workflow 的主术语，不再引入与之平行的新概念名词。
- 将当前固定四阶段提升为可配置的 `WorkflowStepDefinition` 列表。
- 让 `omega-session` 成为 tools 与 skills 的统一资产管理入口。
- 让 step 运行时默认继承 session 资产，并支持增补、屏蔽和按需加载。
- 让所有 root / child workflow step 都以有界最小 agent loop 运行，而不是把 analysis / plan / report / routing step 固定成无工具单响应。
- 引入 session-owned 的 step context，让后续 step 明确消费“之前任务总结的上下文”，而不是只隐式依赖 raw message history。
- 为未来的 subagent、team 等会话内执行体复用同一套资产管理机制。
- 为 future scene-aware child workflow delegation 预留稳定的 step-level 扩展点。

## Non-Goals

- 本次规划不要求实现 DAG、条件分支或循环回边。
- 本次规划不要求一步做成完整的持久化 artifact store、跨 session memory graph 或数据库化上下文仓库。
- 本次规划不要求把 `omega-tui` 变成 step 运行器；TUI 仍然只消费状态更新。未来若引入 `omega-app` 装配层，也由 app 负责 session/tui wiring，而不是改变 step 领域所有权。
- 本次规划不要求现在拆出新的 crate；优先在现有 `omega-workflow`、`omega-session`、`omega-core` 边界内演进。

## Terminology

- `step`: workflow 中的最小执行单元，也是当前和后续实现的正式术语。
- `workflow`: 有序 step 序列。
- `session assets`: 当前 session 可分配给 step、subagent、team 的 tools 与 skills 能力集合。
- `flow`: 仅保留为历史讨论中的口语说法；在规格和实现中不再作为主名词。

## Problem Statement

当前模型存在几个扩展性问题：

- `omega-session` 仍然按固定四阶段写执行分支，新增第五个阶段时需要修改 orchestration 代码。
- tools 与 skills 目前是初始化时一次性装配，没有 session 级统一分配边界。
- step 还不能表达“继承默认能力、追加能力、屏蔽能力、能力不足时触发加载”的运行时诉求。
- 当前所有 step 已统一进入 bounded agent loop，但 step 之间仍缺少 session-owned 的结构化 summary/context 传递。
- root routing step 当前仍主要依赖 assistant 自由文本 + token matching 传递 scene / workflow 决策，语义过弱。
- 后续 step 还拿不到显式的前序任务总结，只能依赖 raw message history 或隐含 routing 状态。

## Architectural Direction

### Core Decision

保持 `omega-workflow` 拥有 step definition 与 workflow 顺序，`omega-session` 拥有 session 资产管理与 step 编排，`omega-core` 继续只提供底层 agent loop 与工具调用能力，`omega-tui` 继续只展示当前 step。

### Ownership

- `omega-workflow`: 拥有 `WorkflowStepDefinition`、workflow 配置、step prompt 文件加载。
- `omega-session`: 拥有 `SessionToolCatalog`、`SessionSkillCatalog`、step 进入/退出、step 资产解析与状态更新协议。
- `omega-core`: 拥有底层 `Agent`、single-response、tool-loop、工具执行，不理解 workflow 结构。
- `omega-tui`: 只消费结构化 step 更新，不持有 workflow 运行逻辑。
- `omega-app`: 负责把 `omega-session` 产出的 step/update channel 装配到具体前端，不拥有 step policy。

这与当前仓库的交互层边界一致，避免把非 UI 运行态重新塞回 `omega-tui`，也避免未来把 session 资产策略错误塞进 `omega-app`。

## Proposed Model

### WorkflowStepDefinition

```rust
pub struct WorkflowStepDefinition {
    pub id: String,
    pub label: String,
    pub prompt_path: PathBuf,
    pub loop_mode: StepLoopMode,
    pub max_iterations: u32,
    pub tool_request: StepToolRequest,
    pub skill_request: StepSkillRequest,
    pub enabled: bool,
}

pub enum StepLoopMode {
    AgentLoop,
}

pub enum StepToolRequest {
    Inherit,
    Extend(Vec<String>),
    Block(Vec<String>),
}

pub enum StepSkillRequest {
    MatchTask,
    Append(Vec<String>),
    Disable,
}
```

默认 `analysis / plan / execute / report` 仍然是内建 step，但执行器不再依赖固定分支名才能工作。当前实现已经将主路径统一到 `AgentLoop`；`SingleResponse` / `ToolLoop` 只保留为配置兼容别名，不再代表运行时分叉。

### WorkflowDefinition

```rust
pub struct WorkflowDefinition {
    pub name: String,
    pub steps: Vec<WorkflowStepDefinition>,
}
```

这里保留 `steps` 作为配置和模型字段名，不再把外部配置迁移成 `flows`。

## Session Asset Management

### SessionToolCatalog

session 统一持有默认工具集，并提供 step 级能力解析：

- 默认继承当前 session 可用工具
- `Extend(...)` 在继承基础上尝试追加工具
- 若工具尚未装入但存在已知 loader，则由 session 侧触发装载
- 被 `Block(...)` 标记的工具不会出现在该 step 的可见能力中

### SessionSkillCatalog

session 统一持有 skills 描述与加载器，并负责：

- 默认按当前任务做 matched preloading
- 根据 step 的 `Append(...)` 追加显式 skills
- `Disable` 时关闭该 step 的自动 skill 装配

这样一来，skills 不再只是 workflow 的局部技巧，而是 session 资产层的一部分；后续 subagent、team 同样可以复用。

## Session Context Model

session 需要拥有一份可组合、可总结、可供后续 step 消费的上下文，而不再只把上下文寄托在 raw transcript 里：

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
    /// 粗略 token 估算（4 chars ≈ 1 token），用于 context budget 裁剪。
    pub estimated_tokens: u32,
}

pub struct StepExecutionInput {
    pub base_system: String,
    pub resolved_tools: ResolvedToolSet,
    pub resolved_skills: ResolvedSkillSet,
    pub session_context: SessionContext,
    pub step: WorkflowStepDefinition,
    pub step_prompt: String,
}

pub struct StepExecutionResult {
    pub final_text: String,
    pub summary: StepSummary,
    pub transition: StepTransition,
}

pub enum StepTransition {
    Continue,
    StartWorkflow { workflow_id: String },
    FinishTurn,
    /// Step 执行失败时，session 仍需要完成 turn 清理。
    /// 首轮只做 turn 终止；后续可扩展为 retry / fallback policy。
    Error { message: String },
}
```

约束：

- step summary 是 session-owned 的 typed context，不等于直接复用上一个 step 的整段 assistant 原文。
- 后续 step 默认读取 `SessionContext.step_summaries`，而不是假设模型会从长 transcript 中自行稳定找回关键结论。
- routing 结果同样进入 `SessionContext.routing`，而不是只通过弱结构化自由文本在 root workflow 内传递。

### Context Budget Model

`SessionContext` 注入到 step system prompt 时，不能无限增长。session 必须在组装 step 输入之前进行 budget-aware 裁剪。

**配置来源**：`.omega/model.toml` 新增 `[context]` section，提供 `context_window` 字段（默认 200000），表示模型完整上下文窗口大小。`[request].max_tokens` 保持现有语义，仅控制单次 assistant 响应预算。两者关系为：

```
available_input_budget = context_window - max_output_tokens - safety_margin
```

其中 `safety_margin` 为固定保守值（如 2000 tokens），避免边界溢出。

**裁剪策略**：当 `step_summaries` 的 `estimated_tokens` 总和加上 system prompt 与 step prompt 超过 `available_input_budget` 时，session 应从最早的 summary 开始丢弃或截断，直到总量回到预算内。具体规则：

1. 保留最近一个 step 的 summary 不裁剪（保证工作链连续性）
2. 剩余 summary 按时间从旧到新依次丢弃
3. routing context 不参与裁剪（体积固定且很小）

这不要求精确 tokenizer，粗估（4 chars ≈ 1 token）在首轮即可满足。后续若需精确 token 统计，可接入 tiktoken 或类似库。

### Summary Generation Strategy

`StepSummary.summary` 不等于 step 的原始 assistant 全文：

1. **截断路径**（首轮实现）：从 step 最终文本中截取前 N 个字符（如 2000 chars ≈ 500 tokens），作为 summary。
2. **LLM 摘要路径**（后续扩展）：对长文本调用独立的 summarization prompt 生成结构化摘要。
3. `estimated_tokens` 在截断路径下直接由 `summary.len() / 4` 计算。

分阶段迁移：

- **Phase 1**（Task 15F-9）：截断路径，固定 2000 chars 上限。
- **Phase 2**（独立任务）：引入可配置的 summary strategy（truncate / llm-summarize），作为 `omega-compression` 的消费场景。

### Current Gap After Task 15F-9

当前实现只完成了“step 之间可传递文本摘要”的最小链路，还没有完成 workflow-owned 的结构化上下文闭环。现状限制如下：

- `analysis` 产物仍只是 `StepSummary.summary` 的截断文本，不是结构化分析资产。
- `plan` 没有稳定产出 ordered tasks / validation targets；它只是给 `execute` 暴露一段 plan 文本摘要。
- `execute` 没有消费 workflow-owned task queue；当前 `todo` 只是通用工具，`omega-core` 只会在 3 轮未更新时注入 reminder，不会把 `todo` 变成 execute 的完成判据或循环锚点。
- `report` 当前主要依赖 raw transcript 与前序文本 summary，不会稳定读取“完成了哪些 planned items、哪些验证通过/失败、还有哪些 open items”这类结构化执行结果。

因此，当前 `SessionContext` 足以支撑“上一阶段给下一阶段一个文本提示”，但还不足以满足 `analysis -> plan -> execute -> report` 逐阶段通过结构化上下文闭环协作的目标。

### Next-Stage Workflow Context Evolution

下一阶段需要把 `SessionContext` 从"routing + text summaries"演进为"routing + step data contracts + structured outputs + diagnostics"的 session-owned 工作上下文。

#### Step Data Contract

之前的设计（`WorkflowArtifacts { analysis, plan, execution }` 硬编码槽位）把结构化数据绑定到 feature workflow 的固定四阶段，无法复用于其他 workflow 形状或自定义 step。重新审查后，结构化输入输出应作为 **step 级通用能力**，而不是某个 workflow 的专属特性。

核心设计原则：

1. **每个 step 独立声明其输入输出数据契约**，而不是由 session 维护一套全局 artifact 类型。
2. **输入和输出分开配置**，支持四种组合：(None, None)、(None, Required)、(Required, None)、(Required, Required)，以及 Optional 变体。
3. **当 Required 数据缺失时，有明确的流程级处理**：输入缺失 → 阻止启动；输出缺失 → 重试+反馈 → 超限后 Error。
4. **结构化数据需要自测**：系统级 JSON 提取与 schema 校验 + LLM 收到校验反馈后自行修正。

##### Step Input Contract

```rust
/// 声明 step 对前序 step 结构化输出的消费需求。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum StepInputContract {
    /// 不需要结构化输入（如 analysis 直接读用户请求）。
    #[default]
    None,
    /// 必须从指定 source step 获取结构化输出；任一 source 缺失则阻止启动。
    Required { sources: Vec<String> },
    /// 如果 source step 有结构化输出则注入，缺失不阻塞。
    Optional { sources: Vec<String> },
}
```

##### Step Output Contract

```rust
/// 声明 step 必须/可选产出的结构化输出及校验策略。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum StepOutputContract {
    /// 不产出结构化输出（如 report 只产出自由文本摘要）。
    #[default]
    None,
    /// 必须产出符合指定格式的结构化输出，校验失败可重试。
    Required {
        format: DataFormat,
        schema_path: Option<PathBuf>,
        max_retries: u32,
    },
    /// 尝试产出结构化输出，校验失败不阻塞流程。
    Optional {
        format: DataFormat,
        schema_path: Option<PathBuf>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DataFormat {
    #[default]
    Json,
}
```

##### WorkflowStep 扩展

```rust
pub struct WorkflowStep {
    // ... existing fields ...
    pub input_contract: StepInputContract,
    pub output_contract: StepOutputContract,
}
```

##### SessionContext 扩展

```rust
struct SessionContext {
    pub latest_user_turn: String,
    pub routing: RoutingContext,
    pub step_summaries: Vec<StepSummary>,
    /// step_id → 该 step 产出的结构化输出，供后续 step 的 input_contract 消费。
    pub step_outputs: BTreeMap<String, serde_json::Value>,
}
```

`StepSummary` 继续保留，承担 budget-aware 的文本压缩摘要职责。`step_outputs` 负责精确的结构化数据传递，二者互补。

##### StepExecutionInput / Result 扩展

```rust
struct StepExecutionInput {
    // ... existing fields ...
    /// 从 input_contract.sources 解析合并后的结构化输入。
    pub structured_input: Option<serde_json::Value>,
}

struct StepExecutionResult {
    // ... existing fields ...
    /// 从 step 最终输出中提取并校验后的结构化输出。
    pub structured_output: Option<serde_json::Value>,
}
```

##### TOML 配置格式

与现有 `tool_request` / `skill_request` 风格一致，使用内联表：

```toml
[[steps]]
id = "analysis"
label = "Analyze"
prompt = ".omega/prompt/step/analysis.md"
max_iterations = 8
tool_request = { mode = "block", items = ["bash", "edit_file", "todo", "write_file"] }
output_contract = { mode = "required", format = "json", max_retries = 2 }

[[steps]]
id = "plan"
label = "Plan"
prompt = ".omega/prompt/step/plan.md"
max_iterations = 8
input_contract = { mode = "required", sources = ["analysis"] }
output_contract = { mode = "required", format = "json", max_retries = 2 }

[[steps]]
id = "execute"
label = "Execute"
prompt = ".omega/prompt/step/execute.md"
max_iterations = 16
input_contract = { mode = "required", sources = ["plan"] }
output_contract = { mode = "optional", format = "json" }

[[steps]]
id = "report"
label = "Report"
prompt = ".omega/prompt/step/report.md"
max_iterations = 8
input_contract = { mode = "required", sources = ["analysis", "plan", "execute"] }
```

##### 内建 step 默认 I/O Contract

| Step               | Input Contract               | Output Contract              |
|--------------------|------------------------------|------------------------------|
| scene-recognition  | None                         | Required(Json, max_retries=1)|
| select-workflow    | None                         | Required(Json, max_retries=1)|
| chat               | None                         | None                         |
| analysis           | None                         | Required(Json, max_retries=2)|
| plan               | Required(["analysis"])       | Required(Json, max_retries=2)|
| execute            | Required(["plan"])           | Optional(Json)               |
| report             | Required(["analysis","plan","execute"]) | None            |

注意：root routing steps（scene-recognition / select-workflow）**已经在 `finalize_step()` 中做结构化 JSON 解析**，只是当前是硬编码的特殊分支。Step Data Contract 将把这套逻辑泛化为通用机制，使 root routing 成为 data contract 的首个消费者而不是唯一特例。

#### Validation & Flow-level Handling

##### 输出校验流程

step 完成后，若 `output_contract` 不是 `None`：

1. **JSON 提取**：从 `final_text` 中尝试提取结构化数据
   - 优先查找 ` ```json ... ``` ` 代码块
   - 若无代码块，尝试整体 `serde_json::from_str()`
   - 若都失败，标记为提取失败

2. **Schema 校验**（若 `schema_path` 存在）：
   - 加载 JSON Schema 文件，对提取到的 JSON 做结构校验
   - 首轮实现可只做"是否为合法 JSON + 是否包含必须 key"级别的轻量校验
   - 后续可引入完整 JSON Schema validator

3. **失败处理**：
   - 若 `Required` 且校验失败且重试次数未用尽：
     - 向 agent conversation 注入一条系统反馈消息，描述校验失败原因
     - 不 advance step，重新进入 agent loop 的下一轮迭代
     - 这就是"自测"：LLM 收到自己输出的校验结果，自行修正
   - 若 `Required` 且重试耗尽：`StepTransition::Error { message }`
   - 若 `Optional` 且校验失败：继续流程，`structured_output` 设为 `None`

##### 输入校验流程

step 启动前，若 `input_contract` 不是 `None`：

1. 从 `session_context.step_outputs` 中查找 `sources` 列出的所有 step_id
2. 若 `Required` 且任一 source 缺失 → 立即 `StepTransition::Error`
3. 若 `Optional` 且部分缺失 → 只注入已有的 source
4. 将收集到的结构化数据合并为 `structured_input`（按 source step_id 为 key 的 JSON object）

##### Prompt 自动注入

当 step 拥有 data contract 时，`build_step_system_prompt()` 自动追加：

- 若有 `output_contract`：追加 `<output_contract>` 段，描述期望格式与 schema 要求
- 若有 `structured_input`：追加 `<structured_input source="...">` 段，注入前序 step 的结构化输出

这使得 step prompt 文件不需要手动重复格式要求；格式约束由 data contract 机制统一注入。

#### Todo-Driven Execute Contract

Step Data Contract 建立通用框架后，feature workflow 的 `plan → execute → report` 链路可基于此实现具体的领域绑定：

1. `analysis` 产出结构化输出（objective / constraints / risks / affected paths），通过 `output_contract: Required(Json)` 保证
2. `plan` 消费 analysis 输出，产出结构化计划（ordered tasks / validation targets），通过 `input_contract: Required(["analysis"])` + `output_contract: Required(Json)` 保证
3. `plan` 的结构化输出中的 tasks 可映射到 `TodoManager`，使 todo 成为 workflow context 的正式投射而非旁路
4. `execute` 消费 plan 输出，以 todo list 为主循环锚点推进，每次聚焦当前 `in_progress` item
5. `execute` 可选产出执行结果（completed tasks / validation results / changed paths），通过 `output_contract: Optional(Json)` 声明
6. `report` 消费 analysis + plan + execute 的结构化输出，组织最终总结，通过 `input_contract: Required(["analysis","plan","execute"])` 保证

todo 不再只是通用工具旁路；它需要和 workflow context 建立稳定映射：

- `plan` 产出的 ordered tasks 可直接映射到 `TodoManager`
- `execute` 的完成条件与 todo completion 语义对齐
- `report` 能稳定看到哪些 items 完成、哪些未完成、哪些验证失败

#### Context Observability

当 workflow context 演进为 step data contract + structured outputs，就必须具备可观察性，否则 TUI 中的 failure 仍然只能看到"模型说了什么"，而看不到"session context 里到底存了什么"。

截至 2026-03-23，Task 15F-12 已完成首轮落地，当前观测面包含：

- 在 tracing / jsonl 中记录每次 step 输入时注入了哪些 summaries + structured inputs
- 记录 output contract 校验结果（成功/失败/重试次数）
- 记录各阶段对 `SessionContext.step_outputs` 的 snapshot/diff 写入变化
- 记录 plan-generated todo 与 execute-completed todo 的 before/after 变化轨迹
- 在 TUI Diagnostics 侧栏与 detail overlay 中提供 data contract 状态 drill-down

当前实现已足以回答“这个 step 读了什么、写了什么、校验是否通过”；后续扩展重点转向更长期的 compression / retention 策略，而不是继续补一套新的 diagnostics 主路径。

### Routing State Convergence

当前 `omega-session` 内部使用 `WorkflowRoutingState { recognized_scene_id, selected_workflow_id }` 驱动 root workflow 执行。spec 同时定义了 `SessionContext.routing: RoutingContext`，两者存在语义重叠。

**收敛决策**：`RoutingContext` 替代 `WorkflowRoutingState` 成为运行时唯一的路由状态容器。`WorkflowTurnRunner` 内部操作 `RoutingContext`，root step 完成后同步更新，组装 child step 输入时从 `SessionContext.routing` 取值。不允许两份路由状态独立存在。

这同时消除了 `build_step_system_prompt()` 的散落参数问题：当前函数接受 `routing_context: Option<&str>` 等独立参数，后续应统一为 `StepExecutionInput` 作为单一输入，函数签名收敛为：

```rust
fn build_step_system_prompt(input: &StepExecutionInput) -> String;
```

### Why Session Owns Assets

- tools 与 skills 都是“会话内可分配能力”，本质上不是某一个 step 私有。
- 若把工具和技能配置直接塞进 step runner，会导致 step、subagent、team 分别复制一套装配逻辑。
- 由 `omega-session` 统一管理，可避免后续出现多套资产解析规则。
### Structure Guard: 避免 God Object

`SessionToolCatalog` 与 `SessionSkillCatalog` 必须作为**独立的组合型结构体**存在，而不是直接在 `AgentSession` 上追加方法：

- 各 catalog 独立文件、独立单元测试
- 提供纯方法 `resolve_for_step(step: &WorkflowStepDefinition) -> ResolvedToolSet`，不持有可变会话状态
- `AgentSession` 通过组合（`tool_catalog: SessionToolCatalog`）持有它们，只调用 catalog.resolve，不自己处理 Inherit/Extend/Block 逻辑
- 后续 `ResolvedToolSet` 同样适用于 subagent 和 team 场景

### Multi-Consumer Readiness

当前 `AgentSession` 使用单 agent slot（`agent.take()` 独占），整个 turn 期间无法启动第二个 agent 执行。后续 subagent / team 会需要多个并发执行体。

资产 catalog 的设计应预留多消费者访问能力：

- catalog 设计为 `Arc<SessionToolCatalog>`（只读 resolve），不持有可变状态
- 可变部分（如 lazy-load 新工具）用内部锁或 channel 隔离
- resolve 方法为纯函数，不修改 catalog 内部状态
## Agent Dynamic Tool Switching

当前 `omega-core::Agent` 在构造时接受 `ToolDispatcher`，生成 `tool_definitions: Vec<ToolDefinition>` 后一次性固化。运行时没有方法切换可见工具集。要让 step 级资产解析生效，`omega-core` 必须暴露动态工具切换 API。

### 核心问题

Agent 内部存在两份需要同步切换的状态：

- `dispatcher: ToolDispatcher` — 实际路由工具调用
- `tool_definitions: Vec<ToolDefinition>` — 发给 LLM 的 schema 列表

这两份状态必须一致：LLM 能看到的工具必须都能被 dispatcher 执行。

### 方案：过滤式 API

在 `omega-core::Agent` 上新增方法，从已注册工具中过滤可见子集：

```rust
impl Agent {
    /// 切换本轮可见工具集。`names` 为 None 时暴露全部已注册工具。
    /// 返回实际生效的工具名列表，忽略未注册的名字。
    pub fn set_visible_tools(&mut self, names: Option<&[&str]>) -> Vec<String>;
}
```

配合 `ToolDispatcher` 新增辅助方法：

```rust
impl ToolDispatcher {
    /// 返回仅包含指定工具名的 schema 子集（顺序稳定）。
    pub fn to_schemas_filtered(&self, names: &[&str]) -> Vec<Value>;
}
```

### 设计约束

- `ToolDispatcher` 本身**不变**——仍然持有全部已注册 handler，dispatch 时仍然路由到完整 handler 集合。过滤只影响 LLM 看到的 `tool_definitions`，不影响执行路由。这样即使 LLM 意外返回了 blocked 工具的调用，dispatcher 仍能返回结果（但 session 层可选择拒绝）。
- `set_visible_tools(None)` 恢复为全量工具，作为安全默认值。
- `run_single_response` 可继续作为 `omega-core` 的低层兼容 API 保留，但 workflow 主路径不应再依赖它承载 step 编排。

### 前置依赖

此 API 应在 Task 15F-2A Step 4 实现，作为 `omega-session` 资产层调用 `omega-core` 的唯一依赖点。

## Execution Model

每次进入一个 step，`omega-session` 必须显式组装该 step 的执行输入：

- 当前 session message history
- 基础 system prompt
- 从 `SessionSkillCatalog` 解析得到的 step skill 片段
- 从 `SessionToolCatalog` 解析得到的 step tool 集合
- 当前 `SessionContext`，其中至少包含：latest user turn、routing state、之前 step 的 summary
- 当前 step 的功能身份（step id / label）与 step prompt

然后所有 step 都走同一条有界最小循环：

1. `agent.set_visible_tools(Some(&resolved_names))`
2. 按当前 step 的 `max_iterations` 进入 bounded agent loop
3. LLM 产生 response
4. 若 `stop_reason == tool_use`，执行工具、追加结果并继续 loop
5. 若不是 `tool_use`，以该 step 的最终文本结束本 step
6. session 从本 step 最终结果中生成 `StepSummary` 与 `StepTransition`
7. 将 summary 写回 `SessionContext`，再进入下一 step 或 child workflow

这意味着 root routing、chat、analysis、plan、execute、report 共享同一种 loop contract，只是工具子集、预算和 prompt 约束不同，而不是继续拆成两套执行语义。

## Forward Direction: Scene-Aware Workflow Delegation

scene-aware routing 的下一阶段要求是：workflow selection 本身也被表达为 step，而不是额外平行机制。这带来两个边界要求：

- `scene-recognition` 与 `select-workflow` 作为 root workflow step，仍然由 `omega-session` 用通用 step runner 执行。
- 当某个 step 需要触发 child workflow 时，session 应维护 workflow stack / active workflow state，而不是把 delegation 逻辑散落到 UI 或 app shell。

因此，后续模型应允许 step completion 保留 `StartWorkflow { workflow_id }` 这类通用过渡语义，并把 scene recognition / workflow selection 的结果先写入 `SessionContext.routing`。首轮 scene routing 只要求 `select-workflow` 使用它，但不应把它设计成不可复用的专用分支。

## Step Identity Model: Enum → String Migration

当前 `omega-workflow` 使用闭合枚举 `WorkflowStepKind { Analysis, Plan, Execute, Report }` 作为 step 身份标识，TOML 配置的 `id` 字段直接反序列化为该枚举。这意味着任何非四值 id 都会被配置校验拒绝。

规格中 `WorkflowStepDefinition` 提出 `id: String`，要支持任意自定义 step。这是一次隐含的断裂式变更，涉及：

- `WorkflowPrompts` 从固定 4 字段 → `HashMap<String, String>`
- `prompt_for(kind: WorkflowStepKind)` → `prompt_for(id: &str)`
- `build_step_system_prompt` 中 `step_kind.default_label()` / `step_kind.file_stem()` → 从 step definition 取 label 和 prompt path
- TOML 配置校验规则从 "id 只允许四值" → "id 是任意非空字符串"

### 分步迁移策略

**阶段 1（15F-2B 内完成）**: 泛化内部模型

- 将 `WorkflowStep.kind: WorkflowStepKind` 改为 `WorkflowStep.id: String` + `WorkflowStep.loop_mode: StepLoopMode`
- 保留 4 个 canonical id（`analysis`, `plan`, `execute`, `report`）作为内建默认值
- `WorkflowPrompts` 改为 `HashMap<String, String>`，内建默认仍覆盖 4 个 canonical 的 prompt
- 配置校验仍只允许 4 个 canonical id（向后兼容）

**阶段 2（15F-2B 后，独立小任务）**: 开放自定义 id

- 放开 TOML `id` 字段验证，允许任意 `[a-z0-9_-]+` 格式
- 自定义 step 的 prompt 通过 `prompt_path` 字段显式指定
- 新增配置校验：自定义 step 必须提供 `prompt_path`

不在一步内同时做模型泛化和开放自定义，避免迁移复杂度失控。

## Configuration Direction

`.omega/workflow.toml` 继续以 `steps` 为外部配置入口：

```toml
name = "default"

[[steps]]
id = "analysis"
label = "Analyze"
prompt = ".omega/prompt/step/analysis.md"
loop_mode = "agent_loop"
max_iterations = 6
tool_request = { mode = "inherit" }
skill_request = { mode = "match_task" }
enabled = true

[[steps]]
id = "execute"
label = "Execute"
prompt = ".omega/prompt/step/execute.md"
loop_mode = "agent_loop"
max_iterations = 12
tool_request = { mode = "inherit" }
skill_request = { mode = "match_task" }
enabled = true
```

后续新增 step 的操作变为：

1. 新建一个 step prompt 文件
2. 在 workflow 配置里定义一个 step
3. 通过 tool/skill request 说明它如何从 session 资产层取能力
4. 把它插入目标位置

## Session Integration

`omega-session` 需要演进出两类职责：

### 1. Session 资产层

- 管理默认 tools
- 管理可按需加载的附加 tools
- 管理 skills 描述、匹配与显式加载
- 对 step、subagent、team 暴露统一解析接口

### 2. Step 编排层

- 遍历 enabled `steps`
- 为每个 step 解析 tool/skill 资产
- 根据 `loop_mode` 调用 `omega-core`
- 发送结构化 step 状态更新
- 使用 `omega-workflow::WorkflowRun` 作为 step 编排的运行时容器（支持 advance/current/remaining 查询），替代当前 `for step in enabled_steps()` 的直接迭代

后续当 runtime-visible 能力继续增长时，step 编排层发给前端的更新不应继续停留在 workflow 专属 update 集合，而应收敛到统一 runtime UI message/effect contract。届时 `omega-session` 仍然是归一层，`omega-tui` 仍然只是消费者。

## Event Contract

当前 `SessionUpdate::WorkflowStepChanged` 使用 `step_label` 充当身份标识，没有独立的 `step_id`。当 step 支持自定义 label 后，两个不同 step 可能有相同 label（如用户把 analysis 和 plan 都标为 "Prepare"），TUI 或日志将无法区分。

15F-2B 中必须增加 `step_id` 字段：

```rust
SessionUpdate::WorkflowStepChanged {
    turn_id: u64,
    step_id: String,     // 稳定身份标识，对应配置中的 id
    step_label: String,  // 展示用名称
    index: usize,
    total: usize,
}
```

- `step_id` 用于日志、调试和程序化匹配
- `step_label` 仅用于 TUI 展示
- TUI 仍然只消费前端协议，但事件不再依赖 label 充当身份标识

## Context Direction

`context` 不再继续只做保留术语，而是进入下一轮主线；但首轮只做 session-owned 的 step summary 与 routing context，不一步扩展到完整 artifact 平台：

- 当前优先级是让后续 step 能稳定消费前序总结，而不是继续只依赖 raw transcript
- 首轮 context 写入以 `StepSummary` 与 `RoutingContext` 为主
- 更细粒度的 `analysis_notes`、`plan_outline`、`verification_summary`、structured artifact schema 可在其后继续扩展

## Migration Plan

### Phase 1: 全 step 最小循环

- 将内建 root / chat / feature workflow step 全部切换为 `AgentLoop`
- 让 analysis / plan / report / routing step 与 execute 共享同一套 loop contract
- 用 step-level tool filtering 和 max-iteration budget 控制行为，而不是通过 `SingleResponse` 物理分叉

### Phase 2: Session-owned Step Context

- 在 `omega-session` 中引入 `SessionContext`、`StepSummary` 与 `StepExecutionResult`
- 让每个 step 在完成后产出 summary，并显式写回 session context
- 让 root routing step 产出 typed routing context 与 `StartWorkflow` transition，替代弱结构化 token matching

### Phase 3: Step Data Contract Framework

- 为 `WorkflowStep` 引入 `StepInputContract` 与 `StepOutputContract`，使结构化 I/O 成为 step 级通用能力
- 在 `SessionContext` 中新增 `step_outputs: BTreeMap<String, Value>`，存储各 step 的结构化输出
- 实现 JSON 提取、output 校验、校验失败重试与 prompt 自动注入机制
- 为内建 step 设置默认 I/O contract（root routing / feature workflow / chat）
- TOML 配置新增 `input_contract` / `output_contract` 字段

### Phase 4: Feature Workflow Schema Binding & Todo Integration

- 定义 analysis / plan / execute 的具体输出 JSON schema
- 更新 step prompt 与 TOML 配置使用 data contract
- 让 `plan` 的结构化输出映射到 `TodoManager`
- 让 `execute` 围绕 todo item 推进，结果写回 structured output

### Phase 5: Context Observability And Compression

- 让 `omega-subagent` 与 `omega-compression` 直接建立在 `SessionContext + step data contracts` 之上，而不是重新发明上下文边界
- 在现有 context snapshot / diff observability 之上继续演进 compression / retention 策略，包括 data contract 校验状态的长期保留方式
- 支持更多 step 模板，如 `verify`、`delegate`、`summarize`
- 继续细化 schema 定义、tool lazy-loading 和 skill 显式装配策略

## Risks

- 如果把 session 资产层做成万能中心，会出现新的 God Object 风险。**缓解**: catalog 作为独立组合型结构体，暴露纯 resolve 方法，`AgentSession` 只持有不内联。
- 如果 tool 与 skill 的解析接口不统一，后续 subagent 和 team 仍会复制逻辑。**缓解**: `ResolvedToolSet` / `ResolvedSkillSet` 类型统一，任何消费者都通过相同接口获取。
- 如果把 context 直接扩成完整 artifact 平台，会把任务复杂度推高到不必要的程度。**缓解**: 首轮仅实现 `StepSummary` + `RoutingContext`，不同时引入完整 schema 仓库。
- `WorkflowStepKind` enum → `id: String` 是断裂式变更。**缓解**: 分两阶段迁移，15F-2B 内只泛化内部模型，保持 4 个 canonical id 配置兼容；之后独立小任务开放自定义。
- `Agent` 当前没有动态工具切换 API。**缓解**: 15F-2A Step 4 先落地 `set_visible_tools` + `to_schemas_filtered`，验证后再做资产层接入。
- 单 agent slot 模式与多执行体愿景有张力。**缓解**: catalog 设计为 `Arc` 只读 resolve，可变部分用内部锁隔离，不阻塞后续 subagent 并发。
- 如果 routing 继续依赖自由文本 token matching，root workflow 的稳定性会持续偏弱。**缓解**: 在下一轮 context/task 中引入 typed `StepSummary` / `StepTransition`，把 routing 结果写进 session context。

## Testing Strategy

- `omega-workflow` 单测：验证 step 配置解析、默认四阶段到通用 step definition 的兼容。
- `omega-session` 单测：验证 tool/skill 资产解析、step 继承/追加/屏蔽规则、不同 `loop_mode` 执行路径。
- `omega-core` 单测：验证 agent 能按 step 解析结果切换可见工具集。
- `omega-tui` 单测：验证底部状态栏继续展示当前 step，且不依赖硬编码四阶段名。

## Technical Decisions

| Decision | Choice | Rationale |
|---------|--------|-----------|
| primary term | `step` | 与当前实现一致，避免再引入第二套并行名词 |
| asset owner | `omega-session` | tools/skills 是会话内共享资产，应统一分配 |
| workflow owner | `omega-workflow` | step definition 与 prompt/config 仍属于 workflow 领域 |
| workflow delegation owner | `omega-session` | scene route 与 child workflow stack 仍然属于会话编排，而不是 UI / app shell |
| context scope | reserved only | 先避免把未来 artifact 设计绑进当前任务 |
| extensibility path | add step + resolve assets from session | 新增执行流程时，不再复制装配逻辑 |

---

### Change Log
- 2026-03-20: 初版规划，将当前固定四阶段 workflow 演进为由 `FlowDefinition` 组成的 ordered flow sequence，并预留 flow context、tool policy 与 skill policy。
- 2026-03-20: 根据进一步架构收敛，明确以 `step` 为正式术语，由 `omega-session` 统一管理 tools/skills 资产，`context` 延后单独设计。
- 2026-03-20: 15F-2A 首轮实现完成：`SessionToolCatalog` / `SessionSkillCatalog` 已落地，`omega-core::Agent` 已支持 `set_visible_tools`，当前固定四阶段 runner 已通过 session 资产层切换工具可见性并保持既有行为稳定。
- 2026-03-20: 补充下一阶段方向：scene-aware routing 仍以 step 和 session 编排为核心，`select-workflow` 只是首个 child workflow delegation step，而不是例外机制。
- 2026-03-20: 将“所有 step 进入有界最小 agent loop”和“session-owned step context”提升为下一主线，明确后续 step 输入应由 session 资源、历史 step summary、routing state 与当前 step prompt 共同组成。
- 2026-03-20: Task 15F-8 完成，root / chat / feature step 已统一进入 bounded `AgentLoop`，并通过 `tool_request + max_iterations + step prompt` 控制行为；下一主线收敛到 session-owned step context 与 typed root-child handoff。
- 2026-03-21: 架构审查收敛更新（8 findings → 3 convergence items）：(1) 新增 context budget model — `.omega/model.toml` 新增 `[context].context_window`，`request_max_tokens` 重命名为 `max_output_tokens`，session 组装 step 输入时执行 budget-aware 裁剪。(2) 锁定 summary generation strategy — 首轮截断路径 2000 chars，`StepSummary` 新增 `estimated_tokens` 字段。(3) routing state 收敛 — `RoutingContext` 替代 `WorkflowRoutingState` 成为唯一路由状态容器，`build_step_system_prompt` 收敛为接受 `StepExecutionInput` 单一输入。(4) `StepTransition` 新增 `Error` 变体。
- 2026-03-23: Task 15F-9 实现完成：`omega-session` 已持久化 `SessionContext`，每个 step 通过 `StepExecutionInput` 组装技能、工具、routing state 与可裁剪的 `StepSummary` 历史；root workflow 改为 JSON 主路径输出 `recognized_scene_id` / `selected_workflow_id`，child workflow delegation 与后续 turn 共享同一份 session context，并通过 `cargo test -p omega-workflow -p omega-session -p omega-app -p omega-tui` 与 `cargo clippy -p omega-workflow -p omega-session -p omega-app -p omega-tui --all-targets -- -D warnings` 验证通过。
