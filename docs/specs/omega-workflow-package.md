---
status: implemented
owner: omega-team
created: 2026-03-19
updated: 2026-03-20
version: 0.1
supersedes: []
related_prds: []
---

# Omega Workflow Package Specification

## Overview

当前 Omega 的单轮执行虽然已经具备 `todo`、`skills`、`session update` 与 TUI 可见性基础，但“这一轮到底处于哪个阶段”仍然隐含在 prompt、日志和工具调用顺序里。结果是用户只能看到 `Running`，却看不到系统当前是在分析问题、生成计划、执行动作，还是整理最终报告。

本规格规划新增独立 crate `omega-workflow`，把单轮执行过程收敛为一个可配置、可观测的工作流系统。当前实现仍保持四个 canonical steps：`analysis -> plan -> execute -> report`，但内部模型已提升为 string-keyed step definition，并在 step 上显式承载 `prompt_path`、`loop_mode`、`tool_request`、`skill_request`。工作流定义从外部 `.omega/workflow.toml` 加载；阶段提示词从 `.omega/prompt/step/*.md` 加载；`omega-session` 负责驱动阶段推进并向前端发出 typed update；`omega-tui` 只负责在底部状态栏显示当前阶段。

截至当前实现，这四个 canonical steps 实际上对应内建的 `feature` execution workflow。下一阶段若要在其之上加入 `scene`、`chat` workflow 与主路由 workflow，应以 [docs/specs/omega-scene-routing.md](omega-scene-routing.md) 为规划基线，而不是直接改写本规格中的“当前已实现状态”。

## Goals

- 新增独立 `omega-workflow` crate，集中承载工作流定义、默认阶段、配置加载、校验与运行时阶段状态模型。
- 为单轮执行提供稳定的四阶段语义：`analysis`、`plan`、`execute`、`report`，作为当前内建 `feature` workflow。
- 支持通过用户可编辑的 `.omega/workflow.toml` 对默认工作流做外部配置。
- 支持通过 `.omega/prompt/step/analysis.md`、`plan.md`、`execute.md`、`report.md` 独立配置四阶段提示词。
- 让 `omega-tui` 底部状态栏可以显示当前工作流阶段，而不是只显示 `Idle / Running`。
- 保持 `omega-core`、`omega-session` 与 `omega-tui` 的职责边界清晰，不把 widget 语义下沉到工作流包。

## Non-Goals

- 当前已实现版本不实现任意 DAG、条件分支、循环回边、并行 step 或 scene-aware 多工作流切换；后者的下一阶段规划见 [docs/specs/omega-scene-routing.md](omega-scene-routing.md)。
- 当前阶段仍不开放任意自定义 step id；仅允许在四个 canonical steps 上做顺序、标签和启用控制，内部模型泛化已先完成。
- 首期不要求运行时热重载 `.omega/workflow.toml`。
- 不把具体的 UI 样式逻辑塞进 `omega-workflow`。

## Problem Statement

当前缺少显式工作流层带来几个问题：

- 用户无法知道系统当前运行到哪一步，只能看到泛化的 `Running`。
- 分析、计划、执行、报告这些阶段目前只以 prompt 约定存在，缺少独立所有权。
- 如果未来接入 `subagent`、`background`、`tasks`，没有统一阶段模型时，状态栏和运行态事件会继续碎片化。
- 外部配置目前已经有 `.omega/keymap.toml` 与 `.omega/theme.toml`，但执行流程还不能像按键和主题一样被明确配置和回退。

## Proposed Crate

新增：`crates/omega-workflow`

职责：

- 定义通用 `WorkflowStep` 模型、默认四阶段顺序与 canonical step 元数据。
- 提供 `.omega/workflow.toml` 默认模板、解析、校验与 fallback 加载逻辑。
- 提供 `.omega/prompt/step/*.md` 默认模板、读取与 fallback 加载逻辑。
- 提供运行时工作流状态结构，例如当前 `step_id`、阶段序号、总阶段数。
- 暴露 `omega-session` 可消费的 API，用于启动、推进、完成或中断工作流。

非职责：

- 不直接调用 LLM、工具、subagent 或 UI。
- 不包含 TUI badge 文本拼接、颜色或布局规则。
- 不拥有 turn 生命周期；turn 仍由 `omega-session` 编排。

## Boundary Design

### Ownership

- `omega-workflow`: 拥有阶段定义、配置模型、默认工作流与阶段推进状态机。
- `omega-session`: 拥有“一轮执行何时进入下一阶段”的编排决策，并把阶段变化映射为前端更新协议。
- `omega-tui`: 只消费阶段更新，决定底部状态栏如何展示当前阶段。
- `omega-core`: 保持前端无关，不依赖 TUI 展示语义。

### Dependency Direction

- `omega-session` -> `omega-workflow`
- `omega-tui` -> `omega-session`
- `omega-workflow` -> `serde` / `toml`
- `omega-workflow` 不依赖 `omega-tui`

### Architectural Rule

- 新增阶段语义、配置字段或推进规则时，优先修改 `omega-workflow`。
- 新增“什么时候切换阶段”的 turn-level 决策时，修改 `omega-session`。
- 新增“如何把阶段显示到状态栏 / Activity / overlay”的呈现逻辑时，修改 `omega-tui`。

## Workflow Model

当前内建 `feature` workflow 固定 canonical steps：

1. `analysis`
2. `plan`
3. `execute`
4. `report`

语义定义：

- `analysis`: 理解需求、抽取约束、识别边界和风险。
- `plan`: 形成执行步骤、依赖顺序和成功标准。
- `execute`: 实际调用工具、修改代码、运行验证。
- `report`: 汇总结果、验证、风险和下一步。

首期模型仍然是线性流，但配置层允许：

- 为每个阶段定义显示标签
- 启用或禁用特定阶段
- 调整四个阶段的顺序，只要每个 canonical step 最多出现一次

## Workflow File

默认路径：`.omega/workflow.toml`

设计沿用 `.omega/keymap.toml` 与 `.omega/theme.toml` 的本地配置目录约定。

### Startup Behavior

- 若文件存在，则读取、解析并校验。
- 若文件缺失，则生成默认 `.omega/workflow.toml`。
- 若配置非法，则回退到内置四阶段默认工作流，并给出 warning。

## Flow Prompt Files

默认目录：`.omega/prompt/step/`

默认文件：

- `.omega/prompt/step/analysis.md`
- `.omega/prompt/step/plan.md`
- `.omega/prompt/step/execute.md`
- `.omega/prompt/step/report.md`

启动行为：

- 若文件缺失，则自动写入内建默认 prompt。
- 若文件可读，则原样加载，允许用户完全自定义四阶段提示词。
- 若文件读取失败，则仅该阶段回退到内建默认 prompt，并给出 warning。

## Configuration Format

当前实现格式：

```toml
name = "default"

[[steps]]
id = "analysis"
label = "Analyze"
prompt = ".omega/prompt/step/analysis.md"
loop_mode = "single_response"
skill_request = { mode = "match_task" }
enabled = true

[[steps]]
id = "plan"
label = "Plan"
prompt = ".omega/prompt/step/plan.md"
loop_mode = "single_response"
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

[[steps]]
id = "report"
label = "Report"
prompt = ".omega/prompt/step/report.md"
loop_mode = "single_response"
skill_request = { mode = "match_task" }
enabled = true
```

校验规则：

- `id` 只允许 `analysis` / `plan` / `execute` / `report`
- 每个 `id` 最多出现一次
- 至少保留一个启用阶段
- 若 `report` 与 `execute` 同时启用，`report` 不能出现在 `execute` 前面
- `prompt` 为空路径视为错误；缺失时回退到 canonical 默认 prompt 路径
- `tool_request.mode = "inherit"` 与 `skill_request.mode = "match_task" | "disable"` 不接受 `items`
- 未知字段视为错误，避免用户误以为配置生效

## API Shape

首期建议结构：

```rust
pub struct WorkflowDefinition {
    pub name: String,
    pub steps: Vec<WorkflowStep>,
}

pub struct WorkflowStep {
    pub id: String,
    pub label: String,
    pub prompt_path: PathBuf,
    pub loop_mode: StepLoopMode,
    pub tool_request: StepToolRequest,
    pub skill_request: StepSkillRequest,
    pub enabled: bool,
}

pub struct WorkflowRun {
    // exposes current_step() / advance() over enabled steps
}

pub struct LoadedWorkflow {
    pub definition: WorkflowDefinition,
    pub prompts: WorkflowPrompts,
    pub source: WorkflowSource,
    pub warnings: Vec<String>,
}
```

## Session Integration

`omega-session` 应把工作流当作 turn 级执行状态，而不是 UI 状态：

- turn 开始后先进入 `analysis`，使用 `analysis.md` 发起一次无工具模型调用
- `analysis` 完成后进入 `plan`，使用 `plan.md` 发起一次无工具模型调用
- `plan` 完成后进入 `execute`，使用 `execute.md` 驱动工具循环
- `execute` 完成后进入 `report`，使用 `report.md` 发起最终无工具模型调用
- turn 完成后清空当前 workflow run，状态栏回到 `Idle`

建议新增 typed update，例如：

```rust
SessionUpdate::WorkflowStepChanged {
    turn_id: u64,
    step_id: String,
    step_label: String,
    index: usize,
    total: usize,
}
```

这样 TUI 无需理解内部状态机，只消费结构化阶段更新。

这也修复了早期实现中“用户一提交输入就立即跳到 `execute`”的问题。阶段推进应绑定到真实的模型/工具里程碑，而不是线程启动顺序。

## TUI Integration

底部状态栏应新增 workflow slot：

- `mode`
- `model`
- `state`
- `flow`

建议展示：

- `flow Analyze`
- `flow Plan`
- `flow Execute`
- `flow Report`

规则：

- 仅在 turn 运行中显示 workflow slot；空闲时可显示 `flow Idle` 或直接隐藏该 slot
- 窄终端下可退化为单词标签，例如 `A` / `P` / `X` / `R`
- 当前阶段是系统状态摘要，完整阶段历史不进入底部状态栏；若未来需要历史，应进入 `Activity` view

## Activity / Future Extensibility

首期只要求底部状态栏显示当前阶段，不强制新增专门的 `Workflow` Activity view。

未来可扩展：

- 在 `Activity` 中展示阶段历史、耗时和失败点
- 将 `subagent`、`background` 等能力映射到 `execute` 阶段内部的子状态
- 为不同任务类型选择不同 workflow presets
- 将固定四阶段演进为通用 step definition，并由 `omega-session` 统一分配 step 所需的 tools 与 skills

## Planned Evolution

- 当前实现仍以固定四阶段为默认结构，这是可运行的 v1。
- 下一阶段规划会保留 `step` 术语，并把这些阶段提升为通用 step definition。
- tools 与 skills 的分配边界会收敛到 `omega-session`，供 step 以及后续 subagent、team 复用。
- workflow 产生的 step 正文结果、phase change 与 tool-preview 邻接输出，后续应通过统一 runtime UI message/effect contract 接入前端，而不是继续为 TUI 单独加特例 update。
- 该演进以 `docs/specs/omega-step-session-asset-model.md` 为主规格，当前文档继续作为已实现 v1 行为的说明。

## Technical Decisions

| Decision | Choice | Rationale |
|---------|--------|-----------|
| crate name | `omega-workflow` | 与现有 workspace 命名一致，职责直接 |
| phase model | fixed canonical four-step linear workflow | 先建立明确语义和可见性，再考虑 DAG |
| config path | `.omega/workflow.toml` | 与 keymap/theme 配置约定保持一致 |
| session boundary | typed step update via `omega-session` | 保持 `omega-tui` 只消费前端协议 |
| TUI surface | bottom status bar slot | 当前阶段属于持续摘要，而不是长文本详情 |

## Risks

- 如果 phase 切换条件定义不清，状态栏会变成不可信噪音。
- 如果允许首期就支持任意自定义 steps，复杂度会明显超过当前收益。
- 如果 `omega-tui` 直接读取 `omega-workflow` 运行态而绕过 `omega-session`，会破坏既有 UI 边界。
- 如果工作流和 `todo` / `tasks` 语义混淆，会让用户看不清“执行阶段”和“任务清单”的区别。

## Testing Strategy

- `omega-workflow` 单测：验证默认工作流、配置缺失、配置非法、阶段顺序校验，以及默认 flow prompt 文件生成与 fallback。
- `omega-session` 单测：验证阶段更新按预期推进、各阶段使用正确 prompt、完成或中断后正确清理。
- `omega-tui` 单测：验证底部状态栏在运行时展示当前 workflow step，空闲时正确退化。

---

### Change Log
- 2026-03-19: 初版规格，规划新增独立 `omega-workflow` crate，支持外部 `.omega/workflow.toml` 配置与 `analysis -> plan -> execute -> report` 四阶段执行模型。
- 2026-03-19: `omega-workflow` crate 已实现，并接入 `omega-session` 阶段更新与 `omega-tui` 底部 `flow` 状态槽。
- 2026-03-20: 将四阶段提示词外置到 `.omega/prompt/step/*.md`，并把阶段推进修正为真实的 `analysis -> plan -> execute -> report` 受控执行序列。
- 2026-03-20: 记录下一阶段规划，准备将固定阶段模型演进为通用 step definition，并把 tools/skills 收敛到 session 资产管理边界。