---
status: draft
owner: omega-team
created: 2026-03-20
updated: 2026-03-20
version: 0.1
supersedes: []
related_prds: []
---

# Omega Scene Routing Specification

## Overview

当前 Omega 的执行入口仍然默认直接进入单个 execution workflow。这对 feature-oriented coding task 是可行的，但对纯对话、轻量澄清或未来其他工作模式并不合适。下一阶段需要在 workflow 之上新增 `scene` 概念，把“当前是什么工作场景”作为显式路由前提。

本规格定义一个 scene-aware routing 层：系统先运行一个主 workflow，依次执行 `scene-recognition` 与 `select-workflow` 两个 step；识别出 scene 后，再委派到匹配的 child workflow 中稳定执行。scene 允许用户配置，并预置两种场景：`chat` 与 `feature`。

默认预置映射：

- `chat` -> `chat`
- `feature` -> `feature`

其中：

- `chat` workflow = `chat`
- `feature` workflow = `analysis -> plan -> execute -> report`

同时，本规格预留后续扩展点：未来某个 step 内部也可以再次做 scene judgment，并触发 child workflow / subworkflow delegation，而不需要重写整个编排模型。

## Goals

- 在 execution workflow 之上引入 `scene` 作为顶层工作路由概念。
- 让系统先识别 scene，再选择匹配 workflow，而不是默认把所有输入都送入同一条 feature flow。
- 允许用户通过配置声明 scene catalog 与 scene -> workflow 绑定关系。
- 预置 `chat` 与 `feature` 两种 scene，并分别映射到轻量 chat workflow 与四阶段 feature workflow。
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
- `chat` 这类轻量场景不应该承担 `analysis -> plan -> execute -> report` 的完整成本。
- 未来某些 step 可能需要按 scene 或局部语义触发 child workflow，但当前没有显式 delegation 模型。
- scene 目前既不可配置，也不是前端可见状态，用户无法确认系统为什么选择了某条 workflow。

## Architecture

### Ownership

- `omega-workflow`: 拥有 scene definition、workflow catalog、root workflow definition、默认预设与配置加载/校验。
- `omega-session`: 拥有主 workflow 执行、scene recognition、workflow selection、child workflow delegation 与 workflow stack/runtime state。
- `omega-tui`: 只展示当前 scene、当前 root step 与 active child workflow，不持有路由策略。
- `omega-app`: 只负责装配与 wiring，不参与 scene policy。

### Core Model

- `scene`: 工作场景，如 `chat`、`feature`。
- `root workflow`: 主路由 workflow，首轮固定为 `scene-recognition -> select-workflow`。
- `child workflow`: scene 选中后真正执行的 workflow，例如 `chat` 或 `feature`。
- `workflow delegation`: 某个 step 结束后启动另一个 workflow 的行为。首轮只要求 `select-workflow` 使用该能力，但模型要允许未来其他 step 复用。

### Architectural Rule

- scene 的定义、配置、默认预设与 workflow binding 必须放在 `omega-workflow`。
- “何时识别 scene、如何从 root workflow 切到 child workflow、如何维护 active workflow state” 必须放在 `omega-session`。
- UI 只能消费路由结果，不能拥有 scene recognition 或 workflow selection 逻辑。

## Default Presets

### Scenes

| Scene | Purpose | Selected Workflow |
|-------|---------|-------------------|
| `chat` | 纯对话、问答、澄清、轻量讨论 | `chat` |
| `feature` | 需要分析、计划、执行与汇报的交付型任务 | `feature` |

### Workflows

| Workflow | Steps |
|----------|-------|
| `root` | `scene-recognition`, `select-workflow` |
| `chat` | `chat` |
| `feature` | `analysis`, `plan`, `execute`, `report` |

说明：当前已经实现的四阶段 execution workflow，在本规格落地后应被重新解释为内建 `feature` workflow，而不是默认唯一 workflow。

## Configuration Direction

推荐把现有单一 `.omega/workflow.toml` 演进为 scene + workflow catalog 结构：

- `.omega/scenes.toml`: scene catalog、root workflow id、default scene、scene -> workflow binding
- `.omega/workflows/root.toml`: 主路由 workflow
- `.omega/workflows/chat.toml`: chat workflow
- `.omega/workflows/feature.toml`: feature workflow

默认 prompt 文件则扩展为：

- `.omega/prompt/step/scene-recognition.md`
- `.omega/prompt/step/select-workflow.md`
- `.omega/prompt/step/chat.md`
- 现有 `.omega/prompt/step/analysis.md`
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
id = "feature"
label = "Feature"
workflow = "feature"
```

## Session Integration

`omega-session` 的目标执行路径应演进为：

1. turn 开始后先进入 `root` workflow。
2. `scene-recognition` step 识别当前输入属于哪个 scene。
3. `select-workflow` step 根据 recognized scene 解析 child workflow。
4. session 启动 child workflow run，并把它作为当前稳定执行流。
5. child workflow 执行结束后，turn 才进入完成态。

这里的关键是：scene recognition 与 workflow selection 本身也是 step，而不是写在 `main`、TUI 事件处理或 session 外围的预处理分支里。

### Runtime State Direction

为支持 scene-aware routing，`omega-session` 后续需要显式拥有：

- 当前 root workflow
- 当前 recognized scene
- 当前 selected workflow id
- active child workflow run
- 未来可能的 workflow stack / nested workflow state

## Step-Level Delegation Direction

本规格要求一开始就为“step 触发 child workflow”预留扩展点。

首轮只要求 `select-workflow` 使用该能力，但模型不应把 delegation 硬编码成专属特例。推荐方向是让 step completion 能表达类似语义：

```rust
enum StepTransition {
    Continue,
    StartWorkflow { workflow_id: String },
}
```

这样未来即使 `analysis`、`chat` 或其他自定义 step 需要再次触发 scene judgment、verify flow、delegate flow，也不需要推翻现有 session 编排边界。

## Runtime UI Implications

scene-aware routing 是 runtime-visible 行为，因此后续实现至少要让前端能看到：

- 当前 recognized scene
- 当前 active workflow
- 当前 root workflow step 与 child workflow step 的区别

首轮不强制在本规格中确定最终 UI 契约细节，但实现任务必须同步更新 runtime UI contract 与 TUI runtime experience 文档，避免 scene routing 变成“后台隐形逻辑”。

## Migration Plan

### Phase 1: Scene / Workflow Catalog Model

- 新增 scene definition 与 workflow catalog 规格
- 固定 `root` / `chat` / `feature` 三个内建 workflow preset
- 固定 `chat` / `feature` 两个内建 scene preset
- 明确旧 `.omega/workflow.toml` 的兼容回退策略

### Phase 2: Root Workflow Execution

- 在 `omega-session` 中执行 `scene-recognition -> select-workflow`
- 把 scene 识别结果转为 selected child workflow
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
