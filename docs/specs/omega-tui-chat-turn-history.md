---
content_revision: 174
created: 2026-06-05
generation_id: gen_000087_r000174
language: bilingual
last_verified_commit: d8c30e3e9e310ce38cffa965be4688ed55a87787
owner: omega-team
projection_version: 87
related_prds: "[]"
source_doc_id: "spec:docs-specs-omega-tui-chat-turn-history"
source_path: docs/specs/omega-tui-chat-turn-history.md
status: draft
supersedes: "[]"
updated: 2026-06-05
---

# Omega TUI Chat-Turn History 规范

## Overview

经过 `Task 47 ~ 51`（卡片化）、`Task 39A`（组件抽象）、`Task 52 ~ 57`（Flex 原语 + StepUnit + StepDetailOverlay）几轮迭代，`Agent Response` 已经具备：
- 每个 `MsgKind` 一行 header + 摘要
- Step/FinalAnswer/Thinking/Command 在主面板只露 summary，详情在弹窗
- FlexContainer 装配 + 1 行空 kind 切换

但是用户提出 Agent Response 应当**保持聊天的记录模式**（chat-log display mode）：

1. **一轮聊天 = 一个视觉单元**：用户发了什么（一条 User 记录）+ Agent 怎么回答（N 条 Step/Thinking/Command/FinalAnswer 记录），合在一起是一个 turn。
2. **多轮聊天全部保留**：随着用户多次发送消息，所有 turn 都在面板里（不截断不丢弃），用户可以滚回去看旧 turn。
3. **回答里可继续用现有的 summary + 弹窗详情**：Step/Thinking/Command 各自的详情照旧在 StepDetailOverlay 里。
4. **Turn 之间的视觉节奏更明显**：现在每 kind 之间 1 行空，多了；turn 之间应当 1~2 行空（比 kind 切换稍宽），turn 内部各 sub-record 之间可以紧贴或 1 行空。

本规格定义 T-58 ~ T-62 这一组任务：把"卡片化"升级为"聊天记录化"，引入 `ChatTurn` 作为新的视觉单元，并补齐多 turn 滚动 affordances。

## Goals

- 在 `crates/omega-tui/src/render/chat_turn.rs` 新增 `ChatTurn` 视觉单元：包含 1 个 User 子单元 + N 个 Agent sub-record（Step/Thinking/Command/FinalAnswer）子单元；用 `FlexContainer { Column, gap=0 }` 装配内部，turn 自身作为外层 FlexContainer 的一个 child。
- 重构 `render_response_panel`：外层用 `FlexContainer { Column, gap=2 }` 装 turn 列表，turn 之间留 2 行空（比 T-56 的 1 行空稍宽，标识"新 turn 开始"）。每个 turn 内部 sub-record 之间的视觉节奏保持（kind 切换时 1 行空；同 kind 紧贴）。
- 多 turn 历史在 `App` 端已经持久化（`output_msgs: Vec<Msg>` 全量保留）；渲染路径只是**不要因为可见区域不够就裁掉**——已经做到了，验证即可。
- 新增 turn 级别的导航 affordance：`End`/`G` 跳到最新 turn、`Home`/`g` 跳到最早 turn、`n`/`p` 跳到下/上 turn。
- 新增 `OverlayState::TurnDetail(TurnDetailOverlay)`：turn 级别的详情弹窗，把整轮所有 sub-record 一次性平铺展示（不靠用户一个一个点开 StepDetail）。
- User msg 在面板里继续保留 `▶ You` badge + 内容（已有 T-51），但**视觉上更明确是 chat bubble**（加 prefix 间距让它和 Agent response 视觉上区分开）。

## Non-Goals

- 不改 data layer（`omega_session` / `response.rs` 不动）。
- 不重写 `omega-tui` 的 event loop，只在 `event/key.rs` 加 4 个 hotkey 路由。
- 不在 TurnDetail 里重新画子 overlay（StepDetailOverlay 仍然是子单元的入口）。TurnDetail 是一个**聚合视图**，不嵌套 StepDetail。
- 不实现跨 turn 的全局搜索/索引（T-21+ 已有 spec 但不在本轮范围）。
- 不持久化 `TurnDetail` 的滚动位置到 `App` 端（每次打开从 0 开始）。

## Architecture

### A. ChatTurn (Task 58)

新文件 `crates/omega-tui/src/render/chat_turn.rs`：

```rust
/// A "chat turn" = 1 user message + N agent response records.
/// Renders as a vertical stack of sub-units (one per message in
/// the turn) with `FlexContainer { Column, gap=0 }` (so the
/// per-kind gap inside a turn is handled by each sub-unit's own
/// `StepUnit::render` or by the outer container's gap when
/// adjacent kinds differ).
pub struct ChatTurn {
    /// The 1 user message (always present at the top of the turn).
    pub user_msg: ResponseDisplayLine,
    /// The N agent response messages (Step/Thinking/Command/
    /// FinalAnswer/...); may be empty if the agent hasn't replied
    /// yet.
    pub agent_msgs: Vec<ResponseDisplayLine>,
    /// The 1-based turn index (for display in the chat-bubble
    /// header).
    pub turn_index: usize,
}

impl ChatTurn {
    /// Build a `ChatTurn` from a slice of `ResponseDisplayLine`s.
    /// A new turn starts at the first `MsgKind::User` line and
    /// continues until the next `MsgKind::User` or the end of the
    /// slice.
    pub fn from_lines(
        lines: &[ResponseDisplayLine],
        turn_index: usize,
    ) -> Option<Self>;

    /// Render the turn into `area`. Returns the per-sub-unit rects.
    pub fn render(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        inner_w: usize,
        colors: &ColorScheme,
    ) -> Vec<Rect>;
}
```

`ChatTurn::render` 内部用 `FlexContainer { Column, gap=1 }` 装：user_msg 渲染成 1 行（chat bubble 样式），agent_msgs 各渲染成 1 行（用 T-53 的 `StepUnit::render` 或 T-54 的 `build_single_line_unit`）。同 turn 内 kind 切换插入 1 行空（同 T-56 行为，但这次发生在 turn 内部）。

### B. AgentResponse container (T-58 + T-56 合并)

`render_response_panel` 改用 2 层 FlexContainer：

```
[Flex: Column, gap=2]                     ← 外层 turn 列表
  turn_1  (ChatTurn, N rows)
  turn_2  (ChatTurn, M rows)
  ...
```

每个 turn 是一个 child child。`gap=2` 是 turn 之间的间距（2 行空，比 kind 切换的 1 行空稍宽）。

### C. User message chat-bubble (Task 59)

User 消息的视觉强化：
- `▶ You` badge（前缀 glyph `▶` + `You` label）+ 内容
- BOLD 标题
- 背景色 = `panel_bg`（与面板同色，视觉上"嵌入"面板）
- 缩进 = 0（与 panel border 对齐）

具体渲染（每个 user message 一行）：

```
▶ You
  Tell me a joke.
```

- `▶ You` 行 BOLD + `user_badge_fg` 色
- 内容行普通 `text` 色，2 字符缩进

```rust
fn render_user_bubble(line: &ResponseDisplayLine, colors: &ColorScheme) -> Line {
    // 标题行 "▶ You"
    let header = Line::from(vec![
        Span::styled(format!("{} ", Glyph::BULLET), ...),  // ▶ glyph
        Span::styled("You", Style::default().fg(colors.user_badge_fg).add_modifier(Modifier::BOLD)),
    ]);
    // 内容行：2 字符缩进
    let body = Line::from(Span::styled(
        format!("  {}", line.text),
        Style::default().fg(colors.text),
    ));
    // 这两行是同一个 user msg 的多行表示
    vec![header, body]
}
```

### D. Turn-level scroll affordances (Task 60)

新增 4 个 hotkey（在 `event/key.rs` 的 response panel 处理路径里）：

- `End` (or `G`): `jump_to_latest_turn()` — 滚动到当前最新 turn
- `Home` (or `g`): `jump_to_oldest_turn()` — 滚动到最早 turn
- `n`: `next_turn()` — 跳到下一个 turn（保留 response_state 在该 turn 内的位置）
- `p`: `prev_turn()` — 跳到上一个 turn

实现：
- `App` 维护 `response_turn_index: usize`（默认 0）
- `response_turn_count: usize`（在 `render_response_panel` 末尾由 `response_display_lines` 派生）
- 在面板标题区加 "turn N of M" 提示（可选，复用 T-51 的 `focus_border` 色）

### E. TurnDetail overlay (Task 61)

新增 `OverlayState::TurnDetail(TurnDetailOverlay)`：

```rust
pub struct TurnDetailOverlay {
    pub origin_panel: Panel,
    pub turn_index: usize,
    pub title: String,                   // "Turn 2: Tell me a joke."
    pub sections: Vec<TurnDetailSection>,  // 1 per sub-record
    pub scroll: usize,
}

pub struct TurnDetailSection {
    pub kind: MsgKind,
    pub label: String,                   // "Step 1" / "Final Answer" / "Thinking"
    pub body: Vec<String>,                // flat text lines
}
```

- `TurnDetail` 一次性平铺整个 turn 的内容（user msg + 所有 sub-record 全文）
- 不嵌套 `StepDetail`（避免 overlay 栈过深）
- Esc 一层退

### F. Aggregate summary on user msg (Task 62)

User msg 在 turn 里的"摘要"= 紧跟其后的 agent 响应记录数：
- `▶ You` (1 line) — title
- `Tell me a joke.` (1 line) — body
- (下一行起) 紧贴 agent 的 Step / FinalAnswer 子单元

这与当前 T-51 行为一致，无需新代码。但要确保 turn 内部的"摘要"逻辑正确：user msg 不再需要 "variable" 行（variable 是 Step/FinalAnswer 自己的事）。

## Data-Model

不变。复用现有 `output_msgs: Vec<Msg>` + `response_display_lines() -> Vec<ResponseDisplayLine>`。

新加的 typed contract（在 `omega-tui` 里）：

```rust
/// Snapshot of a chat turn's content (used by TurnDetail overlay
/// and the chat-bubble render path). Built on demand by
/// `App::snapshot_chat_turn(turn_index)`.
pub struct ChatTurnSnapshot {
    pub turn_index: usize,
    pub user_text: String,
    pub user_message_id: Option<String>,
    pub agent_sections: Vec<AgentSectionSnapshot>,
}

pub struct AgentSectionSnapshot {
    pub kind: MsgKind,
    pub message_id: Option<String>,
    pub header_text: String,
    pub body_lines: Vec<String>,
}
```

## Testing

按每 Task 配测试：

1. **T-58 ChatTurn unit test** (5 case)
   - `from_lines` 把连续 1 user + 3 agent 切成 1 turn
   - `from_lines` 把 2 个 user 分成 2 turn
   - `render` 0 高度 area → 返回空
   - 1 turn 内部 user + agent 各渲染到自己的 rect
   - kind 切换时 turn 内部 1 行空

2. **T-59 User bubble test** (3 case)
   - User msg 渲染包含 `▶ You` badge
   - User msg 内容 2 字符缩进
   - Agent response summary 的 glyph 区分（Step=◉, FinalAnswer=◆, Thinking=◦）

3. **T-60 Scroll affordances** (4 case)
   - `End` 跳到最新 turn
   - `Home` 跳到最早 turn
   - `n`/`p` 在 turn 间跳转
   - 状态条 "turn N of M" 正确

4. **T-61 TurnDetail overlay** (4 case)
   - 打开 TurnDetail → 平铺 user + 全部 sub-record
   - 多 turn 时只显示当前 turn 的内容
   - Esc 退到外层
   - 内容按 sub-record 顺序（user → Step → FinalAnswer）

5. **T-62 Multi-turn visibility** (3 case)
   - 推 5 turn → 面板里都有，auto-scroll 默认到第 5 turn
   - 手动 `Home` 跳到 turn 1
   - `End` 跳回 turn 5（重新 auto-follow）

回归：T-58 ~ T-62 完成后所有原 258 个 omega-tui 测试仍然 100% 绿。

## Change Log

- 2026-06-05: 初稿。定义 `ChatTurn` 视觉单元、User chat-bubble 样式、多 turn 滚动 affordances、TurnDetail 聚合 overlay。配套 Task 58 ~ 62。
