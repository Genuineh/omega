---
status: draft
owner: omega-team
created: 2026-03-20
updated: 2026-03-20
version: 0.1
supersedes: []
related_prds: []
---

# Omega Step And Session Asset Model Specification

## Overview

当前 `omega-workflow` 已经能表达四阶段工作流，并把每个阶段的 prompt 外置到 `.omega/prompt/step/*.md`。下一阶段不再把“flow”理解成一套独立于现有 `step` 的新概念，而是明确把现在的 `step` 视为工作流中的最小执行单元，只是在讨论时曾临时把它叫作 flow。

本规格的目标，是在保持 `step` 术语不变的前提下，把当前固定四阶段 step 提升为通用 step definition，并引入 session 级资产管理边界。`omega-session` 统一管理 tools 与 skills 这类会话内资产，workflow step、后续 subagent、team 等执行体只从 session 的资产管理层申请、继承、扩展或屏蔽自己需要的能力；`context` 本轮仅保留为未来能力，不进入首轮实现范围。

下一阶段还会在此基础上加入 scene-aware routing：`scene-recognition` 与 `select-workflow` 也将被建模为 step，而不是 session 外围的特殊预处理逻辑；未来个别 step 也可能触发 child workflow delegation。相关规划见 [docs/specs/omega-scene-routing.md](omega-scene-routing.md)。

截至 2026-03-20，Task 15F-2B 已完成首轮落地：`omega-workflow` 内部模型已从 enum-centric 四阶段切换为 string-keyed `WorkflowStep`，`WorkflowPrompts` 已改为按 `step_id` 查找的映射结构，`omega-session` 已基于 `WorkflowRun` + `StepLoopMode` 驱动通用 step 编排，`SessionUpdate::WorkflowStepChanged` 也已补齐稳定的 `step_id` 字段。

## Goals

- 保持 `step` 作为 workflow 的主术语，不再引入与之平行的新概念名词。
- 将当前固定四阶段提升为可配置的 `WorkflowStepDefinition` 列表。
- 让 `omega-session` 成为 tools 与 skills 的统一资产管理入口。
- 让 step 运行时默认继承 session 资产，并支持增补、屏蔽和按需加载。
- 为未来的 subagent、team 等会话内执行体复用同一套资产管理机制。
- 为 future scene-aware child workflow delegation 预留稳定的 step-level 扩展点。

## Non-Goals

- 本次规划不要求实现 DAG、条件分支或循环回边。
- 本次规划不要求立即实现 `context` 的结构化读写与 artifact schema。
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
- `context` 的未来方向已经明确存在，但现在还不适合和 step 泛化一起打包实现。

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
    pub tool_request: StepToolRequest,
    pub skill_request: StepSkillRequest,
    pub enabled: bool,
}

pub enum StepLoopMode {
    SingleResponse,
    ToolLoop,
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

默认 `analysis / plan / execute / report` 仍然是内建 step，但执行器不再依赖固定分支名才能工作。

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
- `run_single_response` 当前显式拒绝工具调用。未来如果出现"单次响应但允许看到工具 schema"的需求，应新增 `StepLoopMode::SingleResponseWithTools` 而不是修改现有语义。

### 前置依赖

此 API 应在 Task 15F-2A Step 4 实现，作为 `omega-session` 资产层调用 `omega-core` 的唯一依赖点。

## Execution Model

每次进入一个 step，`omega-session` 组装：

- 当前 session message history
- 基础 system prompt
- step prompt
- 从 `SessionSkillCatalog` 解析得到的 step skill 片段
- 从 `SessionToolCatalog` 解析得到的 step tool 集合

然后按 `loop_mode` 选择执行方式：

- `SingleResponse`: 单次模型响应，不暴露工具。调用 `agent.set_visible_tools(Some(&[]))` 清空可见工具后执行。
- `ToolLoop`: 进入标准 agent tool loop。先调用 `agent.set_visible_tools(Some(&resolved_names))` 设置该 step 的工具子集。

这意味着当前 `analysis`、`plan`、`report` 仍然可以走无工具单响应，而 `execute` 走工具循环，但这个行为将来自 step 定义与 session 资产解析，而不是硬编码 if/else。

## Forward Direction: Scene-Aware Workflow Delegation

scene-aware routing 的下一阶段要求是：workflow selection 本身也被表达为 step，而不是额外平行机制。这带来两个边界要求：

- `scene-recognition` 与 `select-workflow` 作为 root workflow step，仍然由 `omega-session` 用通用 step runner 执行。
- 当某个 step 需要触发 child workflow 时，session 应维护 workflow stack / active workflow state，而不是把 delegation 逻辑散落到 UI 或 app shell。

因此，后续模型应允许 step completion 保留类似 `StartWorkflow { workflow_id }` 的通用过渡语义。首轮 scene routing 只要求 `select-workflow` 使用它，但不应把它设计成不可复用的专用分支。

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
loop_mode = "single_response"
tool_request = { mode = "block", items = ["bash", "read_file", "write_file", "edit_file"] }
skill_request = { mode = "match_task" }
enabled = true

[[steps]]
id = "execute"
label = "Execute"
prompt = ".omega/prompt/step/execute.md"
loop_mode = "tool_loop"
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

`context` 本轮不实现，只做方向保留：

- 不在 `Task 15F-2A / 15F-2B` 中加入结构化 context 读写
- 后续若要做 `analysis_notes`、`plan_outline`、`verification_summary` 等 artifact，再单独起规格和任务
- 当前 step 共享能力仍以 session message history 为主

## Migration Plan

### Phase 1: Session 资产管理基础

- 在 `omega-session` 中抽出 tool/skill 资产解析边界
- 保持现有默认工具和技能行为不变
- 为 step、subagent、team 复用建立统一接口

### Phase 2: 通用 Step 编排

- 在 `omega-workflow` 中把固定四阶段提升为 `WorkflowStepDefinition`
- 让 step 通过 session 资产层拿到工具和技能
- 让 `omega-session` 改为通用 step runner

### Phase 3: 后续增强

- 支持更多 step 模板，如 `verify`、`delegate`、`summarize`
- 细化 tool lazy-loading 和 skill 显式装配策略
- 为未来 `context` 设计单独规格

## Risks

- 如果把 session 资产层做成万能中心，会出现新的 God Object 风险。**缓解**: catalog 作为独立组合型结构体，暴露纯 resolve 方法，`AgentSession` 只持有不内联。
- 如果 tool 与 skill 的解析接口不统一，后续 subagent 和 team 仍会复制逻辑。**缓解**: `ResolvedToolSet` / `ResolvedSkillSet` 类型统一，任何消费者都通过相同接口获取。
- 如果首轮就把 context 一起做进去，会把任务复杂度推高到不必要的程度。**缓解**: context 仅保留术语，不进入 15F-2A/2B 实现范围。
- `WorkflowStepKind` enum → `id: String` 是断裂式变更。**缓解**: 分两阶段迁移，15F-2B 内只泛化内部模型，保持 4 个 canonical id 配置兼容；之后独立小任务开放自定义。
- `Agent` 当前没有动态工具切换 API。**缓解**: 15F-2A Step 4 先落地 `set_visible_tools` + `to_schemas_filtered`，验证后再做资产层接入。
- 单 agent slot 模式与多执行体愿景有张力。**缓解**: catalog 设计为 `Arc` 只读 resolve，可变部分用内部锁隔离，不阻塞后续 subagent 并发。

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
