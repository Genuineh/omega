---
content_revision: 101
created: 2026-03-31
generation_id: gen_000017_r000101
last_verified_commit: N/A
owner: omega-team
projection_version: 17
related_prds: []
source_doc_id: "spec:docs-specs-omega-tui-message-display-polish"
status: implemented
supersedes: []
updated: 2026-04-02
---

# Omega TUI Message Display Polish Specification

## Overview

Status note (2026-04-02): `Task 15B-40 ~ 15B-46` 已完成。本文档现作为已实现的 message-display 基线保留，用于后续 polish 和回归检查。

当前 `Agent Response` 的内容呈现能力停留在纯文本 + 结构化 section header 阶段：Markdown 原样输出、消息间缺少视觉分隔、长回复不便扫读、代码块与正文在同一颜色/字体下混排。本规格定义一组面向"美观、有序、阅读容易"的渐进优化任务，涵盖 Markdown 渲染、消息分隔与角色标识、代码块视觉区分、step/tool/thinking 信息密度优化、以及 Final Answer 阅读体验强化。

所有优化在现有 `omega-tui` / `omega-theme` / `omega-session` 三层边界内完成，不改变 runtime UI contract 核心协议。

## Problem Statement

日常使用中阅读体验的主要痛点：

1. **Markdown 原样呈现**：标题、列表、粗体/斜体、行内代码、代码块全部以纯文本展示，用户必须自行在脑中解析结构。
2. **消息角色不清**：User / Assistant / System / Tool 消息在 response panel 中仅靠颜色差异区分，没有清晰的前缀标识或视觉分段。
3. **代码块无边界**：Agent 回复中的代码块与正文在同一背景色下混排，代码开始/结束边界不明确，语言标识丢失。
4. **长回复扫读困难**：多段落、多代码块回复没有段间距或分隔线，整屏看起来是一面无差异的文字墙。
5. **Step/Tool 摘要信息密度高**：tool lane 当前是紧凑的单行列表，10+ tool 时容易淹没 step 正文。
6. **Final Answer 不够突出**：final answer 是用户最关注的结果，但视觉权重与普通 step 正文差异不足。
7. **Thinking 折叠摘要弱**：折叠后的 `= reasoning · N lines · ...` 在深色背景下辨识度不够。

## Goals

- 让 Agent 回复中的 Markdown 结构在终端中可识别：标题加粗/颜色区分、列表缩进正确、行内代码反色、代码块背景色区分。
- 让不同消息源（User / Assistant / System / Tool）在 Response 中具有一眼可辨的视觉标识。
- 让代码块有独立的视觉容器感（背景色、边框或缩进），并显示语言标注。
- 让长回复通过段间距、分隔线或空行提升可扫读性。
- 让 Final Answer 区块在视觉上比普通 step 更醒目。
- 让 tool lane 在工具数量多时可折叠摘要。
- 让 Thinking 折叠摘要更加醒目可辨。

## Non-Goals

- 不做完整 tree-sitter / syntect 代码语法高亮（留给 Task 15B-9）。
- 不做输入历史记录（留给 Task 15B-10）。
- 不做面板内搜索（留给 Task 15B-11）。
- 不做面板宽度拖拽（留给 Task 15B-12）。
- 不改变 `RuntimeUiMessage` / `RuntimeUiEnvelope` 等核心 runtime UI contract。
- 不引入外部 WebView 或图形渲染。

## Design Principles

- **渐进增强**：每个子任务独立可交付，后续子任务在前序基础上叠加但不依赖全部完成。
- **主题可配**：所有新增颜色/样式通过 `RenderPalette` / `theme.toml` 暴露，用户可自定义。
- **退化安全**：窄终端（< 60 cols）或无色终端下，回退到纯文本 + 缩进，不产生渲染错乱。
- **性能无感**：渲染逻辑在每帧 < 1ms 内完成，不引入惰性解析以外的额外内存分配。

---

## Task Breakdown

### Task 15B-40: Markdown 基础渲染

- **Priority**: High
- **Complexity**: L
- **Dependencies**: 无
- **Affected crates**: `omega-tui`
- **Description**:
  在 `response.rs` 的消息文本渲染管线中引入轻量 Markdown 解析层，支持以下元素的终端友好呈现：
  - **标题**（`#` ~ `###`）：加粗 + 颜色区分层级（H1 最亮，H3 较暗）。
  - **列表**（`-` / `*` / 数字）：自动缩进 2 字符，嵌套列表递增缩进。
  - **行内代码**（`` ` ``）：反色（fg/bg 互换）或使用 `inline_code_bg` 主题色。
  - **粗体/斜体**（`**` / `*`）：分别映射到 `BOLD` / `ITALIC` modifier。
  - **分隔线**（`---`）：渲染为 `─` 重复填充行宽的水平线。
  - 不要求解析嵌套结构或引用块；第一版只处理顶层元素。
- **Implementation approach**:
  - 在 `ResponseDisplayLine` 上新增 `spans: Vec<StyledSpan>` 字段（替代目前整行单一 style），每行可包含多段不同样式的文本片段。
  - 新增 `omega-tui/src/render/markdown.rs` 模块，逐行解析 Markdown 元素并输出 `Vec<StyledSpan>`。
  - `RenderPalette` 新增 `heading_1_fg`、`heading_2_fg`、`heading_3_fg`、`inline_code_bg`、`hr_fg` 等主题色。
  - `layout.rs` 的 `List` item 渲染从 `ListItem::new(Span::styled(...))` 改为 `ListItem::new(Line::from(spans))`。
- **Acceptance criteria**:
  - Agent 回复包含 `## Title` 时，标题行加粗且颜色与正文不同。
  - `- item` 列表项有 2 字符缩进。
  - `` `code` `` 行内代码有反色或背景色区分。
  - `**bold**` 显示为粗体。
  - `---` 渲染为水平分隔线。
  - 窄终端（< 60 cols）下不出现渲染错乱。

### Task 15B-41: 代码块视觉容器

- **Priority**: High
- **Complexity**: M
- **Dependencies**: Task 15B-40
- **Affected crates**: `omega-tui`, `omega-theme`
- **Description**:
  对 Markdown 代码块（` ``` `）提供独立的视觉容器感：
  - 代码块第一行显示语言标注（如 `rust`、`python`），使用 `code_lang_fg` 主题色，右对齐或左前缀。
  - 代码块所有行使用 `code_block_bg` 背景色（如 `#1e1e2e`），与正文背景形成区分。
  - 代码块首尾各添加一行视觉边界（`─` 或 `▁▔` 等 box-drawing 字符）。
  - 代码块内文本不做 Markdown 解析，避免误触 `*` / `_` 等符号。
- **Implementation approach**:
  - `markdown.rs` 解析 `` ```lang `` 开始和 `` ``` `` 结束，将中间行标记为 `CodeBlock` span 类型。
  - `style.rs` 对 `CodeBlock` 行应用 `code_block_bg` 背景。
  - `RenderPalette` 新增 `code_block_bg`、`code_lang_fg`、`code_border_fg`。
- **Acceptance criteria**:
  - Agent 回复中的 ` ```rust ... ``` ` 块有背景色与正文不同。
  - 语言标注 `rust` 在块首行可见。
  - 代码块内 `*variable*` 不被解析为斜体。

### Task 15B-42: 消息角色标识与分段

- **Priority**: High
- **Complexity**: M
- **Dependencies**: 无
- **Affected crates**: `omega-tui`, `omega-theme`
- **Description**:
  让消息源在 Response 中一目了然：
  - **User 消息**：前缀 `▶ ` 或 `> `，使用 `user_badge_fg` 颜色，与 assistant 回复之间插入 1 行空行。
  - **Assistant 正文**（非 step block 时）：前缀 `◆ ` 或无前缀但使用独立 `assistant_badge_fg`。
  - **System/Warning 消息**：前缀 `⚠ ` 或 `! `，使用 `warning_badge_fg`。
  - **Error 消息**：前缀 `✗ `，使用 `error_badge_fg`。
  - **消息间分隔**：不同源的连续消息之间自动插入 1 行空行或细分隔线，避免视觉粘连。
- **Implementation approach**:
  - `render_message_lines()` 中根据 `MsgKind` 为首行添加 badge prefix。
  - `response_display_lines()` 中在源切换时插入 `MsgKind::Separator` 空行。
  - `RenderPalette` 新增 `user_badge_fg`、`assistant_badge_fg`、`warning_badge_fg`、`error_badge_fg`。
- **Acceptance criteria**:
  - User 输入与 Agent 回复在视觉上有清晰间隔和角色标识。
  - Warning / Error 消息有醒目的前缀符号。
  - 连续同源消息不会被额外分隔干扰。

### Task 15B-43: Final Answer 视觉强化

- **Priority**: Medium
- **Complexity**: S
- **Dependencies**: Task 15B-40
- **Affected crates**: `omega-tui`, `omega-theme`
- **Description**:
  让 Final Answer 成为 turn 中视觉权重最高的区块：
  - header 行使用加粗 + 更亮的前景色（如 `#50fa7b` 绿色系）并增加 `━` 顶部装饰线。
  - body 区域左边距增加 2 字符缩进（与 step 的 2 字符拉开到 4 字符），或使用竖线边饰 `│ `。
  - 如果 Final Answer 内容应用了 Task 15B-40 的 Markdown 渲染，标题/列表/代码块的样式在 Final Answer 内保持一致。
- **Implementation approach**:
  - `render_message_lines()` 中为 `MsgKind::FinalAnswer` 的 header 前插一行 `━` 装饰。
  - body 行前缀从 `"  "` 改为 `"  │ "` 竖线边饰。
  - `RenderPalette` 新增 `final_answer_accent_fg`、`final_answer_border_fg`。
- **Acceptance criteria**:
  - Final Answer 区块在视觉上与 Step 区块有明确差异。
  - 长 Final Answer 的 body 有连续竖线引导阅读。
  - 窄终端下竖线退化为普通缩进，不溢出。

### Task 15B-44: Tool Lane 折叠与密度优化

- **Priority**: Medium
- **Complexity**: M
- **Dependencies**: 无
- **Affected crates**: `omega-tui`
- **Description**:
  当 step 内工具调用数量多（≥ 6）时，默认折叠为摘要行，用户可展开查看完整列表：
  - 默认：`  tools  12 total · 2 running` （单行摘要，可点击/按键展开）。
  - 展开后：恢复当前完整列表，尾部附 `[collapse]` 提示。
  - 3 个以内：始终展开，不显示折叠控制。
  - 折叠状态跟随 section，不跨 step 联动。
  - 工具摘要行对齐：tool name 列宽按当前 step 内最长 tool name 对齐，status 列固定宽度。
- **Implementation approach**:
  - `Msg` 新增 `tool_lane_collapsed: bool` 字段。
  - `render_message_lines()` 的 tool lane 分支根据 tool 数量和折叠状态决定输出行数。
  - `ResponseLineAction` 新增 `ToggleToolLane(section_id)` variant。
  - tool summary 格式化使用固定列宽对齐。
- **Acceptance criteria**:
  - ≥ 6 个 tool 时默认只显示摘要行。
  - 按键展开后可见完整列表，再按可折叠。
  - Tool name 列左对齐、status 列右对齐。
  - ≤ 3 个 tool 时始终展开。

### Task 15B-45: Thinking 折叠摘要视觉增强

- **Priority**: Medium
- **Complexity**: S
- **Dependencies**: 无
- **Affected crates**: `omega-tui`, `omega-theme`
- **Description**:
  增强 Thinking 折叠摘要的辨识度：
  - 折叠态摘要：使用 `DIM` + `ITALIC` modifier + 专用 `thinking_summary_fg`（如 `#6272a4`），并在前缀使用 `▸` 替代 `=` 表示可展开。
  - 展开态头部：使用 `▾` 替代 `=` 表示已展开。
  - Streaming 态：使用脉冲式 `⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏` spinner 替代静态 `|` 前缀，与 status bar spinner 一致。
  - 完成态 body 行：使用较暗的 `thinking_body_fg`（如 `#44475a`）配合 `│ ` 竖线前缀，与 step 正文拉开对比。
- **Implementation approach**:
  - `thinking_placeholder_text()` 和 `summarize_thinking_text()` 更新前缀字符。
  - `thinking_summary_style()` / `thinking_body_style()` 使用新主题色。
  - `RenderPalette` 新增 `thinking_summary_fg`、`thinking_body_fg`。
  - Spinner 复用 `omega-tui` 已有的 spinner frame 逻辑。
- **Acceptance criteria**:
  - 折叠后的 Thinking 一眼可辨而非与 step 正文混淆。
  - Streaming 态有动态指示。
  - 展开后 Thinking body 与 step body 有明确色差。

### Task 15B-46: 段间距与长回复可扫读性

- **Priority**: Medium
- **Complexity**: S
- **Dependencies**: Task 15B-40
- **Affected crates**: `omega-tui`
- **Description**:
  提升多段落长回复的扫读体验：
  - Markdown 段落间（两个换行分隔）自动插入 1 行空白行。
  - 代码块前后自动保留 1 行空白行（如果原文没有）。
  - 列表结束后与下一段正文之间插入 1 行空白行。
  - Step 之间的空行确保至少 1 行（当前已部分实现，此任务统一标准化）。
- **Implementation approach**:
  - `markdown.rs` 段落检测逻辑在连续两个 `\n` 时输出空行 span。
  - 代码块解析时在首尾行前后检查并插入空行。
  - `response_display_lines()` 中 step 之间确保空行。
- **Acceptance criteria**:
  - 3+ 段落的回复中，段间有可见空行分隔。
  - 代码块前后有空行，不紧贴正文。
  - 不因空行过多浪费可视面积（最多连续 1 行空白）。

---

## Implementation Order

推荐执行顺序（括号内为并行机会）：

```
Phase 1 — 基础渲染能力
  Task 15B-40  Markdown 基础渲染          ← 最大价值，解锁后续
  Task 15B-42  消息角色标识与分段         ← 可与 15B-40 并行

Phase 2 — 深化代码与结果体验
  Task 15B-41  代码块视觉容器            ← 依赖 15B-40
  Task 15B-43  Final Answer 视觉强化     ← 依赖 15B-40
  Task 15B-46  段间距与可扫读性          ← 依赖 15B-40

Phase 3 — 信息密度与细节打磨
  Task 15B-44  Tool Lane 折叠密度优化     ← 独立
  Task 15B-45  Thinking 折叠摘要增强      ← 独立
```

Phase 1 完成后即有显著体验提升；Phase 2 & 3 可按需交付。

## Related Files

- [crates/omega-tui/src/app/response.rs](crates/omega-tui/src/app/response.rs) — 消息渲染主逻辑
- [crates/omega-tui/src/render/style.rs](crates/omega-tui/src/render/style.rs) — 行样式选择
- [crates/omega-tui/src/render/layout.rs](crates/omega-tui/src/render/layout.rs) — 面板布局与 List 渲染
- [crates/omega-tui/src/app.rs](crates/omega-tui/src/app.rs) — App 状态模型
- [crates/omega-theme/src/lib.rs](crates/omega-theme/src/lib.rs) — RenderPalette 主题系统
- [crates/omega-session/src/runtime_ui.rs](crates/omega-session/src/runtime_ui.rs) — runtime UI 消息契约
- [docs/specs/omega-tui-response-thinking-experience.md](docs/specs/omega-tui-response-thinking-experience.md) — 现有 response 结构规格
- [docs/specs/omega-tui-step-tool-thinking-refinement.md](docs/specs/omega-tui-step-tool-thinking-refinement.md) — tool/thinking 精修规格

## Open Questions

1. Markdown 解析应引入外部 crate（如 `pulldown-cmark`）还是自建轻量逐行解析器？轻量解析器更可控但不支持嵌套结构。
2. `ResponseDisplayLine` 改为多 span 后，现有单行整体样式的测试是否需要全部迁移？
3. 代码块背景色在 16-color 终端下是否有合理退化？可能需要通过 `COLORTERM` 环境变量检测。
