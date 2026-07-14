---
content_revision: 174
created: 2026-06-05
generation_id: gen_000087_r000174
language: bilingual
last_verified_commit: d8c30e3e9e310ce38cffa965be4688ed55a87787
owner: omega-team
projection_version: 87
related_prds: "[]"
source_doc_id: "spec:docs-specs-omega-tui-chat-log-summary-cleanup"
source_path: docs/specs/omega-tui-chat-log-summary-cleanup.md
status: draft
supersedes: "[]"
updated: 2026-06-05
---

# Omega TUI Chat-Log 摘要清理 (Chat-Log Summary Cleanup) 规范

## Overview

`Task 47 ~ 51` + `Task 52 ~ 62` 两轮升级让 `Agent Response` 面板变得混乱。本规格分析两轮升级的相互影响，定位 bug，并设计清理方案。

### 用户原话（节选）

> 现在 Agent Response 对聊天记录的展示有点混乱，会被之前的消息组件的改造变得不见。
> 我希望 Agent Response 页面就是聊天记录，而处理的动态都在聊天记录的摘要区简单展示，详情弹窗。

### 核心问题

`T-47 ~ 51` 的设计意图是"每条记录 = 主面板 1~2 行摘要 + 弹窗详情"：
- 摘要行 = `kind glyph + formatted title + state`
- 详情 = popup（`StepDetailOverlay` / `TurnDetailOverlay`）

`T-53 ~ 57` 实施时，`StepUnit` 简化为 `Vec<Line>`，每行由 `build_response_lines` 渲染 —— 这个函数返回的是 data layer 的**完整输出**（header + body），不是摘要。

`T-58` 的 `ChatTurn::render` 沿用 `StepUnit::render`，**结果每个 sub-record 在主面板里渲染了 header 行 + body 行**：
- Step header（`◉ step workflow_id Section ●`）✅ 摘要
- Step body（`  Found 3 jokes.`）❌ 应该是 popup 内容
- FinalAnswer header（`◆ final workflow_id Section ●`）✅ 摘要
- FinalAnswer body（`  │ Why did the chicken cross the road? ...`）❌ 应该是 popup 内容

实测 dump 验证（30-row terminal，1 turn = User + Step + FinalAnswer）：

```
│· You                                                                        │   ← user bubble title
│  ▶ Tell me a joke                                                           │   ← user bubble body
│                                                                             │   ← gap
│◉ step  child:wf-1  Search  ●                                                │   ← Step SUMMARY (1 row)
│                                                                             │   ← gap (shouldn't exist)
│  Found 3 jokes.                                                             │   ← Step BODY (should be in popup)
│                                                                             │   ← gap
│◆ final  child:wf-1  Answer  ●                                               │   ← FinalAnswer SUMMARY (1 row)
│                                                                             │   ← gap
│  │ Why did the chicken cross the road? To get to the other side!            │   ← FinalAnswer BODY (should be in popup)
```

**6 行内容（应该 4 行）**。Body 行在主面板和 popup 里**重复展示**。

## Goals

- 让 `ChatTurn::render` 在主面板只渲染**摘要**（每 sub-record 1 行）；body 行**只**在 popup 里展示。
- 摘要行 = 沿用 `build_header_line`（kind glyph + formatted title + state），per-kind 配色保留 T-51 配色：
  - Step: `◉ step workflow_id Section ●`（tool_status 决定 glyph）
  - FinalAnswer: `◆ final workflow_id Section ●` + 前 80 字符 body preview
  - Thinking: `◦ Thinking` 折叠态 + 摘要（"`Thinking · 5 lines`"）
  - Command: `◆ /cmd` + N lines
  - User / Agent / Error: badge + 整段 text
  - Routing / Separator: 1 行
- popup（`StepDetailOverlay` / `TurnDetailOverlay`）保持不变 —— 它们本来就消费 `message.text` 全量。
- 删除 `StepUnit::lines: Vec<Line>` 设计 —— `StepUnit` 改为只持有"摘要 + 详情"两种视图：
  - `summary_line: Line<'static>` —— 主面板用的 1 行
  - `body_lines: Vec<String>` —— popup 用的全量
- 同步更新测试：原本期望"header + body 2 行"的 T-47 快照测试改为期望"摘要 1 行"。

## Non-Goals

- 不动 `omega-session` / data layer。`message.text` 全量保留，popup 直接消费它。
- 不动 popup 渲染层（`render/overlay.rs::render_step_detail` + `render_turn_detail`）—— 它们已经走 `App::open_step_detail_overlay` / `open_turn_detail_overlay` 拿全量。
- 不重写 `Panel` / `Section` / `Card` / `Frame` / `FlexContainer` 这些 T-39 / T-52 的低层抽象。
- 不实现跨 turn 折叠/展开（保持 1 行摘要固定）。

## Architecture

### A. Per-Kind Summary Renderer (Task 63)

新文件 `crates/omega-tui/src/render/subunit_summary.rs`（或并入 `step_unit.rs`）：

```rust
/// Render the chat-log summary line for one `ResponseDisplayLine`.
/// Returns 1 Line that fits the inner width. The body content
/// of multi-line records (Step, FinalAnswer, etc.) is NOT
/// included here — the popup is the source of truth for the
/// full content.
pub fn build_subunit_summary(
    line: &ResponseDisplayLine,
    colors: &ColorScheme,
    inner_w: usize,
) -> Line<'static>;
```

`build_subunit_summary` 的内容（per kind）：
- `User` / `Agent` / `Error` / `Separator` / `Routing` → 走 `build_header_line`（已经在 T-51 写过）
- `Step` → `build_header_line` 同样的 kind glyph + title 格式；tool_status 决定 glyph
- `FinalAnswer` → kind glyph + title + 第一行 body 摘要（截断到 inner_w - glyph_width - 2）
- `Thinking` → `build_header_line` 同样的 kind glyph + 折叠态摘要（如 `Thinking · 5 lines`）
- `Command` → kind glyph + command name + N lines 摘要

### B. StepUnit 重构 (Task 63)

`StepUnit` 简化为：

```rust
pub struct StepUnit {
    /// 1-line summary for the chat log (T-63).
    pub summary_line: Line<'static>,
    /// Full body content for the popup (T-55 / T-61).
    pub body_lines: Vec<String>,
    /// Optional detail request for the Enter handler.
    pub detail_target: Option<StepDetailRequest>,
}
```

`StepUnit::render` 改为只渲染 `summary_line`（不渲染 body）。

`build_step_unit` / `build_single_line_unit` 工厂改用 `build_subunit_summary` 构造 summary。

### C. ChatTurn 改用 Summary (Task 63)

`chat_turn.rs::ChatTurn::render`：

```rust
// Old: each agent_msg uses build_response_lines (header + body, 2+ rows)
// New: each agent_msg uses build_subunit_summary (1 row only)
for line in &self.agent_msgs {
    if let Some(prev) = prev_agent_kind { ... gap ... }
    let summary = build_subunit_summary(line, colors, inner_w);
    let body = line.text.clone();
    children.push(FlexChild::length(Length(1), move |frame, rect| {
        let p = Paragraph::new(summary);
        frame.render_widget(p, rect);
    }));
    // Store the body for the popup (via detail_target).
    prev_agent_kind = Some(line.kind);
}
```

Chat log 视觉：

```
│· You                                          ← user bubble title (1 row)
│  ▶ Tell me a joke                             ← user bubble body (1 row, if text non-empty)
│                                               ← gap
│◉ step  child:wf-1  Search  ●                  ← Step SUMMARY (1 row)
│                                               ← gap
│◆ final  child:wf-1  Answer  Why did the...   ← FinalAnswer SUMMARY + preview (1 row)
```

**4 行内容**（user bubble 2 行 + 2 行 agent 摘要）。Body 在 popup 里。

### D. 弹窗不变

`App::open_step_detail_overlay` / `open_turn_detail_overlay` / `event/overlay_handlers.rs` 中现有的 `StepDetail` 弹出逻辑保持不变 —— 它们本来就走 `m.text` 全量（StepDetail rail 的 Tools / Output / Diagnostics + TurnDetail 的 sections）。

### E. 旧 inline body 渲染代码清理

- `response_card.rs::build_response_lines` 仍然保留（被 `StepUnit::render` 内部在 popup 数据流里使用），但 `ChatTurn::render` 不再调用它。
- 删除 `chat_turn.rs::ChatTurn::render` 中 `build_response_lines` 的调用代码。

## Data-Model

不变。`output_msgs: Vec<Msg>`、`message.text`、`ToolRun`、`StepSubflowStatus` 全部不动。

## Testing

### T-63 unit + integration tests

1. `build_subunit_summary` 单元测试（每 kind 1 个）：验证 summary line 包含 kind glyph + title + 适当的状态/预览
2. `ChatTurn::render` 单元测试更新：原本期望"header + body 2 行"，改为"摘要 1 行"
3. `iter_turns` 行为不变（之前 8 个 unit test 应全绿）

### T-64 snapshot tests

1. 替换 `snapshot_t47_step_renders_header_with_glyph_and_body` 为 `snapshot_t63_step_summary_is_one_row` —— 验证 orphan Step 的 chat log 只占 1 行
2. 类似替换 `snapshot_t47_same_kind_messages_have_no_blank_gap` 等
3. 保留 `snapshot_t58_user_msg_creates_chat_turn_with_bubble` 等
4. 新增 `snapshot_t63_chat_log_shows_summary_not_body`：User + Step + FinalAnswer 三类 record 推入后，chat log 只占 4 行（user title + user body + step summary + final summary），body 行不在主面板

### T-65 popup regression tests

1. 现有 T-57 / T-61 集成测试不变 —— 它们测试 Enter → popup，验证 popup 推入
2. 新增 `t65_step_detail_overlay_still_shows_full_body`：Step 推入后，open_step_detail_overlay 返回的 `Tools` / `Output` 包含 body 全文
3. 新增 `t65_turn_detail_overlay_still_shows_full_body`：FinalAnswer 推入后，open_turn_detail_overlay 返回的 sections 包含 body 全文

### T-66 文档更新

- 更新 `docs/TODO.md`：
  - T-47 的 implementation note 改为"1 行摘要 + 详情弹窗"
  - T-53 的 implementation note 改为"`StepUnit` 现在持有 `summary_line` + `body_lines`（不再用 `lines: Vec<Line>`）"
  - T-58 的 implementation note 补充"`ChatTurn::render` 用 `build_subunit_summary` 渲染 1 行摘要"
- 新增 ADR-011：`docs/decisions/011-tui-chat-log-summary-cleanup.md`，记录这次清理的动机、决策、影响

## Change Log

- 2026-06-05: 初稿。诊断 T-47~51 + T-52~62 冲突：ChatTurn::render 用 build_response_lines 渲染了完整 body 而非摘要。设计 T-63 改用 1 行摘要渲染，body 仅在 popup 展示。
