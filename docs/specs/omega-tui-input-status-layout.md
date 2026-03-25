---
status: implemented
owner: omega-team
created: 2026-03-19
updated: 2026-03-19
version: 0.2
supersedes: []
related_prds: []
---

# Omega TUI Input And Bottom Status Layout Specification

## Overview

当前 `omega-tui` 的底部区域仍沿用“输入框下方 hint bar + 顶部 header 状态条”的双源布局。随着 `Sidebar`、`Overlay`、后续 runtime badges 与会话统计继续增长，这种布局会把输入上下文、快捷键提示、模型状态和系统摘要拆散到屏幕两端，既增加视觉往返，也让后续状态接入缺少稳定落点。

本规格定义一套新的底部布局：把原先输入框下方承载提示与状态的内容，上移为“输入框上方、位于 `Response` / `Sidebar` 下方的固定上下文带”；再把原先顶部 header 中真正需要持续显示的底部状态信息，下移为“输入框下方的固定状态带”。新状态带只保留模型名与运行态摘要，并封装为便于后续扩展的底部状态槽。

## Goals

- 把与当前输入直接相关的提示、leader 状态、短消息提示聚合到输入框上方，减少视线在顶部和底部之间来回跳转。
- 移除顶部大块 header，把模型名和运行态放到底部固定状态带。
- 为后续 `skills/subagent/background/message/team/worktree` 等能力接入底部状态显示预留稳定扩展点。
- 保持 `Response` / `Sidebar` 主体区不因状态扩展而继续碎片化。

## Non-Goals

- 不在本规格中重新设计 `Sidebar`、`Overlay` 或 Activity 内容本身。
- 不要求本次同时落地 token 统计、轮次统计、后台任务摘要等具体业务状态，只要求先定义承载位。
- 不恢复顶部 `Omega Agent | KM | Focus | Mode` 这种密集 header 形式。

## Layout Model

终端主布局调整为五段：

1. `Main content`: `Response | Sidebar`
2. `Input context bar`: 输入框上方的固定上下文带
3. `Input box`: 主输入区
4. `Bottom status bar`: 输入框下方的固定状态带
5. `Overlay`: 若激活则浮于以上区域之上

### Input Context Bar

该区域位于主体区和输入框之间，用于承载：

- leader pending 提示
- 当前输入模式下的快捷键提示
- 与当前输入或当前聚焦区域直接相关的短消息
- 需要立刻被用户感知、但不应长期占据底部状态带的瞬时反馈

规则：

- 优先显示当前交互上下文，而不是系统级长周期状态。
- 文案应短、面向当前操作，避免堆叠多个 badge。
- 若终端高度紧张，该区域优先保留单行，不应扩展成多行面板。

### Bottom Status Bar

该区域位于输入框下方，替代旧顶部 header，承担持续性状态摘要。

首期只保留：

- `model name`
- `runtime state`，例如 `Idle` / `Running`

明确移除：

- `Omega Agent`
- `KM`
- `Focus`
- `Mode`

这些信息如果仍有必要，应通过上下文带、overlay、局部标题或未来更轻量的交互方式表达，而不是重新挤回固定状态栏。

## Bottom Status Slot Model

为避免后续每加一个功能就继续拼接字符串，底部状态带应抽象为稳定的 slot 模型。

建议最小结构：

- `primary slot`: 模型名
- `state slot`: 运行态，例如 `Idle` / `Running`
- `workflow slot`: 当前执行阶段，例如 `Explore` / `Plan` / `Execute` / `Report`
- `extension slots`: 预留给后续功能的附加状态项

扩展规则：

- 各 slot 只接收结构化状态项，不直接拼接完整展示字符串。
- 展示顺序固定，避免每个功能自由插队。
- 窄终端下优先保留 `primary slot`、`state slot` 和 `workflow slot`，其余 slot 可截断、折叠或隐藏。

## Interaction Rules

- `Input context bar` 负责“当前正在发生什么”，例如 leader 序列等待、当前模式下的快捷键提示、局部 notice。
- `Bottom status bar` 负责“系统当前处于什么状态”，例如模型、运行中/空闲、当前 workflow step 与未来的统计和运行态摘要。
- overlay 激活时，上下文带可切换为 overlay 专属提示；底部状态带仍保留基础状态摘要。
- `Sidebar` 收起或展开不应影响底部两条固定区域的结构。

## Relationship To Existing Specs

- `docs/specs/omega-tui-runtime-experience.md` 应更新为：状态摘要的主要承载区位于输入框下方，而不是顶部 header。
- `docs/specs/omega-tui-collapsible-sidebar.md` 中提到的状态栏摘要，应理解为新的底部状态带，而非旧顶部 header。
- `docs/specs/omega-tui-overlay-popups.md` 中的短时提示，应优先复用输入上下文带，而不是污染底部状态带。
- `docs/specs/omega-theme-package.md` 应承接输入框、上下状态条、边框形态和状态语义色的共享视觉令牌，避免这些定义继续散落在 `omega-tui` 的单个渲染文件中。
- `docs/specs/omega-workflow-package.md` 应承接 workflow step 的定义、外部配置与阶段推进模型；底部状态带只消费结构化阶段摘要。

## Task Planning Impact

建议新增：

- `Task 15B-17`: `omega-tui` — 输入上下文带与底部状态带重构

建议依赖关系：

- `Task 15B-17` 依赖 `Task 15B-16`
- `Task 15B-12` 应建立在 `Task 15B-17` 之上，以便会话统计接入新的底部状态槽

推荐实现顺序：

1. 抽离底部状态模型与渲染入口
2. 把原顶部 header 的模型名与运行态迁到底部状态带
3. 把原输入下方 hint / notice 上移为输入上下文带
4. 清理旧 header 中遗留的 `KM / Focus / Mode / Omega Agent`
5. 让未来状态扩展改为接入 slot，而不是继续直接拼接文本
6. 让输入框、上下状态条和相关语义色迁移到 `omega-theme` 的组件级令牌定义中

## Technical Decisions

| Decision | Choice | Rationale |
|---------|--------|-----------|
| top header treatment | remove dense top header | 降低视觉噪音，避免顶部成为杂项信息堆积区 |
| prompt placement | move hints above input | 与当前输入动作就近，减少视线跳转 |
| status placement | place system summary below input | 为后续状态扩展提供稳定底座 |
| future extensibility | slot-based bottom status model | 避免后续继续手工拼接长字符串 |

## Testing Strategy

- `omega-tui` 单测：验证新布局下输入上下文带与底部状态带都拥有稳定高度。
- `omega-tui` 单测：验证旧顶部 header 被移除后，模型名和运行态仍可在底部状态带看到。
- `omega-tui` 单测：验证 leader pending / notice 提示进入输入上下文带，而不是底部状态带。
- `omega-tui` 单测：验证窄终端下底部状态带至少保留模型名和运行态。

---

### Change Log
- 2026-03-19: `Task 15B-17` 完成实现，主布局改为 `Main content -> Input context bar -> Input box -> Bottom status bar`，底部状态带以 slot 化 segment 渲染模型名与运行态。
- 2026-03-19: 新增输入上下文带与底部状态带布局规格，规划 TUI 底部区域的统一重构方向。
- 2026-03-19: 补充与 `omega-theme` 的关系，明确输入框和上下状态条的视觉令牌后续应由独立主题包统一管理。
- 2026-03-19: 补充 workflow slot 规划，要求底部状态带可显示当前执行阶段，并与 `omega-workflow` 的结构化阶段模型对齐。