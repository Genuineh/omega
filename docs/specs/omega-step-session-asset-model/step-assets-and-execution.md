---
status: active
owner: omega-team
created: 2026-03-25
updated: 2026-03-25
version: 1.0
supersedes: []
related_prds: []
---

# Omega Step Assets And Execution

本文覆盖 step definition、session asset ownership、dynamic tool visibility 与统一 execution model 的基础合同。

## Terminology

- `step`: workflow 中的最小执行单元，也是规格与实现中的正式术语。
- `workflow`: 有序 step 序列。
- `session assets`: 当前 session 可分配给 step、subagent、team 的 tools 与 skills 能力集合。

## Problem Statement

当前系统若继续把 tools、skills、routing state 和 step 执行分散在多个 runner 中，会持续放大以下问题：

- 新增 step 时必须修改 orchestration 分支，而不是只增配 definition
- tools/skills 缺少统一资产分配边界
- 不同执行体会复制可见工具与技能匹配逻辑
- root/chat/feature steps 难以共享同一种 runtime contract

## Architectural Direction

### Core Decision

保持 `omega-workflow` 拥有 step definition 与 workflow 顺序，`omega-session` 拥有 session 资产管理与 step 编排，`omega-core` 只提供底层 agent loop 与工具调用，`omega-tui` 只消费结构化状态更新。

### Ownership

- `omega-workflow`: `WorkflowStepDefinition`、workflow 配置、step prompt 文件加载
- `omega-session`: `SessionToolCatalog`、`SessionSkillCatalog`、step 进入/退出与资产解析
- `omega-core`: `Agent`、tool execution、bounded loop 基础能力
- `omega-tui`: 结构化 step/runtime 状态消费者
- `omega-app`: session/tui wiring 与 runtime 装配

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

### WorkflowDefinition

```rust
pub struct WorkflowDefinition {
    pub name: String,
    pub steps: Vec<WorkflowStepDefinition>,
}
```

## Session Asset Management

### SessionToolCatalog

- 默认继承当前 session 可用工具
- `Extend(...)` 在继承基础上尝试追加工具
- 若工具尚未装入但存在 loader，则由 session 触发装载
- 被 `Block(...)` 标记的工具不会出现在该 step 可见能力中

### SessionSkillCatalog

- 默认按当前任务做 matched preloading
- 可按 step 追加显式 skills
- `Disable` 可关闭该 step 的自动 skill 装配

## Why Session Owns Assets

- tools 与 skills 本质上都是会话内共享能力，而不是单个 step 的私有字段
- 统一由 session 管理可避免 step、subagent、team 各自复制装配逻辑
- 后续 lazy-loading、并发消费者与 diagnostics 都需要共同资产边界

## Structure Guard

为避免 `AgentSession` 退化成 God Object，catalog 必须保持为独立组合型结构体：

- 独立文件、独立单元测试
- 对外暴露纯 `resolve_*` 接口
- `AgentSession` 只组合持有，不内联 `Inherit/Extend/Block` 细节

## Multi-Consumer Readiness

- catalogs 应设计为 `Arc` 可共享的只读解析结构
- 可变部分应通过内部锁或 channel 隔离
- resolve 本身不应修改 catalog 状态，便于未来 subagent/team 并发访问

## Agent Dynamic Tool Switching

为让 step 级工具可见性生效，`omega-core::Agent` 需要动态工具切换 API：

```rust
impl Agent {
    pub fn set_visible_tools(&mut self, names: Option<&[&str]>) -> Vec<String>;
}

impl ToolDispatcher {
    pub fn to_schemas_filtered(&self, names: &[&str]) -> Vec<Value>;
}
```

约束：过滤只影响 LLM 可见 schema，不改变 dispatcher 持有的完整 handler 集。

## Execution Model

每次进入 step 时，`omega-session` 组装：

- 当前 session message history
- 基础 system prompt
- 由 skill catalog 解析出的 step skills
- 由 tool catalog 解析出的 step tool set
- 当前 `SessionContext`
- 当前 step identity 与 step prompt

然后所有 step 共用同一条 bounded agent loop：

1. `agent.set_visible_tools(...)`
2. 进入当前 step 的 `max_iterations` 有界循环
3. 如遇 `tool_use` 则执行工具并继续
4. 否则结束当前 step，生成 summary 与 transition

## Step Identity Model

规格方向是从 `WorkflowStepKind` 闭合枚举迁移到 `id: String`：

- 阶段 1：内部模型泛化，但配置仍只允许 canonical ids
- 阶段 2：开放自定义 id 与显式 `prompt_path`

这样可先完成 runtime contract 收敛，再放开用户自定义 step 形状。

## Configuration Direction

`.omega/workflow.toml` 继续以 `steps` 作为入口：

```toml
[[steps]]
id = "explore"
label = "Explore"
prompt = ".omega/prompt/step/explore.md"
loop_mode = "agent_loop"
max_iterations = 6
tool_request = { mode = "inherit" }
skill_request = { mode = "match_task" }
enabled = true
```

## Session Integration

`omega-session` 需要承担两类职责：

### 1. Session 资产层

- 管理默认 tools
- 管理可按需加载的附加 tools
- 管理 skills 描述、匹配与显式加载
- 对 step、subagent、team 暴露统一解析接口

### 2. Step 编排层

- 遍历 enabled steps
- 为每个 step 解析 tool/skill 资产
- 调用 `omega-core`
- 发出结构化 runtime update

## Event Contract

`WorkflowStepChanged` 这类事件必须拥有稳定 `step_id`，而不是只依赖展示用 `step_label`。这样日志、调试和前端匹配都不再受 label 重名影响。

## Technical Decisions

| Decision | Choice | Rationale |
|---------|--------|-----------|
| primary term | `step` | 与当前实现一致，避免并行名词体系 |
| asset owner | `omega-session` | tools/skills 属于会话内共享能力 |
| workflow owner | `omega-workflow` | step definition 与 prompt/config 仍属于 workflow 领域 |
| execution contract | bounded agent loop | 所有 step 共用一条底层运行路径 |

---

### Change Log

- 2026-03-25: 从入口规格中拆出 step/assets/execution 基础合同。