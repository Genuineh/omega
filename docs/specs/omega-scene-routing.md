---
status: draft
last_verified_commit: N/A
owner: omega-team
created: 2026-03-20
updated: 2026-04-08
version: 0.3
supersedes: []
related_prds: []
---

# Omega Scene Routing Specification

## Overview

当前 Omega 的执行入口仍然默认直接进入单个 execution workflow。这对 feature-oriented coding task 是可行的，但对纯对话、轻量澄清或未来其他工作模式并不合适。下一阶段需要在 workflow 之上新增 `scene` 概念，把“当前是什么工作场景”作为显式路由前提。

本规格定义一个 scene-aware routing 层：session 在每次收到用户新输入后，都先运行 root workflow 中唯一的 `select-workflow` step；该 step 同时完成 scene recognition 与 workflow selection，再委派到匹配的 child workflow 中稳定执行。scene 允许用户配置，并预置四种场景：`chat`、`research`、`deep-research` 与 `feature`。

Status note (2026-04-08): 当前已实现基线已扩展为 root workflow 两步 `select-workflow -> select-skills`。scene/workflow routing 仍由 `select-workflow` 决定；后续追加的 `select-skills` 只负责 turn-scoped skill selection，不改变 scene/workflow routing 的 ownership 边界。详见 `docs/specs/omega-root-skill-routing.md`。

默认预置映射：

- `chat` -> `chat`
- `research` -> `research`
- `deep-research` -> `deep-research`
- `feature` -> `feature`

其中：

- `chat` workflow = `chat`
- `research` workflow = `explore -> report`
- `deep-research` workflow = `explore -> plan -> execute -> report`
- `feature` workflow = `explore -> plan -> execute -> report`

同时，本规格预留后续扩展点：未来某个 step 内部也可以再次做 scene judgment，并触发 child workflow / subworkflow delegation，而不需要重写整个编排模型。

## Goals

- 在 execution workflow 之上引入 `scene` 作为顶层工作路由概念。
- 让系统在一次 root routing step 中同时识别 scene 并选择匹配 workflow，而不是默认把所有输入都送入同一条 feature flow。
- 允许用户通过配置声明 scene catalog 与 scene -> workflow 绑定关系。
- 预置 `chat`、`research`、`deep-research` 与 `feature` 四种 scene，并分别映射到轻量 chat workflow、轻量只读 research workflow、系统性只读 deep-research workflow 与四阶段 feature workflow。
- 明确 session / root workflow / child workflow 的生命周期关系：session 持续存在，root workflow 是每个用户 turn 的统一入口，child workflow 是 root workflow 触发的本轮执行流。
- 让 routing 结果进入 session-owned 的 typed context，而不是继续依赖弱结构化自由文本传递 scene / workflow 决策。
- 保持 `omega-workflow` 拥有 scene/workflow definition，`omega-session` 拥有执行与 delegation，`omega-tui` 只消费可见状态。
- 为未来 step-level workflow delegation 预留稳定模型，不把 scene routing 特判硬编码为一次性逻辑。

## Non-Goals

- 首轮不要求开放任意复杂 DAG、并行 workflow 或无限递归的 workflow graph。
- 首轮不要求 scene 识别具备复杂 policy engine 或多轮投票机制。
- 不把 scene routing 放进 `omega-app` 或 `omega-tui`；这两层不拥有 workflow policy。
- 不要求第一版就完成所有 scene 的 UI 交互细节；先固定模型、配置与 session orchestration。

## Problem Statement

当前模型的主要缺口是：

- 所有输入默认走单一 execution workflow，无法区分聊天场景与实现/交付场景。
- `chat` 这类轻量场景不应该承担 `explore -> plan -> execute -> report` 的完整成本。
- 未来某些 step 可能需要按 scene 或局部语义触发 child workflow，但当前没有显式 delegation 模型。
- scene 目前既不可配置，也不是前端可见状态，用户无法确认系统为什么选择了某条 workflow。
- root routing 当前仍主要消费自由文本结果再做 token 级解析，scene / workflow 关系没有沉淀为稳定的 session context。

## Architecture

### Ownership

- `omega-workflow`: 拥有 scene definition、workflow catalog、root workflow definition、默认预设与配置加载/校验。
- `omega-session`: 拥有主 workflow 执行、scene recognition、workflow selection、child workflow delegation 与 workflow stack/runtime state。
- `omega-tui`: 只展示当前 scene、当前 root step 与 active child workflow，不持有路由策略。
- `omega-app`: 只负责装配与 wiring，不参与 scene policy。

### Core Model

- `scene`: 工作场景，如 `chat`、`research`、`feature`。
- `root workflow`: 主路由 workflow，首轮固定为单步 `select-workflow`。
- `child workflow`: scene 选中后真正执行的 workflow，例如 `chat` 或 `feature`。
- `workflow delegation`: 某个 step 结束后启动另一个 workflow 的行为。首轮只要求 `select-workflow` 使用该能力，但模型要允许未来其他 step 复用。

### Session / Workflow Relationship

- `session` 是持续存在的对话容器，持有 message history、session assets 与 session context。
- `root workflow` 是每个用户 turn 的统一入口 workflow，而不是只在 session 启动时跑一次的初始化逻辑。
- `child workflow` 是 root workflow 在当前 turn 内委派出的实际执行流，例如 `chat` 或 `feature`。
- 当 child workflow 结束后，turn 完成；下一条用户输入到来时，session 会再次从 root workflow 开始，而不是绕过 root workflow 直接进入上次的 child workflow。

### Architectural Rule

- scene 的定义、配置、默认预设与 workflow binding 必须放在 `omega-workflow`。
- “何时识别 scene、如何从 root workflow 切到 child workflow、如何维护 active workflow state” 必须放在 `omega-session`。
- UI 只能消费路由结果，不能拥有 scene recognition 或 workflow selection 逻辑。

## Default Presets

### Scenes

| Scene | Purpose | Selected Workflow |
|-------|---------|-------------------|
| `chat` | 纯对话、问答、澄清、轻量讨论 | `chat` |
| `research` | 聚焦型、较轻量的只读分析与探索任务 | `research` |
| `deep-research` | 系统性、全局性、深入式的只读分析与探索任务 | `deep-research` |
| `feature` | 需要分析、计划、执行与汇报的交付型任务 | `feature` |

### Workflows

| Workflow | Steps |
|----------|-------|
| `root` | `select-workflow` |
| `chat` | `chat` |
| `research` | `explore`, `report` |
| `deep-research` | `explore`, `plan`, `execute`, `report` |
| `feature` | `explore`, `plan`, `execute`, `report` |

说明：当前已经实现的四阶段 execution workflow，在本规格落地后应被重新解释为内建 `feature` workflow，而不是默认唯一 workflow。

## Configuration Direction

推荐把现有单一 `.omega/workflow.toml` 演进为 scene + workflow catalog 结构：

- `.omega/scenes.toml`: scene catalog、root workflow id、default scene、scene -> workflow binding
- `.omega/workflows/root.toml`: 主路由 workflow
- `.omega/workflows/chat.toml`: chat workflow
- `.omega/workflows/research.toml`: research workflow
- `.omega/workflows/deep-research.toml`: deep-research workflow
- `.omega/workflows/feature.toml`: feature workflow

默认 prompt 文件则扩展为：

- `.omega/prompt/step/select-workflow.md`
- `.omega/prompt/step/chat.md`
- 现有 `.omega/prompt/step/explore.md`
- 现有 `.omega/prompt/step/plan.md`
- 现有 `.omega/prompt/step/execute.md`
- 现有 `.omega/prompt/step/report.md`

### Compatibility Strategy

为避免一次性打断当前实现，迁移阶段应保留兼容策略：

- 若只存在旧的 `.omega/workflow.toml`，则将其视为 `feature` workflow 的兼容来源。
- 若不存在新的 `.omega/scenes.toml` 与 `.omega/workflows/*.toml`，则生成内建 `chat` / `feature` 预设。
- scene routing 配置非法时，回退到 `feature` scene + `feature` workflow，并给出 warning。

### Example Direction

`scenes.toml` 推荐形态：

```toml
root_workflow = "root"
default_scene = "feature"

[[scenes]]
id = "chat"
label = "Chat"
workflow = "chat"

[[scenes]]
id = "research"
label = "Research"
workflow = "research"

[[scenes]]
id = "feature"
label = "Feature"
workflow = "feature"
```

## Session Integration

`omega-session` 的目标执行路径应演进为：

1. session 收到新的用户输入后，先进入 `root` workflow。
2. `select-workflow` step 基于最新用户输入、已有 session context 与 root step prompt 运行最小 agent loop，并同时产出 `recognized_scene_id`、`selected_workflow_id` 与 `StartWorkflow` transition。
4. session 启动 child workflow run，并把它作为当前 turn 的稳定执行流。
5. child workflow 的每个 step 都向同一个 session context 写回 summary，供后续 step 与下一轮 routing 使用。
6. child workflow 执行结束后，turn 才进入完成态。

这里的关键是：scene recognition 与 workflow selection 本身也是 step，而不是写在 `main`、TUI 事件处理或 session 外围的预处理分支里。

### Routing Result Contract

scene routing 的目标不是继续让 root workflow 依赖“assistant 文本里碰巧出现 `chat` / `research` / `feature` token”来完成路由，而是让 root step 写出 typed routing result：

- `select-workflow` 写入 `recognized_scene_id`
- `select-workflow` 写入 `selected_workflow_id`
- `select-workflow` 产出 `StartWorkflow { workflow_id }`

当前 token matching 只应被视为兼容路径，而不是长期主模型。

### Ambiguity And Fallback Policy

- builtin scene catalog 的 `default_scene` 是 `feature`，因此当 `select-workflow` 没有解析出已配置 scene 时，runtime fallback 应落到 `feature`，而不是 `chat`。
- `chat` 只用于明确的轻量只读对话、解释和直接问答；聚焦型只读分析请求应落到 `research`；系统性、全局性、深入式的只读分析/探索请求应落到 `deep-research`；而明确要求修改代码、文档、配置、prompt、测试或其他仓库文件的请求应落到 `feature`。
- 为避免模型把 research / deep-research / implementation 请求误分到错误 scene，`omega-session` 可以在 root routing 结果落地前做 domain-specific promotion：当最新用户请求明显要求深度研究而模型仍返回 `chat` / `research` / `feature` 时，runtime 会提升回 `deep-research`；当最新用户请求是一般只读分析而模型仍返回 `chat` / `feature` 时，runtime 会提升回 `research`；当最新用户请求明显要求交付或变更而模型仍返回 `chat` / `research` / `deep-research` 时，runtime 会提升回 `feature` scene 及其映射 workflow。
- 这种 promotion 是收敛性保护，不替代 typed routing contract；主路径仍然要求 `select-workflow` 输出结构化结果。

### Runtime State Direction

为支持 scene-aware routing，`omega-session` 后续需要显式拥有：

- 当前 root workflow
- 当前 recognized scene
- 当前 selected workflow id
- 规划中的 routed skill selection state（详见 `docs/specs/omega-root-skill-routing.md`）
- active child workflow run
- 未来可能的 workflow stack / nested workflow state
- 当前 turn 已累积的 step summaries

### Routing State Convergence (与 step-session-asset-model 对齐)

当前实现使用 `WorkflowRoutingState { recognized_scene_id, selected_workflow_id }` 作为 `WorkflowTurnRunner` 的内部状态。spec 同时定义了 `SessionContext.routing: RoutingContext`。为避免双写风险，运行时应统一使用 `RoutingContext` 作为唯一路由状态容器：

- `WorkflowTurnRunner::run()` 初始化 `SessionContext`，其中 `routing: RoutingContext::default()`
- `select-workflow` step 完成后更新 `session_context.routing.recognized_scene_id`
- `select-workflow` step 完成后更新 `session_context.routing.selected_workflow_id`
- child workflow delegation 从 `session_context.routing` 取值，不再维护独立的 `WorkflowRoutingState`

详见 [omega-step-session-asset-model.md](omega-step-session-asset-model.md) 的入口说明，以及 [omega-step-session-asset-model/routing-repair-and-diagnostics.md](omega-step-session-asset-model/routing-repair-and-diagnostics.md) 中的 "Routing State Convergence" 小节。

## Step-Level Delegation Direction

本规格要求一开始就为“step 触发 child workflow”预留扩展点。

首轮只要求 `select-workflow` 使用该能力，但模型不应把 delegation 硬编码成专属特例。推荐方向是让 step completion 能表达类似语义：

```rust
enum StepTransition {
    Continue,
    StartWorkflow { workflow_id: String },
}
```

这样未来即使 `explore`、`chat` 或其他自定义 step 需要再次触发 scene judgment、verify flow、delegate flow，也不需要推翻现有 session 编排边界。对 scene routing 来说，`select-workflow` 是第一个写入 `SessionContext.routing` 并触发 `StartWorkflow` 的 step。

## Runtime UI Implications

scene-aware routing 是 runtime-visible 行为，因此后续实现至少要让前端能看到：

- 当前 recognized scene
- 当前 active workflow
- 当前 root workflow step 与 child workflow step 的区别

首轮不强制在本规格中确定最终 UI 契约细节，但实现任务必须同步更新 runtime UI contract 与 TUI runtime experience 文档，避免 scene routing 变成“后台隐形逻辑”。

## Migration Plan

### Phase 1: Scene / Workflow Catalog Model

- 新增 scene definition 与 workflow catalog 规格
- 固定 `root` / `chat` / `research` / `feature` 四个内建 workflow preset
- 固定 `chat` / `research` / `feature` 三个内建 scene preset
- 明确旧 `.omega/workflow.toml` 的兼容回退策略

### Phase 2: Root Workflow Execution

- 在 `omega-session` 中执行单步 `select-workflow`
- 把 scene 识别结果与 workflow 选择结果写入 session context
- 把 selected child workflow 明确表达为 typed transition，而不是自由文本 token 解析
- 让 child workflow 成为真正的稳定执行流

### Phase 3: UI Visibility

- 在 TUI 底部状态带与 Activity 中展示 scene / workflow routing 结果
- 明确 root workflow 与 child workflow 的可见性与退化规则

### Phase 4: Generalized Delegation

- 开放非 `select-workflow` step 触发 child workflow
- 在不改变 session 边界的前提下支持 nested workflow / subworkflow

## Risks

| Risk | Level | Mitigation |
|------|-------|------------|
| scene routing 逻辑回流到 `omega-app` / `omega-tui` | High | 明确只允许 `omega-session` 执行 router workflow |

## Changelog

- 2026-03-24: 新增 `research` scene 与 `research` workflow，用于承接深度复杂的综合分析和探索型只读任务；builtin / repo-local scene catalog、workflow preset 与 routing ambiguity policy 已同步更新。
| scene config 与 workflow config 耦合过重 | Medium | scene 只绑定 workflow id，workflow 定义保持独立 catalog |
| 兼容迁移打断现有 `.omega/workflow.toml` 用户 | High | 保留 `feature` workflow fallback，并自动生成新配置 |
| step-level delegation 以后扩展困难 | Medium | 首轮就预留通用 `StartWorkflow` 语义，而不是只写 `select-workflow` 特例 |

## Testing Strategy

- `omega-workflow` 单测：scene/workflow config 加载、默认预设生成、兼容回退、scene -> workflow 绑定校验。
- `omega-session` 单测：root workflow 执行、scene recognition 结果处理、workflow delegation、child workflow 生命周期。
- `omega-tui` 单测：scene / workflow 路由状态的显示与 root/child workflow 区分。
- 集成验证：默认 `chat` 输入应走 `chat` workflow；feature-oriented 输入应走 `feature` workflow。

---

### Change Log

- 2026-03-20: 初版规格，提出 `scene` 作为 workflow 之上的顶层工作场景，并规划 `scene-recognition -> select-workflow -> child workflow` 的主路由模型。
- 2026-03-20: 明确 session / root workflow / child workflow 的生命周期关系：session 持续存在，root workflow 是每个用户 turn 的统一入口，child workflow 是 root workflow 触发的本轮执行流；routing 结果应进入 typed session context。
- 2026-03-23: Task 15F-9 实现完成：root routing 现以 JSON 为主路径产出结构化 routing handoff，`RoutingContext` 成为 `omega-session` 的唯一路由状态容器，child workflow delegation 与跨 turn session context 已在运行时落地。
- 2026-04-02: root workflow 收敛为单步 `select-workflow`，同时把只读分析拆成轻量 `research` 与系统性 `deep-research` 两条默认 child workflow。
- 2026-03-24: 明确 scene ambiguity policy：未识别 scene 的 fallback 继续落到 builtin `default_scene = feature`，而不是 `chat`；同时为明显的实现类请求补充了 `chat -> feature` 的 runtime promotion 保护，减少 root routing 误判。
- 2026-04-08: 补充 root skill routing 规划说明；scene/workflow routing 继续只负责 child workflow 选择，turn-scoped skill selection 另由 companion spec `omega-root-skill-routing.md` 定义。
- 2026-04-08: root skill routing 实现落地后，root workflow 当前基线更新为 `select-workflow -> select-skills`；scene/workflow 选择仍由前者负责，后者只负责 child workflow 前的 routed skill handoff。
