---
status: draft
owner: omega-team
created: 2026-04-08
updated: 2026-04-08
version: 0.1
supersedes: []
related_prds: []
---

# Omega TUI Message Cards Specification

## Overview

当前 `Response` 已经完成结构化 timeline、Markdown 基础渲染、thinking/final answer 分离，以及第一阶段的 surface hierarchy 刷新；但消息主体仍以“逐行列表”作为最终渲染心智模型。结果是：虽然信息已经分段，用户感受到的仍更像“样式更好的日志流”，而不是“可浏览、可扫读、可定位的消息卡片”。

本规格定义下一轮 `Response` 的消息卡片化方向：让 routing、step、command、thinking、final answer、delivery summary 都以真正的 message card 形式呈现，拥有块级容器、稳定 header、正文内边距、局部摘要与状态语义，而不是继续依赖单行 header + 若干 body line 的轻量模拟。

## Goals

- 把 `Response` 从“按行渲染的 timeline”提升为“按消息块渲染的卡片式 timeline”。
- 让每类消息都拥有稳定的卡片头部语义：类型、状态、来源、摘要、可执行动作。
- 让长 turn 中的 step / final answer / delivery summary 更容易被定位、扫读和回看。
- 保持现有 runtime contract 不变前提下，先在 `omega-tui` 内建立 card-aware view model 和渲染路径。
- 为后续响应区密度模式、折叠策略和交互操作提供块级而非行级基础。

## Non-Goals

- 不在本轮改变 `omega-session` 的 response section contract。
- 不引入终端不可靠的伪阴影、复杂 box-drawing 动画或多列自由布局。
- 不把 Sidebar 的 dashboard card 任务与 Response message cards 混为一体。
- 不要求首轮就让每一类卡片都支持完整交互菜单。

## Problem Statement

当前实现的核心限制不是颜色或间距，而是 view model：

1. `ResponseDisplayLine` 仍是主要渲染单位，意味着视觉上只能“像卡片”，难以“成为卡片”。
2. header/body/tool lane/thinking/final answer 虽然已经分组，但共享相同的 list row 管线，容器感弱。
3. 不同消息类型的标题区缺少统一结构，用户很难建立稳定扫描习惯。
4. 行级选择与激活虽然可用，但对于 message-scale 浏览和折叠并不自然。
5. `Task 15B-53` 当前定义的“timeline rhythm”过于抽象，无法约束真正的 card 化实现切面。

## Design Direction

### Message Card Anatomy

每个 card 至少应包含：

- `Card header`: 类型标签、标题、状态、可选来源/工作流摘要。
- `Card meta row`: 对 step / command / delivery 等高信息密度消息显示轻量 metadata。
- `Card body`: 主体文本区域，应用现有 Markdown / thinking / tool lane 渲染能力。
- `Card footer` 或 action hint: 仅在存在 drill-down、expand/collapse、detail overlay 时显示。

### Card Taxonomy

建议首轮覆盖以下卡片类型：

- `RoutingCard`
- `StepCard`
- `CommandCard`
- `ThinkingCard`
- `FinalAnswerCard`
- `DeliverySummaryCard`
- `ErrorCard`

这些类型不要求在 runtime contract 中新增枚举；首轮可由 `MsgKind + section metadata` 在 TUI 内部映射。

### Card Styling Rules

- header 与 body 必须共享同一块级容器边界，而不是只靠单行标题暗示分组。
- 当前活跃或 streaming card 的 header accent 可以更强，但 body 不应全块高饱和。
- final answer 与 delivery summary 应拥有更高视觉权重，但不能压过正文可读性。
- thinking card 应明确表现为“次级过程卡片”，而不是与主 step body 混成一体。

## Architecture Direction

### Current Constraint

当前 `omega-tui` 的关键模型是：

- `Msg`: 逻辑消息段
- `ResponseDisplayLine`: 渲染行

这对于“结构化列表”足够，但对 message cards 不够。首轮 card 化应在两者之间新增一层 block-aware 表达，例如：

- `ResponseCard`
- `ResponseCardLine`
- 或等价的 `ResponseBlock` view model

要求：

- 一个 card 能生成多行，但这些行共享 card identity、card type、card chrome 和 selection semantics。
- 行级激活仍可保留，但要能映射回 card 级操作。

### Proposed Direction

1. 保留现有 `Msg` 作为输入事实。
2. 在 `response_display_lines()` 之前新增 card assembly 阶段。
3. 让 layout/render 层消费“card -> lines”而不是直接消费“msg -> lines”。
4. 让 style/theme 层获得独立的 card surface/header/meta token，而不是复用普通文本样式。

## Technical Decisions

| Topic | Choice | Rationale |
|------|--------|-----------|
| 规划切面 | 新增独立 message-cards spec | `visual refresh` 与 `message display polish` 都不足以约束 block-level card work |
| 第一阶段范围 | 先做 view model + chrome，不动 runtime contract | 当前瓶颈在 TUI 渲染层，而不是 session 契约 |
| 交互粒度 | 保留行级选择，补 card 级 identity | 避免一次性重写所有键鼠逻辑 |
| 任务拆分 | foundation / chrome / folding-density 三段 | 降低单任务复杂度，便于逐步交付 |

## Task Breakdown

### Task 15B-55: Response card view model foundation

- **Priority**: Medium
- **Complexity**: L
- **Dependencies**: Task 15B-51
- **Affected crates**: `omega-tui`
- **Description**: 在 `Msg -> ResponseDisplayLine` 之间增加 card-aware assembly 层，为 routing/step/command/final answer/delivery summary/thinking 提供稳定 block identity、card type 与 shared container metadata，解决当前只能按行模拟卡片的问题。
- **Implementation approach**:
  - 新增 `ResponseCard` 或等价 view model。
  - 先把 `response_display_lines()` 重构为 `response_cards() -> response_card_lines()` 两阶段。
  - 确保现有 `ResponseLineAction` 能保留或平滑映射到 card 级 identity。
- **Acceptance criteria**:
  - 一个 step/final answer/delivery summary 在渲染前先成为独立 card，而不是直接扁平化为行。
  - 现有 tool/thinking/detail overlay 交互不被破坏。
  - 回归测试能覆盖 card assembly 与 line projection。

### Task 15B-56: Message card chrome and semantic headers

- **Priority**: Medium
- **Complexity**: M
- **Dependencies**: Task 15B-55
- **Affected crates**: `omega-tui`, `omega-theme`
- **Description**: 为 Routing/Step/Command/Thinking/Final Answer/Delivery/Error 卡片提供真正的 header/body/meta chrome，包括 card header、accent strip、body inset、meta row 与不同类型的标题语义。
- **Implementation approach**:
  - 在 `omega-theme` 新增 message card 级视觉令牌，例如 `card_bg`、`card_border_fg`、`card_header_fg`、`card_meta_fg`、`card_active_bg`。
  - 调整 response render，让同一个 card 的多行共享视觉边界。
  - 为 final answer 与 delivery summary 单独定义更高权重的 card header 变体。
- **Acceptance criteria**:
  - 用户能一眼区分普通 step card、thinking card、final answer card 和 delivery summary card。
  - Header 与 body 形成统一容器感，而不是仅凭空行分隔。
  - 窄终端下不会因为 card chrome 导致内容大面积截断。

### Task 15B-57: Card folding, density, and long-turn scanability

- **Priority**: Medium
- **Complexity**: M
- **Dependencies**: Task 15B-55, Task 15B-56
- **Affected crates**: `omega-tui`
- **Description**: 针对长 turn 优化 card 级浏览体验：支持摘要态/展开态、紧凑 meta、长 body 的更稳节奏，以及 final answer / delivery summary 的快速定位能力。
- **Implementation approach**:
  - 将现有 thinking/tool lane 折叠语义提升到 card 级浏览模型。
  - 为 routing、已完成 step、delivery summary 设计更明确的 collapsed summary。
  - 补齐 card 级 scanability 回归测试。
- **Acceptance criteria**:
  - 长 turn 中用户可以快速扫过卡片标题与摘要，而无需逐行阅读。
  - 已存在的 detail overlay/expand 行为与 card 摘要态保持一致。
  - 响应区在 80 列附近仍保持主要卡片可读。

## Relationship To Existing Specs

- `docs/specs/omega-tui-message-display-polish.md` 解决的是文本与局部块的可读性，不覆盖真正的块级 card view model。
- `docs/specs/omega-tui-visual-refresh.md` 解决的是整体 surface hierarchy 与 dashboard 气质，不足以单独约束 Response message cards。
- `docs/specs/omega-tui-response-thinking-experience.md` 定义了结构化 timeline 的基线，而本规格是在其上把 timeline 进一步提升为 card timeline。

## Testing Strategy

- card assembly 单测：确保 `Msg -> ResponseCard` 的分组与类型判定稳定。
- render 单测：确保不同 card type 的 header/body/meta 能正确投影到渲染行。
- interaction 单测：确保现有 detail overlay、toggle 和 selection 行为不因 card 化回退。