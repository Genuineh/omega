---
content_revision: 174
created: 2026-06-05
generation_id: gen_000087_r000174
language: bilingual
last_verified_commit: d8c30e3e9e310ce38cffa965be4688ed55a87787
owner: omega-team
projection_version: 87
related_prds: "[]"
source_doc_id: "adr:docs-decisions-010-tui-chat-turn-history"
source_path: docs/decisions/010-tui-chat-turn-history.md
status: accepted
supersedes: "[]"
updated: 2026-06-05
---

# 010: Agent Response 聊天记录模式 (Chat-Turn History)

## Status

Accepted (2026-06-05).

## Context

`Task 47 ~ 51`（卡片化）+ `Task 52 ~ 57`（Flex 原语 + StepUnit + StepDetailOverlay）让 `Agent Response` 面板有了：
- 每个 `MsgKind` 渲染成自己的卡片行
- Step/FinalAnswer/Thinking/Command 主面板只露 summary，详情进弹窗
- FlexContainer 装配 + kind 切换时 1 行空

但用户下一轮 review 提出面板应当**保持聊天的记录模式**：
- 一轮聊天 = `1 个 user message` + `N 个 agent response`（Step/Thinking/Command/FinalAnswer 多个 sub-record），合起来是一个 turn
- 多轮聊天时所有 turn 都保留在面板里，autoscroll 到最新但可滚回去
- Turn 之间视觉节奏比 kind 切换稍宽（标识"新 turn 开始"）
- Agent response 内部仍可保留现有的 summary + 弹窗详情模式
- User message 应当用 chat bubble 样式明确和 Agent response 区分

把上述 4 条横向归纳成一个设计语言：**Agent Response = 一系列 ChatTurn；每个 ChatTurn = 1 User + N Agent sub-records；多 turn 持久可见**。这是本 ADR 接受的核心抽象。

## Decision

### 1. 新增 ChatTurn 视觉单元

新文件 `crates/omega-tui/src/render/chat_turn.rs`，定义：

```rust
pub struct ChatTurn {
    pub user_msg: ResponseDisplayLine,
    pub agent_msgs: Vec<ResponseDisplayLine>,
    pub turn_index: usize,
}

impl ChatTurn {
    pub fn from_lines(&[ResponseDisplayLine], turn_index: usize) -> Option<Self>;
    pub fn render(&mut self, frame: &mut Frame, area: Rect, inner_w: usize, colors: &ColorScheme) -> Vec<Rect>;
}
```

Turn 边界 = 第一个 `MsgKind::User` line 开始，下一个 `User` 或切片末尾结束。

### 2. AgentResponse 容器改为 ChatTurn 列表

`render_response_panel` 外层从 `FlexContainer { Column, gap=1 }` 升级为 `FlexContainer { Column, gap=2 }` 装 ChatTurn 列表，turn 之间 2 行空。Turn 内部用 `FlexContainer { Column, gap=1 }` 装 sub-units（保留 T-56 的 kind 切换 1 行空行为）。

### 3. User Message 渲染成 Chat Bubble

User msg 渲染成 2 行：
- `▶ You` 标题行（BOLD + `user_badge_fg` 色）
- 内容行（2 字符缩进，普通 `text` 色）

Agent response sub-units 保持 T-51 配色与 glyph 区分。

### 4. 多 Turn 滚动 Affordances

新增 4 个 hotkey：
- `End` / `G`：跳到最新 turn
- `Home` / `g`：跳到最早 turn
- `n`：下一个 turn
- `p`：上一个 turn

`App` 维护 `response_turn_index: usize`（默认 0 = 跟随最新）和 `response_turn_count: usize`。面板标题区加 "turn N of M" 提示（仅在 user 偏离最新时显示）。

### 5. TurnDetail 聚合详情弹窗

新增 `OverlayState::TurnDetail(TurnDetailOverlay)` 变体，平铺整个 turn 的内容（user msg + 所有 Step/Thinking/Command/FinalAnswer 全文），不嵌套 StepDetail。Enter 在 user msg 行触发 `App::open_turn_detail_overlay(turn_index)`。

## Alternatives Considered

### 替代 A：保留当前的 per-MsgKind 卡片化，不引入 turn 概念

继续按 kind 渲染，turn 之间的视觉节奏靠 kind 切换时的 1 行空暗示。**否决**：用户明确要求"聊天记录模式"，1 行空不足以标识"新 turn 开始"；用户也无法快速跳到"上一个/下一个 turn"。

### 替代 B：让 ChatTurn 直接成为 ResponseCard（data layer 的类型）

在 `app/response.rs` 的 `ResponseCard` 上加 turn 字段。**否决**：`ResponseCard` 已经承载 80% 的 section-level 数据，加 turn 字段会让类型变大且不易扩展；T-58 计划在 render 层做 turn 边界检测，data layer 保持原样。

### 替代 C：TurnDetail 嵌套 StepDetail（点 user msg 后还能下钻到 StepDetail）

在 TurnDetail 里给每个 Step 加一个 "drill down" 入口。**否决**：overlay 栈嵌套深度 ≥ 3（TurnDetail → StepDetail → ToolDetail）对用户负担大；TurnDetail 是**聚合视图**而非 drill-down 入口，需要 drill-down 仍走现有 Enter 路径。

### 替代 D：多 turn 持久性靠 data layer 截断（只保留最近 N turn）

`output_msgs` 设一个 `max_recent = 50`，超过就丢弃。**否决**：用户明确说"始终都能看到聊天记录"，截断违反需求；存储不是瓶颈，渲染侧做好 scroll 即可。

## Consequences

### 正面

- **聊天记录体验**：User/Agent 配对的 turn 模式对齐 ChatGPT/Claude.ai 等 chat UIs，用户熟悉。
- **多 turn 持久可见**：旧 turn 不会因为新 turn 滚动而消失，scroll affordance 让用户能找回任何 turn。
- **可扩展**：`ChatTurn` 是 render 层的抽象，未来加 turn-level 操作（复制/分享/收藏/翻译）只需要扩展 `ChatTurn` 的接口。
- **可测**：`from_lines` 纯函数 + `render` 纯渲染，单元测试覆盖率高。

### 负面

- **视觉密度**：turn 内部 sub-record 紧贴 + turn 之间 2 行空，节奏比之前稍紧但比 chat 风格的"每条一行"更结构化。
- **hotkey 增加 4 个**：与现有 `End`/`Home`（terminal 默认行为）有冲突，需要明确"在 response panel focus 时才生效"。
- **TurnDetail 是新弹窗类型**：复用现有 `OverlayState` 枚举，但新增一个变体，所有 match site 都要更新（与 T-55 一样的负担）。

## Implementation Plan

按 Task 58 ~ 62 落地，每 Task 独立 PR：

| Task | 内容 | 依赖 | 估时 |
|---|---|---|---|
| T-58 | `chat_turn.rs` + `ChatTurn::from_lines` + `render_response_panel` 改用 ChatTurn 列表 | T-52~57 | 0.5 day |
| T-59 | User msg chat-bubble 样式 (`render_user_bubble` helper) | T-58 | 0.25 day |
| T-60 | 4 个 hotkey + `response_turn_index` state + "turn N of M" 提示 | T-58 | 0.5 day |
| T-61 | `OverlayState::TurnDetail` + `App::open_turn_detail_overlay` + 路由 | T-58, T-59, T-60 | 0.5 day |
| T-62 | 多 turn 持久性集成测试 | T-58~61 | 0.25 day |

总计 ~2 工作日，5 个独立 PR。

## References

- `docs/specs/omega-tui-chat-turn-history.md` — 设计规格
- `docs/specs/omega-tui-flex-layout-and-step-unit.md` — T-52 ~ T-57 上一轮规格
- `docs/decisions/009-tui-flex-layout-primitives.md` — ADR-009 (flex layout system)
- `docs/decisions/008-tui-component-architecture-refactor.md` — ADR-008 (Panel/Section/Card)
- `docs/TODO.md` — Task 58 ~ 62
