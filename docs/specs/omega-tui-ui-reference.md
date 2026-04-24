---
content_revision: 120
created: 2026-04-08
generation_id: gen_000046_r000120
last_verified_commit: N/A
owner: omega-team
projection_version: 46
related_prds:
  - docs/specs/omega-tui-visual-refresh.md
  - docs/specs/omega-tui-message-cards.md
  - docs/specs/omega-tui-overlay-popups.md
source_doc_id: "spec:docs-specs-omega-tui-ui-reference"
status: draft
supersedes: []
updated: 2026-04-10
---

# Omega TUI UI Reference

## Overview

本文档把当前 `omega-tui` 已实现的视觉系统、默认颜色、布局关系、交互提示和核心 UI 元素整理成统一参考表，供后续美化、主题预设和密度调优时作为单一基线使用。

它描述的是“当前实现状态”，不是下一轮视觉目标本身。凡是已经在代码中落地的默认值、层级关系和 panel/overlay 行为，都应优先以本文档为准。

## Goals

- 给后续 TUI 美化提供一份统一的样式与布局盘点表。
- 明确哪些颜色和边框来自 `.omega/theme.toml`，哪些仍是内建派生值。
- 把 `Response`、`Sidebar`、`Input`、`Status`、`Overlay` 的布局关系和默认比例写清楚。
- 把行型语义、focus 标记、badge、preview clamp 和 overlay 尺寸规则显式列出来。

## Non-Goals

- 不提出新的视觉方案或替代当前实现。
- 不把所有未来主题预设先设计出来。
- 不覆盖 runtime contract、事件流和内容语义本身。

## Design Summary

| 维度 | 当前默认实现 |
|------|--------------|
| 总体方向 | `Dark Industrial Report Console` |
| 视觉气质 | 深色、克制、工业感、结果优先、终端原生 |
| 信息重心 | `Response` 为主阅读区，`Sidebar` 为摘要/监管/辅助区 |
| 强调色策略 | 只把高亮留给 focus、running、final answer、metric、status |
| 面板语言 | 深色分层 surface + rounded border + 低饱和背景 |
| 输出形态 | 从日志流转向结构化报告卡片与 section |
| 侧栏策略 | rail + section card + row taxonomy + preview first |
| 底部控制带 | 左列 `Context bar + Input shell` 与全宽 `Status bar` 共享 `Response` dark surface，形成统一控制带 |

## Theme Configuration Surface

| 配置段 | 当前是否写入默认 `.omega/theme.toml` | 用途 |
|--------|--------------------------------------|------|
| `[input]` | 是 | 输入框边框、文本、光标 |
| `[context_bar]` | 是 | 输入上方提示条 |
| `[status_bar]` | 是 | 底部状态条 |
| `[surfaces]` | 是 | 主 panel、sidebar、section 等 surface |
| `[report]` | 是 | 报告式 section、metric、meta、table |
| `[messages]` | 是 | 用户/Agent/Tool/Error/Separator 消息基色 |
| `[overlay]` | 否，但代码支持 | 弹窗遮罩、背景、按钮 |

## Default Theme Tokens

### Surfaces

| Token | 默认值 | 用途 |
|-------|--------|------|
| `surfaces.border_type` | `rounded` | 主 panel 与 sidebar card 边框类型 |
| `surfaces.text_fg` | `#DBE5EE` | 默认正文文本 |
| `surfaces.muted_text_fg` | `#97A3B3` | 次级文本基色 |
| `surfaces.border_dim_fg` | `#2C3846` | 非聚焦边框、分隔线 |
| `surfaces.focus_border_fg` | `#71D2C2` | 聚焦边框、运行态强调 |
| `surfaces.panel_bg` | `#11161D` | 主 `Response` panel 背景 |
| `surfaces.sidebar_bg` | `#0D1117` | Sidebar 外壳背景 |
| `surfaces.sidebar_rail_bg` | `#131922` | Sidebar rail 背景 |
| `surfaces.section_bg` | `#182231` | Sidebar rail 选中行与局部强调面层，不再用于 response header 背景 |
| `surfaces.title_fg` | `#EEF2F6` | 标题文本 |

### Input

| Token | 默认值 | 用途 |
|-------|--------|------|
| `input.border_type` | `rounded` | 输入框边框类型 |
| `input.bg` | `#0C1117` | 输入区背景 |
| `input.text_fg` | `#8AB4F8` | Insert mode 输入文本 |
| `input.placeholder_fg` | `#97A3B3` | Normal mode 占位文本和只读输入内容 |
| `input.normal_border_fg` | `#566678` | 保留配置项，当前实际边框跟随 mode 色 |
| `input.insert_border_fg` | `#71D2C2` | 保留配置项，当前实际 Insert 边框跟随 `mode_insert_fg` |
| `input.cursor_fg` | `reset` | 光标前景 |
| `input.cursor_bg` | `#8AB4F8` | 光标背景 |

### Context Bar

| Token | 默认值 | 用途 |
|-------|--------|------|
| `context_bar.bg` | `#0C1117` | 输入上方提示条背景，现与 `Agent Response` panel 背景对齐 |
| `context_bar.label_fg` | `#D7DDE5` | `keys` label，已去除旧灰色前景 |
| `context_bar.hint_fg` | `#9EB6FF` | 热键说明、command hint、notice |

### Status Bar

| Token | 默认值 | 用途 |
|-------|--------|------|
| `status_bar.bg` | `#0C1117` | 底部状态条背景，现与 `Agent Response` panel 背景对齐 |
| `status_bar.label_fg` | `#D7DDE5` | `mode` / `flow` / `project` / `session` / `route` / `item` 等 label，已去除旧灰色前景 |
| `status_bar.divider_fg` | `#D7DDE5` | 分隔符 `·`，已去除旧灰色前景 |
| `status_bar.normal_mode_fg` | `#CBD5DF` | `NORMAL` mode 标签 |
| `status_bar.insert_mode_fg` | `#71D2C2` | `INSERT` mode 标签 |
| `status_bar.idle_fg` | `#7DDC8B` | Idle 状态 |
| `status_bar.running_fg` | `#F2D089` | Running 状态 |

### Report

| Token | 默认值 | 用途 |
|-------|--------|------|
| `report.section_header_fg` | `#CFE0FF` | Section label、报告小标题 |
| `report.metric_emphasis_fg` | `#F2D089` | metric、完成态 subflow、高价值数字 |
| `report.code_fg` | `#D7DDE5` | inline code、codeish 内容 |
| `report.muted_meta_fg` | `#525865` | meta、empty state、降噪说明 |
| `report.table_border_fg` | `#2A3440` | Markdown 表格边框 |
| `report.summary_badge_bg` | `#162235` | rail/summary 等局部强调面层；当前不再用于 response header 整块背景 |

### Messages

| Token | 默认值 | 用途 |
|-------|--------|------|
| `messages.user_fg` | `#80C995` | 用户消息 |
| `messages.agent_fg` | `#DBE5EE` | Agent 默认消息 |
| `messages.tool_fg` | `#E5D78D` | 工具/命令消息 |
| `messages.error_fg` | `#FF7171` | 错误消息 |
| `messages.separator_fg` | `#2C3846` | 分隔行 |

### Overlay

| Token | 默认值 | 用途 |
|-------|--------|------|
| `overlay.border_type` | `rounded` | Overlay 边框 |
| `overlay.bg` | `#171E28` | Overlay 内容背景 |
| `overlay.mask_bg` | `#06080C` | 遮罩背景 |
| `overlay.button_fg` | `#DBE5EE` | 未选中按钮文字 |
| `overlay.selected_button_fg` | `#0C1117` | 选中按钮文字 |
| `overlay.selected_button_bg` | `#71D2C2` | 选中按钮背景 |

## Derived Render Tokens

下表是当前未直接写入默认 `.omega/theme.toml`、但在渲染层固定派生的 token。

| Token | 默认值 | 用途 |
|-------|--------|------|
| `heading_1_fg` | `#F8FAFC` | Markdown `#` heading |
| `heading_2_fg` | `#CFE0FF` | Markdown `##` heading |
| `heading_3_fg` | `#F2D089` | Markdown `###` heading |
| `inline_code_fg` | `#D7DDE5` | inline code 文字，源自 `report.code_fg` |
| `inline_code_bg` | `#141A24` | inline code 背景 |
| `hr_fg` | `#2A3440` | Markdown horizontal rule |
| `code_block_bg` | `#0F151E` | fenced code block 背景 |
| `code_lang_fg` | `#697383` | code block 语言标签 |
| `code_border_fg` | `#1F2937` | code block 顶/底边线 |
| `user_badge_fg` | `#F8FAFC` | `Response` 中用户消息强调色 |
| `assistant_badge_fg` | `#CFE0FF` | `Response` 中 Agent badge/标题 accent |
| `warning_badge_fg` | `#E8BE60` | warning badge |
| `error_badge_fg` | `#D67878` | error badge |
| `final_answer_accent_fg` | `#71D2C2` | final answer header 前景 |
| `final_answer_border_fg` | `#1F2937` | final answer divider |
| `thinking_summary_fg` | `#525865` | thinking summary |
| `thinking_body_fg` | `#4B5563` | thinking body |

## Global Layout Tables

### Top-Level Vertical Frame

| 区域 | 高度规则 | 说明 |
|------|----------|------|
| 主工作区 | `Min(0)` | 左列 control stack + 右列 full-height `Sidebar` |
| Status bar | `Length(1)` | 全宽底部 mode/flow/project-route-item |

### Left Column Stack

| 区域 | 高度规则 | 说明 |
|------|----------|------|
| Response | `Min(0)` | 主阅读区 |
| Context bar | `Length(2)` | 左列输入上方热键/notice/command hint，必要时换行 |
| Input shell | `Length(9)` | 单一共享边框输入容器：上部输入区 + 底部留白 + 一行 input info + 底部留白 |

### Main Horizontal Split

| 终端宽度 | Response | Sidebar | 说明 |
|----------|----------|---------|------|
| `< 60` | `100%` | `0%` | Sidebar 自动隐藏 |
| `60 - 99` | `66%` | `34%` | 中等宽度下左列为 `Response + Context + Input shell`，右列为 full-height `Sidebar` |
| `>= 100` | `66%` | `34%` | 宽屏默认布局 |
| 任意宽度 + `shell_collapsed=true` | `100%` | `0%` | 用户主动折叠 Sidebar |

### Sidebar Shell Structure

| 层级 | 高度规则 | 说明 |
|------|----------|------|
| Sidebar 外壳 | 跟随主工作区全高 | 带标题和边框，不再在其下方留出输入区 |
| Rail | `Length(1)` | 顶部横向 section rail，使用单行 marker + section label + count tab 文本 |
| Rail spacer | `Length(1)` | rail 与下方 section body 之间的呼吸间隔 |
| Section body | `Min(0)` | Diagnostics/Delivery/Skills/Knowledge/Todos/Logs；每个子面板使用顶部文字标题，当 section 过多时使用 rail/focus 锚定的 viewport，只显示当前窗口能装下的一段 |

### Overlay Sizes

| OverlaySize | 宽度比例 | 高度比例 | 最小宽度 | 最小高度 |
|-------------|----------|----------|----------|----------|
| `Small` | `52%` | `28%` | `36` | `7` |
| `Medium` | `68%` | `42%` | `50` | `10` |
| `Large` | `82%` | `60%` | `64` | `14` |

Overlay 最终尺寸还会被终端边界再裁一次，最大值是 `area - 2`，并保持居中。

## Response UI Inventory

### Response Container

| 元素 | 当前实现 |
|------|----------|
| 标题 | `Agent Response`，聚焦时追加 `◆` |
| 背景 | `surfaces.panel_bg` |
| 边框类型 | `surfaces.border_type` |
| 聚焦边框 | `surfaces.focus_border_fg` + `BOLD` |
| 非聚焦边框 | `surfaces.border_dim_fg` |
| 内容换行 | 按 panel 内宽硬换行；styled spans 也会 wrap |

### Response Message Kinds

| `MsgKind` | Header 风格 | Body 风格 | 备注 |
|----------|-------------|-----------|------|
| `User` | 无单独 header | `user_badge_fg` | 用户消息前景 |
| `Agent` | 无单独 header | `assistant_badge_fg` | 常规 Agent 输出 |
| `Error` | 无单独 header | `error_badge_fg` | 错误输出 |
| `Separator` | 无 | `separator_message` | turn 分隔 |
| `Routing` | `status color + bold`，无 header 背景底色 | meta 为 `muted_meta_fg`，正文为 `context_hint` | root/scene/workflow 选择说明 |
| `Step` | `status color + bold`，无 header 背景底色 | subflow/status 特判；meta 降噪；正文为 `agent_message` | 执行步骤块 |
| `FinalAnswer` | `status color + bold`，无 header 背景底色 | divider 用 `final_answer_border_fg`；meta 降噪；正文 `text` | 最终结果块 |
| `Command` | `status color + bold`，无 header 背景底色 | meta 降噪；正文 `agent_message` | slash command 区块 |
| `Thinking` | header 依状态变色且无整块底色 | summary 为 dim/italic；body 更暗 | 推理轨道 |

### Response Meta and Status Rules

| 规则 | 当前实现 |
|------|----------|
| Meta 行识别 | `scene/result/items/delivery/skills/knowledge/document/memory/source/selection/reason` 前缀会降噪到 `muted_meta_fg` |
| Subflow 行识别 | 以 `subflow` 开头的 step body 行 |
| Subflow `running` | `focus_border + bold` |
| Subflow `failed` | `error_message + bold` |
| Subflow `complete/done` | `metric_emphasis_fg` |
| Thinking `streaming` header | `status_running_fg + bold`；仅状态符号本身闪烁 |
| Thinking `complete` header | `status_idle_fg + bold` |
| Thinking `failed` header | `error_message + bold` |

### Markdown Rendering

| 元素 | 当前实现 |
|------|----------|
| Heading | 支持 `#` / `##` / `###` |
| List | 支持 `-` / `*` / `1.`，并按缩进产生层级 |
| Inline code | 使用 `inline_code_fg` + `inline_code_bg` |
| Bold | `BOLD` 修饰 |
| Italic | `ITALIC` 修饰 |
| Horizontal rule | 渲染为 40 个 `─` |
| Code block | 支持 fenced code block；顶部/底部边线 + 语言标签 + 独立背景 |
| Blank line | 连续空行折叠为单个段落分隔 |

## Sidebar UI Inventory

### Sidebar Defaults

| 项目 | 默认值 |
|------|--------|
| `shell_collapsed` | `false` |
| `rail_selection` | `Diagnostics` |
| `diagnostics_expanded` | `false` |
| `delivery_expanded` | `false` |
| `skills_expanded` | `false` |
| `project_expanded` | `true` |
| `knowledge_expanded` | `true` |
| `todos_expanded` | `true` |
| `logs_expanded` | `false` |

### Sidebar Rail

| 元素 | 当前实现 |
|------|----------|
| section 顺序 | Diagnostics → Delivery → Skills → Project → Knowledge → Todos → Logs |
| 布局形式 | rail 位于 Sidebar 顶部，单行横向排列；body 在其下方 |
| item 形式 | 当前使用 `▾/▸ + section label + count` 的单行 tab 文本样式 |
| 展开 section 样式 | `▾` marker + `context_hint` / 当前选中时 `title_fg` 或 `focus_border` |
| 收起 section 样式 | `▸` marker + `context_label` / 当前选中时 `title_fg` 或 `focus_border` |
| 选中 + rail 聚焦 | `focus_border + sidebar_rail_bg + bold` |
| 选中 + 非聚焦 | `title_fg + sidebar_rail_bg + bold` |
| 横向滚动 | 当 rail 总宽度超过可用宽度时，渲染窗口自动跟随当前 `rail_selection`，保证所选 tab 保持可见 |
| Section 文案 | Diagnostics / Delivery / Skills / Project / Knowledge / Todos / Logs |
| badge 组合 | 保留原计数语义，但压缩成短数字格式：invalid/pending 数、LLM/changed files、loaded/recognized、doc/mem 命中数、todo 完成数、log 行数 |

### Sidebar Panel Titles

| Panel | 标题 |
|-------|------|
| Diagnostics | `Contract Diagnostics`，有 invalid 时显示 `(!n)` |
| Delivery | `Delivery` |
| Skills | `Skills` |
| Knowledge | `Knowledge` |
| Todos | `Todos`，ready 时显示 `completed/total`，stale 时追加 `(stale)` |
| Logs | `Activity & Logs` |

### Sidebar Row Taxonomy

| Row Kind | 触发条件 | 样式 |
|----------|----------|------|
| `EmptyState` | `No ...` / `... no ...` / `... yet.` | `muted_meta_fg + italic` |
| `Hint` | overflow hint 或 `… ` 开头 | `context_label + italic` |
| `SectionLabel` | `usage:` / `activity:` / `hits:` / `...:` | `section_header_fg + bold` |
| `Summary` | 普通摘要行、`- ` 列表项 | `text` |
| `Metric` | totals/store/queries/recognized/loaded 等计数字段 | `metric_emphasis_fg + bold` |
| `StatusOk` | status/health/freshness 中的正常值 | `status_idle_fg + bold` |
| `StatusWarn` | `running/pending/stale` | `status_running_fg + bold` |
| `StatusError` | `failed/error/disabled` | `error_message + bold` |
| `Meta` | 空行、reason/rewrite/recovery 等辅助说明 | `muted_meta_fg` |
| `Preview` | 缩进的正文预览 | `context_hint` |
| `Codeish` | path、`::`、`_`、`.`、`#`、`` ` `` 等 | `code_fg` |
| `TodoDone` | `✓` | `muted_meta_fg + dim` |
| `TodoActive` | `→` | `focus_border + bold` |
| `TodoPending` | `○` | `text` |
| `LogTool` | `[tool]` 或 `$` 开头 | `code_fg` |
| `LogError` | 含 `error/failed/panic` | `error_message` |
| `LogText` | 其他日志 | `context_hint` |

### Sidebar Density and Preview Rules

| Panel | 未聚焦 preview 限额 | 超出后行为 |
|-------|---------------------|------------|
| Diagnostics | `6` 行 | 追加 `… n more lines · focus panel for detail`（panel 足够高时自动显示全部，省略 hint） |
| Delivery | `6` 行 | 同上 |
| Skills | `5` 行 | 同上 |
| Knowledge | `6` 行 | 同上 |
| Todos | `6` 行 | `… n more lines · focus panel to scroll`（panel 足够高时自动显示全部，省略 hint） |
| Logs | `6` 行 | 同上 |
| 任意聚焦 panel | `∞` | 不做 preview clamp |

「足够高」判断：`inner_h = rect.height − 2 > 0 && raw_lines.len() <= inner_h`。满足时强制 `effective_limit = ∞`，skip overflow hint。

### Sidebar Section Weight Rules

Sidebar section body 不再等比分配，而是使用 `base + content_bonus + focus_bonus + running_bonus`。

| Section | Base weight | 额外规则 |
|---------|-------------|----------|
| Diagnostics | `8` | 内容 bonus 最多 `4` |
| Delivery | `14` | 运行中额外 `+2` |
| Skills | `10` | 内容 bonus 最多 `4` |
| Knowledge | `14` | 内容 bonus 最多 `4` |
| Todos | `13` | 内容 bonus 最多 `4` |
| Logs | `9` | 内容 bonus 最多 `4` |
| 任意聚焦 section | `+4` | 当前 focus panel 增大首屏预算 |

其中 `content_bonus = min(line_count, 8) / 2`。

## Input and Control Bands

### Input Box

| 元素 | 当前实现 |
|------|----------|
| 容器关系 | 与下方 `Input info bar` 共用同一套 `input` 边框 |
| 总壳层高度 | 固定 `9` |
| 实际文本区 | 位于共享壳层内部的上部区域，当前可见高度为 `4` |
| 前缀 | 首行左侧前缀为 ` > `；续行与滚动后的可见行使用等宽缩进前缀对齐 |
| 可视宽度 | `inner_width - 3`；超宽内容改为软换行而不是横向滚动 |
| Normal mode | 已有输入以 `input_placeholder` 渲染；空输入显示 `Press Space jk to enter insert mode` |
| Insert mode | 文本用 `input_text`；光标以反色块显示；`Enter=Send`，`Shift+Enter=Newline`，`↑/↓` 在多行输入内按可视行移动光标 |
| 多行行为 | 显式换行和软换行都会占用输入区可见行数；当内容超出 `4` 行可见高度时，viewport 会自动向下滚动以保持当前光标所在行可见；鼠标滚轮落在输入区内时只滚动该 viewport，不会侵入下方 `Input info bar` |
| 边框颜色 | `NORMAL -> mode_normal_fg`，`INSERT -> mode_insert_fg` |

### Context Bar

| 元素 | 当前实现 |
|------|----------|
| label | 固定为 `keys` |
| 优先级 | overlay hint > pending sequence > command hint > status notice > 默认热键说明 |
| 背景 | `context_bar.bg`，与 `Agent Response` panel 背景相同 |
| 行高 | 固定 `2` 行，可自动换行 |
| 作用 | 统一承载热键提示、状态 notice、slash hint、overlay 提示；现只占左列，不再横跨 Sidebar 下方 |

### Input Info Bar

| 元素 | 当前实现 |
|------|----------|
| 行高 | 固定 `1` 行 |
| 位置 | 输入壳层底部，和上方输入区共用同一套边框 |
| 内边距 | 左右各保留 `1` 列；与上方输入区和下方边框之间各保留 `1` 行留白 |
| 左侧内容 | 当前模型名；若有 delivery token，则显示为 `model   12.3k` |
| 最右状态图标 | 空闲时显示 `↑`；运行时使用一行压缩版 `5` 列单点 orbit 动画，只保留一个沿轨道移动的 glyph，并在 `● / ◉ / ◎ / ○ / ·` 间切换后右对齐到壳层末端 |
| delivery | 仅展示 token 摘要，格式为 `12.3k` 这类 `k` 单位一位小数，不再追加单位文案 |
| 背景 | 当前使用 `input.bg`，与上方输入区保持同一底色 |
| 分隔 | 各段之间只保留固定空格，不再使用 `·` 或竖线分隔符 |

### Bottom Status Bar

| 元素 | 当前实现 |
|------|----------|
| label 段 | `mode` / 可选 `flow` / 可选 `project` / `session` / `route` / `item` |
| 背景 | `status_bar.bg`，与 `Agent Response` panel 背景相同 |
| label/divider 前景 | 已去除旧灰色，统一回到正文级亮度 |
| 模式色 | `NORMAL` 蓝、`INSERT` 青绿；位于最左侧 |
| 运行色 | 已移至 `Input info bar`；运行中用 `status_running_fg`，空闲用 `status_idle_fg` |
| 分隔方式 | ` · ` |
| spinner | 底部状态条不再直接显示 running 文本 spinner；运行图标已移至 `Input info bar`，当前为单行压缩版单点 orbit 动画，任一时刻只显示一个 `● / ◉ / ◎ / ○ / ·` glyph |

## Overlay Inventory

| OverlayState | 默认尺寸 | 主要内容 |
|--------------|----------|----------|
| `Search` | `Small` | 搜索输入、当前 panel、匹配数 |
| `SearchResults` | `Large` | 搜索结果列表；超出视口时支持滚轮、`PgUp/PgDn`、`Home/End` 与 footer |
| `Confirm` | `Small` | 消息 + 双按钮 |
| `Detail` | `Large` | 详细内容列表；超出视口时支持滚轮、`PgUp/PgDn`、`Home/End` 与 footer |
| `Picker` | `Medium` | 可选项列表，选中行前缀 `›` |
| `InputPrompt` | `Small` | prompt + 输入框 |

## Focus and Interaction Cues

| 元素 | 当前实现 |
|------|----------|
| 聚焦 panel 标题 | 末尾追加 `◆` |
| 聚焦 panel 边框 | `focus_border + bold` |
| 非聚焦 panel 边框 | `border_dim` |
| 侧栏 rail 选中 section | `focus_border/title_fg + bold` |
| 侧栏子面板鼠标单击 | 顶部标题区与 body 均可响应点击，focus panel 并 seed 当前选中行；`panel_at()` 按完整矩形命中，避免相邻 panel 互相吞掉点击；`normalize_focus()` 在 sidebar rects 完全设定后才调用，防止每帧渲染时错误重置 focus |
| 侧栏 rail 键盘 | `Left/Right` 在 rail 条目间循环，`Enter` 展开并聚焦对应 panel，`x` 切换展开/收起 |
| 侧栏 panel 键盘 | `Ctrl+Left/Right` 在当前可见的 sidebar 子面板之间循环切换 focus |
| 选中文本 | `REVERSED` |
| Todo 高亮项 | 强制提到 `focus_border + bold` |
| 至少一个 sidebar section 保持展开 | 收起最后一个时阻止操作，并给出 notice |

## Current Beautification Boundaries

| 边界 | 当前状态 |
|------|----------|
| 可通过 `.omega/theme.toml` 覆盖的颜色 | surfaces/input/context bar/status bar/report/messages/overlay |
| 尚未暴露为配置项的派生 token | Markdown heading、code block、badge、thinking、final answer 某些固定色 |
| 布局可调参数 | 当前主要硬编码在 `render/layout.rs`、`render/sidebar.rs`、`overlay.rs` |
| 仍适合下一轮抽象化的内容 | density mode、sidebar section budget、derived render token preset、overlay size preset |

## Delta To The Target Aesthetic

下表描述的是这份基线曾经暴露出的主要审美缺口；`Task 15B-61 ~ 15B-64` 已在当前实现中针对这些方向完成第一轮收敛，用来帮助后续继续判断还要不要再往更强的 preset / density system 演进。

| 维度 | 当前实现 | 下一轮目标 |
|------|----------|------------|
| 主调色板 | 语义色较多，仍有 editor-theme 痕迹 | monochrome foundation + single accent |
| 标题与强调 | heading / badge / status 各有不同强调色 | 强调色更少，主要依赖亮度、粗细和空白层级 |
| Sidebar 切分 | 已有 card surface，但仍可感到内部小框与标签噪音 | 更像 quiet bento dashboard，以面层和留白切分 |
| Meta/Thinking | 已降噪，但仍较容易抢到注意力 | 默认更深、更 dim、更接近背景层 |
| Warning/Error | 语义明确，但仍可能有较大面积彩色正文 | 更小面积、更像 badge/prefix，而非整段高饱和文本 |
| Focus / Blur | 主要依赖边框与标题 `◆` | 非焦点区整体退后，接近窗口失焦感 |
| Overlay | 功能完整，但景深仍偏平面 | 更强的浮层感、细边与遮罩层次 |

## Recommended Use As A Baseline

- 后续所有 TUI 美化任务，应先更新本文档，再改具体 token 或布局实现。
- 新增颜色时，优先判断它属于现有 token 还是应新建 token；避免直接写死在 render 层。
- 新增 panel 或 overlay 时，优先补“布局表 + token 表 + 行型/状态表”，保证视觉系统继续可盘点。

## Change Log

- 2026-04-08: 首次整理当前 `omega-tui` 的默认颜色、布局关系、Response/Sidebar/Overlay 样式规则和交互标记，作为后续统一美化参考基线。
- 2026-04-08: 基于这份基线补充“当前实现到目标审美的差距”表，并把下一轮 UI 优化任务显式映射到 `Task 15B-61 ~ 15B-64`。
- 2026-04-08: `Task 15B-61 ~ 15B-64` 完成后，保留这份差距表作为后续 theme preset / density mode 演进的对照面，而不是继续作为未完成项清单。
- 2026-04-09: 同步 `Context bar` / `Status bar` 与 `Agent Response` 的同底色实现，去除 bar 上旧灰色 label/divider 前景；同时更新左列 `Response + Context(2) + Input(6)` / 右列 full-height `Sidebar` 的现行布局，以及无浅灰 header 背景、sidebar 子面板鼠标选中等最新实现细节。
- 2026-04-10: 左列控制带进一步收敛为共享边框的 `Input shell`：上部为输入区，下部一行为带内边距的 `Input info bar`；该行现显示 `model + token`，token 仅保留 `k` 值不再带单位，段间只用固定空格分隔，并把 `state` icon 右对齐到壳层末端。`mode` 已回到底部状态条最左侧，底部状态条当前为 `mode + flow + project/session/route/item` 的全局信息栏。输入区现已按多行 viewport 渲染，支持 `Shift+Enter` 插入换行、`↑/↓` 按可视行移动光标，并在内容超出可见高度时支持输入区内自动跟随与鼠标滚轮滚动，而不覆盖 `Input info bar`。运行态图标现已从单 glyph spinner 升级为单行压缩版单点 orbit 动画：glyph 会在 `● / ◉ / ◎ / ○ / ·` 间切换，但任一时刻只显示一个。
- 2026-04-09: 同步 `Task 15B-65 ~ 15B-69` 的现行 contract：`Delivery/Skills` 默认收起、rail/focus 锚定的 sidebar section viewport、统一 `Knowledge` 面板、`✓ / → / ○` todo glyph，以及带 scroll footer 的 `Detail/SearchResults` overlay。
- 2026-04-09: 修复 `normalize_focus()` 在每帧渲染起始时早于 `render_sidebar_body` 调用导致 sidebar panel focus 被错误重置的 bug；`normalize_focus` 现在移至 sidebar 渲染结束后调用，保证 rects 反映当前帧真实状态。新增 `Ctrl+Left/Right` 在可见 sidebar panel 间循环切换 focus；panel 足够高时自动压制 overflow hint（不再显示 `… more lines`）。
- 2026-04-09: Sidebar rail 回到顶部横向布局，当前使用 `▾/▸ + section label + count` 的单行 marker 风格，并让渲染窗口随 `rail_selection` 自动横向滚动。
