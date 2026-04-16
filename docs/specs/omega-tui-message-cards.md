---
content_revision: 117
created: 2026-04-08
generation_id: gen_000033_r000117
last_verified_commit: N/A
owner: omega-team
projection_version: 33
related_prds: []
source_doc_id: "spec:docs-specs-omega-tui-message-cards"
status: draft
supersedes: []
updated: 2026-04-08
---

# Omega TUI Message Cards Specification

## Overview

当前 `Response` 已经完成结构化 timeline、Markdown 基础渲染、thinking/final answer 分离，以及第一阶段的 surface hierarchy 刷新；但最终输出的主要心智模型仍然是“逐行列表”。结果是：虽然信息已经分段，用户感受到的仍更像“样式更好的日志流”，而不是“面向用户的结构化报告”。

对于实现类、分析类和验证类任务，用户真正需要的不是原始 JSON、冗长密集的执行日志，或一串无层次的段落，而是稳定、可扫读的报告结构：例如 `Results Summary`、`Changes Made`、`Usage`、`Optional Next Step` 等明确区块；性能对比、测试结果等指标要优先用 Markdown 表格呈现；关键状态和核心数字要有明确视觉高亮。

本规格定义下一轮 `Response` 的消息卡片化方向：让 routing、step、command、thinking、final answer、delivery summary 不只是“拥有卡片外观”，而是能被装配为真正的 report-oriented message cards。首轮重点是把最终结果组织成结构化报告块，而不是继续依赖单行 header + 若干 body line 的轻量模拟。

## Goals

- 把 `Response` 从“按行渲染的 timeline”提升为“按结构化报告块渲染的 card timeline”。
- 让最终结果默认呈现为面向用户的报告，而不是原始 JSON、密集日志或松散段落。
- 为常见结果块建立稳定 section grammar，例如 `Results Summary`、`Changes Made`、`Usage`、`Optional Next Step`、`Verification`。
- 让性能对比、测试结果、成本摘要等结构化数据优先以 Markdown 表格呈现，而不是退化成长文本。
- 让关键状态、核心数字、文件名、命令和代码片段拥有一致的视觉强调规则。
- 保持现有 runtime contract 不变前提下，先在 `omega-tui` 内建立 report-aware card view model 和渲染路径。
- 为后续响应区密度模式、折叠策略和交互操作提供 section/card 级而非行级基础。

## Non-Goals

- 不在本轮改变 `omega-session` 的 response section contract。
- 不引入终端不可靠的伪阴影、复杂 box-drawing 动画或多列自由布局。
- 不把 Sidebar 的 dashboard card 任务与 Response message cards 混为一体。
- 不要求首轮就让每一类卡片都支持完整交互菜单。
- 不要求所有回答都强制出现固定区块；空区块应省略，而不是生成模板噪音。
- 不在未明确需要时把原始 JSON 或逐条执行日志直接抬升为 primary final answer。

## Problem Statement

当前实现的核心限制不是颜色或间距，而是输出语法和 view model：

1. `ResponseDisplayLine` 仍是主要渲染单位，意味着视觉上只能“像卡片”，难以“成为结构化报告卡片”。
2. header/body/tool lane/thinking/final answer 虽然已经分组，但共享相同的 list row 管线，无法稳定表达 section-level grammar。
3. 当前任务拆分更偏向 card chrome，而没有明确约束“什么内容必须进 section、什么数据必须进表格、什么状态必须高亮”。
4. 不同消息类型的标题区缺少统一结构，用户很难建立稳定扫描习惯，也难以快速定位最终可采纳结果。
5. 行级选择与激活虽然可用，但对于 report-scale 浏览、折叠和扫读并不自然。
6. `Task 15B-53` 当前定义的“timeline rhythm”过于抽象，无法约束真正的 report-oriented card 实现切面。

## Design Direction

### Report-Oriented Card Anatomy

每个最终可读 card 至少应包含：

- `Card header`: 类型标签、标题、状态、可选来源/工作流摘要。
- `Section list`: 一个或多个结构化区块，例如 `Results Summary`、`Changes Made`、`Verification`、`Usage`、`Optional Next Step`。
- `Card meta row`: 对 step / command / delivery 等高信息密度消息显示轻量 metadata。
- `Card body`: 主体文本区域，支持列表、表格、强调文本、代码片段与现有 Markdown 能力。
- `Card footer` 或 action hint: 仅在存在 drill-down、expand/collapse、detail overlay 时显示。

### Section Grammar

建议首轮支持以下 section kinds：

- `ResultsSummarySection`: 最终结论、状态、核心数据。
- `ChangesMadeSection`: 文件改动、设计变更、行为变化。
- `VerificationSection`: 测试结果、性能对比、验证命令、失败说明。
- `UsageSection`: 用法、命令、调用方式、注意事项。
- `OptionalNextStepSection`: 自然延伸的后续动作。
- `KeyPointsSection`: 当内容不适合表格时，以 bullet 方式提炼关键点。
- `RawDetailSection`: 仅作为降级或展开态容器，不应默认抢占主阅读权重。

这些 section kinds 不要求在 runtime contract 中新增枚举；首轮可由 `MsgKind + section metadata + Markdown structure` 在 TUI 内部映射。

### Presentation Rules

- 最终结果卡片应优先按 section 渲染，而不是按段落自然流渲染。
- section 标题应优先使用二级标题语义或等价视觉层级，保证用户能快速扫到区块边界。
- 性能对比、测试结果、成本摘要、状态矩阵等结构化数据必须优先进入 Markdown 表格。
- 关键结论应优先使用 bullet list，而不是长段落堆叠。
- 核心状态与数字应支持更强强调，例如 `**100%**`、`**eliminated**`、`**80.0%**`。
- 文件名、CLI 命令、代码片段和短标识应沿用 code-style 呈现，避免和正文混在一起。
- 原始 JSON、长日志和细碎执行轨迹只应在 detail/expanded 态出现，不应直接占据默认 final answer 主区。
- 结果卡片整体应呈现 quiet, premium, terminal-native 的工业极简气质，而不是高彩度开发者玩具风。
- section 边界、summary badge、table 和 meta row 要形成轻量 bento-like 区块感，但不能依赖厚重 box drawing 才成立。

## Architecture Direction

### Current Constraint

当前 `omega-tui` 的关键模型是：

- `Msg`: 逻辑消息段
- `ResponseDisplayLine`: 渲染行

这对于“结构化列表”足够，但对“结构化报告卡片”不够。首轮 card 化应在两者之间新增一层 block-aware、section-aware 表达，例如：

- `ResponseCard`
- `ResponseReportSection`
- `ResponseCardLine`

要求：

- 一个 card 能生成多行，但这些行共享 card identity、card type、report sections 和 selection semantics。
- 一个 report section 能声明自己的呈现语法，例如 paragraph / bullet list / table / raw detail。
- 行级激活仍可保留，但要能映射回 card 级与 section 级操作。

### Proposed Direction

1. 保留现有 `Msg` 作为输入事实。
2. 在 `response_display_lines()` 之前新增 `card assembly -> report section projection` 阶段。
3. 让 layout/render 层消费“card -> sections -> lines”，而不是直接消费“msg -> lines”。
4. 让 style/theme 层获得独立的 card surface/header/meta token，以及 section/table/highlight token。

## Technical Decisions

| Topic | Choice | Rationale |
|------|--------|-----------|
| 规划切面 | 新增独立 message-cards spec | `visual refresh` 与 `message display polish` 都不足以约束 block-level report work |
| 第一阶段范围 | 先做 report-aware view model + section rendering，不动 runtime contract | 当前瓶颈在 TUI 渲染层，而不是 session 契约 |
| 输出语法 | 用 section kinds 约束报告结构，用 table/list/highlight 约束内容呈现 | 只有卡片外观不足以保证用户读到的是结构化报告 |
| 交互粒度 | 保留行级选择，补 card/section 级 identity | 避免一次性重写所有键鼠逻辑 |
| 任务拆分 | foundation / section-rendering / folding-density 三段 | 降低单任务复杂度，便于逐步交付 |

## Task Breakdown

### Task 15B-55: Response card view model foundation

- **Priority**: Medium
- **Complexity**: L
- **Dependencies**: Task 15B-51
- **Affected crates**: `omega-tui`
- **Description**: 在 `Msg -> ResponseDisplayLine` 之间增加 report-aware card assembly 层，为 routing/step/command/final answer/delivery summary/thinking 提供稳定 block identity、card type、section ordering 与 shared container metadata，解决当前只能按行模拟报告的问题。
- **Implementation approach**:
  - 新增 `ResponseCard` 与 `ResponseReportSection` 或等价 view model。
  - 先把 `response_display_lines()` 重构为 `response_cards() -> response_report_sections() -> response_card_lines()` 三阶段。
  - 为 final answer / delivery summary 预留稳定 section kinds，例如 `ResultsSummary`、`ChangesMade`、`Verification`、`Usage`、`OptionalNextStep`、`KeyPoints`。
  - 确保现有 `ResponseLineAction` 能保留或平滑映射到 card/section 级 identity。
- **Acceptance criteria**:
  - 一个 step/final answer/delivery summary 在渲染前先成为独立 card，并能携带多个有序 report sections，而不是直接扁平化为行。
  - 空 section 会被省略，而不是生成模板噪音。
  - 原始 JSON 或长执行日志不会在默认 final answer 路径中直接提升为 primary section。
  - 现有 tool/thinking/detail overlay 交互不被破坏。
  - 回归测试能覆盖 card assembly、section ordering 与 line projection。

### Task 15B-56: Message card chrome and semantic headers

- **Priority**: Medium
- **Complexity**: M
- **Dependencies**: Task 15B-55
- **Affected crates**: `omega-tui`, `omega-theme`
- **Description**: 为结构化报告卡片提供真正的 section/header/body/meta 渲染能力，包括二级标题语义、bullet key points、Markdown 表格、强调文本、code-style token 与不同类型卡片的标题语义。
- **Implementation approach**:
  - 在 `omega-theme` 新增 message card 与 report section 级视觉令牌，例如 `card_bg`、`card_border_fg`、`card_header_fg`、`section_header_fg`、`table_border_fg`、`metric_emphasis_fg`、`status_emphasis_fg`。
  - 调整 response render，让同一个 card 的多行共享视觉边界，并让 section 标题与 body 形成稳定层次。
  - 为 final answer 与 delivery summary 单独定义更高权重的 section/header 变体。
  - 为 Markdown table、bullet list、strong emphasis、inline code 补齐终端安全的渲染规则。
- **Acceptance criteria**:
  - 用户能一眼区分普通过程卡片与最终结果卡片，并在结果卡片中快速识别 section 边界。
  - 性能对比、测试结果、成本摘要等结构化数据能以 Markdown 表格呈现，而不是退化成密集文本。
  - 关键状态与核心数字拥有明确强调，不与普通正文混淆。
  - 文件名、CLI 命令、代码片段在视觉上与正文区分清楚。
  - Header 与 body 形成统一容器感，而不是仅凭空行分隔。
  - 窄终端下不会因为 card chrome 或 table rendering 导致内容大面积截断。

### Task 15B-57: Card folding, density, and long-turn scanability

- **Priority**: Medium
- **Complexity**: M
- **Dependencies**: Task 15B-55, Task 15B-56
- **Affected crates**: `omega-tui`
- **Description**: 针对长 turn 优化结构化报告的浏览体验：支持 section 级摘要态/展开态、紧凑 meta、长表格与长 body 的稳健退化，以及 `Results Summary` / `Changes Made` / `Usage` / `Optional Next Step` 的快速定位能力。
- **Implementation approach**:
  - 将现有 thinking/tool lane 折叠语义提升到 card/section 级浏览模型。
  - 为 routing、已完成 step、delivery summary 与最终结果区块设计更明确的 collapsed summary。
  - 为长表格提供窄终端退化规则，例如紧凑列裁剪、分段表头或 key-value fallback。
  - 补齐 section jump、scanability 与默认展开策略的回归测试。
- **Acceptance criteria**:
  - 长 turn 中用户可以快速扫过卡片标题与 section 摘要，而无需逐行阅读。
  - 默认优先展开 `Results Summary` 与最终可采纳结果，过程性长日志与原始细节默认降到次级可见性。
  - 已存在的 detail overlay/expand 行为与 section 摘要态保持一致。
  - 响应区在 80 列附近仍保持主要卡片、关键列表与核心表格可读。

## Relationship To Existing Specs

- `docs/specs/omega-tui-message-display-polish.md` 解决的是文本与局部块的可读性，不覆盖真正的块级 card view model。
- `docs/specs/omega-tui-visual-refresh.md` 解决的是整体 surface hierarchy 与 dashboard 气质，不足以单独约束 Response message cards。
- `docs/specs/omega-tui-response-thinking-experience.md` 定义了结构化 timeline 的基线，而本规格是在其上把 timeline 进一步提升为 card timeline。

## Post-Implementation Readability Follow-Up

`Task 15B-55 ~ 15B-57` 已完成 card/section foundation、Markdown table 与 section summary badge，但当前 `Response` 仍存在一类后续可读性缺口：

- 卡片之间已有边界，但卡片内部的 `header / meta / body / footer` 权重还不够稳定。
- 长 turn 中不同类型卡片的纵向节奏仍偏近，容易重新回到“很多块，但还是一股脑往下读”的体验。
- `Results Summary`、`Changes Made`、`Verification` 等 section 虽已出现，但在默认阅读路径中仍缺“先看结论，再看过程”的更强主次。

因此，`Response` 的下一轮工作不再是继续加新 section，而是把已经存在的 section grammar 做成更稳定的阅读节奏。

### Task 15B-58: Response hierarchy and reading rhythm polish

- **Priority**: Medium
- **Complexity**: M
- **Dependencies**: Task 15B-55, Task 15B-56, Task 15B-57
- **Affected crates**: `omega-tui`, `omega-theme`
- **Description**: 继续打磨 `Response` 卡片内部的主次关系，让 header、meta、section title、summary badge、body copy 与 footer hint 拥有更稳定的层级和更流畅的纵向阅读节奏。
- **Implementation approach**:
  - 为 routing / step / command / final answer / delivery summary 定义更稳定的 card spacing、meta prominence 和 section gap 规则。
  - 强化 final answer 与 `Results Summary` 的默认主阅读权重，进一步压低过程性 meta 与原始 detail 的视觉音量。
  - 统一 card 之间与 card 内 section 之间的空行、divider、summary badge 节奏，避免同层级信息看起来像同一种文本块。
- **Acceptance criteria**:
  - 用户能在长 turn 中更快识别“最终结果在哪里、过程信息在哪里、哪些只是辅助说明”。
  - 不同 card type 在不依赖重边框的情况下也能形成稳定的扫描习惯。
  - `Response` 在 80~120 列常见宽度下保持清晰的纵向节奏，而不会因 section 增多而重新塌回日志流。

**Implemented 2026-04-08**

- `Response` header 改为更稳定的 surface treatment：routing / step / command header 使用 panel section surface，final answer header 使用更强的 summary surface。
- 过程性 meta 行现在统一降噪，`scene`、`delivery`、`skills`、`knowledge`、`reason` 等辅助说明不再与正文共享同一视觉权重。
- subflow 状态行补齐运行/失败/完成的语义分层，减少长 turn 中“过程块看起来都一样”的问题。

## Testing Strategy

- card assembly 单测：确保 `Msg -> ResponseCard -> ResponseReportSection` 的分组、类型判定与 section ordering 稳定。
- render 单测：确保 section header、bullet list、Markdown table、强调文本与 inline code 能正确投影到渲染行。
- responsive 单测：确保窄终端下表格与长 section 有稳定退化，而不是直接压坏布局。
- interaction 单测：确保现有 detail overlay、toggle 和 selection 行为不因 card 化回退。

## Change Log

- 2026-04-08: 将 `Task 15B-55 ~ 15B-57` 从“卡片外观优先”重排为“结构化报告优先”，新增 section grammar、表格化数据与关键高亮约束。
- 2026-04-08: 进一步补充 `Modern TUI / Rich CLI` 与 dark minimalist 审美约束，要求结果卡片保持 quiet premium 的 terminal-native 报告感。
- 2026-04-08: `omega-tui` 已落地 `ResponseCard` / `ResponseCardSection` 装配层、section summary header 与 Markdown table 渲染；`omega-theme` 也已补齐 report token，并以包级测试验证通过。
- 2026-04-08: 补充 `Task 15B-58`，专门承接 card foundation 之后的 `Response` 主次层级与阅读节奏打磨。
- 2026-04-08: 完成 `Task 15B-58`，为 response header 增加稳定 surface、压低过程性 meta 音量，并补齐 subflow 状态层级的渲染回归。
