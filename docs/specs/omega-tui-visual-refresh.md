---
status: draft
owner: omega-team
created: 2026-04-08
updated: 2026-04-08
version: 0.1
supersedes: []
related_prds: []
---

# Omega TUI Visual Refresh Specification

## Overview

当前 `omega-tui` 的信息架构已经基本成形：`Response`、`Sidebar`、`Input context bar`、`Bottom status bar` 与 `Overlay` 都有明确职责。但从整体视觉上看，界面仍偏向“功能框堆叠”而不是“可长期使用的工作台”：面板层次较平、右侧辅助区缺少 dashboard 感、标题与正文的视觉权重过近、底部条带虽然信息完整但气质仍偏工程默认值。

本规格定义一轮面向“终端工作台”方向的视觉刷新。参考目标是更克制的深色层次、更强的侧栏卡片感、更清晰的标题 hierarchy，以及把强调色保留给 focus / running / warning 这类真正需要注意的状态，而不是让所有元素都在抢注意力。

## Goals

- 把 TUI 从“多个并列 box”提升为有壳层、分区和节奏的统一控制台界面。
- 让 `Sidebar` 更像任务与运行态摘要面板，而不是一组样式完全相同的列表框。
- 让 `Response`、`Sidebar`、`Input`、`Context/Status` 条带拥有稳定、可区分的 surface 层次。
- 把视觉令牌继续收敛进 `omega-theme`，避免新一轮美化再次回到渲染层硬编码。
- 为后续密度、主题预设和更高质量的 timeline polish 预留稳定任务切面。

## Non-Goals

- 不重写当前 `Response / Sidebar / Overlay` 信息架构。
- 不在本轮引入图形化组件、鼠标拖拽布局器或终端外字体方案。
- 不把 message polish、delivery observability 或 modal keymap 的已实现能力推倒重做。

## Visual Direction

### Surface Hierarchy

- 主响应区使用稳定深色 panel surface，避免正文直接贴在终端默认背景上。
- `Sidebar` shell 比 `Response` 更暗一层，突出“辅助区”角色。
- `Sidebar` 内部 section 使用 card-like surface，与 shell 背景形成第二层分隔。
- `Input context bar`、输入框与 `Bottom status bar` 保持同一视觉家族，形成稳定底部控制带。

### Accent Discipline

- 标题文字使用更高对比的浅色，而不是与正文共享同一亮度。
- focus / active state 使用单一冷色强调，不让多个面板同时高饱和。
- warning / error / running 保留独立语义色，避免被纯装饰性颜色稀释。

### Sidebar Feel

- rail 更接近紧凑 badge strip，而不是纯文本链接列。
- 每个 section card 都应一眼可辨，适合承载 TODO、delivery、skills、document/memory 等运行态摘要。
- 后续 density tuning 应优先提升“单位高度内的信息价值”，而不是继续增加常驻面板数量。

## Phase 1 Baseline

`Task 15B-51` 对应本轮已落地的第一阶段视觉刷新：

- `omega-theme` 新增 surface 级视觉令牌：`panel_bg`、`sidebar_bg`、`sidebar_rail_bg`、`section_bg`、`title_fg`。
- 内建 dark theme 改为更明确的 slate/charcoal 分层，不再大量依赖 `Color::Reset`。
- `Response`、`Sidebar` shell、`Sidebar` rail、section card、输入框、context/status bar、overlay 全部切换到分层 surface 背景。
- `Sidebar` rail 选中项改为 badge-like 表现，展开但未选中项也保留弱强调。
- `Response` 与 sidebar section title 统一提升为更高权重标题样式。

这一步的目标不是“最终审美完成”，而是先把界面的基础气质从默认 box UI 拉到可继续打磨的 visual system。

## Planned Follow-Ups

### Task 15B-52: Sidebar dashboard density tuning

- 把 `Delivery`、`Todos`、`Skills`、`Document/Memory` 的摘要行进一步收敛为更像 dashboard card 的结构。
- 研究不同 section 的优先级、默认展开策略与 badge 信息密度，减少右侧滚动时的信息噪音。

### Task 15B-53: Response timeline rhythm refinement

- 提升 `Response` 内 routing / step / command / final answer / delivery summary 的纵向节奏。
- 强化 section 之间的“读写节奏”，让长任务 turn 更像可扫读的执行报告，而不是连续日志块。
- 该方向在下一轮已细化为独立的 `docs/specs/omega-tui-message-cards.md`，因为真正的消息卡片化需要 block-aware view model，而不只是 spacing polish。

### Task 15B-54: Theme presets and density modes

- 把这轮视觉方向沉淀为用户可覆盖的主题令牌基线。
- 评估 compact / comfortable 两档终端密度模式，以及未来多主题预设是否需要进入 `omega-theme`。

## Acceptance Signals

- 宽屏下 `Sidebar` 与 `Response` 一眼就能看出主次层级，而不是两个相同权重的大框。
- `Sidebar` section 在视觉上更接近“信息卡片”，而不是单纯列表容器。
- 输入区、context bar、status bar 形成统一的底部控制带。
- 用户即使不读文档，也能感受到当前界面的 focus、active 和 summary hierarchy 更清晰。